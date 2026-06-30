//! Conformance tests: verify GixGitOps produces identical results to RealGit
//! against real temporary git repositories.

use git_tidy_core::git::{GitOps, RealGit};
use git_tidy_core::gix_ops::GixGitOps;
use git_tidy_core::testutil::{TestRepo, git};

fn set_up_diverged_repo() -> TestRepo {
    let t = TestRepo::new();
    t.commit_file(&t.main_repo, "base.txt", "base", "base commit");
    git(&t.main_repo, &["checkout", "-b", "feature"]);
    t.commit_file(&t.main_repo, "feat1.txt", "f1", "feature commit 1");
    t.commit_file(&t.main_repo, "feat2.txt", "f2", "feature commit 2");
    git(&t.main_repo, &["checkout", "main"]);
    t.commit_file(&t.main_repo, "main2.txt", "m2", "main commit 1");
    t
}

// ── Phase 2: Reference operations ──────────────────────────────────────

#[test]
fn symbolic_ref_origin_head_matches() {
    let t = TestRepo::new();
    let bare = t.set_up_remote();
    git(&t.main_repo, &["remote", "set-head", "origin", "main"]);

    let real = RealGit;
    let gix = GixGitOps;

    let expected = real.symbolic_ref_origin_head(&t.main_repo).unwrap();
    let actual = gix.symbolic_ref_origin_head(&t.main_repo).unwrap();
    assert_eq!(expected, actual, "symbolic_ref_origin_head mismatch");
    assert_eq!(expected, Some("main".to_string()));

    let _ = bare;
}

#[test]
fn symbolic_ref_origin_head_missing() {
    let t = TestRepo::new();

    let real = RealGit;
    let gix = GixGitOps;

    let expected = real.symbolic_ref_origin_head(&t.main_repo).unwrap();
    let actual = gix.symbolic_ref_origin_head(&t.main_repo).unwrap();
    assert_eq!(expected, actual);
    assert_eq!(expected, None);
}

#[test]
fn rev_parse_verify_existing_ref() {
    let t = TestRepo::new();
    let bare = t.set_up_remote();
    git(&t.main_repo, &["fetch"]);

    let real = RealGit;
    let gix = GixGitOps;

    let expected = real
        .rev_parse_verify(&t.main_repo, "refs/remotes/origin/main")
        .unwrap();
    let actual = gix
        .rev_parse_verify(&t.main_repo, "refs/remotes/origin/main")
        .unwrap();
    assert_eq!(expected, actual);
    assert!(expected);

    let _ = bare;
}

#[test]
fn rev_parse_verify_nonexistent_ref() {
    let t = TestRepo::new();

    let real = RealGit;
    let gix = GixGitOps;

    let expected = real
        .rev_parse_verify(&t.main_repo, "refs/remotes/origin/nonexistent")
        .unwrap();
    let actual = gix
        .rev_parse_verify(&t.main_repo, "refs/remotes/origin/nonexistent")
        .unwrap();
    assert_eq!(expected, actual);
    assert!(!expected);
}

#[test]
fn rev_parse_matches() {
    let t = TestRepo::new();

    let real = RealGit;
    let gix = GixGitOps;

    let expected = real.rev_parse(&t.main_repo, "HEAD").unwrap();
    let actual = gix.rev_parse(&t.main_repo, "HEAD").unwrap();
    assert_eq!(expected, actual);
    assert!(!expected.is_empty());
}

#[test]
fn rev_parse_branch_name() {
    let t = TestRepo::new();
    git(&t.main_repo, &["checkout", "-b", "feature-x"]);
    t.commit_file(&t.main_repo, "feat.txt", "x", "feat commit");
    git(&t.main_repo, &["checkout", "main"]);

    let real = RealGit;
    let gix = GixGitOps;

    let expected = real.rev_parse(&t.main_repo, "feature-x").unwrap();
    let actual = gix.rev_parse(&t.main_repo, "feature-x").unwrap();
    assert_eq!(expected, actual);
}

#[test]
fn list_local_branches_matches() {
    let t = TestRepo::new();
    git(&t.main_repo, &["branch", "feature-a"]);
    git(&t.main_repo, &["branch", "feature-b"]);

    let real = RealGit;
    let gix = GixGitOps;

    let mut expected = real.list_local_branches(&t.main_repo).unwrap();
    let mut actual = gix.list_local_branches(&t.main_repo).unwrap();
    expected.sort();
    actual.sort();
    assert_eq!(expected, actual);
    assert_eq!(expected.len(), 3); // main, feature-a, feature-b
}

