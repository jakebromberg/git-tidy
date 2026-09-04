#!/usr/bin/env bash
#
# Exercises install.sh's --prebuilt path against a fake release served over
# file:// URLs. No network, no cargo, no real binaries: the archives contain
# stub scripts that answer `completions zsh`, which is all install.sh asks of
# them. Run from anywhere: ./scripts/test-install-prebuilt.sh
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
INSTALL_SH="$ROOT/install.sh"

VERSION="v9.9.9"
TARGETS="aarch64-apple-darwin x86_64-apple-darwin x86_64-unknown-linux-musl aarch64-unknown-linux-musl"
BINARIES="git-tidy git-worktree-tidy git-branch-tidy git-stash-tidy git-remote-tidy git-tag-tidy git-repo-tidy git-config-tidy git-lfs-tidy"

WORK="$(mktemp -d)"
trap 'rm -rf "$WORK"' EXIT

failures=0
pass() { printf '  ok    %s\n' "$1"; }
fail() { printf '  FAIL  %s\n' "$1" >&2; failures=$((failures + 1)); }
assert() { local msg="$1"; shift; if "$@"; then pass "$msg"; else fail "$msg"; fi; }

sums_of() {
  # Print sha256 sums for the given files, in the format `sha256sum -c` expects.
  if command -v sha256sum >/dev/null 2>&1; then sha256sum "$@"; else shasum -a 256 "$@"; fi
}

build_fake_release() {
  local base="$1" target stage bin
  mkdir -p "$base/$VERSION"
  for target in $TARGETS; do
    stage="$WORK/stage/git-tidy-$VERSION-$target"
    mkdir -p "$stage"
    for bin in $BINARIES; do
      cat > "$stage/$bin" <<STUB
#!/bin/sh
if [ "\$1" = completions ]; then
  echo "#compdef $bin"
else
  echo "$bin \$*"
fi
STUB
      chmod +x "$stage/$bin"
    done
    cp "$ROOT/README.md" "$stage/README.md"
    tar -czf "$base/$VERSION/git-tidy-$VERSION-$target.tar.gz" \
      -C "$WORK/stage" "git-tidy-$VERSION-$target"
  done
  (cd "$base/$VERSION" && sums_of ./*.tar.gz > SHA256SUMS)
}

run_install() {
  # run_install <logfile> <args...> -- capture output so passing cases stay quiet
  local log="$1"
  shift
  "$INSTALL_SH" "$@" > "$log" 2>&1
}

echo "Staging fake release in $WORK"
build_fake_release "$WORK/releases"
printf '{"tag_name": "%s", "name": "fake"}\n' "$VERSION" > "$WORK/latest.json"

export GIT_TIDY_BASE_URL="file://$WORK/releases"
export GIT_TIDY_API_URL="file://$WORK/latest.json"
export GIT_TIDY_COMPLETIONS_DIR="$WORK/completions"

echo
echo "case: --help"
assert "--help exits 0" "$INSTALL_SH" --help
assert "--help mentions --prebuilt" sh -c "'$INSTALL_SH' --help | grep -q -- --prebuilt"

echo
echo "case: --prebuilt with an explicit --version"
prefix="$WORK/bin-explicit"
if run_install "$WORK/explicit.log" --prebuilt --version "$VERSION" --prefix "$prefix"; then
  pass "install succeeded"
else
  fail "install succeeded"
  cat "$WORK/explicit.log" >&2
fi
for bin in $BINARIES; do
  assert "installed $bin" test -x "$prefix/$bin"
  assert "generated completion _$bin" test -s "$GIT_TIDY_COMPLETIONS_DIR/_$bin"
done
assert "installed binary runs" sh -c "'$prefix/git-tidy' audit | grep -q 'git-tidy audit'"
assert "README.md was not installed as a binary" test ! -e "$prefix/README.md"

echo
echo "case: --prebuilt resolving the latest tag from the API"
prefix="$WORK/bin-latest"
if run_install "$WORK/latest.log" --prebuilt --prefix "$prefix"; then
  pass "install succeeded"
else
  fail "install succeeded"
  cat "$WORK/latest.log" >&2
fi
assert "resolved tag and installed git-tidy" test -x "$prefix/git-tidy"

echo
echo "case: checksum mismatch aborts the install"
bad="$WORK/bad-releases"
mkdir -p "$bad/$VERSION"
cp "$WORK/releases/$VERSION"/*.tar.gz "$bad/$VERSION/"
sed 's/^[0-9a-f]/0/' "$WORK/releases/$VERSION/SHA256SUMS" > "$bad/$VERSION/SHA256SUMS"
prefix="$WORK/bin-tampered"
if GIT_TIDY_BASE_URL="file://$bad" run_install "$WORK/bad.log" \
     --prebuilt --version "$VERSION" --prefix "$prefix"; then
  fail "tampered archive was rejected"
else
  pass "tampered archive was rejected"
fi
assert "nothing installed from tampered archive" test ! -e "$prefix/git-tidy"
assert "failure mentions the checksum" grep -qi checksum "$WORK/bad.log"

echo
echo "case: missing release version fails cleanly"
prefix="$WORK/bin-missing"
if run_install "$WORK/missing.log" --prebuilt --version v0.0.0-nope --prefix "$prefix"; then
  fail "missing release was rejected"
else
  pass "missing release was rejected"
fi
assert "nothing installed for missing release" test ! -e "$prefix/git-tidy"

echo
echo "case: piped to bash, as the README's curl one-liner does"
prefix="$WORK/bin-piped"
# shellcheck disable=SC2002 # the pipe is the point: it mirrors `curl ... | bash -s --`
if cat "$INSTALL_SH" | bash -s -- --prebuilt --version "$VERSION" --prefix "$prefix" \
     > "$WORK/piped.log" 2>&1; then
  pass "install succeeded when read from stdin"
else
  fail "install succeeded when read from stdin"
  cat "$WORK/piped.log" >&2
fi
assert "piped install placed git-tidy" test -x "$prefix/git-tidy"
assert "piped install is free of shell errors" \
  sh -c "! grep -qE 'unbound variable|command not found' '$WORK/piped.log'"

echo
echo "case: unknown flag is rejected"
if run_install "$WORK/unknown.log" --nonsense; then
  fail "unknown flag was rejected"
else
  pass "unknown flag was rejected"
fi

echo
if [ "$failures" -eq 0 ]; then
  echo "All install.sh prebuilt checks passed."
else
  echo "$failures check(s) failed." >&2
fi
exit "$failures"
