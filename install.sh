#!/usr/bin/env bash
set -euo pipefail

for crate in crates/git-worktree-tidy crates/git-branch-tidy crates/git-stash-tidy \
             crates/git-remote-tidy crates/git-tag-tidy crates/git-tidy; do
  if [ -d "$crate" ]; then
    name=$(basename "$crate")
    echo "Installing $name..."
    cargo install --path "$crate"
  fi
done

echo "All tools installed."
