# git-tidy

A Cargo workspace for Git housekeeping tools that share classification logic (merged/landed/active/local) and a common git abstraction layer.

## Usage

`git tidy` is the unified entry point. It dispatches to individual tools or runs
an audit across all installed tools.

```bash
# Dispatch to individual tools
git tidy worktrees scan ~/Developer
git tidy branches clean --yes
git tidy config lint
git tidy lfs scan --size-threshold 5MB

# Audit all installed tools (default)
git tidy ~/Developer
git tidy audit ~/Developer --json
```

See individual tool sections below for full flag reference.

## Tools

### git-tidy (audit runner + dispatch)

Unified entry point for the git-tidy suite. Dispatches `git tidy <alias> [args...]` to the corresponding `git-*-tidy` binary, and runs a consolidated audit across all installed tools when no alias is given.

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

### git-repo-tidy

Scans a directory of Git repos, classifies them by activity (stale, orphaned, active), and removes stale or orphaned repos. This is the most destructive tool in the suite -- dirty repos require `--force` to delete.
### git-config-tidy

Lints local git config for common issues (orphaned branch tracking config, aliases shadowing built-in commands) and auto-fixes the fixable ones.

### git-lfs-tidy

Scans a directory of Git repos for LFS health issues: large blobs not tracked by LFS, missing LFS objects, orphaned prunable objects, and healthy tracked files. Optionally prunes orphaned LFS objects.

## Installation

Requires Rust 1.93.0 or later (edition 2024).

```bash
# Install all tools at once
./install.sh

# Or install individually
cargo install --path crates/git-tidy              # audit runner
cargo install --path crates/git-worktree-tidy
cargo install --path crates/git-branch-tidy
cargo install --path crates/git-stash-tidy
cargo install --path crates/git-remote-tidy
cargo install --path crates/git-tag-tidy
cargo install --path crates/git-repo-tidy
cargo install --path crates/git-config-tidy
cargo install --path crates/git-lfs-tidy
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

# Filter by worktree name (substring match, case-insensitive)
git-worktree-tidy --match tubafrenzy scan ~/Developer              # only tubafrenzy worktrees
git-worktree-tidy --match tubafrenzy --match wxyc scan ~/Developer # OR semantics
git-worktree-tidy --match tubafrenzy clean --dry-run ~/Developer   # clean only matching
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

### git-repo-tidy

```bash
# Scan repos (default command)
git-repo-tidy scan ~/Developer
git-repo-tidy ~/Developer                # scan is the default
git-repo-tidy scan ~/Developer --json     # JSON output
git-repo-tidy scan ~/Developer --porcelain  # machine-readable

# Offline mode (skip reachability checks)
git-repo-tidy --offline scan ~/Developer

# Custom stale threshold (12 months instead of 6)
git-repo-tidy --stale-months 12 scan ~/Developer

# Clean stale repos
git-repo-tidy clean ~/Developer                  # remove stale + orphaned repos
git-repo-tidy clean ~/Developer --stale-only      # only stale repos
git-repo-tidy clean ~/Developer --orphaned-only   # only orphaned repos
git-repo-tidy clean ~/Developer --force            # allow deleting dirty repos
git-repo-tidy clean ~/Developer --dry-run          # preview deletions
### git-config-tidy

```bash
# Lint config (default command)
git-config-tidy lint ~/Developer
git-config-tidy ~/Developer                # lint is the default
git-config-tidy lint ~/Developer --json     # JSON output
git-config-tidy lint ~/Developer --porcelain  # machine-readable

# Fix auto-fixable issues
git-config-tidy fix ~/Developer                    # fix interactively
git-config-tidy fix ~/Developer --dry-run           # preview fixes
git-config-tidy fix ~/Developer --yes               # skip confirmation
```

### git-lfs-tidy

```bash
# Scan for LFS health issues (default command)
git-lfs-tidy scan ~/Developer
git-lfs-tidy ~/Developer                          # scan is the default
git-lfs-tidy scan ~/Developer --json               # JSON output
git-lfs-tidy scan ~/Developer --porcelain          # machine-readable

# Custom thresholds
git-lfs-tidy scan ~/Developer --size-threshold 500KB  # flag files above 500KB
git-lfs-tidy scan ~/Developer --depth 500              # scan last 500 commits

# Clean up orphaned LFS objects
git-lfs-tidy clean ~/Developer --prune             # prune orphaned LFS objects
git-lfs-tidy clean ~/Developer --prune --dry-run   # preview what would be pruned
git-lfs-tidy clean ~/Developer --prune --yes       # skip confirmation
```

### git-tidy (audit + dispatch)

```bash
# Dispatch to individual tools
git tidy worktrees scan ~/Developer
git tidy branches clean --yes
git tidy config lint

# Audit all installed tools (default command)
git-tidy ~/Developer
git-tidy audit ~/Developer          # explicit subcommand
git-tidy ~/Developer --json          # JSON output
git-tidy ~/Developer --porcelain     # machine-readable
git-tidy ~/Developer -v              # verbose (shows tool paths)
git-tidy ~/Developer --tools branch,tag  # run only specific tools
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

### LFS classifications

| Classification | Meaning | Clean action |
|----------------|---------|--------------|
| **untracked** | Large blob (above `--size-threshold`) not in LFS tracking | Info only (recommend `git lfs migrate`) |
| **missing** | LFS pointer exists but object missing locally | Info only (recommend `git lfs fetch --all`) |
| **orphaned** | Prunable LFS objects (no branch refs) | `git lfs prune` (requires `--prune`) |
| **healthy** | Properly tracked and present | Keep |

### Remote classifications

| Classification | Meaning | Removal safety |
|----------------|---------|----------------|
| **unreachable** | `git ls-remote` fails or times out (10s) | Safe |
| **orphaned** | Tracking refs exist but remote is not configured | Safe (refs pruned) |
| **active** | Remote is reachable | Keep |

### Repo classifications

| Classification | Meaning | Removal safety |
|----------------|---------|----------------|
| **stale** | No commits in N months (default 6), has reachable remote | Safe (can re-clone) |
| **orphaned** | No remote, or all remotes unreachable | Review recommended |
| **active** | Recent commits and/or reachable remote | Keep |

Dirty status is tracked independently; dirty repos require `--force` to delete regardless of classification.
### Config issue types

| Issue | Severity | Auto-fixable | Meaning |
|-------|----------|-------------|---------|
| **orphaned_branch_config** | Warning | Yes | `branch.foo.remote`/`branch.foo.merge` exist but branch `foo` does not |
| **alias_shadows_builtin** | Info | No | `alias.X` in local config shadows a built-in git command |

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

## Development

After cloning, enable the pre-commit hook (runs `cargo fmt --check`):

```bash
git config core.hooksPath .githooks
```

## Workspace Structure

```
crates/
  git-tidy/               Audit runner binary (no core dependency)
  git-tidy-core/          Shared classification, git abstraction, output helpers, test utilities
  git-worktree-tidy/      Worktree scanner/cleaner binary
  git-branch-tidy/        Branch scanner/cleaner binary
  git-stash-tidy/         Stash scanner/cleaner binary
  git-remote-tidy/        Remote scanner/cleaner binary
  git-tag-tidy/           Tag scanner/cleaner binary
  git-repo-tidy/          Repo scanner/cleaner binary
  git-config-tidy/        Config linter/fixer binary
  git-lfs-tidy/           LFS health scanner/cleaner binary
```
