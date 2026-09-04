#!/usr/bin/env bash
#
# Installs every git-tidy binary plus zsh completions, either by compiling the
# workspace with cargo (default) or by downloading prebuilt binaries from a
# GitHub release (--prebuilt).
set -euo pipefail

# Falls back to $0 when the script is piped into bash (curl ... | bash -s --),
# where BASH_SOURCE is unset. Only source installs use ROOT.
ROOT="$(cd "$(dirname "${BASH_SOURCE[0]:-$0}")" 2>/dev/null && pwd || pwd)"

REPO="${GIT_TIDY_REPO:-jakebromberg/git-tidy}"
DOWNLOAD_BASE_URL="${GIT_TIDY_BASE_URL:-https://github.com/${REPO}/releases/download}"
LATEST_API_URL="${GIT_TIDY_API_URL:-https://api.github.com/repos/${REPO}/releases/latest}"
COMPLETIONS_DIR="${GIT_TIDY_COMPLETIONS_DIR:-/usr/local/share/zsh/site-functions}"

mode="source"
version=""
prefix="${GIT_TIDY_PREFIX:-$HOME/.local/bin}"
scratch=""

cleanup() {
  if [ -n "$scratch" ]; then
    rm -rf "$scratch"
  fi
}
trap cleanup EXIT

usage() {
  cat <<'EOF'
Usage: install.sh [options]

Installs every git-tidy binary plus zsh completions. With no options the
workspace is compiled and installed with cargo.

Options:
  --prebuilt         Download prebuilt binaries from a GitHub release instead
                     of compiling. No Rust toolchain required.
  --version <tag>    Release tag to download, e.g. v0.1.0 (default: latest).
                     Implies --prebuilt.
  --prefix <dir>     Directory to install prebuilt binaries into
                     (default: ~/.local/bin). Implies --prebuilt.
  -h, --help         Show this help.

Environment:
  GIT_TIDY_TARGET            Override the detected target triple.
  GIT_TIDY_PREFIX            Default prefix for --prebuilt installs.
  GIT_TIDY_COMPLETIONS_DIR   Where zsh completions are written
                             (default: /usr/local/share/zsh/site-functions).
  GIT_TIDY_REPO              owner/name to download releases from.
EOF
}

die() {
  echo "error: $*" >&2
  exit 1
}

need() {
  command -v "$1" >/dev/null 2>&1 || die "$1 is required for --prebuilt but was not found"
}

# Map the running machine onto one of the target triples the release workflow
# publishes. Anything else has to build from source.
detect_target() {
  if [ -n "${GIT_TIDY_TARGET:-}" ]; then
    printf '%s\n' "$GIT_TIDY_TARGET"
    return
  fi
  local os arch
  os="$(uname -s)"
  arch="$(uname -m)"
  case "$os $arch" in
    "Darwin arm64")             echo "aarch64-apple-darwin" ;;
    "Darwin x86_64")            echo "x86_64-apple-darwin" ;;
    "Linux x86_64")             echo "x86_64-unknown-linux-musl" ;;
    "Linux aarch64"|"Linux arm64") echo "aarch64-unknown-linux-musl" ;;
    *) die "no prebuilt binaries for $os/$arch -- run install.sh without --prebuilt to build from source" ;;
  esac
}

# Read tag_name out of the GitHub "latest release" JSON without requiring jq.
resolve_latest_version() {
  local json tag
  json="$(curl -fsSL "$LATEST_API_URL")" || die "could not query $LATEST_API_URL"
  tag="$(printf '%s\n' "$json" | tr ',' '\n' | grep -m1 '"tag_name"' |
         sed -E 's/.*"tag_name"[[:space:]]*:[[:space:]]*"([^"]+)".*/\1/')" || true
  [ -n "$tag" ] || die "could not determine the latest release tag from $LATEST_API_URL"
  printf '%s\n' "$tag"
}

# Verify one `sha256sum`-format line, read from stdin, against the cwd.
check_sha256_line() {
  if command -v sha256sum >/dev/null 2>&1; then
    sha256sum -c --status -
  elif command -v shasum >/dev/null 2>&1; then
    shasum -a 256 -c --status -
  else
    die "neither sha256sum nor shasum is available to verify the download"
  fi
}

