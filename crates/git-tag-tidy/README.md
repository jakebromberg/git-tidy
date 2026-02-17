# git-tag-tidy

Scan, classify, and remove stale Git tags across multiple repositories.

## Installation

```bash
cargo install --path .
```

## Usage

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
git-tag-tidy clean ~/Developer --force             # allow synced tags, bypass release protection
git-tag-tidy clean ~/Developer --include-remote    # also delete from remote
git-tag-tidy clean ~/Developer --dry-run           # preview removals
```

## Classifications

| Classification | Priority | Trigger | Clean default |
|----------------|----------|---------|---------------|
| **stale** | 0 | Tag points at a commit not reachable from any branch | Yes |
| **local_only** | 1 | Tag exists locally but not on any configured remote | Yes |
| **remote_only** | 2 | Tag exists on remote but not locally | No (info only) |
| **synced** | 3 | Tag exists both locally and on remote, commit is reachable | No |

## Clean defaults

- **Default**: removes stale + local_only tags
- `--stale-only`: only stale
- `--local-only`: only local_only
- `--all`: stale + local_only + remote_only (not synced)
- `--force`: allow synced tags and bypass release tag warnings
- `--include-remote`: also delete remote copies when cleaning
- `--dry-run`: preview without removing

## Release tag protection

Tags matching release version patterns (e.g., `v1.0.0`, `1.2.3`, `v1.0.0-rc1`) are skipped during cleanup with a warning. Use `--force` to override.

## Offline mode

With `--offline`, remote tag queries via `git ls-remote` are skipped. Local tags that are reachable default to Synced since local-only cannot be distinguished from synced without remote data.

## Human output example

```
my-repo (4 tags)
  stale         old-experiment       abc1234   lightweight
  local_only    feature-v2-wip       def5678   lightweight
  synced        v1.0.0               789abcd   annotated    2024-06-15T10:00:00+00:00
  synced        v2.0.0               bcd1234   annotated    2024-12-01T10:00:00+00:00

4 tags scanned: 1 stale, 1 local_only, 0 remote_only, 2 synced
```

## Porcelain output

Tab-delimited columns: `repo_path`, `name`, `classification`, `commit`, `is_annotated`, `tagger_date`, `is_release_tag`, `remote_names`
