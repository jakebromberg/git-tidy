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

# Generate and install zsh completions
COMP_DIR="/usr/local/share/zsh/site-functions"
if [ -d "$COMP_DIR" ] || mkdir -p "$COMP_DIR" 2>/dev/null; then
  echo "Installing zsh completions to $COMP_DIR..."

  # Sub-tool completions
  for crate in crates/*/; do
    [ -f "$crate/src/main.rs" ] || continue
    name=$(basename "$crate")
    echo "  Generating _${name}..."
    "$name" completions zsh > "$COMP_DIR/_${name}"
  done

  # Dispatcher completion (custom zsh function)
  echo "  Generating _git-tidy..."
  git-tidy completions zsh > "$COMP_DIR/_git-tidy"

  echo "Zsh completions installed. Run 'rm -f ~/.zcompdump*; exec zsh' to activate."
else
  echo "Skipping zsh completions: $COMP_DIR is not writable."
  echo "To install manually, run:"
  echo "  git-tidy completions zsh > /path/to/your/completions/_git-tidy"
fi
