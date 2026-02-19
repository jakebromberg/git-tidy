#!/usr/bin/env bash
set -euo pipefail

# Auto-discover binary crates (those with src/main.rs), install git-tidy last
for crate in crates/*/; do
  [ -f "$crate/src/main.rs" ] || continue
  [[ "$crate" == crates/git-tidy/ ]] && continue
  name=$(basename "$crate")
  echo "Installing $name..."
  cargo install --path "$crate"
done

# Install the umbrella binary last
if [ -d crates/git-tidy ]; then
  echo "Installing git-tidy..."
  cargo install --path crates/git-tidy
fi

echo "All tools installed."
