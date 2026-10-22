#!/bin/sh
# Builds optimised binaries into dist/.
set -e
cargo build --release -p forge-studio -p forge-cli
mkdir -p dist
cp target/release/forge-studio target/release/forge-cli dist/
cp packaging/forge-studio.desktop dist/
cp assets/app-icon/logo.png dist/forge-studio.png
echo "done: dist/"
