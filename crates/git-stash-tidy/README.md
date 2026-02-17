# git-stash-tidy

Scan, classify, and interactively drop stale Git stashes across multiple repositories.

## Installation

```bash
cargo install --path .
```

## Usage

```bash
# Scan stashes (default command)
git-stash-tidy scan ~/Developer
git-stash-tidy ~/Developer                # scan is the default
git-stash-tidy scan ~/Developer --json     # JSON output
git-stash-tidy scan ~/Developer --porcelain  # machine-readable

# Clean stale stashes
git-stash-tidy clean ~/Developer                         # drop committed + orphaned
git-stash-tidy clean ~/Developer --committed-only --yes   # non-interactive, committed only
git-stash-tidy clean ~/Developer --aged-only              # only drop aged stashes
git-stash-tidy clean ~/Developer --all                    # drop everything except active
git-stash-tidy clean ~/Developer --dry-run                # preview drops

# Custom age threshold (default: 90 days)
git-stash-tidy --age-threshold 30 scan ~/Developer
```

## Classifications

| Classification | Priority | Trigger | Drop safety |
|----------------|----------|---------|-------------|
| **committed** | 0 | Stash diff matches the branch tip (`diff_similarity >= 0.5`) | Safe |
| **orphaned** | 1 | Branch from stash message no longer exists locally | Safe |
| **aged** | 2 | Older than `--age-threshold` days (default 90) | Review recommended |
| **active** | 3 | None of the above | Keep |

## Clean defaults

- **Default**: drops committed + orphaned stashes
- `--committed-only`: only committed stashes
- `--aged-only`: only aged stashes
- `--all`: everything except active
- `--dry-run`: preview without dropping

## Key design: drop ordering

Stashes are dropped in descending index order per repo (`stash@{2}`, then `stash@{1}`, then `stash@{0}`) to prevent index renumbering side effects.

## Human output example

```
my-repo (3 stashes)
  committed  stash@{0}  WIP on feature-x: abc1234 Add login     23 days ago
  orphaned   stash@{1}  WIP on deleted-branch: def5678 Fix UI   45 days ago
  active     stash@{2}  WIP on main: ghi9012 Temp changes        2 days ago

3 stashes scanned: 1 committed, 1 orphaned, 0 aged, 1 active
```

## Porcelain output

Tab-delimited columns: `repo_path`, `stash_ref`, `classification`, `branch`, `age_days`, `message`
