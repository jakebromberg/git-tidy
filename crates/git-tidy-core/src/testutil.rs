use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::process::Command;

use crate::error::Error;
use crate::git::{GitOps, GitResult};

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

    pub fn with_diff_commit_files(mut self, repo: &Path, commit: &str, files: Vec<String>) -> Self {
        self.diff_commit_files
            .insert((repo.to_path_buf(), commit.to_string()), files);
        self
    }

    pub fn with_diff_commit(mut self, repo: &Path, commit: &str, diff: &str) -> Self {
        self.diff_commit
            .insert((repo.to_path_buf(), commit.to_string()), diff.to_string());
        self
    }

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

    pub fn with_diff_commit_on_ref(mut self, repo: &Path, commit: &str, diff: &str) -> Self {
        self.diff_commit_on_ref
            .insert((repo.to_path_buf(), commit.to_string()), diff.to_string());
        self
    }

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

    pub fn with_worktree_remove_error(mut self, path: &Path, error: &str) -> Self {
        self.worktree_remove_errors
            .insert(path.to_path_buf(), error.to_string());
        self
    }

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
        if let Some(err) = self
            .worktree_remove_errors
            .get(&worktree_path.to_path_buf())
        {
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

/// Helper to set up a real git repo with worktrees in a tempdir for integration tests.
pub struct TestRepo {
    pub dir: tempfile::TempDir,
    pub main_repo: PathBuf,
}

impl Default for TestRepo {
    fn default() -> Self {
        Self::new()
    }
}

impl TestRepo {
    /// Create a new test repo with an initial commit and a `main` branch.
    pub fn new() -> Self {
        let dir = tempfile::tempdir().unwrap();
        let main_repo = dir.path().join("main-repo");
        std::fs::create_dir_all(&main_repo).unwrap();

        // Initialize repo
        git(&main_repo, &["init", "-b", "main"]);
        git(&main_repo, &["config", "user.email", "test@test.com"]);
        git(&main_repo, &["config", "user.name", "Test"]);

        // Create initial commit
        let readme = main_repo.join("README.md");
        std::fs::write(&readme, "# Test\n").unwrap();
        git(&main_repo, &["add", "README.md"]);
        git(&main_repo, &["commit", "-m", "Initial commit"]);

        TestRepo { dir, main_repo }
    }

    /// Add a linked worktree with the given directory name and branch.
    pub fn add_worktree(&self, name: &str, branch: &str) -> PathBuf {
        let wt_path = self.dir.path().join(name);
        git(
            &self.main_repo,
            &["worktree", "add", "-b", branch, &wt_path.to_string_lossy()],
        );
        wt_path
    }

    /// Commit a file in the given worktree.
    pub fn commit_file(&self, worktree: &Path, filename: &str, content: &str, message: &str) {
        let file = worktree.join(filename);
        std::fs::write(&file, content).unwrap();
        git(worktree, &["add", filename]);
        git(worktree, &["commit", "-m", message]);
    }
}

/// Run a git command in the given directory, panicking on failure.
pub fn git(dir: &Path, args: &[&str]) -> String {
    let output = Command::new("git")
        .arg("-C")
        .arg(dir)
        .args(args)
        .output()
        .expect("failed to run git");
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        panic!("git {:?} failed in {}: {}", args, dir.display(), stderr);
    }
    String::from_utf8_lossy(&output.stdout).trim().to_string()
}
