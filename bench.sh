#!/usr/bin/env bash
# Benchmark script for tmux-companion.
# Measures round-trip latency for each segment and a full parallel refresh.
# Requires gdate (brew install coreutils) for nanosecond timing on macOS.

set -euo pipefail

BINARY="${1:-./target/release/tmux-companion}"
REPO="${2:-$(git rev-parse --show-toplevel 2>/dev/null || pwd)}"
RUNS="${BENCH_RUNS:-10}"
PANE_PID=$$

# ── Prerequisites ────────────────────────────────────────────────────────────

if ! command -v gdate &>/dev/null; then
  echo "ERROR: gdate not found. Install with: brew install coreutils" >&2
  exit 1
fi

if [[ ! -x "$BINARY" ]]; then
  echo "ERROR: binary not found or not executable: $BINARY" >&2
  echo "Build first with: cargo build --release" >&2
  exit 1
fi

# ── Helpers ──────────────────────────────────────────────────────────────────

now_ms() { echo $(( $(gdate +%s%N) / 1000000 )); }

# Run a command $RUNS times; print min/avg/max in ms.
# Usage: bench_cmd LABEL cmd args...
bench_cmd() {
  local label="$1"; shift
  local total=0 min=99999 max=0
  local times=()

  for _ in $(seq 1 "$RUNS"); do
    local s e d
    s=$(gdate +%s%N)
    "$@" > /dev/null 2>&1
    e=$(gdate +%s%N)
    d=$(( (e - s) / 1000000 ))
    times+=($d)
    total=$(( total + d ))
    (( d < min )) && min=$d
    (( d > max )) && max=$d
  done

  local avg=$(( total / RUNS ))
  printf "  %-30s  min=%3dms  avg=%3dms  max=%3dms\n" "$label" "$min" "$avg" "$max"
}

# Run all segments in parallel once; return wall-clock ms.
bench_parallel() {
  local s e
  s=$(gdate +%s%N)
  "$BINARY" gst "$REPO"            > /dev/null &
  "$BINARY" battery                > /dev/null &
  "$BINARY" net                    > /dev/null &
  "$BINARY" clients 1 1            > /dev/null &
  "$BINARY" vim-bg "$PANE_PID"     > /dev/null &
  "$BINARY" window -c -i 1 -n bench -w /tmp > /dev/null &
  wait
  e=$(gdate +%s%N)
  echo $(( (e - s) / 1000000 ))
}

# ── Ensure server is running ─────────────────────────────────────────────────

echo "Starting server..."
pkill -f "tmux-companion server" 2>/dev/null || true
sleep 0.1
"$BINARY" gst "$REPO" > /dev/null   # auto-starts server + warms gst cache
"$BINARY" battery > /dev/null       # warms battery cache (first IOKit call is slow)
sleep 0.1
echo "Server ready."
echo ""

# ── Individual segment benchmarks ────────────────────────────────────────────

echo "Individual segments ($RUNS runs each):"
bench_cmd "gst (cached)"        "$BINARY" gst "$REPO"
bench_cmd "gst --force (miss)"  "$BINARY" gst -f "$REPO"
bench_cmd "battery"             "$BINARY" battery
bench_cmd "net"                 "$BINARY" net
bench_cmd "clients"             "$BINARY" clients 1 1
bench_cmd "vim-bg"              "$BINARY" vim-bg "$PANE_PID"
bench_cmd "window"              "$BINARY" window -c -i 1 -n bench -w /tmp

# ── Parallel refresh simulation ───────────────────────────────────────────────

echo ""
echo "Full refresh — all segments in parallel (${RUNS} runs):"
total=0 min=99999 max=0
for _ in $(seq 1 "$RUNS"); do
  d=$(bench_parallel)
  total=$(( total + d ))
  (( d < min )) && min=$d
  (( d > max )) && max=$d
  printf "  %dms\n" "$d"
done
avg=$(( total / RUNS ))
echo "  ─────────────────────────────────────────"
printf "  min=%dms  avg=%dms  max=%dms\n" "$min" "$avg" "$max"
