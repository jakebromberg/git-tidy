# git-lfs-tidy

Scan a directory of Git repos for LFS health issues and clean up orphaned LFS objects.

## Classifications

| Classification | Meaning | Clean action |
|----------------|---------|--------------|
| **untracked** | Large blob (above `--size-threshold`) not in LFS tracking | Info only (recommend `git lfs migrate`) |
| **missing** | LFS pointer exists but object missing locally | Info only (recommend `git lfs fetch --all`) |
| **orphaned** | Prunable LFS objects (no branch refs) | `git lfs prune` (requires `--prune`) |
| **healthy** | Properly tracked and present | Keep |

## Usage

```bash
# Scan for LFS health issues (default command)
git-lfs-tidy scan ~/Developer
git-lfs-tidy ~/Developer                          # scan is the default

# Output formats
git-lfs-tidy scan ~/Developer --json               # JSON output
git-lfs-tidy scan ~/Developer --porcelain          # machine-readable tab-delimited

# Custom thresholds
git-lfs-tidy scan ~/Developer --size-threshold 500KB  # flag files above 500KB (default: 1MB)
git-lfs-tidy scan ~/Developer --depth 500              # scan up to 500 branch/tag tip trees (default: 1000)

# Clean up orphaned LFS objects
git-lfs-tidy clean ~/Developer --prune             # prune orphaned LFS objects
git-lfs-tidy clean ~/Developer --prune --dry-run   # preview what would be pruned
git-lfs-tidy clean ~/Developer --prune --yes       # skip confirmation
```

## How it works

1. Discovers Git repos under the target directory.
2. If `git lfs` is installed:
   - Lists LFS-tracked files (`git lfs ls-files`) and classifies them as **healthy** or **missing**.
   - Checks for prunable objects (`git lfs prune --dry-run`) and reports **orphaned** count.
3. Scans branch/tag tip trees for large blobs (`git rev-list` + `git ls-tree`) above the size threshold and flags **untracked** files not in LFS.
4. When `git lfs` is not installed, gracefully degrades to only scanning for large untracked blobs.

## Part of git-tidy

This tool is part of the [git-tidy](../../README.md) workspace. See the workspace README for installation and other tools.
