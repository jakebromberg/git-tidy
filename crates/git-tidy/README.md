# git-tidy

Audit runner that discovers installed `git-*-tidy` tools, runs each with `scan --json` (or `lint --json`), and produces a consolidated summary.

## Installation

```bash
cargo install --path .
```

## Usage

```bash
# Run audit across all installed tools (default command)
git-tidy ~/Developer
git-tidy audit ~/Developer          # explicit subcommand

# Output formats
git-tidy ~/Developer --json          # JSON output
git-tidy ~/Developer --porcelain     # machine-readable tab-delimited

# Verbose mode (shows tool paths)
git-tidy ~/Developer -v

# Run only specific tools
git-tidy ~/Developer --tools branch,tag
git-tidy ~/Developer --tools git-branch-tidy,git-tag-tidy
```

## Supported tools

| Binary | Item noun | Scan command | Count field |
|--------|-----------|-------------|-------------|
| git-worktree-tidy | worktrees | scan | classification |
| git-branch-tidy | branches | scan | classification |
| git-stash-tidy | stashes | scan | classification |
| git-remote-tidy | remotes | scan | classification |
| git-tag-tidy | tags | scan | classification |
| git-repo-tidy | repos | scan | classification |
| git-config-tidy | config issues | lint | kind |
| git-lfs-tidy | LFS files | scan | classification |

Only installed tools are run. Missing tools are listed in the output.

## Human output example

```
git-tidy audit: ~/Developer

  worktrees:       8 scanned (1 merged, 2 landed, 5 active)
  branches:       22 scanned (3 merged, 5 landed, 2 partial, 12 active)
  stashes:         6 scanned (2 committed, 1 orphaned, 3 active)
  remotes:         4 scanned (1 unreachable, 3 active)
  tags:           15 scanned (2 stale, 3 local_only, 10 synced)
  config:          2 issues (1 orphaned_branch_config, 1 alias_shadows_builtin)
  LFS:             3 scanned (2 untracked, 1 healthy)

  not installed: git-repo-tidy

Run individual tools for details.
```

## JSON output

```json
{
  "directory": "/Users/jake/Developer",
  "tools_found": ["git-worktree-tidy", "git-branch-tidy"],
  "tools_missing": ["git-repo-tidy"],
  "results": [
    {
      "name": "git-worktree-tidy",
      "item_noun": "worktrees",
      "total": 8,
      "counts": { "active": 5, "landed": 2, "merged": 1 },
      "error": null
    }
  ]
}
```

## Porcelain output

Tab-delimited columns: `tool_name`, `item_noun`, `total`, `counts_json`, `error`
