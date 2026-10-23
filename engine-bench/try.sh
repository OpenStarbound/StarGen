#!/usr/bin/env bash

# Builds the engine variant `opt` from the worktree, checks that it still draws every pixel as the original engine did (golden output) and counts what applying the directives costs.
# Usage: ./try.sh <core files that changed>...      the golden output of the original engine is in $GOLDEN (golden > file)

set -euo pipefail
here=$(dirname "$(realpath "$0")")
cd "$here"
checkout=${CHECKOUT:-$HOME/code/game/OpenStarbound-imageperf}
nice -n 19 ./overlay.sh "$checkout" opt "$@" | tail -1
ENGINE_DIR=$here/engine/opt CARGO_TARGET_DIR=$here/target-opt nice -n 19 cargo build --release --example golden --example work 2>&1 | grep -E '^error' -A8 || true
nice -n 19 ./target-opt/release/examples/golden > /tmp/golden-opt.txt 2>&1
if cmp -s "$GOLDEN" /tmp/golden-opt.txt; then echo "golden: identical ($(wc -l < /tmp/golden-opt.txt) lines)"; else
    echo "golden: DIFFERENT"; diff "$GOLDEN" /tmp/golden-opt.txt | head -8; exit 1; fi
./count.sh ./target-opt/release/examples/work | grep -E 'male_body +(apply|sheet)|^case'
