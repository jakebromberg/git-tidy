mod common;

use std::fs;

use common::TestRepo;
use git_worktree_tidy::discovery;

#[test]
fn discover_linked_worktrees_in_real_repo() {
    let test = TestRepo::new();
    let base = test.dir.path();

    // Add two worktrees
    let _wt1 = test.add_worktree("main-repo-feat1", "feature/feat1");
    let _wt2 = test.add_worktree("main-repo-feat2", "feature/feat2");

    let groups = discovery::discover_worktrees(base).unwrap();

    assert_eq!(groups.len(), 1, "should find exactly one parent repo");

    let worktrees = groups.values().next().unwrap();
    assert_eq!(worktrees.len(), 2, "should find two linked worktrees");

    let mut names: Vec<String> = worktrees
        .iter()
        .map(|wt| wt.path.file_name().unwrap().to_string_lossy().to_string())
        .collect();
    names.sort();
    assert_eq!(names, vec!["main-repo-feat1", "main-repo-feat2"]);
}

#[test]
fn skip_main_worktree_and_non_repo_dirs() {
    let test = TestRepo::new();
    let base = test.dir.path();

    // The main repo itself has a .git directory — should be skipped
    // Create an unrelated non-repo directory
    fs::create_dir_all(base.join("not-a-repo")).unwrap();

    let groups = discovery::discover_worktrees(base).unwrap();
    assert!(groups.is_empty(), "no linked worktrees should be found");
}

#[test]
fn worktrees_grouped_by_parent_repo() {
    let dir = tempfile::tempdir().unwrap();
    let base = dir.path();

    // Create two separate repos
    let repo_a = base.join("repo-a");
    fs::create_dir_all(&repo_a).unwrap();
    git_init(&repo_a);

    let repo_b = base.join("repo-b");
    fs::create_dir_all(&repo_b).unwrap();
    git_init(&repo_b);

    // Add a worktree for each
    let wt_a = base.join("repo-a-wt");
    git_cmd(
        &repo_a,
        &["worktree", "add", "-b", "br-a", &wt_a.to_string_lossy()],
    );

    let wt_b = base.join("repo-b-wt");
    git_cmd(
        &repo_b,
        &["worktree", "add", "-b", "br-b", &wt_b.to_string_lossy()],
    );

    let groups = discovery::discover_worktrees(base).unwrap();
    assert_eq!(groups.len(), 2);
    let repo_a_canon = repo_a.canonicalize().unwrap();
    let repo_b_canon = repo_b.canonicalize().unwrap();
    assert!(groups.contains_key(&repo_a_canon));
    assert!(groups.contains_key(&repo_b_canon));
}

fn git_init(dir: &std::path::Path) {
    git_cmd(dir, &["init", "-b", "main"]);
    git_cmd(dir, &["config", "user.email", "test@test.com"]);
    git_cmd(dir, &["config", "user.name", "Test"]);
    fs::write(dir.join("README.md"), "# Test\n").unwrap();
    git_cmd(dir, &["add", "README.md"]);
    git_cmd(dir, &["commit", "-m", "Initial commit"]);
}

fn git_cmd(dir: &std::path::Path, args: &[&str]) {
    let output = std::process::Command::new("git")
        .arg("-C")
        .arg(dir)
        .args(args)
        .output()
        .expect("git failed to run");
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        panic!("git {:?} failed: {}", args, stderr);
    }
}
