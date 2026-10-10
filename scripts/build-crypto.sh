#!/bin/sh
set -eu
cd "$(dirname "$0")/.."
version=0.2.129
case "$(uname -m)" in x86_64) ;; *) echo 'WASM build helper currently requires Linux x86_64' >&2; exit 1;; esac
mkdir -p .cache/wasm-bindgen
archive=.cache/wasm-bindgen/cli.tar.gz
curl --fail --location --proto '=https' --tlsv1.2 "https://github.com/wasm-bindgen/wasm-bindgen/releases/download/$version/wasm-bindgen-$version-x86_64-unknown-linux-musl.tar.gz" -o "$archive"
echo "82d12bb940e2d4e72e0d5605387fc1b8ca179044e012b620f0ce4e7440e8320e  $archive" | sha256sum --check
tar -xzf "$archive" -C .cache/wasm-bindgen
rustup target add wasm32-unknown-unknown
cargo build --package webts-crypto --target wasm32-unknown-unknown --release --locked
".cache/wasm-bindgen/wasm-bindgen-$version-x86_64-unknown-linux-musl/wasm-bindgen" "${CARGO_TARGET_DIR:-target}/wasm32-unknown-unknown/release/webts_crypto.wasm" --target web --out-dir web/public/crypto
