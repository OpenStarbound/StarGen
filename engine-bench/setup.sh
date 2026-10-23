#!/usr/bin/env bash
# Collects the compile flags and the link line of an OpenStarbound build, so the shim can be built against the real engine.
# Usage: ./setup.sh [path to the OpenStarbound checkout]   (its build/ must exist, the sibling ./OpenStarbound-nixdev provides ninja, as there).
set -euo pipefail

sb=$(realpath "${1:-$HOME/code/game/OpenStarbound}")
nixdev=${NIXDEV:-$sb/../OpenStarbound-nixdev}
out=$(dirname "$(realpath "$0")")/engine
mkdir -p "$out"

# the engine's flags. Those of a utility that uses the same libraries (asset_unpacker) minus the compiler, the output and the input
grep -o '"command": "[^"]*asset_unpacker.cpp[^"]*"' "$sb/build/compile_commands.json" | head -1 \
    | sed -E 's/^"command": "//; s/"$//; s# -o [^ ]*##; s# -c [^ ]*##' \
    | tr ' ' '\n' | tail -n +2 \
    | grep -v -E '^(-rdynamic|-no-pie|-pthread|-W.*)$' > "$out/cxxflags.txt"

# the link line of the same utility with the objects made absolute and its own main and output dropped
nix develop "$nixdev" --command ninja -C "$sb/build" -t commands "$sb/dist/asset_unpacker" 2>/dev/null | tail -1 \
    | tr ' ' '\n' | grep -v -E '^(:|&&|)$' | tail -n +2 \
    | grep -v -E '^-(std|rdynamic|W|no-pie|pthread|D_|DNDEBUG|O3|ffast|o$)|asset_unpacker|link\.d' \
    | sed -E "s#^(extern|core|base|game|application|rendering|platform|frontend|client|utility)/#$sb/build/\1/#" > "$out/link.txt"

echo "engine: $sb ($(git -C "$sb" rev-parse --short HEAD))" | tee "$out/engine.txt"
echo "$(grep -c '\.o$' "$out/link.txt") objects, $(wc -l < "$out/cxxflags.txt") flags"
