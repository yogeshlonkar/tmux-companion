# Design

## Problem

tmux's `status-interval` fires every second.  The original setup called six
separate programs per tick — a Go binary and five zsh scripts.  Each involves
process-fork overhead, shell interpreter startup, and repeated reads of config
files or system interfaces.  On a busy machine this causes visible status-line
lag.

The goal: one persistent daemon that holds all state in memory, with clients
that connect over a Unix socket, send one JSON line, read one JSON line, and
exit.

## Architecture

```
┌──────────────────────────────────────────────────────┐
│  tmux status-interval (every 1 s)                    │
│                                                      │
│  #(tmux-companion gst …)  #(tmux-companion battery)  │
│         │                        │                   │
└─────────┼────────────────────────┼───────────────────┘
          │  Unix socket           │
          ▼                        ▼
┌─────────────────────────────────────────────────────┐
│  tmux-companion server                              │
│                                                     │
│  tokio async runtime, one task per connection       │
│                                                     │
│  ServerState (Arc<Mutex<_>>)                        │
│    net_previous: Option<(u64, u64, Instant)>        │
│    dir_aliases:  HashMap<PathBuf, String>           │
│                                                     │
│  SQLite (rusqlite, bundled)                         │
│    git_status  id TEXT PK, status BLOB, updated_at  │
└─────────────────────────────────────────────────────┘
```

### Same binary, two modes

```
tmux-companion server        # binds socket, accepts connections
tmux-companion <cmd> [args]  # connects, sends request, prints output, exits
```

The client auto-starts the server on first use: it tries to connect; on failure
it forks the server as a detached child, then retries with exponential backoff
(up to 10 attempts, 50 ms intervals).

### IPC protocol

Newline-delimited JSON, one request per connection.

```jsonc
// request  (client → server)
{"cmd": "gst", "args": {"path": "/repo", "force": false}}

// response (server → client)
{"output": "#[fg=color025,bg=color120] …", "error": null}
```

Each connection is handled by a spawned tokio task.  The task reads one line,
dispatches, writes one line, closes.

### Singleton guarantee

Before binding the socket, the server tries to connect to it.  If that
succeeds, another instance is running and the new process exits immediately.
If the connect fails, the stale socket file (if any) is removed and a fresh
`UnixListener::bind` is attempted.  A second concurrent startup race is benign:
only one bind wins; the loser exits.

## Module map

```
src/
  main.rs              clap CLI (Cmd enum), dispatch to client or server
  proto.rs             Request / Response serde types
  client.rs            connect-with-retry, spawn_server, send_and_print
  server/
    mod.rs             UnixListener accept loop
    state.rs           ServerState: net_previous, dir_aliases
    handlers.rs        req.cmd → segment fn
  segments/
    git.rs             git status parse + format + cache (main segment)
    battery.rs         macOS ioreg battery
    network.rs         netstat bandwidth delta
    clients.rs         tmux list-clients count
    vim_bg.rs          pgrep children, state=T + nvim
    window.rs          window title: index icons, path abbreviation, flags
  tmux/
    format.rs          Segment builder, colored_segment(), powerline_segment()
    icons.rs           Nerd Font Unicode codepoints
  db/
    mod.rs             rusqlite schema init, db_path()
    git_cache.rs       get / save GitStatus with TTL
```

## Git status segment

The main segment, ported from the Go `yrl gst` command.

### Caching

Git status is cached in SQLite with a 2-second TTL keyed by `base64(abs_path)`.
The server is persistent, so the cache persists across tmux refreshes for the
same repo.  A cache hit skips all git subprocess invocations — the round-trip
is then just: socket connect → JSON deserialize → SQLite lookup → JSON serialize
→ socket write.

The `--force` flag on `gst` skips the cache read (the result is still written
back so the next normal call benefits).

### Parsing

Runs `git status --untracked-files=all --branch --porcelain=v2` and
`git rev-parse --path-format=absolute --git-dir` concurrently via `tokio::join!`.
A second join concurrently resolves `is_gone` (`git branch -r`) and counts
stash entries (line count of `.git/logs/refs/stash`).

### Formatting

