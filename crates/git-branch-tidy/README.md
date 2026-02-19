# git-branch-tidy

Scan, classify, and interactively remove stale local Git branches across multiple repositories.

## Installation

```bash
cargo install --path .
```

## Usage

### Scan branches

```bash
git-branch-tidy scan ~/Developer          # human-readable output
git-branch-tidy ~/Developer               # scan is the default
git-branch-tidy scan ~/Developer --json    # JSON output
git-branch-tidy scan ~/Developer --porcelain  # machine-readable tab-delimited
```

### Clean stale branches

```bash
git-branch-tidy clean ~/Developer                         # delete landed + landed-content
git-branch-tidy clean ~/Developer --strict --yes           # non-interactive, landed (ancestor proof) only
git-branch-tidy clean ~/Developer --all --force            # force-delete all classifications
git-branch-tidy clean ~/Developer --dry-run                # preview deletions
git-branch-tidy clean ~/Developer --include-remote --yes   # also delete remote tracking branches
```

### Safety

- The default branch is never deleted (excluded during scan)
- The currently checked-out branch is never deleted
- Without `--force`: uses `git branch -d` which refuses to delete non-ancestor branches
- With `--force`: uses `git branch -D` to force-delete
- `--dry-run` prints what would be deleted without making any changes
- Remote branch deletion failures are warned, not fatal

### Options

```
Global:
      <directory>              Directory to scan (default: current directory)
      --behind-threshold N     Commit count for diverged annotation (default: 100)
  -v, --verbose                Show commit-matching details during landed detection

Scan:
      --json                   Output results as JSON
      --porcelain              Machine-readable tab-delimited output

Clean:
  -n, --dry-run                Show what would be deleted without deleting
  -f, --force                  Force-delete branches (git branch -D)
  -y, --yes                    Skip confirmation prompts
      --strict                 Only target landed branches (structural ancestor proof)
      --all                    Include all classifications
      --include-remote         Also delete remote tracking branches
      --json                   Output results as JSON
      --porcelain              Machine-readable tab-delimited output
```
