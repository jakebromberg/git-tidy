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
        let output = Self::run(
            repo,
            &["log", branch_or_ref, "--oneline", "--grep", needle],
        )?;
        let text = String::from_utf8_lossy(&output.stdout).trim().to_string();
        Ok(Self::parse_log_oneline(&text))
    }

    fn diff_commit(&self, repo: &Path, commit: &str) -> GitResult<String> {
        Self::run_success(repo, &["diff-tree", "-p", "--root", commit])
    }

    fn diff_commit_files(&self, repo: &Path, commit: &str) -> GitResult<Vec<String>> {
        let text = Self::run_success(
            repo,
            &["diff-tree", "--root", "--no-commit-id", "-r", "--name-only", commit],
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

    fn is_branch_checked_out(&self, repo: &Path, branch: &str) -> GitResult<bool> {
        let output = Self::run_success(repo, &["worktree", "list", "--porcelain"])?;
        for line in output.lines() {
            if let Some(b) = line.strip_prefix("branch refs/heads/") {
                if b == branch {
                    return Ok(true);
                }
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
        let output = Self::run(
            repo,
            &["log", ref_spec, "--oneline", "--all", "--", file],
        )?;
        let text = String::from_utf8_lossy(&output.stdout).trim().to_string();
        Ok(Self::parse_log_oneline(&text))
    }
}

#[cfg(test)]
pub mod tests {
    use std::collections::HashMap;
    use std::path::PathBuf;

    use super::*;

    /// Builder for constructing a MockGit with canned responses.
    #[derive(Default)]
    pub struct MockGitBuilder {
        symbolic_ref: HashMap<PathBuf, Option<String>>,
        rev_parse_verify: HashMap<(PathBuf, String), bool>,
        is_ancestor: HashMap<(PathBuf, String, String), bool>,
        rev_list_counts: HashMap<(PathBuf, String, String), (usize, usize)>,
        log_exclusive: HashMap<(PathBuf, String, String), Vec<(String, String)>>,
        log_grep: HashMap<(PathBuf, String, String), Vec<(String, String)>>,
        diff_commit: HashMap<(PathBuf, String), String>,
        diff_commit_files: HashMap<(PathBuf, String), Vec<String>>,
        log_touching_files: HashMap<(PathBuf, String), Vec<(String, String)>>,
        diff_commit_on_ref: HashMap<(PathBuf, String), String>,
        status_porcelain: HashMap<PathBuf, Vec<String>>,
        worktree_branch: HashMap<PathBuf, Option<String>>,
        rev_parse: HashMap<(PathBuf, String), String>,
        is_branch_checked_out: HashMap<(PathBuf, String), bool>,
        log_file_history: HashMap<(PathBuf, String, String), Vec<(String, String)>>,
        fetch_prune_calls: std::cell::RefCell<Vec<PathBuf>>,
        remove_calls: std::cell::RefCell<Vec<(PathBuf, PathBuf)>>,
        remove_force_calls: std::cell::RefCell<Vec<(PathBuf, PathBuf)>>,
        prune_calls: std::cell::RefCell<Vec<PathBuf>>,
        branch_delete_calls: std::cell::RefCell<Vec<(PathBuf, String)>>,
        worktree_remove_errors: HashMap<PathBuf, String>,
        worktree_remove_force_errors: HashMap<PathBuf, String>,
    }

    impl MockGitBuilder {
        pub fn new() -> Self {
            Self::default()
        }

        pub fn with_symbolic_ref(mut self, repo: &Path, branch: Option<&str>) -> Self {
            self.symbolic_ref
                .insert(repo.to_path_buf(), branch.map(|s| s.to_string()));
            self
        }

        pub fn with_rev_parse_verify(mut self, repo: &Path, refspec: &str, exists: bool) -> Self {
            self.rev_parse_verify
                .insert((repo.to_path_buf(), refspec.to_string()), exists);
            self
        }

        pub fn with_is_ancestor(
            mut self,
            repo: &Path,
            branch: &str,
            target: &str,
            result: bool,
        ) -> Self {
            self.is_ancestor.insert(
                (repo.to_path_buf(), branch.to_string(), target.to_string()),
                result,
            );
            self
        }

        pub fn with_rev_list_counts(
            mut self,
            repo: &Path,
            left: &str,
            right: &str,
            counts: (usize, usize),
        ) -> Self {
            self.rev_list_counts.insert(
                (repo.to_path_buf(), left.to_string(), right.to_string()),
                counts,
            );
            self
        }

        pub fn with_log_exclusive(
            mut self,
            repo: &Path,
            base: &str,
            branch: &str,
            commits: Vec<(String, String)>,
        ) -> Self {
            self.log_exclusive.insert(
                (repo.to_path_buf(), base.to_string(), branch.to_string()),
                commits,
            );
            self
        }

        #[allow(dead_code)]
        pub fn with_log_grep(
            mut self,
            repo: &Path,
            ref_spec: &str,
            needle: &str,
            results: Vec<(String, String)>,
        ) -> Self {
            self.log_grep.insert(
                (repo.to_path_buf(), ref_spec.to_string(), needle.to_string()),
                results,
            );
            self
        }

        pub fn with_status_porcelain(mut self, path: &Path, lines: Vec<String>) -> Self {
            self.status_porcelain.insert(path.to_path_buf(), lines);
            self
        }

        pub fn with_worktree_branch(mut self, path: &Path, branch: Option<&str>) -> Self {
            self.worktree_branch
                .insert(path.to_path_buf(), branch.map(|s| s.to_string()));
            self
        }

        pub fn with_rev_parse(mut self, repo: &Path, refspec: &str, hash: &str) -> Self {
            self.rev_parse
                .insert((repo.to_path_buf(), refspec.to_string()), hash.to_string());
            self
        }

        #[allow(dead_code)]
        pub fn with_is_branch_checked_out(
            mut self,
            repo: &Path,
            branch: &str,
            checked_out: bool,
        ) -> Self {
            self.is_branch_checked_out
                .insert((repo.to_path_buf(), branch.to_string()), checked_out);
            self
        }

        #[allow(dead_code)]
        pub fn with_diff_commit_files(
            mut self,
            repo: &Path,
            commit: &str,
            files: Vec<String>,
        ) -> Self {
            self.diff_commit_files
                .insert((repo.to_path_buf(), commit.to_string()), files);
            self
        }

        #[allow(dead_code)]
        pub fn with_diff_commit(mut self, repo: &Path, commit: &str, diff: &str) -> Self {
            self.diff_commit
                .insert((repo.to_path_buf(), commit.to_string()), diff.to_string());
            self
        }

        #[allow(dead_code)]
        pub fn with_log_touching_files(
            mut self,
            repo: &Path,
            ref_spec: &str,
            results: Vec<(String, String)>,
        ) -> Self {
            self.log_touching_files
                .insert((repo.to_path_buf(), ref_spec.to_string()), results);
            self
        }

        #[allow(dead_code)]
        pub fn with_diff_commit_on_ref(mut self, repo: &Path, commit: &str, diff: &str) -> Self {
            self.diff_commit_on_ref
                .insert((repo.to_path_buf(), commit.to_string()), diff.to_string());
            self
        }

        #[allow(dead_code)]
        pub fn with_log_file_history(
            mut self,
            repo: &Path,
            ref_spec: &str,
            file: &str,
            results: Vec<(String, String)>,
        ) -> Self {
            self.log_file_history.insert(
                (repo.to_path_buf(), ref_spec.to_string(), file.to_string()),
                results,
            );
            self
        }

        #[allow(dead_code)]
        pub fn with_worktree_remove_error(mut self, path: &Path, error: &str) -> Self {
            self.worktree_remove_errors
                .insert(path.to_path_buf(), error.to_string());
            self
        }

        #[allow(dead_code)]
        pub fn with_worktree_remove_force_error(mut self, path: &Path, error: &str) -> Self {
            self.worktree_remove_force_errors
                .insert(path.to_path_buf(), error.to_string());
            self
        }

        pub fn build(self) -> MockGit {
            MockGit {
                symbolic_ref: self.symbolic_ref,
                rev_parse_verify: self.rev_parse_verify,
                is_ancestor: self.is_ancestor,
                rev_list_counts: self.rev_list_counts,
                log_exclusive: self.log_exclusive,
                log_grep: self.log_grep,
                diff_commit: self.diff_commit,
                diff_commit_files: self.diff_commit_files,
                log_touching_files: self.log_touching_files,
                diff_commit_on_ref: self.diff_commit_on_ref,
                status_porcelain: self.status_porcelain,
                worktree_branch: self.worktree_branch,
                rev_parse: self.rev_parse,
                is_branch_checked_out: self.is_branch_checked_out,
                log_file_history: self.log_file_history,
                fetch_prune_calls: self.fetch_prune_calls,
                remove_calls: self.remove_calls,
                remove_force_calls: self.remove_force_calls,
                prune_calls: self.prune_calls,
                branch_delete_calls: self.branch_delete_calls,
                worktree_remove_errors: self.worktree_remove_errors,
                worktree_remove_force_errors: self.worktree_remove_force_errors,
            }
        }
    }

    pub struct MockGit {
        symbolic_ref: HashMap<PathBuf, Option<String>>,
        rev_parse_verify: HashMap<(PathBuf, String), bool>,
        is_ancestor: HashMap<(PathBuf, String, String), bool>,
        rev_list_counts: HashMap<(PathBuf, String, String), (usize, usize)>,
        log_exclusive: HashMap<(PathBuf, String, String), Vec<(String, String)>>,
        log_grep: HashMap<(PathBuf, String, String), Vec<(String, String)>>,
        diff_commit: HashMap<(PathBuf, String), String>,
        diff_commit_files: HashMap<(PathBuf, String), Vec<String>>,
        log_touching_files: HashMap<(PathBuf, String), Vec<(String, String)>>,
        diff_commit_on_ref: HashMap<(PathBuf, String), String>,
        status_porcelain: HashMap<PathBuf, Vec<String>>,
        worktree_branch: HashMap<PathBuf, Option<String>>,
        rev_parse: HashMap<(PathBuf, String), String>,
        is_branch_checked_out: HashMap<(PathBuf, String), bool>,
        log_file_history: HashMap<(PathBuf, String, String), Vec<(String, String)>>,
        fetch_prune_calls: std::cell::RefCell<Vec<PathBuf>>,
        remove_calls: std::cell::RefCell<Vec<(PathBuf, PathBuf)>>,
        remove_force_calls: std::cell::RefCell<Vec<(PathBuf, PathBuf)>>,
        prune_calls: std::cell::RefCell<Vec<PathBuf>>,
        branch_delete_calls: std::cell::RefCell<Vec<(PathBuf, String)>>,
        worktree_remove_errors: HashMap<PathBuf, String>,
        worktree_remove_force_errors: HashMap<PathBuf, String>,
    }

    #[allow(dead_code)]
    impl MockGit {
        pub fn fetch_prune_calls(&self) -> Vec<PathBuf> {
            self.fetch_prune_calls.borrow().clone()
        }

        pub fn remove_calls(&self) -> Vec<(PathBuf, PathBuf)> {
            self.remove_calls.borrow().clone()
        }

        pub fn remove_force_calls(&self) -> Vec<(PathBuf, PathBuf)> {
            self.remove_force_calls.borrow().clone()
        }

        pub fn branch_delete_calls(&self) -> Vec<(PathBuf, String)> {
            self.branch_delete_calls.borrow().clone()
        }
    }

    impl GitOps for MockGit {
        fn fetch_prune(&self, repo: &Path) -> GitResult<()> {
            self.fetch_prune_calls.borrow_mut().push(repo.to_path_buf());
            Ok(())
        }

        fn symbolic_ref_origin_head(&self, repo: &Path) -> GitResult<Option<String>> {
            Ok(self
                .symbolic_ref
                .get(&repo.to_path_buf())
                .cloned()
                .unwrap_or(None))
        }

        fn rev_parse_verify(&self, repo: &Path, refspec: &str) -> GitResult<bool> {
            Ok(*self
                .rev_parse_verify
                .get(&(repo.to_path_buf(), refspec.to_string()))
                .unwrap_or(&false))
        }

        fn is_ancestor(&self, repo: &Path, branch: &str, target: &str) -> GitResult<bool> {
            Ok(*self
                .is_ancestor
                .get(&(repo.to_path_buf(), branch.to_string(), target.to_string()))
                .unwrap_or(&false))
        }

        fn rev_list_left_right_count(
            &self,
            repo: &Path,
            left: &str,
            right: &str,
        ) -> GitResult<(usize, usize)> {
            Ok(*self
                .rev_list_counts
                .get(&(repo.to_path_buf(), left.to_string(), right.to_string()))
                .unwrap_or(&(0, 0)))
        }

        fn log_exclusive(
            &self,
            repo: &Path,
            base: &str,
            branch: &str,
        ) -> GitResult<Vec<(String, String)>> {
            Ok(self
                .log_exclusive
                .get(&(repo.to_path_buf(), base.to_string(), branch.to_string()))
                .cloned()
                .unwrap_or_default())
        }

        fn log_grep(
            &self,
            repo: &Path,
            branch_or_ref: &str,
            needle: &str,
        ) -> GitResult<Vec<(String, String)>> {
            Ok(self
                .log_grep
                .get(&(
                    repo.to_path_buf(),
                    branch_or_ref.to_string(),
                    needle.to_string(),
                ))
                .cloned()
                .unwrap_or_default())
        }

        fn diff_commit(&self, repo: &Path, commit: &str) -> GitResult<String> {
            Ok(self
                .diff_commit
                .get(&(repo.to_path_buf(), commit.to_string()))
                .cloned()
                .unwrap_or_default())
        }

        fn diff_commit_files(&self, repo: &Path, commit: &str) -> GitResult<Vec<String>> {
            Ok(self
                .diff_commit_files
                .get(&(repo.to_path_buf(), commit.to_string()))
                .cloned()
                .unwrap_or_default())
        }

        fn log_touching_files(
            &self,
            repo: &Path,
            ref_spec: &str,
            _files: &[String],
        ) -> GitResult<Vec<(String, String)>> {
            Ok(self
                .log_touching_files
                .get(&(repo.to_path_buf(), ref_spec.to_string()))
                .cloned()
                .unwrap_or_default())
        }

        fn diff_commit_on_ref(&self, repo: &Path, commit_hash: &str) -> GitResult<String> {
            Ok(self
                .diff_commit_on_ref
                .get(&(repo.to_path_buf(), commit_hash.to_string()))
                .cloned()
                .unwrap_or_default())
        }

        fn status_porcelain(&self, worktree_path: &Path) -> GitResult<Vec<String>> {
            Ok(self
                .status_porcelain
                .get(&worktree_path.to_path_buf())
                .cloned()
                .unwrap_or_default())
        }

        fn worktree_branch(&self, worktree_path: &Path) -> GitResult<Option<String>> {
            Ok(self
                .worktree_branch
                .get(&worktree_path.to_path_buf())
                .cloned()
                .unwrap_or(None))
        }

        fn rev_parse(&self, repo: &Path, refspec: &str) -> GitResult<String> {
            self.rev_parse
                .get(&(repo.to_path_buf(), refspec.to_string()))
                .cloned()
                .ok_or_else(|| Error::GitCommand {
                    command: format!("rev-parse {refspec}"),
                    message: "not found in mock".to_string(),
                })
        }

        fn worktree_remove(&self, repo: &Path, worktree_path: &Path) -> GitResult<()> {
            if let Some(err) = self.worktree_remove_errors.get(&worktree_path.to_path_buf()) {
                return Err(Error::RemovalFailed {
                    path: worktree_path.to_path_buf(),
                    reason: err.clone(),
                });
            }
            self.remove_calls
                .borrow_mut()
                .push((repo.to_path_buf(), worktree_path.to_path_buf()));
            Ok(())
        }

        fn worktree_remove_force(&self, repo: &Path, worktree_path: &Path) -> GitResult<()> {
            if let Some(err) = self
                .worktree_remove_force_errors
                .get(&worktree_path.to_path_buf())
            {
                return Err(Error::RemovalFailed {
                    path: worktree_path.to_path_buf(),
                    reason: err.clone(),
                });
            }
            self.remove_force_calls
                .borrow_mut()
                .push((repo.to_path_buf(), worktree_path.to_path_buf()));
            Ok(())
        }

        fn worktree_prune(&self, repo: &Path) -> GitResult<()> {
            self.prune_calls.borrow_mut().push(repo.to_path_buf());
            Ok(())
        }

        fn branch_delete(&self, repo: &Path, branch: &str) -> GitResult<()> {
            self.branch_delete_calls
                .borrow_mut()
                .push((repo.to_path_buf(), branch.to_string()));
            Ok(())
        }

        fn is_branch_checked_out(&self, repo: &Path, branch: &str) -> GitResult<bool> {
            Ok(*self
                .is_branch_checked_out
                .get(&(repo.to_path_buf(), branch.to_string()))
                .unwrap_or(&false))
        }

        fn log_file_history(
            &self,
            repo: &Path,
            ref_spec: &str,
            file: &str,
        ) -> GitResult<Vec<(String, String)>> {
            Ok(self
                .log_file_history
                .get(&(repo.to_path_buf(), ref_spec.to_string(), file.to_string()))
                .cloned()
                .unwrap_or_default())
        }
    }

    #[test]
    fn parse_log_oneline_basic() {
        let output = "abc1234 Fix the bug\ndef5678 Add feature\n";
        let result = RealGit::parse_log_oneline(output);
        assert_eq!(result.len(), 2);
        assert_eq!(result[0], ("abc1234".to_string(), "Fix the bug".to_string()));
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