#[test]
fn current_branch_matches() {
    let t = TestRepo::new();

    let real = RealGit;
    let gix = GixGitOps;

    let expected = real.current_branch(&t.main_repo).unwrap();
    let actual = gix.current_branch(&t.main_repo).unwrap();
    assert_eq!(expected, actual);
    assert_eq!(expected, Some("main".to_string()));
}

#[test]
fn current_branch_detached_head() {
    let t = TestRepo::new();
    let head = git(&t.main_repo, &["rev-parse", "HEAD"]);
    git(&t.main_repo, &["checkout", &head]);

    let real = RealGit;
    let gix = GixGitOps;

    let expected = real.current_branch(&t.main_repo).unwrap();
    let actual = gix.current_branch(&t.main_repo).unwrap();
    assert_eq!(expected, actual);
    assert_eq!(expected, None);
}

#[test]
fn worktree_branch_matches() {
    let t = TestRepo::new();
    let wt = t.add_worktree("wt-feat", "feat");

    let real = RealGit;
    let gix = GixGitOps;

    let expected = real.worktree_branch(&wt).unwrap();
    let actual = gix.worktree_branch(&wt).unwrap();
    assert_eq!(expected, actual);
    assert_eq!(expected, Some("feat".to_string()));
}

#[test]
fn upstream_branch_matches() {
    let t = TestRepo::new();
    let _bare = t.set_up_remote();

    let real = RealGit;
    let gix = GixGitOps;

    let expected = real.upstream_branch(&t.main_repo, "main").unwrap();
    let actual = gix.upstream_branch(&t.main_repo, "main").unwrap();
    assert_eq!(expected, actual);
    assert_eq!(expected, Some("origin/main".to_string()));
}

#[test]
fn upstream_branch_no_upstream() {
    let t = TestRepo::new();
    git(&t.main_repo, &["branch", "local-only"]);

    let real = RealGit;
    let gix = GixGitOps;

    let expected = real.upstream_branch(&t.main_repo, "local-only").unwrap();
    let actual = gix.upstream_branch(&t.main_repo, "local-only").unwrap();
    assert_eq!(expected, actual);
    assert_eq!(expected, None);
}

// ── Phase 3: Ancestry operations ───────────────────────────────────────

#[test]
fn is_ancestor_true() {
    let t = set_up_diverged_repo();
    let base_hash = git(&t.main_repo, &["rev-parse", "main~1"]);

    let real = RealGit;
    let gix = GixGitOps;

    let expected = real.is_ancestor(&t.main_repo, &base_hash, "main").unwrap();
    let actual = gix.is_ancestor(&t.main_repo, &base_hash, "main").unwrap();
    assert_eq!(expected, actual);
    assert!(expected);
}

#[test]
fn is_ancestor_false() {
    let t = set_up_diverged_repo();

    let real = RealGit;
    let gix = GixGitOps;

    let expected = real.is_ancestor(&t.main_repo, "feature", "main").unwrap();
    let actual = gix.is_ancestor(&t.main_repo, "feature", "main").unwrap();
    assert_eq!(expected, actual);
    assert!(!expected);
}

#[test]
fn rev_list_left_right_count_matches() {
    let t = set_up_diverged_repo();

    let real = RealGit;
    let gix = GixGitOps;

    let expected = real
        .rev_list_left_right_count(&t.main_repo, "main", "feature")
        .unwrap();
    let actual = gix
        .rev_list_left_right_count(&t.main_repo, "main", "feature")
        .unwrap();
    assert_eq!(expected, actual);
    assert_eq!(expected, (1, 2));
}

#[test]
fn rev_list_left_right_count_same_branch() {
    let t = TestRepo::new();

    let real = RealGit;
    let gix = GixGitOps;

    let expected = real
        .rev_list_left_right_count(&t.main_repo, "main", "main")
        .unwrap();
    let actual = gix
        .rev_list_left_right_count(&t.main_repo, "main", "main")
        .unwrap();
    assert_eq!(expected, actual);
    assert_eq!(expected, (0, 0));
}

#[test]
fn is_commit_reachable_true() {
    let t = TestRepo::new();
    let head = git(&t.main_repo, &["rev-parse", "HEAD"]);

    let real = RealGit;
    let gix = GixGitOps;

    let expected = real.is_commit_reachable(&t.main_repo, &head).unwrap();
    let actual = gix.is_commit_reachable(&t.main_repo, &head).unwrap();
    assert_eq!(expected, actual);
    assert!(expected);
}

