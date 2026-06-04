use std::sync::Arc;

use tokio::sync::Mutex;

use crate::{
    proto::{Request, Response},
    segments,
    server::state::ServerState,
};

pub async fn dispatch(req: Request, state: Arc<Mutex<ServerState>>) -> Response {
    let result = match req.cmd.as_str() {
        "gst" => {
            let path = req.args["path"].as_str().map(std::path::PathBuf::from);
            let pid = req.args["pane_pid"].as_u64().map(|n| n as u32);
            let force = req.args["force"].as_bool().unwrap_or(false);
            segments::git::render(path, pid, force).await
        }
        "battery" => segments::battery::render().await,
        "net" => {
            let mut st = state.lock().await;
            segments::network::render(&mut st.net_previous).await
        }
        "clients" => {
            let sa = req.args["session_attached"].as_u64().unwrap_or(0) as u32;
            let wac = req.args["window_active_clients"].as_u64().unwrap_or(0) as u32;
            segments::clients::render(sa, wac).await
        }
        "vim-bg" => {
            let pid = req.args["pane_pid"].as_u64().unwrap_or(0) as u32;
            segments::vim_bg::render(pid).await
        }
        "window" => {
            let dir_aliases = state.lock().await.dir_aliases.clone();
            match serde_json::from_value::<segments::window::WindowArgs>(req.args) {
                Ok(args) => Ok(segments::window::render(&args, &dir_aliases)),
                Err(e) => Err(anyhow::anyhow!("invalid window args: {}", e)),
            }
        }
        other => Err(anyhow::anyhow!("unknown command: {}", other)),
    };

    match result {
        Ok(output) => Response::ok(output),
        Err(e) => Response::err(e),
    }
}
