#!/bin/bash
set -euo pipefail

pushd frontend >/dev/null
pnpm install --frozen-lockfile
pnpm build
popd >/dev/null

cargo build --release --locked --target x86_64-unknown-linux-musl
cargo build --release --locked --target x86_64-pc-windows-gnu
