#!/usr/bin/env bash
set -euo pipefail

for crate in crates/git-*-tidy; do
  name=$(basename "$crate")
  echo "Installing $name..."
  cargo install --path "$crate"
done

echo "All tools installed."
