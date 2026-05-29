use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{Duration, Instant};

use crate::error::Error;

/// Result type alias for git operations.
pub type GitResult<T> = Result<T, Error>;

/// Abstraction over git CLI operations for testability.
pub trait GitOps: Send + Sync {
    /// Run `git fetch --prune --quiet` on the repo.
    fn fetch_prune(&self, repo: &Path) -> GitResult<()>;

    /// Get the symbolic ref for `refs/remotes/origin/HEAD`.
    /// Returns `Some("main")` if it points to `refs/remotes/origin/main`.
    fn symbolic_ref_origin_head(&self, repo: &Path) -> GitResult<Option<String>>;

    /// Check if a ref exists (exit code 0 = exists).
    fn rev_parse_verify(&self, repo: &Path, refspec: &str) -> GitResult<bool>;

    /// Check if `branch` is an ancestor of `target`.
    fn is_ancestor(&self, repo: &Path, branch: &str, target: &str) -> GitResult<bool>;

    /// Get ahead/behind counts: returns `(behind, ahead)`.
    fn rev_list_left_right_count(
        &self,
        repo: &Path,
        left: &str,
        right: &str,
    ) -> GitResult<(usize, usize)>;

    /// List commits unique to `branch` relative to `base`.
    /// Returns `Vec<(full_hash, subject)>`.
    fn log_exclusive(
        &self,
        repo: &Path,
        base: &str,
        branch: &str,
    ) -> GitResult<Vec<(String, String)>>;

    /// Search for commits on `branch_or_ref` whose subject contains `needle`.
    /// Returns matching `(short_hash, subject)` pairs.
    fn log_grep(
        &self,
        repo: &Path,
        branch_or_ref: &str,
        needle: &str,
    ) -> GitResult<Vec<(String, String)>>;

    /// Get the diff for a single commit. Returns the diff text.
    fn diff_commit(&self, repo: &Path, commit: &str) -> GitResult<String>;

    /// Get the list of files changed by a single commit.
    fn diff_commit_files(&self, repo: &Path, commit: &str) -> GitResult<Vec<String>>;

    /// Find commits on `ref_spec` that touch any of the given files.
    /// Returns `(short_hash, subject)` pairs.
    fn log_touching_files(
        &self,
        repo: &Path,
        ref_spec: &str,
        files: &[String],
    ) -> GitResult<Vec<(String, String)>>;

    /// Get the diff for a commit on the default branch (for patch comparison).
    fn diff_commit_on_ref(&self, repo: &Path, commit_hash: &str) -> GitResult<String>;

    /// List files that differ between the working tree and a ref.
    fn diff_working_tree_files(
        &self,
        worktree_path: &Path,
        ref_spec: &str,
    ) -> GitResult<Vec<String>>;

    /// Run `git -C <path> status --porcelain`. Returns raw lines.
    fn status_porcelain(&self, worktree_path: &Path) -> GitResult<Vec<String>>;

    /// Get the branch checked out in a worktree. Returns None for detached HEAD.
    fn worktree_branch(&self, worktree_path: &Path) -> GitResult<Option<String>>;

    /// Get the HEAD commit hash for a worktree or branch.
    fn rev_parse(&self, repo: &Path, refspec: &str) -> GitResult<String>;

    /// Remove a worktree: `git worktree remove <path>`.
    fn worktree_remove(&self, repo: &Path, worktree_path: &Path) -> GitResult<()>;

    /// Force-remove a worktree: `git worktree remove --force <path>`.
    fn worktree_remove_force(&self, repo: &Path, worktree_path: &Path) -> GitResult<()>;

    /// Prune worktree metadata: `git worktree prune`.
    fn worktree_prune(&self, repo: &Path) -> GitResult<()>;

    /// List all linked worktrees for a repo via `git worktree list --porcelain`.
    /// Returns `(path, branch)` pairs. Excludes the main worktree.
    fn worktree_list(&self, repo: &Path) -> GitResult<Vec<(PathBuf, Option<String>)>>;

    /// Delete a local branch: `git branch -D <branch>`.
    fn branch_delete(&self, repo: &Path, branch: &str) -> GitResult<()>;

    /// Check if a branch is checked out in any worktree of the repo.
    fn is_branch_checked_out(&self, repo: &Path, branch: &str) -> GitResult<bool>;

    /// List all local branches in a repo.
    fn list_local_branches(&self, repo: &Path) -> GitResult<Vec<String>>;

    /// Delete a local branch safely: `git branch -d <branch>` (refuses unmerged).
    fn branch_delete_safe(&self, repo: &Path, branch: &str) -> GitResult<()>;

    /// Get the currently checked-out branch. Returns None for detached HEAD.
    fn current_branch(&self, repo: &Path) -> GitResult<Option<String>>;

    /// Get the upstream (tracking) branch for a local branch.
    /// Returns the full upstream ref (e.g. "origin/feature-x") or None.
    fn upstream_branch(&self, repo: &Path, branch: &str) -> GitResult<Option<String>>;

    /// Delete a remote branch: `git push <remote> --delete <branch>`.
    fn delete_remote_branch(&self, repo: &Path, remote: &str, branch: &str) -> GitResult<()>;

    /// Check the history of a file on a ref. Returns commits touching the file.
    fn log_file_history(
        &self,
        repo: &Path,
        ref_spec: &str,
        file: &str,
    ) -> GitResult<Vec<(String, String)>>;

    // --- Remote operations ---

    /// List configured remotes in a repo.
    fn list_remotes(&self, repo: &Path) -> GitResult<Vec<String>>;