#[test]
fn is_commit_reachable_via_remote_only_branch() {
    // Regression: previously `is_commit_reachable` ran `git branch --contains` without `-a`, so a commit reachable only via a remote-tracking branch (the merged-PR-with-tag-left-behind scenario) reported false → git-tag-tidy classified the tag as Stale → default delete. The `-a` flag fixes this; the test locks the behavior in.
    let t = TestRepo::new();
    let bare = t.set_up_remote();

    // Create a feature branch with a unique commit and push it.
    git(&t.main_repo, &["checkout", "-b", "feature"]);
    t.commit_file(&t.main_repo, "feat.txt", "f", "feature commit");
    let feature_sha = git(&t.main_repo, &["rev-parse", "HEAD"]);
    git(&t.main_repo, &["push", "-u", "origin", "feature"]);

    // Delete the local branch so the commit is reachable ONLY via origin/feature.
    git(&t.main_repo, &["checkout", "main"]);
    git(&t.main_repo, &["branch", "-D", "feature"]);

    let real = RealGit;
    let gix = GixGitOps;

    let real_reachable = real
        .is_commit_reachable(&t.main_repo, &feature_sha)
        .unwrap();
    let gix_reachable = gix.is_commit_reachable(&t.main_repo, &feature_sha).unwrap();
    assert_eq!(real_reachable, gix_reachable);
    assert!(
        real_reachable,
        "commit reachable only via origin/feature must report reachable",
    );

    let _ = bare;
}

#[test]
fn is_commit_reachable_orphan() {
    let t = TestRepo::new();
    git(&t.main_repo, &["checkout", "--orphan", "orphan"]);
    t.commit_file(&t.main_repo, "orphan.txt", "o", "orphan commit");
    let orphan_hash = git(&t.main_repo, &["rev-parse", "HEAD"]);
    git(&t.main_repo, &["checkout", "main"]);
    git(&t.main_repo, &["branch", "-D", "orphan"]);

    let real = RealGit;
    let gix = GixGitOps;

    let expected = real
        .is_commit_reachable(&t.main_repo, &orphan_hash)
        .unwrap();
    let actual = gix.is_commit_reachable(&t.main_repo, &orphan_hash).unwrap();
    assert_eq!(expected, actual);
    assert!(!expected);
}

// ── Phase 4: Log and diff ops (pass-through) ──────────────────────────

#[test]
fn log_exclusive_passthrough() {
    let t = set_up_diverged_repo();

    let real = RealGit;
    let gix = GixGitOps;

    let expected = real.log_exclusive(&t.main_repo, "main", "feature").unwrap();
    let actual = gix.log_exclusive(&t.main_repo, "main", "feature").unwrap();
    assert_eq!(expected, actual);
    assert_eq!(expected.len(), 2);
}

#[test]
fn log_grep_passthrough() {
    let t = TestRepo::new();
    t.commit_file(&t.main_repo, "a.txt", "a", "fix: resolve bug");
    t.commit_file(&t.main_repo, "b.txt", "b", "feat: add widget");

    let real = RealGit;
    let gix = GixGitOps;

    let expected = real.log_grep(&t.main_repo, "main", "fix:").unwrap();
    let actual = gix.log_grep(&t.main_repo, "main", "fix:").unwrap();
    assert_eq!(expected, actual);
    assert!(!expected.is_empty());
}

#[test]
fn log_grep_treats_regex_metacharacters_as_literal() {
    // Regression: previously log_grep passed the needle to `git log --grep`, which is POSIX BRE. A subject containing `(`, `[`, `?`, `*`, `+`, `\` would either match the wrong commits or fail the regex parse. With --fixed-strings the needle is matched literally.
    let t = TestRepo::new();
    t.commit_file(
        &t.main_repo,
        "a.txt",
        "a",
        "fix(auth): handle [null] tokens?",
    );
    t.commit_file(&t.main_repo, "b.txt", "b", "feat: add widget");

    let real = RealGit;

    // Use the literal subject (including parens, brackets, question mark) — regex characters that would either match nothing or error under POSIX BRE.
    let matches = real
        .log_grep(&t.main_repo, "main", "fix(auth): handle [null] tokens?")
        .unwrap();
    assert_eq!(matches.len(), 1);
    assert_eq!(matches[0].1, "fix(auth): handle [null] tokens?");

    // A literal needle that is a substring should also match.
    let partial = real.log_grep(&t.main_repo, "main", "[null]").unwrap();
    assert_eq!(partial.len(), 1);
}

#[test]
fn diff_commit_passthrough() {
    let t = TestRepo::new();
    t.commit_file(&t.main_repo, "change.txt", "content", "a change");
    let head = git(&t.main_repo, &["rev-parse", "HEAD"]);

    let real = RealGit;
    let gix = GixGitOps;

    let expected = real.diff_commit(&t.main_repo, &head).unwrap();
    let actual = gix.diff_commit(&t.main_repo, &head).unwrap();
    assert_eq!(expected, actual);
    assert!(!expected.is_empty());
}

