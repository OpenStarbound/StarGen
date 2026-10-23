#!/usr/bin/env bash
# What each operation of a case's directives costs, in instructions, applying one frame (the first one that is reproduced exactly).
# Usage: ./ops.sh <work binary> <case> [cpu]
set -euo pipefail
work=$1
case=$2
cpu=${3:-2}
ops=$("$(dirname "$0")/target/release/examples/dump" "$case" 2>/dev/null || true)

instr() { taskset -c "$cpu" perf stat -x, -e instructions:u "$work" "$case" "upto$1" "$2" 2>&1 >/dev/null | awk -F, '/cpu_core\/instructions/ {print $1}'; }

prev=0
n=0
while IFS= read -r op; do
    a=$(instr "$n" 20); b=$(instr "$n" 60)
    now=$(((b - a) / 40))
    if [ "$n" -gt 0 ]; then printf '%2d %12d  %.70s\n' "$n" $((now - prev)) "$op"; fi
    prev=$now
    n=$((n + 1))
done < <(echo "(start)"; echo "$ops")
