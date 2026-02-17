# git-repo-tidy

Scan, classify, and remove stale Git repositories.

This is the most destructive tool in the git-tidy suite -- it performs `rm -rf` on entire repos. Safety guards include dirty detection, `--force` requirements, `--dry-run`, and confirmation prompts.

## Classification

| Classification | Trigger |
|---------------|---------|
| **Stale** | No commits in N months (default 6), has reachable remote |
| **Orphaned** | No remote, or all remotes unreachable |
| **Active** | Recent commits and/or reachable remote |

Dirty status is tracked independently and gates deletion safety (dirty repos require `--force`).

## Usage

```bash
# Scan repos in current directory (default)
git-repo-tidy

# Scan a specific directory
git-repo-tidy scan ~/Developer

# JSON output
git-repo-tidy scan --json

# Machine-readable output
git-repo-tidy scan --porcelain

# Preview what would be deleted
git-repo-tidy clean --dry-run

# Delete stale + orphaned repos (interactive)
git-repo-tidy clean

# Delete without confirmation
git-repo-tidy clean --yes

# Include dirty repos
git-repo-tidy clean --force

# Only stale or only orphaned
git-repo-tidy clean --stale-only
git-repo-tidy clean --orphaned-only

# Custom stale threshold (12 months instead of 6)
git-repo-tidy --stale-months 12 scan

# Skip network checks
git-repo-tidy --offline scan
```

## Safety

- Active repos are never deleted
- Dirty repos require `--force` to delete (exit code 2 when blocked)
- `--dry-run` previews all actions without side effects
- Interactive confirmation before destructive operations

## Output formats

**Human-readable** (default):
```
  stale      my-old-project       549 days ago    142 MB
  orphaned   archived-service     800 days ago    89 MB     dirty (3 files)
  active     main-app             today           256 MB

3 repos scanned: 1 stale, 1 orphaned, 1 active (1 dirty)
Total: 487 MB (stale + orphaned: 231 MB reclaimable)
```

**Porcelain** (`--porcelain`): tab-delimited fields: path, name, classification, last_commit_date, age_days, disk_bytes, remote_url, branch_count, has_remote, is_dirty, dirty_count.

**JSON** (`--json`): flat array of repo objects.
