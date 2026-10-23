#!/usr/bin/env bash
# Builds an engine variant:
# the engine of engine/, with some of its core files compiled again from another checkout (a worktree that is being worked on), with the flags that the engine's own build compiled each one with.
# Only the files given are compiled, so this takes seconds.
# Headers may change as long as the layout of what other files use stays the same.
# Usage: ./overlay.sh <OpenStarbound checkout> <name> <core file>...
# e.g. ./overlay.sh ~/code/game/OpenStarbound-imageperf opt StarImageProcessing.cpp
# Then build with: ENGINE_DIR=engine/<name> CARGO_TARGET_DIR=target-<name> cargo build --release
set -euo pipefail

here=$(dirname "$(realpath "$0")")
checkout=$(realpath "$1"); name=$2; shift 2
out=$here/engine/$name
rm -rf "$out"; mkdir -p "$out/obj"

built_from=$(sed -n 's/^engine: \(.*\) (.*)$/\1/p' "$here/engine/engine.txt")
cp "$here/engine/cxxflags.txt" "$out/cxxflags.txt"
cp "$here/engine/link.txt" "$out/link.txt"

# the compiler needs the libraries of the engine's dev shell (png.h), so everything is compiled in one go inside it
nixdev=${NIXDEV:-$built_from/../OpenStarbound-nixdev}
commands=$out/compile.sh
echo "set -e" > "$commands"
for file in "$@"; do
    object=$out/obj/${file%.cpp}.o
    command=$(jq -r --arg file "$built_from/source/core/$file" '.[] | select(.file == $file) | .command' "$built_from/build/compile_commands.json")
    [ -n "$command" ] || { echo "$file is not in the engine's build" >&2; exit 1; }
    # the same command, from the checkout, into our object
    command=$(sed -E "s# -o [^ ]*# -o $object#; s# -c [^ ]*# -c $checkout/source/core/$file#; s#$built_from/source#$checkout/source#g" <<< "$command")
    echo "$command" >> "$commands"
    sed -i -E "s#[^ ]*/star_core\.dir/$file\.o\$#$object#" "$out/link.txt"
    grep -q "$object" "$out/link.txt" || { echo "$file is not in the link line" >&2; exit 1; }
done
nix develop "$nixdev" --command bash "$commands" 2>&1 | grep -v "Git tree is dirty" || true
[ -f "$out/obj/$(basename "${1%.cpp}").o" ] || { echo "compilation failed" >&2; exit 1; }
echo "engine/$name: $* from $checkout"
