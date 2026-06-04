# CLAUDE.md

## Build & test

```sh
cargo build --release          # binary → target/release/tmux-companion
cargo test                     # 164 unit tests, no external deps
```

No lint step is configured yet; `cargo clippy` is fine to run but not required.

## Running manually

Kill any stale daemon before testing so you pick up the new binary:

```sh
pkill -f tmux-companion; sleep 0.1
./target/release/tmux-companion server &
sleep 0.3
./target/release/tmux-companion gst /path/to/repo
```

Or let the client auto-start the server:

```sh
./target/release/tmux-companion gst .
```

## Architecture in one paragraph

Same binary, two modes.  `server` binds a Unix socket
(`/tmp/tmux-companion-<uid>.sock`) and serves requests forever.  Every other
subcommand is a client: it connects (auto-starting the server if the socket is
absent), writes one JSON line, reads one JSON line, prints the output field, and
exits.  The server keeps `ServerState` (bandwidth previous-sample, dir-aliases
map) in an `Arc<tokio::sync::Mutex<_>>` and opens a fresh `rusqlite::Connection`
for each SQLite operation (Connection is `!Send`).

## Module responsibilities

| Path | Owns |
|------|------|
| `src/main.rs` | CLI (`Cmd` enum via clap), dispatch |
| `src/client.rs` | connect-with-retry, spawn server, send/print |
| `src/server/mod.rs` | UnixListener accept loop |
| `src/server/handlers.rs` | `req.cmd` → segment fn |
| `src/server/state.rs` | `ServerState` struct + dir-aliases loader |
| `src/segments/git.rs` | git status parse, format, cache; `render(path, force)` |
| `src/segments/battery.rs` | ioreg plist parse, `format_battery_output` |
| `src/segments/network.rs` | netstat parse, IEC format, bandwidth delta |
| `src/segments/clients.rs` | tmux list-clients, `format_client_output` |
| `src/segments/vim_bg.rs` | pgrep+ps, `is_suspended_nvim` |
| `src/segments/window.rs` | path abbreviation, index icons, `render` |
| `src/tmux/format.rs` | `Segment`, `colored_segment`, `powerline_segment`, color consts |
| `src/tmux/icons.rs` | Nerd Font codepoints |
| `src/db/mod.rs` | schema init, `db_path()` |
| `src/db/git_cache.rs` | `get_git_status` / `save_git_status` with TTL |
| `src/proto.rs` | `Request` / `Response` serde types |

## Key invariants

- **`ARROW_RIGHT`** (`src/tmux/icons.rs`) — currently `\u{e0bc}`.  The glyph
  rendered depends on the Nerd Font variant installed.  Changing this constant
  is intentional and the tests use the constant (not a hardcoded codepoint), so
  they track changes automatically.

- **`colored_segment` ARROW_RIGHT special case** — in no-tmux mode (`no_tmux=true`),
  calling `colored_segment(true, fg, bg, ARROW_RIGHT)` emits only the ANSI color
  escape codes, not the arrow glyph.  This matches the Go original's behaviour.
  See `tmux/format.rs` and the test `colored_segment_no_tmux_arrow_right_special_case`.

- **Branch truncation** — truncates when `char_count > 20` (strictly greater),
  and the tail is the last 10 chars (`TAIL_LEN + 1`).  This matches Go's
  `branch[lastIndex-tailLen:]` formula exactly.  See `short_branch` in
  `segments/git.rs` and the `short_branch_exactly_max_len_not_truncated` test.

- **`rusqlite::Connection` is `!Send`** — never put a Connection in shared state.
  Open a connection per DB call inside `spawn_blocking` or directly in a sync
  context.

- **Cache skip on `force`** — `render(path, force)` skips the cache read when
  `force=true` but still writes the fresh result back, so the next normal call
  gets a warm cache.

## Adding a new segment

1. Add a function in `src/segments/<name>.rs`.  Extract all I/O-free logic into
   a pure `fn` so it can be unit-tested.
2. Add `pub mod <name>;` to `src/segments/mod.rs`.
3. Handle the new `cmd` string in `src/server/handlers.rs`.
4. Add the subcommand variant to `Cmd` in `src/main.rs` and build the JSON args.
5. Write unit tests in the same file.

## Changing icon codepoints

Edit `src/tmux/icons.rs`.  Icon constants are used directly in tests via the
constant name, so a codepoint change automatically propagates to all tests —
no manual expected-string updates needed.

After changing a codepoint that appears in tmux output, rebuild and do a
side-by-side visual check with the previous tool:

```sh
cargo build --release
pkill -f tmux-companion
./target/release/tmux-companion gst /some/repo
yrl gst /some/repo   # or the previous shell script
```
