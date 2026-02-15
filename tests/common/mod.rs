use std::path::{Path, PathBuf};
use std::process::Command;

/// Helper to set up a real git repo with worktrees in a tempdir for integration tests.
pub struct TestRepo {
    pub dir: tempfile::TempDir,
    pub main_repo: PathBuf,
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
            &[
                "worktree",
                "add",
                "-b",
                branch,
                &wt_path.to_string_lossy(),
            ],
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

pub fn git(dir: &Path, args: &[&str]) -> String {
    let output = Command::new("git")
        .arg("-C")
        .arg(dir)
        .args(args)
        .output()
        .expect("failed to run git");
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        panic!(
            "git {:?} failed in {}: {}",
            args,
            dir.display(),
            stderr
        );
    }
    String::from_utf8_lossy(&output.stdout).trim().to_string()
}