    /// Get the URL for a configured remote.
    fn remote_url(&self, repo: &Path, remote: &str) -> GitResult<String>;

    /// Check if a remote is reachable via `ls-remote` with a timeout.
    /// Returns `true` if the remote responds, `false` if unreachable or timed out.
    fn ls_remote_check(&self, repo: &Path, remote: &str) -> GitResult<bool>;

    /// Remove a configured remote: `git remote remove <remote>`.
    fn remote_remove(&self, repo: &Path, remote: &str) -> GitResult<()>;

    /// List all remote tracking refs under `refs/remotes/`.
    /// Returns `(short_name, full_refname)` pairs.
    fn list_remote_tracking_refs(&self, repo: &Path) -> GitResult<Vec<(String, String)>>;

    /// Prune tracking refs belonging to a specific remote name.
    /// Returns the number of refs successfully deleted.
    fn prune_remote_refs(&self, repo: &Path, remote: &str) -> GitResult<usize>;

    // --- Stash operations ---

    /// List stashes in a repo.
    /// Returns `Vec<(stash_ref, message, iso_date)>`, e.g. `("stash@{0}", "WIP on main: abc Fix", "2024-01-15T...")`.
    fn list_stashes(&self, repo: &Path) -> GitResult<Vec<(String, String, String)>>;

    /// Get the diff for a stash entry.
    fn stash_diff(&self, repo: &Path, stash_ref: &str) -> GitResult<String>;

    /// Drop a stash entry.
    fn stash_drop(&self, repo: &Path, stash_ref: &str) -> GitResult<()>;

    // --- Tag operations ---

    /// List all local tags in a repo.
    fn list_local_tags(&self, repo: &Path) -> GitResult<Vec<String>>;

    /// List tags on a remote.
    /// Returns `Vec<(tag_name, commit_sha)>`.
    /// For annotated tags, prefers the dereferenced (`^{}`) commit SHA.
    fn list_remote_tags(&self, repo: &Path, remote: &str) -> GitResult<Vec<(String, String)>>;

    /// Get the commit SHA a tag points at (dereferencing annotated tags).
    fn tag_commit(&self, repo: &Path, tag: &str) -> GitResult<String>;

    /// Check if a commit is reachable from any branch.
    fn is_commit_reachable(&self, repo: &Path, commit: &str) -> GitResult<bool>;

    /// Delete a local tag.
    fn tag_delete(&self, repo: &Path, tag: &str) -> GitResult<()>;

    /// Delete a tag on a remote.
    fn tag_delete_remote(&self, repo: &Path, remote: &str, tag: &str) -> GitResult<()>;

    /// Check if a tag is annotated (vs lightweight).
    fn is_tag_annotated(&self, repo: &Path, tag: &str) -> GitResult<bool>;

    /// Get the tagger/creator date for a tag (ISO 8601).
    fn tag_date(&self, repo: &Path, tag: &str) -> GitResult<Option<String>>;

    // --- Repo-level operations ---

    /// Get the author date of the most recent commit.
    /// Returns an ISO 8601 date string, or `None` for empty repos.
    fn last_commit_date(&self, repo: &Path) -> GitResult<Option<String>>;
    // --- Config operations ---

    /// List all local config key=value pairs.
    fn config_list_local(&self, repo: &Path) -> GitResult<Vec<(String, String)>>;

    /// Remove an entire config section (e.g., `branch.foo`).
    fn config_remove_section(&self, repo: &Path, section: &str) -> GitResult<()>;

    /// List built-in git command names (global, not repo-specific).
    fn list_builtin_commands(&self) -> GitResult<Vec<String>>;

    // --- LFS operations ---

    /// Check if git-lfs is installed.
    fn lfs_installed(&self) -> GitResult<bool>;

    /// List LFS tracked files.
    /// Returns `(oid, status, path)` where status is `*` (present) or `-` (missing).
    fn lfs_ls_files(&self, repo: &Path) -> GitResult<Vec<(String, char, String)>>;

    /// Get current LFS tracking patterns from `.gitattributes`.
    fn lfs_track_patterns(&self, repo: &Path) -> GitResult<Vec<String>>;

    /// Dry-run LFS prune: returns `(count, bytes)` of prunable objects.
    fn lfs_prune_dry_run(&self, repo: &Path) -> GitResult<(usize, u64)>;

    /// Remove orphaned LFS objects.
    fn lfs_prune(&self, repo: &Path) -> GitResult<()>;

    /// Find blobs above `threshold` bytes across up to `depth` branch/tag tip trees.
    /// Returns `(hash, size, path)` tuples.
    fn find_large_blobs(
        &self,
        repo: &Path,
        threshold: u64,
        depth: usize,
    ) -> GitResult<Vec<(String, u64, String)>>;
}

/// Parse `git worktree list --porcelain` output into `(path, branch)` pairs.
///
/// Skips the first (main) worktree and any prunable entries (worktrees whose
/// directories have been deleted but whose metadata hasn't been pruned).
fn parse_worktree_list_porcelain(text: &str) -> Vec<(PathBuf, Option<String>)> {
    let mut result = Vec::new();
    let mut current_path: Option<PathBuf> = None;
    let mut current_branch: Option<String> = None;
    let mut is_prunable = false;
    let mut is_first = true;

    for line in text.lines().chain(std::iter::once("")) {
        if line.is_empty() {
            // Block boundary: emit previous entry (skip first/main and prunable)
            if let Some(path) = current_path.take() {
                if !is_first && !is_prunable {
                    result.push((path, current_branch.take()));
                } else {
                    is_first = false;
                }
            }
            current_branch = None;
            is_prunable = false;
            continue;
        }
        if let Some(path_str) = line.strip_prefix("worktree ") {
            current_path = Some(PathBuf::from(path_str));
        } else if let Some(branch_ref) = line.strip_prefix("branch ") {
            current_branch = branch_ref
                .strip_prefix("refs/heads/")
                .map(|s| s.to_string());
        } else if line.starts_with("prunable ") {
            is_prunable = true;
        }
    }

    result
}

