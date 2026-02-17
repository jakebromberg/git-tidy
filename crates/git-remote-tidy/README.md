# git-remote-tidy

Scan, classify, and remove stale Git remotes and orphaned tracking refs across multiple repositories.

## Installation

```bash
cargo install --path .
```

## Usage

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

## Classifications

| Classification | Priority | Trigger | Removal safety |
|----------------|----------|---------|----------------|
| **unreachable** | 0 | `git ls-remote` fails or times out (10s) | Safe |
| **orphaned** | 1 | Tracking refs exist under `refs/remotes/<name>/` but remote is not configured | Safe (refs pruned) |
| **active** | 2 | Remote is reachable | Keep |

## Clean defaults

- **Default**: removes unreachable remotes only
- `--all`: unreachable + orphaned
- `--force`: allow removing the `origin` remote (skipped by default)
- `--dry-run`: preview without removing

## Origin safety

The `origin` remote is never removed without `--force`. When skipped, a warning is emitted.

## Orphaned remote cleanup

Orphaned remotes (tracking refs with no config entry) are cleaned by deleting individual refs via `git update-ref -d`, since there is no config entry for `git remote remove` to act on.

## Offline mode

With `--offline`, reachability checks via `git ls-remote` are skipped. Configured remotes default to Active. Only orphaned refs (detectable without network) are classified.

## Human output example

```
backend (2 remotes)
  unreachable   origin       https://github.com/old-org/backend.git          (12 tracking branches)
  active        upstream     https://github.com/new-org/backend.git          (5 tracking branches)

2 remotes scanned: 1 unreachable, 0 orphaned, 1 active
```

## Porcelain output

Tab-delimited columns: `repo_path`, `name`, `classification`, `url`, `tracking_count`, `is_origin`
