# git-tidy-core

Shared library behind the [git-tidy](https://github.com/jakebromberg/git-tidy) suite: the `GitOps` abstraction over Git, branch and worktree classification, the scan and clean pipeline seams, and the shared output formatters.

This crate exists to be depended on by the `git-*-tidy` binaries. Its API is not yet stable — expect breaking changes within `0.x`.

## Installation

```bash
cargo add git-tidy-core
```

## Testing support

The `testutil` feature exposes `MockGit`, `MockGitBuilder`, and `TestRepo` for tests that need canned Git responses or real repositories in temp directories:

```toml
[dev-dependencies]
git-tidy-core = { version = "0.1", features = ["testutil"] }
```

## Modules

- `git` — `GitOps` trait and the `RealGit` implementation
- `classification` — `classify_branch` and `classify_worktree`
- `scan` — `parallel_classify` and the `run_pipeline` seam
- `clean` — the shared `run_clean` loop
- `output` — table, JSON, and porcelain formatters plus the `TidyItem` trait
- `caching` — `CachingGitOps`, a memoizing `GitOps` wrapper
