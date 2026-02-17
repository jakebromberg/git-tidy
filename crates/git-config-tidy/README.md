# git-config-tidy

Lint and fix common Git config issues across a directory of repos.

## Issue Types

### orphaned_branch_config (Warning, auto-fixable)

`branch.foo.remote` and `branch.foo.merge` entries exist in local config, but branch `foo` no longer exists locally. This happens when branches are deleted via low-level ref manipulation (`git update-ref -d`) rather than `git branch -D` (which cleans up config automatically).

Fixed via `git config --remove-section branch.foo`.

### alias_shadows_builtin (Info, no auto-fix)

`alias.X` in local config where `X` matches a built-in git command name. Informational only.

## Usage

```bash
# Lint (default command)
git-config-tidy lint ~/Developer
git-config-tidy ~/Developer                    # lint is the default
git-config-tidy lint ~/Developer --json         # JSON output
git-config-tidy lint ~/Developer --porcelain    # machine-readable

# Fix auto-fixable issues
git-config-tidy fix ~/Developer                 # fix interactively
git-config-tidy fix ~/Developer --dry-run       # preview fixes
git-config-tidy fix ~/Developer --yes           # skip confirmation
```

## Installation

```bash
cargo install --path crates/git-config-tidy
```
