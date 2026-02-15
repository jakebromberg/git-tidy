# git-worktree-tidy

## Build and Test

```bash
cargo build
cargo test
cargo test -- --test-threads=1  # if tests interfere with each other
```

## Architecture

- **`GitOps` trait** (`git.rs`): All git operations go through this trait. `RealGit` shells out to `git`; `MockGitBuilder` (in `git.rs` under `#[cfg(test)]`) returns canned data.
- **Output to `&mut dyn Write`**: Enables unit testing output formatters without process capture.
- **`thiserror`** for errors: Known, finite variants with exit code mapping (1=error, 2=dirty-blocked).
- **Sequential repo processing**: No parallelism needed for typical workloads (~9 repos, ~39 worktrees).

## Conventions

- All modules that call git take `&dyn GitOps` to enable mocking.
- Tests use `tempfile::tempdir()` for isolation with real git repos.
- Path canonicalization in discovery handles macOS `/var` -> `/private/var` symlinks.
- Classification priority order: merged (0) > landed (1) > partial (2) > active (3) > local (4).

## Project Layout

```
src/
  main.rs              # CLI dispatch
  lib.rs               # Public module exports for integration tests
  cli.rs               # clap derive definitions
  types.rs             # Classification, WorktreeInfo, ScanResult, etc.
  error.rs             # thiserror Error enum
  git.rs               # GitOps trait + RealGit + MockGitBuilder (#[cfg(test)])
  discovery.rs         # .git file parsing, parent repo derivation
  classification.rs    # Classification pipeline
  dirty.rs             # Status parsing with noise filtering
  landed.rs            # Subject matching, fuzzy, patch similarity
  scan.rs              # Full scan pipeline (discover -> fetch -> classify)
  output.rs            # Human-readable, JSON, porcelain formatters
  clean.rs             # Interactive prompting and removal
tests/
  common/mod.rs        # TestRepo scaffolding for integration tests
  integration_*.rs     # Real git repos in tempdirs
```
