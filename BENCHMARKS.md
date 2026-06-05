# Benchmarks

## Setup

Hardware: Apple Intel MacBook (macOS 25.5.0 Darwin)  
Method: wall-clock measured with `gdate +%s%N` (nanosecond resolution)  
Runs per segment: 10  
Server: already running, caches warm (gst SQLite + battery IOKit pre-warm at startup)  
`PANE_PID`: the benchmarking shell itself (realistic — a shell with no suspended children)

Run the benchmark yourself:

```sh
cargo build --release
bash bench.sh                          # uses ./target/release/tmux-companion
bash bench.sh /path/to/binary /repo    # custom binary and repo
BENCH_RUNS=20 bash bench.sh            # more samples
```

---

## Current results (native-library build)

Subprocesses replaced with in-process Rust libraries: `netstat` → `sysinfo`, `pgrep`+`ps` → `sysinfo`, `ioreg`+`plist` → `battery` crate. Battery result cached 30s server-side.

### Individual segment latency

| Segment | min | avg | max | Notes |
|---|---|---|---|---|
| `gst` (cached) | 29ms | 30ms | 35ms | SQLite TTL hit — no git subprocess |
| `gst --force` (cache miss) | 47ms | 48ms | 51ms | Full `git status --porcelain=v2` run |
| `battery` (cached ≤30s) | 38ms | 43ms | 44ms | IOKit pre-warmed at server start; 30s TTL |
| `net` | 21ms | 22ms | 23ms | `sysinfo::Networks` — no subprocess, no stalls |
| `clients` | 23ms | 26ms | 32ms | `tmux list-clients` (external, kept) |
| `vim-bg` | 47ms | 48ms | 49ms | `sysinfo::System` process tree scan |
| `window` | 17ms | 18ms | 22ms | Pure in-process path abbreviation |

### Full refresh (all 6 segments in parallel)

```
run 1:  56ms    run 6:  53ms
run 2:  51ms    run 7:  53ms
run 3:  57ms    run 8:  51ms
run 4:  55ms    run 9:  50ms
run 5:  53ms    run 10: 51ms
─────────────────────────────
min=50ms  avg=53ms  max=57ms
```

---

## Comparison with old setup

The old setup ran `yrl gst` (Go binary) and five `.zsh` scripts per status refresh. Each was invoked by tmux in parallel.

| Segment | Old (shell) | v1 (subprocess) | v2 (native libs) | Change v1→v2 |
|---|---|---|---|---|
| `gst` (cached) | ~55ms (yrl) | 24ms | 30ms | +6ms (binary grew slightly) |
| `gst --force` | ~55ms (yrl) | 36ms | 48ms | — |
| `battery` | ~90ms (zsh+ioreg) | 38ms | 43ms cached | IOKit only called once per 30s |
| `net` | ~32ms / **1100ms stall** | 15ms | **22ms**, no stalls | stall bug eliminated |
| `clients` | ~18ms | 15ms | 26ms | same external call |
| `vim-bg` | ~42ms | 40ms | 48ms | — |
| `window` | ~12ms | 10ms | 18ms | — |

### Full parallel refresh

| Scenario | Old shell | v1 subprocess | v2 native libs |
|---|---|---|---|
| Typical | ~100–130ms | 49ms | **53ms** |
| Worst case | **>1100ms** (netstat stall) | 64ms | **57ms** |

The v2 typical refresh is ~3ms slower than v1 on average (sysinfo adds ~7ms vs bare netstat on the non-stall path). The trade-off is the complete elimination of the netstat stall: old worst-case >1100ms is now capped at 57ms.

### Server-side load improvement (battery caching)

Before: every tmux refresh called `ioreg` via subprocess (~25ms CPU server-side).  
After: IOKit called once at server startup, then once per 30s.  
Over 30 refreshes: 30×25ms = 750ms CPU → 1×25ms + 29×~1ms = 54ms CPU (**14× less**).

### Why native libraries help

| Change | Benefit |
|---|---|
| `sysinfo::Networks` replaces `netstat` | Eliminates the ~1-in-3 netstat stall (was >1100ms) |
| `sysinfo::System` replaces `pgrep`+`ps` | Eliminates 2 sequential subprocesses per vim-bg call |
| `battery` crate replaces `ioreg`+`plist` | Combined with 30s cache: ~14× less server CPU for battery |
| 5s timeouts on all remaining subprocesses | `git` or `tmux` hang can no longer block the server indefinitely |
| Battery pre-warm at server startup | No 600ms IOKit cold-start on first tmux refresh after server launch |

---

## Historical baseline (v1 vs old shell scripts)

The numbers below are from the initial subprocess-based implementation, for reference.

### v1 individual segment latency

| Segment | min | avg | max |
|---|---|---|---|
| `gst` (cached) | 21ms | 24ms | 28ms |
| `gst --force` | 34ms | 36ms | 39ms |
| `battery` | 34ms | 38ms | 48ms |
| `net` | 14ms | 15ms | 16ms |
| `clients` | 15ms | 15ms | 17ms |
| `vim-bg` | 35ms | 40ms | 51ms |
| `window` | 10ms | 10ms | 12ms |

### v1 full refresh

```
min=44ms  avg=49ms  max=64ms
```
