# git-tidy

## Build and Test

```bash
cargo build --workspace
cargo test --workspace
cargo test --workspace -- --test-threads=1  # if tests interfere with each other
```

## Git Hooks

After cloning, enable the pre-commit hook (runs `cargo fmt --check`):

```bash
git config core.hooksPath .githooks
```

## Architecture

This is a Cargo workspace with a shared core library and seven binary crates.

- **`git-tidy`**: Unified entry point: dispatches `git tidy <alias> [args...]` to sub-tool binaries, and runs a consolidated audit when no alias matches. By default, the audit runs **in-process** using `CachingGitOps` to deduplicate expensive git operations across tools; `--subprocess` reverts to the old behavior of shelling out to each binary.
- **`git-tidy-core`**: Shared library containing git abstraction, classification logic, output helpers, and test utilities.
- **`git-worktree-tidy`**: Binary crate for scanning and cleaning stale git worktrees.
- **`git-branch-tidy`**: Binary crate for scanning and cleaning stale local git branches.
- **`git-stash-tidy`**: Binary crate for scanning and cleaning stale git stashes.
- **`git-remote-tidy`**: Binary crate for scanning and removing stale git remotes and orphaned tracking refs.
- **`git-tag-tidy`**: Binary crate for scanning, classifying, and removing stale git tags.
- **`git-repo-tidy`**: Binary crate for scanning, classifying, and removing stale or orphaned git repos. Most destructive tool in the suite (`rm -rf` on entire repos).
- **`git-config-tidy`**: Binary crate for linting and fixing common git config issues (orphaned branch config, alias shadowing).
- **`git-lfs-tidy`**: Binary crate for scanning repos for LFS health issues and cleaning up orphaned LFS objects.

### Core patterns

- **`GitOps` trait** (`git-tidy-core/src/git.rs`): All git operations go through this trait (`Send + Sync` for thread safety). `RealGit` shells out to `git`; `MockGit` (in `testutil.rs`) returns canned data. `MockGit` uses `Mutex` for call-tracking fields.
- **Output to `&mut dyn Write`**: Enables unit testing output formatters without process capture.
- **Shared output helpers** (`git-tidy-core/src/output.rs`): `write_summary_line`, `write_warnings`, `write_explain_hint`, `format_ahead_behind`, `format_annotations`, `format_landed_ratio`, `repo_display_name`, `write_json_pretty`. All tools call these to avoid duplicating formatting logic.
- **Shared CLI utilities** (`git-tidy-core/src/cli.rs`): `resolve_directory` for resolving optional directory arguments, `SharedCommands` enum (hidden `completions` subcommand flattened into all tools via `#[command(flatten)]`), used by all tools.
- **Shared error handling** (`git-tidy-core/src/error.rs`): `exit_with_error` for consistent error-exit behavior across all tools.
- **Shared type helpers** (`git-tidy-core/src/types.rs`): `extract_landed_fields` for extracting landed ratio/total/unmatched from `Classification` for JSON serialization.
- **`thiserror`** for errors: Known, finite variants with exit code mapping (1=error, 2=dirty-blocked).
- **Parallel fetch** (`git-tidy-core/src/fetch.rs`): `parallel_fetch` runs `git fetch --prune` concurrently across repos using `thread::scope`. Used by worktree-tidy and branch-tidy before classification.
- **Library-first design**: `scan.rs` and `clean.rs` are library functions; `main.rs` is thin dispatch. Enables `git-tidy` to call each tool's scan/lint as a library.
- **`CachingGitOps`** (`git-tidy-core/src/caching.rs`): `GitOps` wrapper that memoizes read-only queries (`fetch_prune`, `symbolic_ref_origin_head`, `rev_parse_verify`, `list_local_branches`, `list_remotes`, `ls_remote_check`, `list_builtin_commands`, `lfs_installed`, `log_grep`, `diff_commit`, `diff_commit_files`, `diff_commit_on_ref`) via `Mutex<HashMap>`. Used by the in-process audit runner to avoid redundant git calls across tools. A `delegate_git_ops!` macro forwards uncached methods to the inner `GitOps`.

### CLI pattern (shared `resolve_directory`, per-crate `clap` definitions)

