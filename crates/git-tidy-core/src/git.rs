use std::path::Path;
use std::process::Command;

use crate::error::Error;

/// Result type alias for git operations.
pub type GitResult<T> = Result<T, Error>;

/// Abstraction over git CLI operations for testability.
pub trait GitOps {
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

    fn parse_log_oneline(output: &str) -> Vec<(String, String)> {
        output
            .lines()
            .filter(|l| !l.is_empty())
            .filter_map(|line| {
                let (hash, subject) = line.split_once(' ')?;
                Some((hash.to_string(), subject.to_string()))
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
}
