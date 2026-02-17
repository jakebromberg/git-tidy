# git-tidy

A Cargo workspace for Git housekeeping tools that share classification logic (merged/landed/active/local) and a common git abstraction layer.

## Tools

### git-worktree-tidy

Scans a directory for linked Git worktrees, classifies them by staleness, and interactively removes the stale ones.

### git-branch-tidy

Scans a directory of Git repos, classifies local branches by staleness, and interactively removes stale branches.

### git-stash-tidy

Scans a directory of Git repos, classifies stash entries by staleness (committed, orphaned, aged, active), and interactively drops stale stashes.

### git-remote-tidy

Scans a directory of Git repos, classifies remotes by reachability (unreachable, orphaned, active), and interactively removes stale remotes and orphaned tracking refs.

### git-tag-tidy

Scans a directory of Git repos, classifies tags by staleness and sync status (stale, local_only, remote_only, synced), and interactively removes stale or local-only tags.

## Installation

Requires Rust 1.93.0 or later (edition 2024).

```bash
cargo install --path crates/git-worktree-tidy
cargo install --path crates/git-branch-tidy
cargo install --path crates/git-stash-tidy
cargo install --path crates/git-remote-tidy
cargo install --path crates/git-tag-tidy
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

### git-stash-tidy

```bash
# Scan stashes (default command)
git-stash-tidy scan ~/Developer
git-stash-tidy ~/Developer                # scan is the default
git-stash-tidy scan ~/Developer --json     # JSON output
git-stash-tidy scan ~/Developer --porcelain  # machine-readable

# Clean stale stashes
git-stash-tidy clean ~/Developer                         # drop committed + orphaned
git-stash-tidy clean ~/Developer --committed-only --yes   # non-interactive, committed only
git-stash-tidy clean ~/Developer --all                    # drop everything except active
git-stash-tidy clean ~/Developer --dry-run                # preview drops
git-stash-tidy clean ~/Developer --age-threshold 30       # custom age threshold (default 90)
```

### git-remote-tidy

```bash
# Scan remotes (default command)
git-remote-tidy scan ~/Developer
git-remote-tidy ~/Developer                # scan is the default
git-remote-tidy scan ~/Developer --json     # JSON output
git-remote-tidy scan ~/Developer --porcelain  # machine-readable

# Offline mode (skip reachability checks)
git-remote-tidy --offline scan ~/Developer

# Clean stale remotes
git-remote-tidy clean ~/Developer                  # remove unreachable remotes
git-remote-tidy clean ~/Developer --all             # also prune orphaned tracking refs
git-remote-tidy clean ~/Developer --force            # allow removing origin
git-remote-tidy clean ~/Developer --dry-run          # preview removals
```

### git-tag-tidy

```bash
# Scan tags (default command)
git-tag-tidy scan ~/Developer
git-tag-tidy ~/Developer                # scan is the default
git-tag-tidy scan ~/Developer --json     # JSON output
git-tag-tidy scan ~/Developer --porcelain  # machine-readable

# Offline mode (skip remote tag queries)
git-tag-tidy --offline scan ~/Developer

# Clean stale tags
git-tag-tidy clean ~/Developer                    # remove stale + local-only
git-tag-tidy clean ~/Developer --stale-only        # only stale tags
git-tag-tidy clean ~/Developer --local-only        # only local-only tags
git-tag-tidy clean ~/Developer --all               # stale + local-only + remote-only
git-tag-tidy clean ~/Developer --include-remote    # also delete from remote
git-tag-tidy clean ~/Developer --dry-run           # preview removals
```

## Classifications

### Worktree and branch classifications

| Classification | Meaning | Removal safety |
|----------------|---------|----------------|
| **merged** | Branch tip is an ancestor of the default branch | Safe |
| **landed** | All branch commits matched on the default branch (rebase/squash/cherry-pick) | Safe |
| **partial** | Some branch commits matched (reports ratio like "4/6 landed") | Review required |
| **active** | Has a remote tracking branch; not merged or landed | Keep |
| **local** | No remote tracking branch; not merged or landed | Keep |

### Stash classifications

| Classification | Meaning | Drop safety |
|----------------|---------|-------------|
| **committed** | Stash diff matches the branch tip (content already committed) | Safe |
| **orphaned** | Branch from stash message no longer exists locally | Safe |
| **aged** | Older than `--age-threshold` days (default 90) | Review recommended |
| **active** | None of the above; still relevant | Keep |

### Tag classifications

| Classification | Meaning | Removal safety |
|----------------|---------|----------------|
| **stale** | Tag points at a commit not reachable from any branch | Safe |
| **local_only** | Tag exists locally but not on any configured remote | Safe |
| **remote_only** | Tag exists on remote but not locally | Info only |
| **synced** | Tag exists both locally and on remote, commit is reachable | Keep |

### Remote classifications

| Classification | Meaning | Removal safety |
|----------------|---------|----------------|
| **unreachable** | `git ls-remote` fails or times out (10s) | Safe |
| **orphaned** | Tracking refs exist but remote is not configured | Safe (refs pruned) |
| **active** | Remote is reachable | Keep |

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
  git-stash-tidy/         Stash scanner/cleaner binary
  git-remote-tidy/        Remote scanner/cleaner binary
  git-tag-tidy/           Tag scanner/cleaner binary
```