install_completions() {
  local bindir="$1"
  shift
  if ! { [ -d "$COMPLETIONS_DIR" ] || mkdir -p "$COMPLETIONS_DIR" 2>/dev/null; } ||
     [ ! -w "$COMPLETIONS_DIR" ]; then
    echo "Skipping zsh completions: $COMPLETIONS_DIR is not writable."
    echo "To install manually, run:"
    echo "  git-tidy completions zsh > /path/to/your/completions/_git-tidy"
    return 0
  fi

  echo "Installing zsh completions to $COMPLETIONS_DIR..."
  local name bin
  for name in "$@"; do
    bin="$bindir/$name"
    [ -x "$bin" ] || bin="$name" # fall back to PATH lookup
    echo "  Generating _${name}..."
    # Completions are a convenience: never fail the install over them.
    if ! "$bin" completions zsh > "$COMPLETIONS_DIR/_${name}" 2>/dev/null; then
      rm -f "$COMPLETIONS_DIR/_${name}"
      echo "    Skipped: could not run $bin"
    fi
  done
  echo "Zsh completions installed. Run 'rm -f ~/.zcompdump*; exec zsh' to activate."
}

install_from_source() {
  local crate name names=""

  # Auto-discover binary crates (those with src/main.rs), install git-tidy last
  # so the dispatcher never shadows a half-installed sub-tool.
  for crate in "$ROOT"/crates/*/; do
    [ -f "$crate/src/main.rs" ] || continue
    [ "$crate" = "$ROOT/crates/git-tidy/" ] && continue
    name="$(basename "$crate")"
    echo "Installing $name..."
    cargo install --path "$crate"
    names="$names $name"
  done

  if [ -d "$ROOT/crates/git-tidy" ]; then
    echo "Installing git-tidy..."
    cargo install --path "$ROOT/crates/git-tidy"
    names="$names git-tidy"
  fi

  echo "All tools installed."
  # shellcheck disable=SC2086 # names is a deliberate space-separated list
  install_completions "${CARGO_HOME:-$HOME/.cargo}/bin" $names
}

install_prebuilt() {
  need curl
  need tar

  local target archive url tmp line file name names=""
  target="$(detect_target)"
  if [ -z "$version" ]; then
    echo "Resolving the latest release..."
    version="$(resolve_latest_version)"
  fi
  archive="git-tidy-${version}-${target}.tar.gz"
  url="${DOWNLOAD_BASE_URL}/${version}/${archive}"

  scratch="$(mktemp -d)" # removed by the EXIT trap
  tmp="$scratch"

  echo "Downloading $archive ..."
  curl -fsSL "$url" -o "$tmp/$archive" || die "download failed: $url"
  curl -fsSL "${DOWNLOAD_BASE_URL}/${version}/SHA256SUMS" -o "$tmp/SHA256SUMS" ||
    die "could not download SHA256SUMS for $version"

  echo "Verifying checksum..."
  # The sums file names archives either bare or as ./<archive>; both resolve
  # against $tmp, so the matched line is checked as-is.
  line="$(grep -E "[[:space:]]\.?/?${archive}\$" "$tmp/SHA256SUMS")" ||
    die "checksum for $archive is missing from SHA256SUMS"
  printf '%s\n' "$line" | (cd "$tmp" && check_sha256_line) ||
    die "checksum mismatch for $archive -- refusing to install"

  mkdir -p "$tmp/unpacked"
  tar -xzf "$tmp/$archive" -C "$tmp/unpacked" --strip-components=1

  mkdir -p "$prefix"
  for file in "$tmp"/unpacked/git-*; do
    [ -f "$file" ] || continue
    name="$(basename "$file")"
    echo "Installing $name -> $prefix/$name"
    install -m 0755 "$file" "$prefix/$name"
    names="$names $name"
  done
  [ -n "$names" ] || die "$archive contained no binaries"

  echo "All tools installed to $prefix."
  case ":$PATH:" in
    *":$prefix:"*) ;;
    *) echo "Note: $prefix is not on your PATH. Add it with:"
       echo "  export PATH=\"$prefix:\$PATH\"" ;;
  esac

  # shellcheck disable=SC2086 # names is a deliberate space-separated list
  install_completions "$prefix" $names
}

main() {
  while [ $# -gt 0 ]; do
    case "$1" in
      --prebuilt) mode="prebuilt" ;;
      --version) [ $# -ge 2 ] || die "--version requires a tag"; version="$2"; mode="prebuilt"; shift ;;
      --version=*) version="${1#*=}"; mode="prebuilt" ;;
      --prefix) [ $# -ge 2 ] || die "--prefix requires a directory"; prefix="$2"; mode="prebuilt"; shift ;;
      --prefix=*) prefix="${1#*=}"; mode="prebuilt" ;;
      -h|--help) usage; exit 0 ;;
      *) usage >&2; die "unknown option: $1" ;;
    esac
    shift
  done

  if [ "$mode" = "prebuilt" ]; then
    install_prebuilt
  else
    install_from_source
  fi
}

main "$@"