/// Real git implementation that shells out to the `git` binary.
pub struct RealGit;

impl RealGit {
    fn run(repo: &Path, args: &[&str]) -> GitResult<std::process::Output> {
        let output = Command::new("git")
            .arg("-C")
            .arg(repo)
            .args(args)
            .output()
            .map_err(|e| Error::GitCommand {
                command: format!("git -C {} {}", repo.display(), args.join(" ")),
                message: e.to_string(),
            })?;
        Ok(output)
    }

    fn run_success(repo: &Path, args: &[&str]) -> GitResult<String> {
        let output = Self::run(repo, args)?;
        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
            return Err(Error::GitCommand {
                command: format!("git -C {} {}", repo.display(), args.join(" ")),
                message: stderr,
            });
        }
        Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
    }

    /// Run a git command with a timeout. Returns `None` if timed out.
    fn run_with_timeout(
        repo: &Path,
        args: &[&str],
        timeout: Duration,
    ) -> GitResult<Option<std::process::Output>> {
        let mut child = Command::new("git")
            .arg("-C")
            .arg(repo)
            .args(args)
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .spawn()
            .map_err(|e| Error::GitCommand {
                command: format!("git -C {} {}", repo.display(), args.join(" ")),
                message: e.to_string(),
            })?;

        let start = Instant::now();
        loop {
            match child.try_wait() {
                Ok(Some(_status)) => {
                    let output = child.wait_with_output().map_err(|e| Error::GitCommand {
                        command: format!("git -C {} {}", repo.display(), args.join(" ")),
                        message: e.to_string(),
                    })?;
                    return Ok(Some(output));
                }
                Ok(None) => {
                    if start.elapsed() >= timeout {
                        let _ = child.kill();
                        let _ = child.wait();
                        return Ok(None);
                    }
                    std::thread::sleep(Duration::from_millis(100));
                }
                Err(e) => {
                    return Err(Error::GitCommand {
                        command: format!("git -C {} {}", repo.display(), args.join(" ")),
                        message: e.to_string(),
                    });
                }
            }
        }
    }

    /// Run a git command without `-C <repo>` (for global queries like `git lfs version`).
    fn run_global(args: &[&str]) -> GitResult<std::process::Output> {
        let output = Command::new("git")
            .args(args)
            .output()
            .map_err(|e| Error::GitCommand {
                command: format!("git {}", args.join(" ")),
                message: e.to_string(),
            })?;
        Ok(output)
    }

    /// Run a global git command and return trimmed stdout as a String.
    /// Fails if the command exits with a non-zero status.
    fn run_global_text(args: &[&str]) -> GitResult<String> {
        let output = Self::run_global(args)?;
        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
            return Err(Error::GitCommand {
                command: format!("git {}", args.join(" ")),
                message: stderr,
            });
        }
        Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
    }

    fn parse_log_oneline(output: &str) -> Vec<(String, String)> {
        output
            .lines()
            .filter(|l| !l.is_empty())
            .map(|line| match line.split_once(' ') {
                Some((hash, subject)) => (hash.to_string(), subject.to_string()),
                // A commit with an empty subject still has a hash on its own line.
                // `split_once(' ')` returns None for it, which previously dropped the
                // commit from the result and skewed the `total` count in
                // LandedByContent { matched, total } — flipping the decision in
                // boundary cases. Treat the whole line as the hash with an empty subject.
                None => (line.to_string(), String::new()),
            })
            .collect()
    }
}

impl GitOps for RealGit {
    fn fetch_prune(&self, repo: &Path) -> GitResult<()> {
        Self::run_success(repo, &["fetch", "--prune", "--quiet"])?;
        Ok(())
    }

    fn symbolic_ref_origin_head(&self, repo: &Path) -> GitResult<Option<String>> {
        let output = Self::run(repo, &["symbolic-ref", "refs/remotes/origin/HEAD"])?;
        if !output.status.success() {
            return Ok(None);
        }
        let refname = String::from_utf8_lossy(&output.stdout).trim().to_string();
        // "refs/remotes/origin/main" -> "main"
        let branch = refname
            .strip_prefix("refs/remotes/origin/")
            .unwrap_or(&refname)
            .to_string();
        Ok(Some(branch))
    }

    fn rev_parse_verify(&self, repo: &Path, refspec: &str) -> GitResult<bool> {
        let output = Self::run(repo, &["rev-parse", "--verify", refspec])?;
        Ok(output.status.success())
    }

    fn is_ancestor(&self, repo: &Path, branch: &str, target: &str) -> GitResult<bool> {
        let output = Self::run(repo, &["merge-base", "--is-ancestor", branch, target])?;
        Ok(output.status.success())
    }

    fn rev_list_left_right_count(
        &self,
        repo: &Path,
        left: &str,
        right: &str,
    ) -> GitResult<(usize, usize)> {
        let range = format!("{left}...{right}");
        let text = Self::run_success(repo, &["rev-list", "--left-right", "--count", &range])?;
        let parts: Vec<&str> = text.split('\t').collect();
        if parts.len() != 2 {
            return Err(Error::GitCommand {
                command: format!("rev-list --left-right --count {range}"),
                message: format!("unexpected output: {text}"),
            });
        }
        let left_count: usize = parts[0].parse().unwrap_or(0);
        let right_count: usize = parts[1].parse().unwrap_or(0);
        Ok((left_count, right_count))
    }