`status_line_mode(status, no_tmux)` is a direct port of yrl's `StatusLine` Go
function.  It builds a `Segment` (a `Vec<String>` whose elements are joined on
`to_string()`) and calls `colored_segment` / `powerline_segment` to produce
either `#[fg=colorXX,bg=colorYY]` (tmux format) or `\x1b[38;5;XXm\x1b[48;5;YYm`
(ANSI/no-tmux format).

One special case inherited from the Go original: in no-tmux mode,
`colored_segment(fg, bg, ARROW_RIGHT)` emits only the color-change escape codes,
not the arrow glyph itself.  The arrow appears only from the terminal reset
sequence appended at the end.

### Branch truncation

Matches Go's `shortBranch` logic exactly:
- Strip known prefix (`feat/`, `bugfix/`, `hotfix/`, `chore/`, `release/`) and
  prepend the matching icon.
- Truncate when `char_count > 20`: keep first 8 chars + `...` + last 10 chars.
  (Go iterates byte indices 0..len-1; truncation fires when index ≥ 20, i.e.
  when len > 20.  Tail is `branch[len-1-9:]` = last 10 chars.)

## Window segment

Port of `window-status.zsh`.

**Path abbreviation** (`abbreviate_path`):
1. Strip home prefix → replace with `~`.
2. Discard `RootDir` component; treat it as a prefix `/`.
3. Abbreviate every component except the last to its first character
   (dot-prefixed components keep two characters: `.c` for `.config`).
4. Ellipsize basename at 17 chars (head 7 + `…` + tail 7).
5. Apply dir-logo substitutions in priority order (most specific first):
   `~/g/mysetup`, `~/g/`, `~/b/`, `~/`, `~`, `/`.

**Dir aliases** are loaded once at server startup from `~/.yrl/lib/dir-aliases`
into `ServerState.dir_aliases` and cloned per request.

**Index icons**: 10 selected (filled) and 10 unselected (outline) number-circle
icons.  Codepoints extracted directly from `window-status.zsh` via byte
inspection.

**Process animation**: when `process ≠ zsh` and the window is not current, the
index color cycles through 20 colours keyed on `epoch_secs % 20`.

## Network monitor

`ServerState.net_previous` holds `(rx_bytes, tx_bytes, Instant)` from the
previous call.  On the first call the state is stored and an empty string is
returned.  On subsequent calls the delta divided by elapsed seconds gives the
throughput.  Speeds below 20 480 B/s are suppressed.

`netstat -ibn` output is parsed by `parse_netstat_output` (pure fn, unit
tested).  Loopback interfaces (`lo*`) are excluded.

## SQLite

`rusqlite` with the `bundled` feature compiles SQLite from source — the binary
has no runtime dependency on a system SQLite.

Each DB operation opens its own connection and closes it on return.
`rusqlite::Connection` is `!Send`, so it cannot be shared across tokio tasks
via `Arc<Mutex<_>>`.  Per-operation connections avoid this entirely; the overhead
is negligible for this access pattern (one query per status refresh, local file).

Schema:

```sql
CREATE TABLE IF NOT EXISTS git_status (
    id         TEXT PRIMARY KEY,   -- base64(abs_path)
    status     BLOB NOT NULL,      -- serde_json-encoded GitStatus
    updated_at INTEGER NOT NULL    -- Unix epoch seconds
);
```

TTL check: `now_secs - updated_at < 2`.

## Testing

164 unit tests.  All pure functions are extracted from async render functions so
they can run without spawning any subprocesses or reading hardware.

Key test patterns:
- `segments/git.rs` — ports all 17 Go status_line test cases from
  `yrl/pkg/git/statusline_test.go`, plus parsing, Area, and helper tests.
- `db/git_cache.rs` — uses in-memory rusqlite connections for isolation.
- `segments/network.rs` — tests `parse_netstat_output` and `format_bandwidth`
  with fixture strings.
- `tmux/format.rs` — verifies every `Segment` method and both output modes of
  `colored_segment`, including the ARROW_RIGHT special case.
- `segments/window.rs` — tests `abbreviate_path` for root, home, deep nesting,
  dotfiles, truncation, and all dir-logo substitutions; tests `render` for icon
  selection, color, flags, and process animation.
