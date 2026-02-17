# git-tidy

A Cargo workspace for Git housekeeping tools that share classification logic (merged/landed/active/local) and a common git abstraction layer.

## Tools

### git-worktree-tidy

Scans a directory for linked Git worktrees, classifies them by staleness, and interactively removes the stale ones.

### git-branch-tidy

Scans a directory of Git repos, classifies local branches by staleness, and interactively removes stale branches.

## Installation

Requires Rust 1.93.0 or later (edition 2024).

```bash
cargo install --path crates/git-worktree-tidy
cargo install --path crates/git-branch-tidy
```

## Usage

### git-worktree-tidy

```bash
# Scan worktrees (default command)
git-worktree-tidy scan ~/Developer
git-worktree-tidy ~/Developer              # scan is the default
git-worktree-tidy scan ~/Developer --json   # JSON output
git-worktree-tidy scan ~/Developer --porcelain  # machine-readable

# Clean stale worktrees
git-worktree-tidy clean ~/Developer                    # interactive
git-worktree-tidy clean ~/Developer --merged-only --yes  # non-interactive, merged only
git-worktree-tidy clean ~/Developer --landed --yes       # merged + fully landed
git-worktree-tidy clean ~/Developer --dry-run            # preview removals
```

### git-branch-tidy

```bash
# Scan branches (default command)
git-branch-tidy scan ~/Developer
git-branch-tidy ~/Developer                # scan is the default
git-branch-tidy scan ~/Developer --json     # JSON output
git-branch-tidy scan ~/Developer --porcelain  # machine-readable

# Clean stale branches
git-branch-tidy clean ~/Developer                         # delete merged + landed
git-branch-tidy clean ~/Developer --merged-only --yes      # non-interactive, merged only
git-branch-tidy clean ~/Developer --all --force            # force-delete all classifications
git-branch-tidy clean ~/Developer --dry-run                # preview deletions
git-branch-tidy clean ~/Developer --include-remote --yes   # also delete remote branches
```

## Classifications

| Classification | Meaning | Removal safety |
|----------------|---------|----------------|
| **merged** | Branch tip is an ancestor of the default branch | Safe |
| **landed** | All branch commits matched on the default branch (rebase/squash/cherry-pick) | Safe |
| **partial** | Some branch commits matched (reports ratio like "4/6 landed") | Review required |
| **active** | Has a remote tracking branch; not merged or landed | Keep |
| **local** | No remote tracking branch; not merged or landed | Keep |

## Annotations

- **remote deleted**: Remote tracking branch no longer exists after `fetch --prune`
- **diverged**: Branch is more than `--behind-threshold` (default: 100) commits behind
- **dirty** (worktree-tidy only): Working tree has meaningful uncommitted changes

## Noise Configuration

When a worktree's only untracked files match noise patterns, it is not marked as dirty. This prevents lockfiles, editor swap files, and OS artifacts from blocking worktree removal.

### Default noise patterns

`.DS_Store`, `*.pyc`, `__pycache__`, `uv.lock`, `package-lock.json`, `Podfile.lock`, `yarn.lock`

### CLI flags

Add extra noise patterns or disable defaults entirely:

```bash
git-worktree-tidy scan ~/Developer --noise-pattern "*.swp"
git-worktree-tidy scan ~/Developer --noise-pattern "*.swp" --noise-pattern ".envrc"
git-worktree-tidy scan ~/Developer --no-default-noise --noise-pattern "*.swp"
```

### Config file

Create `~/.config/git-worktree-tidy/config.toml` (or `$XDG_CONFIG_HOME/git-worktree-tidy/config.toml`) to set persistent noise preferences:

```toml
[noise]
extra = ["*.swp", "*.swo", ".envrc"]
exclude = ["package-lock.json"]
```

- **extra**: Additional patterns to treat as noise.
- **exclude**: Default patterns to stop treating as noise.

Merge order: `(defaults - exclude) + config extra + CLI extra`. The `--no-default-noise` flag clears all defaults, keeping only config extras and CLI extras.

## Exit Codes

| Code | Meaning |
|------|---------|
| 0 | Success, or nothing to clean |
| 1 | Error during scan or removal |
| 2 | Dirty worktrees blocked removal (rerun with `--force`) |

## Workspace Structure

```
crates/
  git-tidy-core/          Shared classification, git abstraction, output helpers, test utilities
  git-worktree-tidy/      Worktree scanner/cleaner binary
  git-branch-tidy/        Branch scanner/cleaner binary
```
