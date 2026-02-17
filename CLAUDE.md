# git-tidy

## Build and Test

```bash
cargo build --workspace
cargo test --workspace
cargo test --workspace -- --test-threads=1  # if tests interfere with each other
```

## Architecture

This is a Cargo workspace with a shared core library and two binary crates.

- **`git-tidy-core`**: Shared library containing git abstraction, classification logic, output helpers, and test utilities.
- **`git-worktree-tidy`**: Binary crate for scanning and cleaning stale git worktrees.
- **`git-branch-tidy`**: Binary crate for scanning and cleaning stale local git branches.

### Core patterns

- **`GitOps` trait** (`git-tidy-core/src/git.rs`): All git operations go through this trait. `RealGit` shells out to `git`; `MockGit` (in `testutil.rs`) returns canned data.
- **Output to `&mut dyn Write`**: Enables unit testing output formatters without process capture.
- **Shared output helpers** (`git-tidy-core/src/output.rs`): `write_summary_line`, `write_warnings`, `format_ahead_behind`, `format_annotations`, `format_landed_ratio`. Both tools call these to avoid duplicating formatting logic.
- **`thiserror`** for errors: Known, finite variants with exit code mapping (1=error, 2=dirty-blocked).
- **Sequential repo processing**: No parallelism needed for typical workloads (~9 repos, ~39 worktrees).
- **Library-first design**: `scan.rs` and `clean.rs` are library functions; `main.rs` is thin dispatch. Enables future tools to call scan as a library.

### CLI pattern (documented convention, not shared code)

Both tools follow the same CLI shape:
- **Global args**: `directory` (positional, default cwd), `--behind-threshold` (default 100), `--verbose` / `-v`
- **Scan subcommand** (default): `--json`, `--porcelain`
- **Clean subcommand**: `--dry-run` / `-n`, `--force` / `-f`, `--yes` / `-y`, `--merged-only`, `--landed`, `--all`, `--json`, `--porcelain`
- Tool-specific flags: worktree-tidy has `--delete-branches`, branch-tidy has `--include-remote`

## Conventions

- All modules that call git take `&dyn GitOps` to enable mocking.
- Tests use `tempfile::tempdir()` for isolation with real git repos.
- Path canonicalization in discovery handles macOS `/var` -> `/private/var` symlinks.
- Classification priority order: merged (0) > landed (1) > partial (2) > active (3) > local (4).
- Shared test utilities (MockGitBuilder, TestRepo, git helper) live in `git-tidy-core/src/testutil.rs`, gated behind the `testutil` feature. Binary crates depend on `git-tidy-core = { features = ["testutil"] }` in `[dev-dependencies]`.
- `classify_branch` is the core classification function shared by both tools. `classify_worktree` is a thin wrapper that adds dirty detection.
- Discovery is inverted between tools: worktree discovery finds `.git` files (linked worktrees), branch discovery finds `.git` directories (repos).

## Project Layout

```
Cargo.toml                                    # Workspace root
crates/
  git-tidy-core/                              # Shared library
    src/
      lib.rs                                  # Module exports
      git.rs                                  # GitOps trait + RealGit implementation
      types.rs                                # Classification, BranchClassification, WorktreeInfo, etc.
      error.rs                                # thiserror Error enum
      classification.rs                       # classify_branch + classify_worktree
      config.rs                               # Noise pattern configuration (file + CLI + defaults)
      dirty.rs                                # Status parsing with noise filtering
      discovery.rs                            # Repo discovery (shared by all tools)
      landed.rs                               # Subject matching, fuzzy, patch similarity
      output.rs                               # Shared output helpers (summary, warnings, formatting)
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
      integration_git.rs                      # GitOps integration tests
  git-branch-tidy/                            # Branch scanner/cleaner binary
    src/
      main.rs                                 # CLI dispatch
      lib.rs                                  # Public module exports for integration tests
      cli.rs                                  # clap derive definitions
      discovery.rs                            # Re-exports git_tidy_core::discovery::discover_repos
      scan.rs                                 # Branch enumeration and classification
      output.rs                               # Human-readable, JSON, porcelain formatters
      clean.rs                                # Branch deletion with safety guards
      types.rs                                # BranchInfo, BranchRepoGroup, BranchScanResult
    tests/
      common/mod.rs                           # Re-exports from git_tidy_core::testutil
      integration_scan.rs                     # Real git repos with branch scanning
      integration_clean.rs                    # Real git repos with branch cleanup
```