    fn log_exclusive(
        &self,
        repo: &Path,
        base: &str,
        branch: &str,
    ) -> GitResult<Vec<(String, String)>> {
        let range = format!("{base}..{branch}");
        let text = Self::run_success(repo, &["log", &range, "--format=%H %s"])?;
        Ok(Self::parse_log_oneline(&text))
    }

    fn log_grep(
        &self,
        repo: &Path,
        branch_or_ref: &str,
        needle: &str,
    ) -> GitResult<Vec<(String, String)>> {
        let output = Self::run(repo, &["log", branch_or_ref, "--oneline", "--grep", needle])?;
        let text = String::from_utf8_lossy(&output.stdout).trim().to_string();
        Ok(Self::parse_log_oneline(&text))
    }

    fn diff_commit(&self, repo: &Path, commit: &str) -> GitResult<String> {
        Self::run_success(repo, &["diff-tree", "-p", "--root", commit])
    }

    fn diff_commit_files(&self, repo: &Path, commit: &str) -> GitResult<Vec<String>> {
        let text = Self::run_success(
            repo,
            &[
                "diff-tree",
                "--root",
                "--no-commit-id",
                "-r",
                "--name-only",
                commit,
            ],
        )?;
        Ok(text.lines().map(|l| l.to_string()).collect())
    }

    fn log_touching_files(
        &self,
        repo: &Path,
        ref_spec: &str,
        files: &[String],
    ) -> GitResult<Vec<(String, String)>> {
        let mut args = vec!["log", ref_spec, "--oneline", "-20", "--"];
        let file_refs: Vec<&str> = files.iter().map(|s| s.as_str()).collect();
        args.extend(file_refs);
        let output = Self::run(repo, &args)?;
        let text = String::from_utf8_lossy(&output.stdout).trim().to_string();
        Ok(Self::parse_log_oneline(&text))
    }

    fn diff_commit_on_ref(&self, repo: &Path, commit_hash: &str) -> GitResult<String> {
        Self::run_success(repo, &["diff-tree", "-p", "--root", commit_hash])
    }

    fn diff_working_tree_files(
        &self,
        worktree_path: &Path,
        ref_spec: &str,
    ) -> GitResult<Vec<String>> {
        let text = Self::run_success(worktree_path, &["diff", "--name-only", ref_spec])?;
        Ok(text
            .lines()
            .filter(|l| !l.is_empty())
            .map(|l| l.to_string())
            .collect())
    }

    fn status_porcelain(&self, worktree_path: &Path) -> GitResult<Vec<String>> {
        let text = Self::run_success(worktree_path, &["status", "--porcelain"])?;
        Ok(text.lines().map(|l| l.to_string()).collect())
    }

    fn worktree_branch(&self, worktree_path: &Path) -> GitResult<Option<String>> {
        let output = Self::run(worktree_path, &["symbolic-ref", "--short", "HEAD"])?;
        if !output.status.success() {
            return Ok(None); // detached HEAD
        }
        let branch = String::from_utf8_lossy(&output.stdout).trim().to_string();
        if branch.is_empty() {
            Ok(None)
        } else {
            Ok(Some(branch))
        }
    }

    fn rev_parse(&self, repo: &Path, refspec: &str) -> GitResult<String> {
        Self::run_success(repo, &["rev-parse", refspec])
    }

    fn worktree_remove(&self, repo: &Path, worktree_path: &Path) -> GitResult<()> {
        Self::run_success(
            repo,
            &["worktree", "remove", &worktree_path.to_string_lossy()],
        )?;
        Ok(())
    }

    fn worktree_remove_force(&self, repo: &Path, worktree_path: &Path) -> GitResult<()> {
        Self::run_success(
            repo,
            &[
                "worktree",
                "remove",
                "--force",
                &worktree_path.to_string_lossy(),
            ],
        )?;
        Ok(())
    }

    fn worktree_prune(&self, repo: &Path) -> GitResult<()> {
        Self::run_success(repo, &["worktree", "prune"])?;
        Ok(())
    }

    fn worktree_list(&self, repo: &Path) -> GitResult<Vec<(PathBuf, Option<String>)>> {
        let text = Self::run_success(repo, &["worktree", "list", "--porcelain"])?;
        Ok(parse_worktree_list_porcelain(&text))
    }

    fn branch_delete(&self, repo: &Path, branch: &str) -> GitResult<()> {
        Self::run_success(repo, &["branch", "-D", branch])?;
        Ok(())
    }

    fn list_local_branches(&self, repo: &Path) -> GitResult<Vec<String>> {
        let text = Self::run_success(repo, &["branch", "--format=%(refname:short)"])?;
        Ok(text
            .lines()
            .filter(|l| !l.is_empty())
            .map(|l| l.to_string())
            .collect())
    }

    fn branch_delete_safe(&self, repo: &Path, branch: &str) -> GitResult<()> {
        Self::run_success(repo, &["branch", "-d", branch])?;
        Ok(())
    }

    fn current_branch(&self, repo: &Path) -> GitResult<Option<String>> {
        let output = Self::run(repo, &["symbolic-ref", "--short", "HEAD"])?;
        if !output.status.success() {
            return Ok(None); // detached HEAD
        }
        let branch = String::from_utf8_lossy(&output.stdout).trim().to_string();
        if branch.is_empty() {
            Ok(None)
        } else {
            Ok(Some(branch))
        }
    }

