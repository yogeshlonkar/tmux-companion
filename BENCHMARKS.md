# Benchmarks

## Setup

Hardware: Apple Silicon MacBook (macOS 25.5.0 Darwin)  
Method: wall-clock measured with `gdate +%s%N` (nanosecond resolution)  
Runs per segment: 10  
Server: already running, cache warm for `gst` cached rows  
`PANE_PID`: the benchmarking shell itself (realistic — a shell with no suspended children)

Run the benchmark yourself:

```sh
cargo build --release
bash bench.sh                          # uses ./target/release/tmux-companion
bash bench.sh /path/to/binary /repo    # custom binary and repo
BENCH_RUNS=20 bash bench.sh            # more samples
```

---

## New setup results (tmux-companion)

### Individual segment latency

| Segment | min | avg | max | Notes |
|---|---|---|---|---|
| `gst` (cached) | 21ms | 24ms | 28ms | SQLite TTL hit — no git subprocess |
| `gst --force` (cache miss) | 34ms | 36ms | 39ms | Full `git status --porcelain=v2` run |
| `battery` | 34ms | 38ms | 48ms | `ioreg -arc AppleSmartBattery` |
| `net` | 14ms | 15ms | 16ms | `netstat -ibn` parse |
| `clients` | 15ms | 15ms | 17ms | `tmux list-clients` |
| `vim-bg` | 35ms | 40ms | 51ms | `pgrep -P` + `ps` per child |
| `window` | 10ms | 10ms | 12ms | Pure in-process path abbreviation |

### Full refresh (all 6 segments in parallel)

```
run 1:  47ms    run 6:  44ms
run 2:  52ms    run 7:  46ms
run 3:  47ms    run 8:  50ms
run 4:  49ms    run 9:  48ms
run 5:  47ms    run 10: 64ms
─────────────────────────────
min=44ms  avg=49ms  max=64ms
```

---

## Comparison with old setup

The old setup ran `yrl gst` (Go binary) and five `.zsh` scripts per status refresh. Each was invoked by tmux in parallel.

| Segment | Old (avg) | New (avg) | Change |
|---|---|---|---|
| `gst` | ~55ms (yrl Go binary) | **24ms** cached / 36ms forced | **2.3× faster** (cached) |
| `battery` | ~90ms (zsh + ioreg) | **38ms** | **2.4× faster** |
| `net` | ~32ms stable, **~1100ms** on stall | **15ms**, no stalls | reliable |
| `clients` | ~18ms | 15ms | same |
| `vim-bg` | ~42ms | 40ms | same |
| `window` | ~12ms | 10ms | same |

### Full parallel refresh

| Run | Old | New |
|---|---|---|
| Typical | ~100–130ms | **49ms** |
| Worst case | **>1100ms** (netstat stall) | 64ms |

**The `net-monitor.zsh` netstat stall was the largest issue in practice** — roughly 1 in 3 runs blocked for over a second, freezing the entire status bar until tmux's `status-interval` ticked again.

### Why it's faster

| Factor | Impact |
|---|---|
| No zsh interpreter startup per segment | Saves ~15ms per shell script invocation |
| SQLite cache for git (2s TTL) | `gst` skips `git status` subprocess on typical refreshes — most calls are cache hits |
| No netstat timeout | `network.rs` uses async `tokio::process::Command`; no shell `wait`-based timeout needed |
| Thin client binary | Each client call is just Unix socket connect + JSON write/read, not a full language runtime |

The Go binary (`yrl gst`) already had a SQLite cache of its own, which is why it was 55ms rather than ~150ms. The remaining gap over tmux-companion is mainly Go runtime startup (~30ms) vs Rust + tokio Unix socket (~5ms).