#[test]
fn diff_commit_files_passthrough() {
    let t = TestRepo::new();
    t.commit_file(&t.main_repo, "file_a.txt", "a", "add file_a");
    let head = git(&t.main_repo, &["rev-parse", "HEAD"]);

    let real = RealGit;
    let gix = GixGitOps;

    let expected = real.diff_commit_files(&t.main_repo, &head).unwrap();
    let actual = gix.diff_commit_files(&t.main_repo, &head).unwrap();
    assert_eq!(expected, actual);
    assert!(expected.contains(&"file_a.txt".to_string()));
}

#[test]
fn log_touching_files_passthrough() {
    let t = TestRepo::new();
    t.commit_file(&t.main_repo, "touched.txt", "v1", "first version");
    t.commit_file(&t.main_repo, "touched.txt", "v2", "second version");

    let real = RealGit;
    let gix = GixGitOps;

    let expected = real
        .log_touching_files(&t.main_repo, "main", &["touched.txt".to_string()])
        .unwrap();
    let actual = gix
        .log_touching_files(&t.main_repo, "main", &["touched.txt".to_string()])
        .unwrap();
    assert_eq!(expected, actual);
    assert!(expected.len() >= 2);
}

/// A worktree with a very large diff against the default branch yields a huge
/// pathspec list. Passing it on the command line overflows `ARG_MAX` and the
/// `execve` fails with `E2BIG` ("Argument list too long"). `log_touching_files`
/// must stream pathspecs via stdin so an arbitrarily large file set still works.
#[test]
fn log_touching_files_handles_arg_max_overflow() {
    let t = TestRepo::new();
    t.commit_file(&t.main_repo, "real.txt", "v1", "first version");

    // ~30k long, non-matching paths — comfortably past ARG_MAX (1 MiB on macOS)
    // so the old command-line implementation would fail to spawn `git`.
    let huge: Vec<String> = (0..30_000)
        .map(|i| format!("deeply/nested/generated/path/segment/number/{i:08}/file_{i:08}.json"))
        .collect();

    let real = RealGit;
    let result = real.log_touching_files(&t.main_repo, "main", &huge);

    // None of the fake paths were touched, so the result is empty — but the call
    // must succeed rather than erroring on a too-long argument list.
    assert!(
        result.is_ok(),
        "log_touching_files overflowed ARG_MAX: {:?}",
        result.err()
    );
    assert!(result.unwrap().is_empty());
}

#[test]
fn diff_commit_on_ref_passthrough() {
    let t = TestRepo::new();
    t.commit_file(&t.main_repo, "ref.txt", "data", "ref commit");
    let head = git(&t.main_repo, &["rev-parse", "HEAD"]);

    let real = RealGit;
    let gix = GixGitOps;

    let expected = real.diff_commit_on_ref(&t.main_repo, &head).unwrap();
    let actual = gix.diff_commit_on_ref(&t.main_repo, &head).unwrap();
    assert_eq!(expected, actual);
}

#[test]
fn log_file_history_passthrough() {
    let t = TestRepo::new();
    t.commit_file(&t.main_repo, "history.txt", "v1", "create file");
    t.commit_file(&t.main_repo, "history.txt", "v2", "update file");

    let real = RealGit;
    let gix = GixGitOps;

    let expected = real
        .log_file_history(&t.main_repo, "main", "history.txt")
        .unwrap();
    let actual = gix
        .log_file_history(&t.main_repo, "main", "history.txt")
        .unwrap();
    assert_eq!(expected, actual);
    assert!(expected.len() >= 2);
}

#[test]
fn last_commit_date_matches() {
    let t = TestRepo::new();

    let real = RealGit;
    let gix = GixGitOps;

    let expected = real.last_commit_date(&t.main_repo).unwrap();
    let actual = gix.last_commit_date(&t.main_repo).unwrap();
    assert!(expected.is_some());
    assert!(actual.is_some());
    assert_eq!(
        expected, actual,
        "last_commit_date mismatch: expected={expected:?}, actual={actual:?}"
    );
}

#[test]
fn last_commit_date_empty_repo() {
    let dir = tempfile::tempdir().unwrap();
    let repo = dir.path().join("empty-repo");
    std::fs::create_dir_all(&repo).unwrap();
    git(&repo, &["init", "-b", "main"]);

    let real = RealGit;
    let gix = GixGitOps;

    let expected = real.last_commit_date(&repo).unwrap();
    let actual = gix.last_commit_date(&repo).unwrap();
    assert_eq!(expected, actual);
    assert_eq!(expected, None);
}