    fn upstream_branch(&self, repo: &Path, branch: &str) -> GitResult<Option<String>> {
        let output = Self::run(
            repo,
            &[
                "rev-parse",
                "--abbrev-ref",
                &format!("{branch}@{{upstream}}"),
            ],
        )?;
        if !output.status.success() {
            return Ok(None);
        }
        let upstream = String::from_utf8_lossy(&output.stdout).trim().to_string();
        if upstream.is_empty() {
            Ok(None)
        } else {
            Ok(Some(upstream))
        }
    }

    fn delete_remote_branch(&self, repo: &Path, remote: &str, branch: &str) -> GitResult<()> {
        Self::run_success(repo, &["push", remote, "--delete", branch])?;
        Ok(())
    }

    fn is_branch_checked_out(&self, repo: &Path, branch: &str) -> GitResult<bool> {
        let output = Self::run_success(repo, &["worktree", "list", "--porcelain"])?;
        for line in output.lines() {
            if line.strip_prefix("branch refs/heads/") == Some(branch) {
                return Ok(true);
            }
        }
        Ok(false)
    }

    fn log_file_history(
        &self,
        repo: &Path,
        ref_spec: &str,
        file: &str,
    ) -> GitResult<Vec<(String, String)>> {
        let output = Self::run(repo, &["log", ref_spec, "--oneline", "--all", "--", file])?;
        let text = String::from_utf8_lossy(&output.stdout).trim().to_string();
        Ok(Self::parse_log_oneline(&text))
    }

    // --- Remote operations ---

    fn list_remotes(&self, repo: &Path) -> GitResult<Vec<String>> {
        let text = Self::run_success(repo, &["remote"])?;
        Ok(text
            .lines()
            .filter(|l| !l.is_empty())
            .map(|l| l.to_string())
            .collect())
    }

    fn remote_url(&self, repo: &Path, remote: &str) -> GitResult<String> {
        Self::run_success(repo, &["remote", "get-url", remote])
    }

    fn ls_remote_check(&self, repo: &Path, remote: &str) -> GitResult<bool> {
        let timeout = Duration::from_secs(10);
        match Self::run_with_timeout(
            repo,
            &["ls-remote", "--exit-code", "--heads", remote],
            timeout,
        )? {
            Some(output) => Ok(output.status.success()),
            None => Ok(false), // timed out
        }
    }

    fn remote_remove(&self, repo: &Path, remote: &str) -> GitResult<()> {
        Self::run_success(repo, &["remote", "remove", remote]).map_err(|_| {
            Error::RemoteRemovalFailed {
                repo: repo.to_path_buf(),
                remote: remote.to_string(),
                reason: "git remote remove failed".to_string(),
            }
        })?;
        Ok(())
    }

    fn list_remote_tracking_refs(&self, repo: &Path) -> GitResult<Vec<(String, String)>> {
        let output = Self::run(
            repo,
            &[
                "for-each-ref",
                "--format=%(refname:short)%00%(refname)",
                "refs/remotes/",
            ],
        )?;
        let text = String::from_utf8_lossy(&output.stdout);
        let mut result = Vec::new();
        for line in text.lines() {
            if line.is_empty() {
                continue;
            }
            if let Some((short, full)) = line.split_once('\0') {
                result.push((short.to_string(), full.to_string()));
            }
        }
        Ok(result)
    }

    fn prune_remote_refs(&self, repo: &Path, remote: &str) -> GitResult<usize> {
        let refs = self.list_remote_tracking_refs(repo)?;
        let prefix = format!("{remote}/");
        let mut deleted = 0;
        for (short, full) in &refs {
            if (short.starts_with(&prefix) || short == remote)
                && Self::run_success(repo, &["update-ref", "-d", full]).is_ok()
            {
                deleted += 1;
            }
        }
        Ok(deleted)
    }

    // --- Stash operations ---

    fn list_stashes(&self, repo: &Path) -> GitResult<Vec<(String, String, String)>> {
        let output = Self::run(repo, &["stash", "list", "--format=%gd%x00%gs%x00%aI"])?;
        let text = String::from_utf8_lossy(&output.stdout);
        let mut result = Vec::new();
        for line in text.lines() {
            if line.is_empty() {
                continue;
            }
            let parts: Vec<&str> = line.splitn(3, '\0').collect();
            if parts.len() == 3 {
                result.push((
                    parts[0].to_string(),
                    parts[1].to_string(),
                    parts[2].to_string(),
                ));
            }
        }
        Ok(result)
    }

    fn stash_diff(&self, repo: &Path, stash_ref: &str) -> GitResult<String> {
        let output = Self::run(repo, &["stash", "show", "-p", stash_ref])?;
        Ok(String::from_utf8_lossy(&output.stdout).to_string())
    }

    fn stash_drop(&self, repo: &Path, stash_ref: &str) -> GitResult<()> {
        Self::run_success(repo, &["stash", "drop", stash_ref]).map_err(|_| {
            Error::StashDropFailed {
                repo: repo.to_path_buf(),
                stash_ref: stash_ref.to_string(),
                reason: "git stash drop failed".to_string(),
            }
        })?;
        Ok(())
    }

    // --- Tag operations ---

    fn list_local_tags(&self, repo: &Path) -> GitResult<Vec<String>> {
        let text = Self::run_success(repo, &["tag", "-l"])?;
        Ok(text
            .lines()
            .filter(|l| !l.is_empty())
            .map(|l| l.to_string())
            .collect())
    }

