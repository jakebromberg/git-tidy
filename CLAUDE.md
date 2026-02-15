# git-tidy

## Build and Test

```bash
cargo build --workspace
cargo test --workspace
cargo test --workspace -- --test-threads=1  # if tests interfere with each other
```

## Architecture

This is a Cargo workspace with a shared core library and (currently) one binary crate.

- **`git-tidy-core`**: Shared library containing git abstraction, classification logic, and test utilities.
- **`git-worktree-tidy`**: Binary crate for scanning and cleaning stale git worktrees.

### Core patterns

- **`GitOps` trait** (`git-tidy-core/src/git.rs`): All git operations go through this trait. `RealGit` shells out to `git`; `MockGit` (in `testutil.rs`) returns canned data.
- **Output to `&mut dyn Write`**: Enables unit testing output formatters without process capture.
- **`thiserror`** for errors: Known, finite variants with exit code mapping (1=error, 2=dirty-blocked).
- **Sequential repo processing**: No parallelism needed for typical workloads (~9 repos, ~39 worktrees).

## Conventions

- All modules that call git take `&dyn GitOps` to enable mocking.
- Tests use `tempfile::tempdir()` for isolation with real git repos.
- Path canonicalization in discovery handles macOS `/var` -> `/private/var` symlinks.
- Classification priority order: merged (0) > landed (1) > partial (2) > active (3) > local (4).
- Shared test utilities (MockGitBuilder, TestRepo, git helper) live in `git-tidy-core/src/testutil.rs`, gated behind the `testutil` feature. Binary crates depend on `git-tidy-core = { features = ["testutil"] }` in `[dev-dependencies]`.

## Project Layout

```
Cargo.toml                                    # Workspace root
crates/
  git-tidy-core/                              # Shared library
    src/
      lib.rs                                  # Module exports
      git.rs                                  # GitOps trait + RealGit implementation
      types.rs                                # Classification, WorktreeInfo, ScanResult, etc.
      error.rs                                # thiserror Error enum
      classification.rs                       # Classification pipeline
      config.rs                               # Noise pattern configuration (file + CLI + defaults)
      dirty.rs                                # Status parsing with noise filtering
      landed.rs                               # Subject matching, fuzzy, patch similarity
      testutil.rs                             # MockGitBuilder, MockGit, TestRepo, git() helper
  git-worktree-tidy/                          # Worktree scanner/cleaner binary
    src/
      main.rs                                 # CLI dispatch
      lib.rs                                  # Public module exports for integration tests
      cli.rs                                  # clap derive definitions
      discovery.rs                            # .git file parsing, parent repo derivation
      output.rs                               # Human-readable, JSON, porcelain formatters
      clean.rs                                # Interactive prompting and removal
    tests/
      common/mod.rs                           # Re-exports from git_tidy_core::testutil
      integration_discovery.rs                # Real git repos in tempdirs
      integration_git.rs                      # Root commit handling tests
```