// ── Phase 5: Status and config ops ────────────────────────────────────

#[test]
fn status_porcelain_passthrough() {
    let t = TestRepo::new();
    std::fs::write(t.main_repo.join("untracked.txt"), "new").unwrap();

    let real = RealGit;
    let gix = GixGitOps;

    let expected = real.status_porcelain(&t.main_repo).unwrap();
    let actual = gix.status_porcelain(&t.main_repo).unwrap();
    assert_eq!(expected, actual);
    assert!(!expected.is_empty());
}

#[test]
fn status_porcelain_clean() {
    let t = TestRepo::new();

    let real = RealGit;
    let gix = GixGitOps;

    let expected = real.status_porcelain(&t.main_repo).unwrap();
    let actual = gix.status_porcelain(&t.main_repo).unwrap();
    assert_eq!(expected, actual);
    assert!(expected.is_empty());
}

#[test]
fn config_list_local_matches() {
    let t = TestRepo::new();
    git(
        &t.main_repo,
        &["config", "--local", "user.email", "test@test.com"],
    );
    git(&t.main_repo, &["config", "--local", "user.name", "Test"]);

    let real = RealGit;
    let gix = GixGitOps;

    let mut expected = real.config_list_local(&t.main_repo).unwrap();
    let mut actual = gix.config_list_local(&t.main_repo).unwrap();
    expected.sort();
    actual.sort();
    assert_eq!(expected, actual, "config_list_local mismatch");
    assert!(!expected.is_empty());
}

#[test]
fn config_list_local_with_subsection() {
    let t = TestRepo::new();
    let _bare = t.set_up_remote();

    let real = RealGit;
    let gix = GixGitOps;

    let mut expected = real.config_list_local(&t.main_repo).unwrap();
    let mut actual = gix.config_list_local(&t.main_repo).unwrap();
    expected.sort();
    actual.sort();

    let has_remote = expected
        .iter()
        .any(|(k, _)| k.starts_with("remote.origin."));
    assert!(has_remote, "should have remote.origin config entries");
    assert_eq!(expected, actual, "config with subsections mismatch");
}

#[test]
fn config_remove_section_matches() {
    let t = TestRepo::new();
    git(
        &t.main_repo,
        &["config", "--local", "test-section.key", "value"],
    );

    let gix = GixGitOps;
    gix.config_remove_section(&t.main_repo, "test-section")
        .unwrap();

    let real = RealGit;
    let entries = real.config_list_local(&t.main_repo).unwrap();
    assert!(
        !entries.iter().any(|(k, _)| k.starts_with("test-section.")),
        "section should have been removed"
    );
}

#[test]
fn config_remove_section_nonexistent() {
    let t = TestRepo::new();

    let gix = GixGitOps;
    let result = gix.config_remove_section(&t.main_repo, "nonexistent");
    assert!(result.is_err());
}

// ── Phase 6: Remote ops ──────────────────────────────────────────────

#[test]
fn list_remotes_matches() {
    let t = TestRepo::new();
    let _bare = t.set_up_remote();
    t.add_remote("upstream", "/tmp/fake-upstream");

    let real = RealGit;
    let gix = GixGitOps;

    let mut expected = real.list_remotes(&t.main_repo).unwrap();
    let mut actual = gix.list_remotes(&t.main_repo).unwrap();
    expected.sort();
    actual.sort();
    assert_eq!(expected, actual, "list_remotes mismatch");
    assert!(expected.contains(&"origin".to_string()));
    assert!(expected.contains(&"upstream".to_string()));
}

#[test]
fn list_remotes_empty() {
    let t = TestRepo::new();

    let real = RealGit;
    let gix = GixGitOps;

    let expected = real.list_remotes(&t.main_repo).unwrap();
    let actual = gix.list_remotes(&t.main_repo).unwrap();
    assert_eq!(expected, actual);
    assert!(expected.is_empty());
}

#[test]
fn remote_url_matches() {
    let t = TestRepo::new();
    let bare = t.set_up_remote();

    let real = RealGit;
    let gix = GixGitOps;

    let expected = real.remote_url(&t.main_repo, "origin").unwrap();
    let actual = gix.remote_url(&t.main_repo, "origin").unwrap();
    assert_eq!(expected, actual);
    assert!(expected.contains(&bare.to_string_lossy().to_string()));
}

#[test]
fn remote_url_nonexistent() {
    let t = TestRepo::new();

    let gix = GixGitOps;
    let result = gix.remote_url(&t.main_repo, "noremote");
    assert!(result.is_err());
}

