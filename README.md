# git-worktree-tidy

A CLI tool that scans a directory for linked Git worktrees, classifies them by staleness, and interactively removes the stale ones.

## Installation

```bash
cargo install --path .
```

## Usage

### Scan worktrees (default command)

```bash
git-worktree-tidy scan ~/Developer
git-worktree-tidy ~/Developer              # scan is the default
git-worktree-tidy scan ~/Developer --json   # JSON output
git-worktree-tidy scan ~/Developer --porcelain  # machine-readable
```

### Clean stale worktrees

```bash
git-worktree-tidy clean ~/Developer                    # interactive
git-worktree-tidy clean ~/Developer --merged-only --yes  # non-interactive, merged only
git-worktree-tidy clean ~/Developer --landed --yes       # merged + fully landed
git-worktree-tidy clean ~/Developer --dry-run            # preview removals
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
- **dirty**: Working tree has meaningful uncommitted changes

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

Create `~/.config/git-worktree-tidy/config.toml` to set persistent noise preferences:

```toml
[noise]
extra = ["*.swp", "*.swo", ".envrc"]
exclude = ["package-lock.json"]
```

- **extra**: Additional patterns to treat as noise.
- **exclude**: Default patterns to stop treating as noise.

Merge order: `(defaults - exclude) + config extra + CLI extra`. The `--no-default-noise` flag clears all defaults, keeping only config extras and CLI extras.

## CLI Flags

```
Options:
  -n, --dry-run              Show what would be removed without removing
  -f, --force                Remove worktrees with meaningful uncommitted changes
  -y, --yes                  Skip confirmation prompts (accept all defaults)
      --merged-only          Only target merged worktrees
      --landed               Target merged and fully landed worktrees (not partial)
      --all                  Include active and local worktrees in interactive clean
      --behind-threshold N   Commit count for diverged annotation (default: 100)
      --delete-branches      Delete local branches after removing their worktrees
      --noise-pattern PAT    Additional file pattern to treat as noise (repeatable)
      --no-default-noise     Disable all built-in noise patterns
      --json                 Output scan results as JSON
      --porcelain            Machine-readable tab-delimited output
  -v, --verbose              Show commit-matching details during landed detection
  -h, --help                 Show help
```

## Exit Codes

| Code | Meaning |
|------|---------|
| 0 | Success, or nothing to clean |
| 1 | Error during scan or removal |
| 2 | Dirty worktrees blocked removal (rerun with `--force`) |
