#!/bin/sh
# Builds rigbrewd for release and puts it where Tauri looks for the sidecar
# (src-tauri/binaries/rigbrewd-<target triple>). Run by `pnpm release` before the app is built.
set -eu
cd "$(dirname "$0")/../.."
triple=$(rustc -vV | sed -n 's/^host: //p')
cargo build --release --locked -p rigbrewd
mkdir -p app/src-tauri/binaries
cp "target/release/rigbrewd" "app/src-tauri/binaries/rigbrewd-$triple"
echo "rigbrewd -> app/src-tauri/binaries/rigbrewd-$triple"