    fn list_remote_tags(&self, repo: &Path, remote: &str) -> GitResult<Vec<(String, String)>> {
        let output = Self::run(repo, &["ls-remote", "--tags", remote])?;
        let text = String::from_utf8_lossy(&output.stdout);
        let mut map: std::collections::HashMap<String, String> = std::collections::HashMap::new();
        for line in text.lines() {
            if line.is_empty() {
                continue;
            }
            let Some((sha, refname)) = line.split_once('\t') else {
                continue;
            };
            let refname = refname.trim();
            let Some(tag_ref) = refname.strip_prefix("refs/tags/") else {
                continue;
            };
            if let Some(tag_name) = tag_ref.strip_suffix("^{}") {
                // Dereferenced annotated tag -- overwrite entry
                map.insert(tag_name.to_string(), sha.to_string());
            } else {
                // Only insert if we haven't seen a ^{} entry yet
                map.entry(tag_ref.to_string())
                    .or_insert_with(|| sha.to_string());
            }
        }
        Ok(map.into_iter().collect())
    }

    fn tag_commit(&self, repo: &Path, tag: &str) -> GitResult<String> {
        let refspec = format!("{tag}^{{commit}}");
        Self::run_success(repo, &["rev-parse", &refspec])
    }

    fn is_commit_reachable(&self, repo: &Path, commit: &str) -> GitResult<bool> {
        // `-a` widens the search to include remote-tracking branches. Without it a commit reachable only from origin/feature (after the local branch was deleted, a common state for merged PRs that left a tag behind) is reported as unreachable, and git-tag-tidy classifies the tag as Stale → eligible for default deletion.
        let output = Self::run(repo, &["branch", "-a", "--contains", commit])?;
        let text = String::from_utf8_lossy(&output.stdout);
        Ok(!text.trim().is_empty())
    }

    fn tag_delete(&self, repo: &Path, tag: &str) -> GitResult<()> {
        Self::run_success(repo, &["tag", "-d", tag]).map_err(|_| Error::TagDeletionFailed {
            repo: repo.to_path_buf(),
            tag: tag.to_string(),
            reason: "git tag -d failed".to_string(),
        })?;
        Ok(())
    }

    fn tag_delete_remote(&self, repo: &Path, remote: &str, tag: &str) -> GitResult<()> {
        let refspec = format!("refs/tags/{tag}");
        Self::run_success(repo, &["push", remote, "--delete", &refspec]).map_err(|_| {
            Error::TagDeletionFailed {
                repo: repo.to_path_buf(),
                tag: tag.to_string(),
                reason: format!("git push {remote} --delete refs/tags/{tag} failed"),
            }
        })?;
        Ok(())
    }

    fn is_tag_annotated(&self, repo: &Path, tag: &str) -> GitResult<bool> {
        let text = Self::run_success(repo, &["cat-file", "-t", tag])?;
        Ok(text.trim() == "tag")
    }

    fn tag_date(&self, repo: &Path, tag: &str) -> GitResult<Option<String>> {
        let refspec = format!("refs/tags/{tag}");
        let text = Self::run_success(
            repo,
            &[
                "for-each-ref",
                "--format=%(creatordate:iso-strict)",
                &refspec,
            ],
        )?;
        let trimmed = text.trim();
        if trimmed.is_empty() {
            Ok(None)
        } else {
            Ok(Some(trimmed.to_string()))
        }
    }

    // --- Repo-level operations ---

    fn last_commit_date(&self, repo: &Path) -> GitResult<Option<String>> {
        let output = Self::run(repo, &["log", "-1", "--format=%aI"])?;
        let text = String::from_utf8_lossy(&output.stdout).trim().to_string();
        if text.is_empty() {
            Ok(None)
        } else {
            Ok(Some(text))
        }
    }
    // --- Config operations ---

    fn config_list_local(&self, repo: &Path) -> GitResult<Vec<(String, String)>> {
        let output = Self::run(repo, &["config", "--list", "--local"])?;
        let text = String::from_utf8_lossy(&output.stdout);
        let mut result = Vec::new();
        for line in text.lines() {
            if line.is_empty() {
                continue;
            }
            // Split on first `=` only, since values can contain `=`
            if let Some((key, value)) = line.split_once('=') {
                result.push((key.to_string(), value.to_string()));
            }
        }
        Ok(result)
    }

    // --- LFS operations ---

    fn lfs_installed(&self) -> GitResult<bool> {
        let output = Self::run_global(&["lfs", "version"])?;
        Ok(output.status.success())
    }

    fn lfs_ls_files(&self, repo: &Path) -> GitResult<Vec<(String, char, String)>> {
        let output = Self::run(repo, &["lfs", "ls-files", "--long"])?;
        let text = String::from_utf8_lossy(&output.stdout);
        let mut result = Vec::new();
        for line in text.lines() {
            if line.is_empty() {
                continue;
            }
            // Format: "<oid> <status> <path>"
            // e.g. "abc123def456... * large-file.bin"
            // e.g. "abc123def456... - missing-file.bin"
            if let Some((oid, rest)) = line.split_once(' ')
                && let Some((status_str, path)) = rest.split_once(' ')
            {
                let status = status_str.chars().next().unwrap_or('*');
                result.push((oid.to_string(), status, path.to_string()));
            }
        }
        Ok(result)
    }

    fn config_remove_section(&self, repo: &Path, section: &str) -> GitResult<()> {
        Self::run_success(repo, &["config", "--remove-section", section]).map_err(|_| {
            Error::ConfigRemoveSectionFailed {
                repo: repo.to_path_buf(),
                section: section.to_string(),
                reason: "git config --remove-section failed".to_string(),
            }
        })?;
        Ok(())
    }