#[test]
fn list_remote_tracking_refs_matches() {
    let t = TestRepo::new();
    let _bare = t.set_up_remote();
    git(&t.main_repo, &["fetch"]);

    let real = RealGit;
    let gix = GixGitOps;

    let mut expected = real.list_remote_tracking_refs(&t.main_repo).unwrap();
    let mut actual = gix.list_remote_tracking_refs(&t.main_repo).unwrap();
    expected.sort();
    actual.sort();
    assert_eq!(expected, actual, "list_remote_tracking_refs mismatch");
    assert!(!expected.is_empty());
}

#[test]
fn list_remote_tracking_refs_no_remote() {
    let t = TestRepo::new();

    let real = RealGit;
    let gix = GixGitOps;

    let expected = real.list_remote_tracking_refs(&t.main_repo).unwrap();
    let actual = gix.list_remote_tracking_refs(&t.main_repo).unwrap();
    assert_eq!(expected, actual);
    assert!(expected.is_empty());
}

#[test]
fn ls_remote_check_passthrough() {
    let t = TestRepo::new();
    let _bare = t.set_up_remote();

    let real = RealGit;
    let gix = GixGitOps;

    let expected = real.ls_remote_check(&t.main_repo, "origin").unwrap();
    let actual = gix.ls_remote_check(&t.main_repo, "origin").unwrap();
    assert_eq!(expected, actual);
    assert!(expected);
}

#[test]
fn remote_remove_passthrough() {
    let t = TestRepo::new();
    t.add_remote("temp", "/tmp/fake");

    let gix = GixGitOps;
    gix.remote_remove(&t.main_repo, "temp").unwrap();

    let real = RealGit;
    let remotes = real.list_remotes(&t.main_repo).unwrap();
    assert!(!remotes.contains(&"temp".to_string()));
}

#[test]
fn prune_remote_refs_passthrough() {
    let t = TestRepo::new();
    let _bare = t.set_up_remote();
    git(&t.main_repo, &["fetch"]);

    let gix = GixGitOps;
    let result = gix.prune_remote_refs(&t.main_repo, "origin");
    assert!(result.is_ok());
}

// ── Phase 7: Tag ops ─────────────────────────────────────────────────

#[test]
fn list_local_tags_matches() {
    let t = TestRepo::new();
    t.create_tag("v1.0");
    t.create_annotated_tag("v2.0", "Release v2.0");

    let real = RealGit;
    let gix = GixGitOps;

    let mut expected = real.list_local_tags(&t.main_repo).unwrap();
    let mut actual = gix.list_local_tags(&t.main_repo).unwrap();
    expected.sort();
    actual.sort();
    assert_eq!(expected, actual, "list_local_tags mismatch");
    assert!(expected.contains(&"v1.0".to_string()));
    assert!(expected.contains(&"v2.0".to_string()));
}

#[test]
fn list_local_tags_empty() {
    let t = TestRepo::new();

    let real = RealGit;
    let gix = GixGitOps;

    let expected = real.list_local_tags(&t.main_repo).unwrap();
    let actual = gix.list_local_tags(&t.main_repo).unwrap();
    assert_eq!(expected, actual);
    assert!(expected.is_empty());
}

#[test]
fn tag_commit_lightweight() {
    let t = TestRepo::new();
    t.create_tag("v1.0");

    let real = RealGit;
    let gix = GixGitOps;

    let expected = real.tag_commit(&t.main_repo, "v1.0").unwrap();
    let actual = gix.tag_commit(&t.main_repo, "v1.0").unwrap();
    assert_eq!(expected, actual);

    let head = git(&t.main_repo, &["rev-parse", "HEAD"]);
    assert_eq!(expected, head);
}

#[test]
fn tag_commit_annotated() {
    let t = TestRepo::new();
    t.create_annotated_tag("v2.0", "Release v2.0");

    let real = RealGit;
    let gix = GixGitOps;

    let expected = real.tag_commit(&t.main_repo, "v2.0").unwrap();
    let actual = gix.tag_commit(&t.main_repo, "v2.0").unwrap();
    assert_eq!(
        expected, actual,
        "tag_commit should peel annotated tag to commit"
    );

    let head = git(&t.main_repo, &["rev-parse", "HEAD"]);
    assert_eq!(expected, head);
}

#[test]
fn is_tag_annotated_true() {
    let t = TestRepo::new();
    t.create_annotated_tag("v2.0", "Release v2.0");

    let real = RealGit;
    let gix = GixGitOps;

    let expected = real.is_tag_annotated(&t.main_repo, "v2.0").unwrap();
    let actual = gix.is_tag_annotated(&t.main_repo, "v2.0").unwrap();
    assert_eq!(expected, actual);
    assert!(expected);
}