All tools follow a similar CLI shape:
- **Global args**: `directory` (positional, default cwd), plus tool-specific thresholds
- **Scan subcommand** (default): `--json`, `--porcelain`
- **Clean subcommand**: `--dry-run` / `-n`, classification filters, `--all`, `--json`, `--porcelain`. Clean runs non-interactively; there is no confirmation prompt.
- Worktree-tidy global args: `--behind-threshold` (default 100), `--verbose` / `-v`, `--match` (repeatable substring filter on worktree basenames, OR semantics, case-insensitive)
- Branch-tidy global args: `--behind-threshold` (default 100), `--verbose` / `-v`
- Stash-tidy global arg: `--age-threshold` (default 90) instead of `--behind-threshold`/`--verbose`
- Remote-tidy global arg: `--offline` instead of `--behind-threshold`/`--verbose`
- Tag-tidy global arg: `--offline` instead of `--behind-threshold`/`--verbose`
- Repo-tidy global args: `--stale-months` (default 6), `--offline`
- Tool-specific flags: worktree-tidy has `--delete-branches`, branch-tidy has `--include-remote` (on both scan and clean: discovers remote-only branches and deletes them)/`--force`, remote-tidy has `--force` (allow removing origin)/`--all` (include orphaned), tag-tidy has `--stale-only`/`--local-only`/`--include-remote`/`--force` (bypass release protection)/`--all`, repo-tidy has `--force` (allow deleting dirty repos)/`--stale-only`/`--orphaned-only`/`--all`, lfs-tidy has `--prune` (enable orphaned LFS object removal)
- git-tidy (audit runner + dispatch): Pre-clap alias dispatch in `main.rs` checks `args[1]` against `ToolSpec::aliases` and execs the binary. Falls through to `Audit` subcommand (default) with `--json`/`--porcelain`/`--verbose`/`--tools`/`--subprocess`. `Explain` subcommand prints a terminology glossary. Default mode calls tool scan/lint functions in-process with `CachingGitOps`; `--subprocess` shells out via `ToolRunner` trait.
- Config-tidy uses **lint/fix** subcommands instead of scan/clean (config issues are "lint findings")
- LFS-tidy scan args: `--size-threshold` (default "1MB"), `--depth` (default 1000)

## Conventions

- All modules that call git take `&dyn GitOps` to enable mocking.
- Tests use `tempfile::tempdir()` for isolation with real git repos.
- Path canonicalization in discovery handles macOS `/var` -> `/private/var` symlinks.
- Classification priority order: landed (0) = landed-stale (0) > landed-content (1) > partial (2) > active (3) > local (4). `LandedStale` is for worktrees whose branch ref was deleted (typically after a PR merge).
- Shared test utilities (MockGitBuilder, TestRepo, git helper) live in `git-tidy-core/src/testutil.rs`, gated behind the `testutil` feature. Binary crates depend on `git-tidy-core = { features = ["testutil"] }` in `[dev-dependencies]`.
- `classify_branch` is the core classification function shared by both tools. `classify_worktree` is a thin wrapper that adds dirty detection. `classify_remote_branch` classifies remote-only branches (on origin but no local counterpart), using `origin/<branch>` as the git ref.
- Discovery is inverted between tools: worktree discovery finds `.git` files (linked worktrees), branch discovery finds `.git` directories (repos).
- `delete_fn` pattern: `run_clean` takes `&dyn Fn(&Path) -> io::Result<()>` so tests can verify deletion logic without `rm -rf`. Used in repo-tidy.
- Injectable `du_fn`: `run_scan_with_du` takes a disk-usage function parameter so unit tests can provide canned sizes without hitting the filesystem. Used in repo-tidy.

## Project Layout