    fn lfs_track_patterns(&self, repo: &Path) -> GitResult<Vec<String>> {
        let output = Self::run(repo, &["lfs", "track"])?;
        let text = String::from_utf8_lossy(&output.stdout);
        let mut patterns = Vec::new();
        for line in text.lines() {
            // Lines look like: "    *.bin (.gitattributes)"
            let trimmed = line.trim();
            if let Some(pattern) = trimmed.strip_suffix(')')
                && let Some((pat, _attr_file)) = pattern.rsplit_once(" (")
            {
                patterns.push(pat.to_string());
            }
        }
        Ok(patterns)
    }

    fn lfs_prune_dry_run(&self, repo: &Path) -> GitResult<(usize, u64)> {
        let output = Self::run(repo, &["lfs", "prune", "--dry-run"])?;
        let text = String::from_utf8_lossy(&output.stdout);
        // Parse output like: "4 local objects, 1 retained\npruning 2 files, (1.2 MB)"
        // or: "✔ 3 local objects, 1 retained, done.\n✔ Pruning 2 files, (1.2 MB)"
        parse_lfs_prune_dry_run(&text)
    }

    fn lfs_prune(&self, repo: &Path) -> GitResult<()> {
        Self::run_success(repo, &["lfs", "prune"]).map_err(|_| Error::LfsPruneFailed {
            repo: repo.to_path_buf(),
            reason: "git lfs prune failed".to_string(),
        })?;
        Ok(())
    }

    fn list_builtin_commands(&self) -> GitResult<Vec<String>> {
        let text = Self::run_global_text(&["--list-cmds=main,others"])?;
        Ok(text
            .lines()
            .filter(|l| !l.is_empty())
            .map(|l| l.to_string())
            .collect())
    }

    fn find_large_blobs(
        &self,
        repo: &Path,
        threshold: u64,
        depth: usize,
    ) -> GitResult<Vec<(String, u64, String)>> {
        use std::collections::HashSet;

        // Get unique root tree hashes from all ref tips.
        // Do NOT use --max-count here — it changes --no-walk semantics.
        let output = Self::run(repo, &["rev-list", "--all", "--no-walk", "--format=%T"])?;
        let text = String::from_utf8_lossy(&output.stdout);

        let unique_trees: HashSet<&str> = text
            .lines()
            .filter(|l| !l.starts_with("commit ") && !l.is_empty())
            .collect();

        if unique_trees.is_empty() {
            return Ok(Vec::new());
        }

        // For each unique tree (up to depth), scan with ls-tree -r -l.
        let mut seen_hashes: HashSet<String> = HashSet::new();
        let mut results: Vec<(String, u64, String)> = Vec::new();

        for tree in unique_trees.iter().take(depth) {
            let output = Self::run(repo, &["ls-tree", "-r", "-l", tree])?;
            let tree_text = String::from_utf8_lossy(&output.stdout);

            for line in tree_text.lines() {
                // Format: "<mode> <type> <hash> <size>\t<path>"
                let Some((metadata, path)) = line.split_once('\t') else {
                    continue;
                };
                let parts: Vec<&str> = metadata.split_whitespace().collect();
                if parts.len() != 4 {
                    continue;
                }
                if parts[1] != "blob" {
                    continue;
                }
                let Ok(size) = parts[3].parse::<u64>() else {
                    continue;
                };
                if size < threshold {
                    continue;
                }
                let hash = parts[2];
                if !seen_hashes.insert(hash.to_string()) {
                    continue;
                }
                results.push((hash.to_string(), size, path.to_string()));
            }
        }

        // Sort by size descending for readability
        results.sort_by_key(|b| std::cmp::Reverse(b.1));
        Ok(results)
    }
}

/// Parse the output of `git lfs prune --dry-run`.
///
/// Handles formats like:
/// - `"pruning 2 files, (1.2 MB)"`
/// - `"Pruning 2 files, (1.2 MB)"`
///
/// Returns `(count, bytes)`. Returns `(0, 0)` if the output cannot be parsed.
fn parse_lfs_prune_dry_run(text: &str) -> GitResult<(usize, u64)> {
    for line in text.lines() {
        let lower = line.to_lowercase();
        if let Some(rest) = lower.strip_prefix("pruning ").or_else(|| {
            // Handle lines with unicode checkmark prefix
            lower
                .find("pruning ")
                .map(|idx| &lower[idx + "pruning ".len()..])
        }) {
            // "2 files, (1.2 MB)"
            if let Some((count_str, _)) = rest.split_once(' ') {
                let count = count_str.parse::<usize>().unwrap_or(0);
                // Extract size from parentheses
                if let Some(paren_start) = line.find('(')
                    && let Some(paren_end) = line.find(')')
                {
                    let size_str = &line[paren_start + 1..paren_end];
                    let bytes = parse_human_bytes(size_str);
                    return Ok((count, bytes));
                }
                return Ok((count, 0));
            }
        }
    }
    Ok((0, 0))
}

