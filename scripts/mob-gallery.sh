#!/bin/sh
# Every mob on a flat stage in the sky (`--mob-gallery`): builds the game if
# needed and opens a fresh creative world at noon (not saved) with all of
# them standing still in rows, the sea creatures in a water channel at the
# front. Fly around to look at them; extra options pass through, e.g.
#   scripts/mob-gallery.sh --time 18000
set -e
cd "$(dirname "$0")/.."
cargo build --release
exec target/release/minecraft --mob-gallery "$@"
