#!/bin/sh
# Builds the Rust DSP core (dsp/) as a universal static library at build/rust/libsampling_reverb_dsp.a.
# Run by `make` and by the extension target's pre-build phase in Xcode.
set -eu

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
export PATH="$HOME/.cargo/bin:$PATH"
if ! command -v cargo >/dev/null 2>&1; then
    echo "error: cargo not found. Install Rust from https://rustup.rs, then:" >&2
    echo "       rustup target add aarch64-apple-darwin x86_64-apple-darwin" >&2
    exit 1
fi

OUT="$ROOT/build/rust"
LIB=libsampling_reverb_dsp.a
mkdir -p "$OUT"
# Xcode's environment sets SDKROOT and deployment-target variables for its own compilers; match
# the app's macOS 13 deployment target and let cargo pick the host linker.
export MACOSX_DEPLOYMENT_TARGET=13.0
unset SDKROOT LIBRARY_PATH 2>/dev/null || true
for target in aarch64-apple-darwin x86_64-apple-darwin; do
    cargo build --quiet --release --manifest-path "$ROOT/dsp/Cargo.toml" --target "$target"
done
lipo -create \
    "$ROOT/dsp/target/aarch64-apple-darwin/release/$LIB" \
    "$ROOT/dsp/target/x86_64-apple-darwin/release/$LIB" \
    -output "$OUT/$LIB.tmp"
# Only replace the library when it changed, so Xcode doesn't relink for nothing.
if cmp -s "$OUT/$LIB.tmp" "$OUT/$LIB"; then rm "$OUT/$LIB.tmp"; else mv "$OUT/$LIB.tmp" "$OUT/$LIB"; fi