#[test]
fn is_tag_annotated_false() {
    let t = TestRepo::new();
    t.create_tag("v1.0");

    let real = RealGit;
    let gix = GixGitOps;

    let expected = real.is_tag_annotated(&t.main_repo, "v1.0").unwrap();
    let actual = gix.is_tag_annotated(&t.main_repo, "v1.0").unwrap();
    assert_eq!(expected, actual);
    assert!(!expected);
}

#[test]
fn tag_date_annotated() {
    let t = TestRepo::new();
    t.create_annotated_tag("v2.0", "Release v2.0");

    let real = RealGit;
    let gix = GixGitOps;

    let expected = real.tag_date(&t.main_repo, "v2.0").unwrap();
    let actual = gix.tag_date(&t.main_repo, "v2.0").unwrap();
    assert!(expected.is_some(), "annotated tag should have a date");
    assert!(actual.is_some());
    assert_eq!(expected, actual, "tag_date mismatch for annotated tag");
}

#[test]
fn tag_date_lightweight() {
    let t = TestRepo::new();
    t.create_tag("v1.0");

    let real = RealGit;
    let gix = GixGitOps;

    let expected = real.tag_date(&t.main_repo, "v1.0").unwrap();
    let actual = gix.tag_date(&t.main_repo, "v1.0").unwrap();
    assert!(expected.is_some());
    assert!(actual.is_some());
    assert_eq!(expected, actual, "tag_date mismatch for lightweight tag");
}

#[test]
fn tag_delete_matches() {
    let t = TestRepo::new();
    t.create_tag("to-delete");

    let gix = GixGitOps;
    gix.tag_delete(&t.main_repo, "to-delete").unwrap();

    let real = RealGit;
    let tags = real.list_local_tags(&t.main_repo).unwrap();
    assert!(
        !tags.contains(&"to-delete".to_string()),
        "tag should have been deleted"
    );
}

#[test]
fn tag_delete_annotated() {
    let t = TestRepo::new();
    t.create_annotated_tag("ann-del", "will delete");

    let gix = GixGitOps;
    gix.tag_delete(&t.main_repo, "ann-del").unwrap();

    let real = RealGit;
    let tags = real.list_local_tags(&t.main_repo).unwrap();
    assert!(!tags.contains(&"ann-del".to_string()));
}

#[test]
fn tag_delete_nonexistent() {
    let t = TestRepo::new();

    let gix = GixGitOps;
    let result = gix.tag_delete(&t.main_repo, "nonexistent");
    assert!(result.is_err());
}

#[test]
fn tag_delete_remote_passthrough() {
    let t = TestRepo::new();
    let _bare = t.set_up_remote();
    t.create_tag("remote-tag");
    t.push_tag("origin", "remote-tag");

    let real = RealGit;
    let gix = GixGitOps;

    let expected = real.tag_delete_remote(&t.main_repo, "origin", "remote-tag");
    // Both should succeed (we test against a fresh bare repo each time)
    // Just verify the gix passthrough works with its own bare repo
    let t2 = TestRepo::new();
    let _bare2 = t2.set_up_remote();
    t2.create_tag("remote-tag2");
    t2.push_tag("origin", "remote-tag2");
    let actual = gix.tag_delete_remote(&t2.main_repo, "origin", "remote-tag2");
    assert!(expected.is_ok());
    assert!(actual.is_ok());
}

#[test]
fn list_remote_tags_passthrough() {
    let t = TestRepo::new();
    let _bare = t.set_up_remote();
    t.create_tag("v1.0");
    t.push_tag("origin", "v1.0");

    let real = RealGit;
    let gix = GixGitOps;

    let mut expected = real.list_remote_tags(&t.main_repo, "origin").unwrap();
    let mut actual = gix.list_remote_tags(&t.main_repo, "origin").unwrap();
    expected.sort();
    actual.sort();
    assert_eq!(expected, actual);
}

// ── Phase 8: Fetch and network ops (pass-through) ─────────────────────

#[test]
fn fetch_prune_passthrough() {
    let t = TestRepo::new();
    let _bare = t.set_up_remote();

    let gix = GixGitOps;
    let result = gix.fetch_prune(&t.main_repo);
    assert!(result.is_ok());
}

#[test]
fn delete_remote_branch_passthrough() {
    let t = TestRepo::new();
    let _bare = t.set_up_remote();
    git(&t.main_repo, &["checkout", "-b", "to-push"]);
    t.commit_file(&t.main_repo, "f.txt", "f", "branch commit");
    git(&t.main_repo, &["push", "origin", "to-push"]);
    git(&t.main_repo, &["checkout", "main"]);

    let gix = GixGitOps;
    let result = gix.delete_remote_branch(&t.main_repo, "origin", "to-push");
    assert!(result.is_ok());
}

