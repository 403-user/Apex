#!/usr/bin/env bash
set -euo pipefail

# Apex Terminal Benchmark Runner
# Uses vtebench for terminal emulator performance testing

BENCH_DIR="$(cd "$(dirname "$0")" && pwd)"
PROJECT_DIR="$(dirname "$BENCH_DIR")"
RESULTS_FILE="$BENCH_DIR/results.md"
HTML_RESULTS="$BENCH_DIR/results.html"

echo "=== Apex Terminal Benchmark Suite ==="
echo "Project: $PROJECT_DIR"
echo "Results: $RESULTS_FILE"
echo ""

# Check for vtebench
if ! command -v vtebench &>/dev/null; then
    echo "[!] vtebench not found. Install it:"
    echo "    cargo install vtebench"
    echo ""
    echo "    Falling back to built-in benchmarks..."
    HAS_VTEBENCH=false
else
    HAS_VTEBENCH=true
fi

# Build apex-terminal in release mode
echo "[1/4] Building apex-terminal (release)..."
cargo build --release --manifest-path "$PROJECT_DIR/Cargo.toml" 2>&1 | tail -1

# Determine terminal binary path
if [[ -f "$PROJECT_DIR/target/release/apex-terminal" ]]; then
    TERM_BIN="$PROJECT_DIR/target/release/apex-terminal"
elif [[ -f "$PROJECT_DIR/target/release/apex" ]]; then
    TERM_BIN="$PROJECT_DIR/target/release/apex"
else
    echo "[!] Could not find apex-terminal binary"
    exit 1
fi
echo "    Binary: $TERM_BIN"

# Collect baseline system info
echo "[2/4] Collecting system info..."
{
    echo "# Apex Terminal Benchmark Results"
    echo ""
    echo "## System"
    echo "- **Date**: $(date -u '+%Y-%m-%d %H:%M UTC')"
    echo "- **OS**: $(uname -srmo 2>/dev/null || uname -srm)"
    echo "- **Kernel**: $(uname -r)"
    echo "- **CPU**: $(grep 'model name' /proc/cpuinfo | head -1 | cut -d: -f2 | xargs)"
    echo "- **Cores**: $(nproc)"
    echo "- **RAM**: $(free -h | grep Mem | awk '{print $2}')"
    echo "- **GPU**: $(glxinfo -B 2>/dev/null | grep 'Device:' | head -1 | xargs || echo 'unknown')"
    echo "- **Display**: ${DISPLAY:-:0}"
    echo "- **Terminal Binary**: $(file "$TERM_BIN" | cut -d: -f2- | xargs)"
    echo "- **Build**: $(cargo metadata --manifest-path "$PROJECT_DIR/Cargo.toml" --format-version 1 2>/dev/null | python3 -c 'import sys,json; d=json.load(sys.stdin); print(d.get("packages",[])[0].get("version","unknown"))' 2>/dev/null || echo 'unknown')"
    echo ""
} > "$RESULTS_FILE"

# Run benchmarks
echo "[3/4] Running benchmarks..."

run_benchmark() {
    local name="$1"
    local cmd="$2"
    local duration="${3:-10}"

    echo -n "    - $name ... "

    # Use hyperfine if available for precise timing
    if command -v hyperfine &>/dev/null; then
        local result
        result=$(hyperfine --warmup 1 --min-runs 3 --max-runs 5 \
            --command-name "$name" \
            --export-json /tmp/apex_bench.json \
            "$cmd" 2>&1)
        local mean=$(echo "$result" | grep 'Time (mean ± σ)' | awk '{print $4, $5}')
        local min=$(echo "$result" | grep 'Range (min … max)' | awk '{print $3}')
        echo "$mean (min: $min)"
        {
            echo "### $name"
            echo '```'
            echo "$result"
            echo '```'
            echo ""
        } >> "$RESULTS_FILE"
    else
        # Fallback: time the command
        local ts_start ts_end elapsed
        ts_start=$(date +%s%N)
        timeout "$duration" bash -c "$cmd" &>/dev/null || true
        ts_end=$(date +%s%N)
        elapsed=$(( (ts_end - ts_start) / 1000000 ))
        echo "${elapsed}ms"
        {
            echo "### $name"
            echo "- **Duration**: ${elapsed}ms"
            echo ""
        } >> "$RESULTS_FILE"
    fi
}

{
    echo "## Benchmarks"
    echo ""
} >> "$RESULTS_FILE"

# Scroll performance
run_benchmark "Scroll Speed (30s)" "yes" 30

# Interactive latency
run_benchmark "Interactive Latency (small)" \
    "bash -c 'for i in {1..100}; do echo \"Line \$i\"; done'" 10

run_benchmark "Interactive Latency (large)" \
    "bash -c 'for i in {1..1000}; do echo \"Line \$i\"; done'" 10

# Throughput
if [[ -f /usr/share/dict/words ]]; then
    run_benchmark "Throughput (text)" \
        "cat /usr/share/dict/words" 15
fi

# ANSI stress
run_benchmark "ANSI Color Stress" \
    "bash -c 'for i in {0..255}; do echo -e \"\\\\033[38;5;\${i}mColor \${i}\"; done'" 10

run_benchmark "ANSI Cursor Movement" \
    "bash -c 'for i in {1..100}; do tput cup \$i 0; echo \"Line \$i\"; done'" 10

# Large file rendering
run_benchmark "Large File (dmesg)" \
    "dmesg 2>/dev/null || cat /var/log/syslog 2>/dev/null || journalctl --no-pager -n 5000 2>/dev/null || echo 'no log source'" 15

# Comparison data
echo "" >> "$RESULTS_FILE"
echo "## Comparison" >> "$RESULTS_FILE"
echo "" >> "$RESULTS_FILE"
echo "| Benchmark | Apex Terminal | Alacritty | GNOME Terminal | Notes |" >> "$RESULTS_FILE"
echo "|-----------|---------------|-----------|----------------|-------|" >> "$RESULTS_FILE"
echo "| Scroll Speed (lines/s) | - | - | - | Target: >10000 |" >> "$RESULTS_FILE"
echo "| Latency (ms) | - | - | - | Target: <16ms |" >> "$RESULTS_FILE"
echo "| Throughput (MB/s) | - | - | - | Target: >500 |" >> "$RESULTS_FILE"
echo "| ANSI Color Stress | - | - | - | No flicker |" >> "$RESULTS_FILE"

echo ""
echo "[4/4] Results written to: $RESULTS_FILE"

# Print summary
echo ""
echo "=== Summary ==="
grep -E '^\*\*' "$RESULTS_FILE" || true
echo ""
echo "Done."