```
Cargo.toml                                    # Workspace root
crates/
  git-tidy/                                   # Audit runner + dispatch binary
    src/
      main.rs                                 # Pre-clap dispatch, then CLI audit
      lib.rs                                  # Public module exports
      cli.rs                                  # clap definitions (Audit, Explain subcommands, after_help aliases)
      completions.rs                          # Custom zsh dispatcher completion generator
      dispatch.rs                             # Alias resolution + Unix exec dispatch
      explain.rs                              # Terminology glossary (GlossaryEntry, write_full, write_term)
      types.rs                                # ToolSpec (with aliases), TOOL_SPECS, ToolResult, AuditResult
      runner.rs                               # ToolRunner trait, RealToolRunner, run_audit (subprocess mode)
      inprocess.rs                            # In-process audit: calls tool scan/lint with CachingGitOps
      output.rs                               # Human-readable, JSON, porcelain formatters
    tests/
      integration.rs                          # End-to-end with FakeToolRunner
  git-tidy-core/                              # Shared library
    src/
      lib.rs                                  # Module exports
      cli.rs                                  # Shared CLI utilities (resolve_directory, SharedCommands)
      git.rs                                  # GitOps trait + RealGit implementation
      types.rs                                # Classification, BranchClassification, WorktreeInfo, etc.
      error.rs                                # thiserror Error enum + exit_with_error
      fetch.rs                               # parallel_fetch: concurrent git fetch --prune via thread::scope
      classification.rs                       # classify_branch + classify_worktree
      config.rs                               # Noise pattern configuration (file + CLI + defaults)
      dirty.rs                                # Status parsing with noise filtering
      discovery.rs                            # Repo discovery (shared by all tools)
      landed.rs                               # Subject matching, fuzzy, patch similarity
      output.rs                               # Shared output helpers (summary, warnings, formatting, JSON)
      testutil.rs                             # MockGitBuilder, MockGit, TestRepo, git() helper
      caching.rs                              # CachingGitOps: memoizing GitOps wrapper + delegate macro
  git-worktree-tidy/                          # Worktree scanner/cleaner binary
    src/
      main.rs                                 # CLI dispatch
      lib.rs                                  # Public module exports for integration tests
      cli.rs                                  # clap derive definitions
      scan.rs                                 # Worktree scan logic (library function)
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
  git-stash-tidy/                            # Stash scanner/cleaner binary
    src/
      main.rs                                 # CLI dispatch
      lib.rs                                  # Public module exports for integration tests
      cli.rs                                  # clap derive definitions
      scan.rs                                 # Stash classification and scanning
      output.rs                               # Human-readable, JSON, porcelain formatters
      clean.rs                                # Stash drop logic (descending index order)
      types.rs                                # StashInfo, StashRepoGroup, StashScanResult
    tests/
      common/mod.rs                           # Re-exports from git_tidy_core::testutil
      integration_scan.rs                     # Real git repos with stash scanning
      integration_clean.rs                    # Real git repos with stash cleanup
  git-remote-tidy/                           # Remote scanner/cleaner binary
    src/
      main.rs                                 # CLI dispatch
      lib.rs                                  # Public module exports for integration tests
      cli.rs                                  # clap derive definitions
      scan.rs                                 # Remote classification and scanning
      output.rs                               # Human-readable, JSON, porcelain formatters
      clean.rs                                # Remote removal and ref pruning logic
      types.rs                                # RemoteInfo, RemoteRepoGroup, RemoteScanResult
    tests/
      common/mod.rs                           # Re-exports from git_tidy_core::testutil
      integration_scan.rs                     # Real git repos with remote scanning
      integration_clean.rs                    # Real git repos with remote cleanup
  git-tag-tidy/                              # Tag scanner/cleaner binary
    src/
      main.rs                                 # CLI dispatch
      lib.rs                                  # Public module exports for integration tests
      cli.rs                                  # clap derive definitions
      scan.rs                                 # Tag classification and scanning
      output.rs                               # Human-readable, JSON, porcelain formatters
      clean.rs                                # Tag deletion with safety guards
      types.rs                                # TagInfo, TagRepoGroup, TagScanResult
    tests/
      common/mod.rs                           # Re-exports from git_tidy_core::testutil
      integration_scan.rs                     # Real git repos with tag scanning
      integration_clean.rs                    # Real git repos with tag cleanup
  git-repo-tidy/                             # Repo scanner/cleaner binary
  git-lfs-tidy/                              # LFS health scanner/cleaner binary
    src/
      main.rs                                 # CLI dispatch
      lib.rs                                  # Public module exports for integration tests
      cli.rs                                  # clap derive definitions
      scan.rs                                 # Repo classification and scanning (with injectable du_fn)
      output.rs                               # Human-readable, JSON, porcelain formatters
      clean.rs                                # Repo deletion with delete_fn injection
      types.rs                                # RepoInfo, RepoCounts, RepoScanResult
    tests/
      common/mod.rs                           # Re-exports from git_tidy_core::testutil
      integration_scan.rs                     # Real git repos with repo scanning
      integration_clean.rs                    # Real git repos with repo cleanup
  git-config-tidy/                           # Config linter/fixer binary
    src/
      main.rs                                 # CLI dispatch
      lib.rs                                  # Public module exports for integration tests
      cli.rs                                  # clap derive definitions (lint/fix subcommands)
      lint.rs                                 # Config issue detection (orphaned branch, alias shadow)
      output.rs                               # Human-readable, JSON, porcelain formatters
      fix.rs                                  # Auto-fix logic (config section removal)
      types.rs                                # ConfigIssue, ConfigRepoGroup, ConfigLintResult
    tests/
      common/mod.rs                           # Re-exports from git_tidy_core::testutil
      integration_lint.rs                     # Real git repos with config linting
      integration_fix.rs                      # Real git repos with config fixing
  git-lfs-tidy/                              # LFS health scanner/cleaner binary
    src/
      main.rs                                 # CLI dispatch
      lib.rs                                  # Public module exports for integration tests
      cli.rs                                  # clap derive definitions
      scan.rs                                 # LFS health scanning (find_large_blobs, lfs_ls_files)
      output.rs                               # Human-readable, JSON, porcelain formatters
      clean.rs                                # LFS prune logic with --prune gate
      types.rs                                # LfsInfo, LfsRepoGroup, LfsScanResult, LfsClassification
    tests/
      common/mod.rs                           # Re-exports from git_tidy_core::testutil
      integration_scan.rs                     # Real git repos with LFS scanning
      integration_clean.rs                    # Real git repos with LFS cleanup
```
