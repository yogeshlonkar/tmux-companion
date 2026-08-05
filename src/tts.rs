use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::{Context, bail};
use base64::Engine as _;
use futures_util::{SinkExt, StreamExt};
use serde::Deserialize;
use tokio_tungstenite::tungstenite::Message;
use tokio_tungstenite::tungstenite::client::IntoClientRequest;

const DEFAULT_URL: &str = "ws://127.0.0.1:8765/synthesize";

// Matches vachan-server's default VACHAN_ALLOWED_ORIGIN_PREFIX so a plain
// client is accepted without any server-side config change.
const ORIGIN: &str = "chrome-extension://tmux-tts-speak";

#[derive(Deserialize)]
#[serde(tag = "type")]
enum ServerMsg {
    #[serde(rename = "CHUNK_READY")]
    ChunkReady {
        #[serde(rename = "audioBase64")]
        audio_base64: String,
    },
    #[serde(rename = "SPEECH_DONE")]
    SpeechDone,
    #[serde(rename = "SPEECH_ERROR")]
    SpeechError { error: String },
}

/// Sends `text` to a vachan-server /synthesize WebSocket and plays each
/// sentence chunk as it streams back, in order.
pub async fn speak(url: Option<&str>, text: &str) -> anyhow::Result<()> {
    let text = text.trim();
    if text.is_empty() {
        return Ok(());
    }
    let url = url.unwrap_or(DEFAULT_URL);
    let player = resolve_player()?;

    let mut request = url.into_client_request()?;
    request.headers_mut().insert("Origin", ORIGIN.parse()?);

    let (mut ws, _) = tokio_tungstenite::connect_async(request)
        .await
        .with_context(|| format!("connecting to {url}"))?;

    let payload = serde_json::json!({ "text": text }).to_string();
    ws.send(Message::Text(payload.into())).await?;

    // Playback runs on its own blocking thread so the read loop below never
    // stalls mid-sentence - if it did, this task would stop polling the
    // socket (including replying to pings) for however long afplay/paplay
    // takes, and the server's ping timeout would kill the connection out
    // from under an in-flight send on its side.
    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<Vec<u8>>();
    let playback = tokio::task::spawn_blocking(move || {
        let mut index = 0u32;
        while let Some(audio) = rx.blocking_recv() {
            if let Err(err) = play_chunk(&player, &audio, index) {
                eprintln!("tts-speak: {err}");
            }
            index += 1;
        }
    });

    let result = async {
        while let Some(msg) = ws.next().await {
            let Message::Text(raw) = msg? else { continue };
            match serde_json::from_str::<ServerMsg>(&raw)? {
                ServerMsg::ChunkReady { audio_base64 } => {
                    let audio = base64::engine::general_purpose::STANDARD.decode(&audio_base64)?;
                    let _ = tx.send(audio);
                }
                ServerMsg::SpeechDone => break,
                ServerMsg::SpeechError { error } => bail!("vachan-server: {error}"),
            }
        }
        Ok(())
    }
    .await;

    drop(tx);
    playback.await.context("playback thread panicked")?;
    result
}

/// Removes its backing file on drop, so a chunk that fails to play (or a
/// player that panics) never leaves audio behind in the temp dir.
struct TempWav(PathBuf);

impl Drop for TempWav {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.0);
    }
}

fn play_chunk(player: &[String], audio: &[u8], index: u32) -> anyhow::Result<()> {
    let nanos = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().subsec_nanos();
    let path = std::env::temp_dir().join(format!("tts-speak-{}-{index}-{nanos}.wav", std::process::id()));
    std::fs::write(&path, audio)?;
    let _guard = TempWav(path.clone());

    let (program, args) = player.split_first().expect("resolved player is never empty");
    std::process::Command::new(program)
        .args(args)
        .arg(&path)
        .status()
        .with_context(|| format!("running {program}"))?;

    Ok(())
}

fn resolve_player() -> anyhow::Result<Vec<String>> {
    if cfg!(target_os = "macos") {
        return Ok(vec!["afplay".into()]);
    }
    if command_exists("paplay") {
        return Ok(vec!["paplay".into()]);
    }
    if command_exists("aplay") {
        return Ok(vec!["aplay".into(), "-q".into()]);
    }
    bail!("no audio player found (need afplay, paplay, or aplay)")
}

fn command_exists(name: &str) -> bool {
    std::env::var_os("PATH")
        .map(|paths| std::env::split_paths(&paths).any(|dir| dir.join(name).is_file()))
        .unwrap_or(false)
}
