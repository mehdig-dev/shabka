#!/bin/sh
set -eu

echo "Publishing shabka-core..."
cargo publish -p shabka-core --no-default-features

echo "Waiting for crates.io to index shabka-core..."
sleep 30

echo "Publishing shabka-cli..."
cargo publish -p shabka-cli --no-default-features

echo "Publishing shabka-mcp..."
cargo publish -p shabka-mcp --no-default-features

echo "✓ All crates published."