/// Parse human-readable byte sizes like "1.2 MB", "500 KB", "2 GB", "1024 B".
fn parse_human_bytes(s: &str) -> u64 {
    let s = s.trim();
    let (num_str, suffix) = s
        .rfind(|c: char| c.is_ascii_digit() || c == '.')
        .map(|idx| (&s[..=idx], s[idx + 1..].trim()))
        .unwrap_or((s, ""));

    let num: f64 = num_str.parse().unwrap_or(0.0);
    let multiplier: f64 = match suffix.to_uppercase().as_str() {
        "B" | "" => 1.0,
        "KB" | "K" => 1_000.0,
        "MB" | "M" => 1_000_000.0,
        "GB" | "G" => 1_000_000_000.0,
        "TB" | "T" => 1_000_000_000_000.0,
        _ => 1.0,
    };
    (num * multiplier) as u64
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_log_oneline_basic() {
        let output = "abc1234 Fix the bug\ndef5678 Add feature\n";
        let result = RealGit::parse_log_oneline(output);
        assert_eq!(result.len(), 2);
        assert_eq!(
            result[0],
            ("abc1234".to_string(), "Fix the bug".to_string())
        );
        assert_eq!(
            result[1],
            ("def5678".to_string(), "Add feature".to_string())
        );
    }

    #[test]
    fn parse_log_oneline_empty() {
        let result = RealGit::parse_log_oneline("");
        assert!(result.is_empty());
    }

    #[test]
    fn parse_log_oneline_keeps_commits_with_empty_subjects() {
        // Regression: previously `split_once(' ')` returned None for a hash-only line, which dropped the commit from the result and shrank the `total` count in LandedByContent — flipping the matched/total ratio in boundary cases.
        let output = "abc1234 with subject\ndef5678\nfff9999 another\n";
        let result = RealGit::parse_log_oneline(output);
        assert_eq!(result.len(), 3);
        assert_eq!(result[1], ("def5678".to_string(), String::new()));
    }

    #[test]
    fn parse_log_oneline_subject_with_spaces() {
        let output = "abc1234 feat: add multi word feature support\n";
        let result = RealGit::parse_log_oneline(output);
        assert_eq!(result.len(), 1);
        assert_eq!(
            result[0],
            (
                "abc1234".to_string(),
                "feat: add multi word feature support".to_string()
            )
        );
    }

    #[test]
    fn parse_lfs_prune_dry_run_basic() {
        let output = "4 local objects, 1 retained\npruning 2 files, (1.2 MB)\n";
        let (count, bytes) = parse_lfs_prune_dry_run(output).unwrap();
        assert_eq!(count, 2);
        assert_eq!(bytes, 1_200_000);
    }

    #[test]
    fn parse_lfs_prune_dry_run_with_checkmark() {
        let output =
            "\u{2714} 3 local objects, 1 retained, done.\n\u{2714} Pruning 2 files, (500 KB)\n";
        let (count, bytes) = parse_lfs_prune_dry_run(output).unwrap();
        assert_eq!(count, 2);
        assert_eq!(bytes, 500_000);
    }

    #[test]
    fn parse_lfs_prune_dry_run_no_match() {
        let output = "nothing to prune\n";
        let (count, bytes) = parse_lfs_prune_dry_run(output).unwrap();
        assert_eq!(count, 0);
        assert_eq!(bytes, 0);
    }

    #[test]
    fn parse_human_bytes_various() {
        assert_eq!(parse_human_bytes("1024 B"), 1024);
        assert_eq!(parse_human_bytes("500 KB"), 500_000);
        assert_eq!(parse_human_bytes("1.2 MB"), 1_200_000);
        assert_eq!(parse_human_bytes("2 GB"), 2_000_000_000);
        assert_eq!(parse_human_bytes("1.5 GB"), 1_500_000_000);
    }

    #[test]
    fn parse_human_bytes_short_suffix() {
        assert_eq!(parse_human_bytes("1 K"), 1_000);
        assert_eq!(parse_human_bytes("5 M"), 5_000_000);
        assert_eq!(parse_human_bytes("1 G"), 1_000_000_000);
    }

    #[test]
    fn parse_human_bytes_no_suffix() {
        assert_eq!(parse_human_bytes("1024"), 1024);
    }

    #[test]
    fn parse_human_bytes_invalid() {
        assert_eq!(parse_human_bytes("not a size"), 0);
        assert_eq!(parse_human_bytes(""), 0);
    }

    #[test]
    fn parse_worktree_list_basic() {
        let porcelain = "\
worktree /main/repo
HEAD abc123
branch refs/heads/main

worktree /worktrees/feat
HEAD def456
branch refs/heads/feat

";
        let result = parse_worktree_list_porcelain(porcelain);
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].0, PathBuf::from("/worktrees/feat"));
        assert_eq!(result[0].1, Some("feat".to_string()));
    }

    #[test]
    fn parse_worktree_list_skips_prunable() {
        let porcelain = "\
worktree /main/repo
HEAD abc123
branch refs/heads/main

worktree /worktrees/feat
HEAD def456
branch refs/heads/feat

worktree /worktrees/deleted
HEAD 789012
branch refs/heads/old-branch
prunable gitdir file points to non-existent location

";
        let result = parse_worktree_list_porcelain(porcelain);
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].0, PathBuf::from("/worktrees/feat"));
    }

    #[test]
    fn parse_worktree_list_detached_head() {
        let porcelain = "\
worktree /main/repo
HEAD abc123
branch refs/heads/main

worktree /worktrees/detached
HEAD def456
detached

";
        let result = parse_worktree_list_porcelain(porcelain);
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].0, PathBuf::from("/worktrees/detached"));
        assert_eq!(result[0].1, None);
    }

    #[test]
    fn parse_worktree_list_all_prunable() {
        let porcelain = "\
worktree /main/repo
HEAD abc123
branch refs/heads/main

worktree /worktrees/gone1
HEAD 111111
branch refs/heads/gone1
prunable gitdir file points to non-existent location

worktree /worktrees/gone2
HEAD 222222
branch refs/heads/gone2
prunable gitdir file points to non-existent location

";
        let result = parse_worktree_list_porcelain(porcelain);
        assert!(result.is_empty());
    }
}