// ── Phase 9: Mutating branch ops ──────────────────────────────────────

#[test]
fn branch_delete_matches() {
    let t = TestRepo::new();
    git(&t.main_repo, &["branch", "delete-me"]);

    let gix = GixGitOps;
    gix.branch_delete(&t.main_repo, "delete-me").unwrap();

    let real = RealGit;
    let branches = real.list_local_branches(&t.main_repo).unwrap();
    assert!(
        !branches.contains(&"delete-me".to_string()),
        "branch should have been deleted"
    );
}

#[test]
fn branch_delete_nonexistent() {
    let t = TestRepo::new();

    let gix = GixGitOps;
    let result = gix.branch_delete(&t.main_repo, "does-not-exist");
    assert!(result.is_err());
}

#[test]
fn branch_delete_safe_passthrough() {
    let t = TestRepo::new();
    t.create_merged_branch("merged-branch");

    let gix = GixGitOps;
    let result = gix.branch_delete_safe(&t.main_repo, "merged-branch");
    assert!(result.is_ok());

    let real = RealGit;
    let branches = real.list_local_branches(&t.main_repo).unwrap();
    assert!(!branches.contains(&"merged-branch".to_string()));
}

#[test]
fn is_branch_checked_out_passthrough() {
    let t = TestRepo::new();

    let real = RealGit;
    let gix = GixGitOps;

    let expected = real.is_branch_checked_out(&t.main_repo, "main").unwrap();
    let actual = gix.is_branch_checked_out(&t.main_repo, "main").unwrap();
    assert_eq!(expected, actual);
    assert!(expected);
}

// ── Phase 10: Stash ops (pass-through) ────────────────────────────────

#[test]
fn list_stashes_passthrough() {
    let t = TestRepo::new();
    t.create_stash("stashed.txt", "stash content");

    let real = RealGit;
    let gix = GixGitOps;

    let expected = real.list_stashes(&t.main_repo).unwrap();
    let actual = gix.list_stashes(&t.main_repo).unwrap();
    assert_eq!(expected, actual);
    assert_eq!(expected.len(), 1);
}

#[test]
fn list_stashes_empty() {
    let t = TestRepo::new();

    let real = RealGit;
    let gix = GixGitOps;

    let expected = real.list_stashes(&t.main_repo).unwrap();
    let actual = gix.list_stashes(&t.main_repo).unwrap();
    assert_eq!(expected, actual);
    assert!(expected.is_empty());
}

#[test]
fn stash_diff_passthrough() {
    let t = TestRepo::new();
    t.create_stash("stashed.txt", "stash content");

    let real = RealGit;
    let gix = GixGitOps;

    let expected = real.stash_diff(&t.main_repo, "stash@{0}").unwrap();
    let actual = gix.stash_diff(&t.main_repo, "stash@{0}").unwrap();
    assert_eq!(expected, actual);
    assert!(!expected.is_empty());
}

#[test]
fn stash_drop_passthrough() {
    let t = TestRepo::new();
    t.create_stash("stashed.txt", "stash content");

    let gix = GixGitOps;
    gix.stash_drop(&t.main_repo, "stash@{0}").unwrap();

    let real = RealGit;
    let stashes = real.list_stashes(&t.main_repo).unwrap();
    assert!(stashes.is_empty());
}

// ── Phase 10: Worktree mutation ops (pass-through) ────────────────────

#[test]
fn worktree_remove_passthrough() {
    let t = TestRepo::new();
    let wt = t.add_worktree("wt-remove", "branch-remove");

    let gix = GixGitOps;
    let result = gix.worktree_remove(&t.main_repo, &wt);
    assert!(result.is_ok());
}

#[test]
fn worktree_prune_passthrough() {
    let t = TestRepo::new();

    let gix = GixGitOps;
    let result = gix.worktree_prune(&t.main_repo);
    assert!(result.is_ok());
}

// ── Phase 10: LFS ops (pass-through) ────────────────────────────────

#[test]
fn list_builtin_commands_passthrough() {
    let real = RealGit;
    let gix = GixGitOps;

    let expected = real.list_builtin_commands().unwrap();
    let actual = gix.list_builtin_commands().unwrap();
    assert_eq!(expected, actual);
    assert!(!expected.is_empty());
}

#[test]
fn lfs_installed_passthrough() {
    let real = RealGit;
    let gix = GixGitOps;

    let expected = real.lfs_installed().unwrap();
    let actual = gix.lfs_installed().unwrap();
    assert_eq!(expected, actual);
}
