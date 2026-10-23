#!/usr/bin/env bash
# What a sprite costs when it is made by `sbgen sprite --closest` and when it is built from nothing:
# the length of the directives, whether the engine draws the sprite, and the instructions of applying them.
# Usage: ./closest.sh <sprite.png> [assets for sbgen...]   (sbgen is ../target/release/sbgen, the engine is built as target-imageperf)
set -euo pipefail
here=$(dirname "$(realpath "$0")")
sprite=$1; shift
bin=${CLOSEST_BIN:-$here/target-imageperf/release/examples/closest}
sbgen=${SBGEN:-$here/../target/release/sbgen}
cpu=${CPU:-2}

instructions() { # image -> instructions of one apply
    local a b
    a=$(taskset -c "$cpu" perf stat -x, -e instructions:u "$bin" "$sprite" "$1" 200 2>&1 >/dev/null | awk -F, '/cpu_core\/instructions/ {print $1}')
    b=$(taskset -c "$cpu" perf stat -x, -e instructions:u "$bin" "$sprite" "$1" 600 2>&1 >/dev/null | awk -F, '/cpu_core\/instructions/ {print $1}')
    echo $(((b - a) / 400))
}

closest=$("$sbgen" sprite "$sprite" --closest "$@" -o - 2>/dev/null)
nothing="/assetmissing.png$("$sbgen" sprite "$sprite" -o - 2>/dev/null || true)"
for what in closest nothing; do
    image=${!what}
    [ "$what" = nothing ] && [ "$image" = /assetmissing.png ] && { printf '%-8s (can not be built from nothing)\n' $what; continue; }
    result=$("$bin" "$sprite" "$image" 2>&1 | tail -1 || true)
    printf '%-8s %7d chars  %10d instructions  %s\n' "$what" "${#image}" "$(instructions "$image")" "$result"
done
