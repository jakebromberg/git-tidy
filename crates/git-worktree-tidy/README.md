# git-worktree-tidy

Scan, classify, and remove stale Git worktrees across multiple repositories.

## Installation

```bash
cargo install git-worktree-tidy   # from crates.io
cargo install --path .            # from a clone
```

## Usage

### Scan worktrees

```bash
git-worktree-tidy scan ~/Developer            # human-readable output
git-worktree-tidy ~/Developer                 # scan is the default
git-worktree-tidy scan ~/Developer --json      # JSON output
git-worktree-tidy scan ~/Developer --porcelain # machine-readable tab-delimited
```

### Clean stale worktrees

```bash
git-worktree-tidy clean ~/Developer                    # remove landed + landed-content
git-worktree-tidy clean ~/Developer --strict           # structurally-proven landed only
git-worktree-tidy clean ~/Developer --dry-run          # preview removals
git-worktree-tidy clean ~/Developer --delete-branches  # also delete the branches
git-worktree-tidy clean ~/Developer --all --force      # every classification, even dirty
```

### Filtering

```bash
git-worktree-tidy scan ~/Developer --match feature --match bugfix  # OR semantics
git-worktree-tidy scan ~/Developer --exclude wip                   # takes precedence
git-worktree-tidy scan ~/Developer --match-repo my-project
```

### Safety

- Worktrees with meaningful uncommitted changes are skipped unless `--force` is given
- Noise files (editor cruft, OS metadata) do not count as uncommitted changes; extend with `--noise-pattern` or disable with `--no-default-noise`
- `--dry-run` prints what would be removed without making any changes
- Exit code 2 means dirty worktrees blocked removal

## Flags

```
Global:
      --behind-threshold <N>   Commit count for diverged annotation [default: 100]
  -v, --verbose                Show commit-matching details during landed detection
      --match <PATTERN>        Filter worktrees by name substring (repeatable)
      --exclude <PATTERN>      Exclude worktrees by name substring
      --match-repo <PATTERN>   Filter repos by name substring (repeatable)
      --exclude-repo <PATTERN> Exclude repos by name substring
      --noise-pattern <GLOB>   Additional file patterns to treat as noise
      --no-default-noise       Disable all default noise patterns

Scan:
      --json                   Output results as JSON
      --porcelain              Machine-readable tab-delimited output

Clean:
  -n, --dry-run                Show what would be removed without removing
  -f, --force                  Remove worktrees with meaningful uncommitted changes
      --strict                 Only target structurally-proven landed worktrees
      --all                    Include active and local worktrees
      --delete-branches        Delete local branches after removing their worktrees
      --json                   Output results as JSON
      --porcelain              Machine-readable tab-delimited output
```
