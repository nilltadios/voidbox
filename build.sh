#!/bin/bash
# Build voidbox and create app launcher binaries
cargo build --release

# Create app launcher copies (users can distribute these)
cp target/release/voidbox target/release/void_brave
cp target/release/voidbox target/release/void_discord
cp target/release/voidbox target/release/void_vscode

echo "Built:"
ls -la target/release/void* target/release/voidbox
