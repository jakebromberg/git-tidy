use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::process::Command;

use crate::error::Error;
use crate::git::{GitOps, GitResult};

/// Canonical key for a `files` argument: sorted + deduped. Lets the mock
/// look up registered responses without depending on caller iteration order.
fn normalize_file_set(files: &[String]) -> Vec<String> {
    let mut v: Vec<String> = files.to_vec();
    v.sort();
    v.dedup();
    v
}

/// Builder for constructing a MockGit with canned responses.
#[derive(Default)]
#[allow(clippy::type_complexity)]
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
    log_touching_files_for_files: HashMap<(PathBuf, String, Vec<String>), Vec<(String, String)>>,
    diff_commit_on_ref: HashMap<(PathBuf, String), String>,
    diff_working_tree_files: HashMap<(PathBuf, String), Vec<String>>,
    diff_working_tree_files_errors: HashMap<(PathBuf, String), String>,
    status_porcelain: HashMap<PathBuf, Vec<String>>,
    worktree_branch: HashMap<PathBuf, Option<String>>,
    rev_parse: HashMap<(PathBuf, String), String>,
    is_branch_checked_out: HashMap<(PathBuf, String), bool>,
    is_branch_checked_out_errors: HashMap<(PathBuf, String), String>,
    current_branch_errors: HashMap<PathBuf, String>,
    list_remotes_errors: HashMap<PathBuf, String>,
    list_remote_tracking_refs_errors: HashMap<PathBuf, String>,
    log_file_history: HashMap<(PathBuf, String, String), Vec<(String, String)>>,
    fetch_prune_calls: std::sync::Mutex<Vec<PathBuf>>,
    remove_calls: std::sync::Mutex<Vec<(PathBuf, PathBuf)>>,
    remove_force_calls: std::sync::Mutex<Vec<(PathBuf, PathBuf)>>,
    prune_calls: std::sync::Mutex<Vec<PathBuf>>,
    branch_delete_calls: std::sync::Mutex<Vec<(PathBuf, String)>>,
    branch_delete_errors: HashMap<(PathBuf, String), String>,
    worktree_remove_errors: HashMap<PathBuf, String>,
    worktree_remove_force_errors: HashMap<PathBuf, String>,
    worktree_list: HashMap<PathBuf, Vec<(PathBuf, Option<String>)>>,
    local_branches: HashMap<PathBuf, Vec<String>>,
    current_branch: HashMap<PathBuf, Option<String>>,
    upstream_branch: HashMap<(PathBuf, String), Option<String>>,
    branch_delete_safe_calls: std::sync::Mutex<Vec<(PathBuf, String)>>,
    branch_delete_safe_errors: HashMap<(PathBuf, String), String>,
    delete_remote_branch_calls: std::sync::Mutex<Vec<(PathBuf, String, String)>>,
    delete_remote_branch_errors: HashMap<(PathBuf, String, String), String>,
    // Stash operations
    stash_list: HashMap<PathBuf, Vec<(String, String, String)>>,
    stash_diff: HashMap<(PathBuf, String), String>,
    stash_drop_errors: HashMap<(PathBuf, String), String>,
    stash_drop_calls: std::sync::Mutex<Vec<(PathBuf, String)>>,
    // Remote operations
    list_remotes: HashMap<PathBuf, Vec<String>>,
    remote_url: HashMap<(PathBuf, String), String>,
    ls_remote_check: HashMap<(PathBuf, String), bool>,
    remote_remove_errors: HashMap<(PathBuf, String), String>,
    remote_remove_calls: std::sync::Mutex<Vec<(PathBuf, String)>>,
    remote_tracking_refs: HashMap<PathBuf, Vec<(String, String)>>,
    prune_remote_refs_result: HashMap<(PathBuf, String), usize>,
    prune_remote_refs_calls: std::sync::Mutex<Vec<(PathBuf, String)>>,
    // Tag operations
    local_tags: HashMap<PathBuf, Vec<String>>,
    remote_tags: HashMap<(PathBuf, String), Vec<(String, String)>>,
    tag_commit: HashMap<(PathBuf, String), String>,
    is_commit_reachable: HashMap<(PathBuf, String), bool>,
    tag_delete_errors: HashMap<(PathBuf, String), String>,
    tag_delete_calls: std::sync::Mutex<Vec<(PathBuf, String)>>,
    tag_delete_remote_errors: HashMap<(PathBuf, String, String), String>,
    tag_delete_remote_calls: std::sync::Mutex<Vec<(PathBuf, String, String)>>,
    is_tag_annotated: HashMap<(PathBuf, String), bool>,
    tag_date: HashMap<(PathBuf, String), Option<String>>,
    // Repo-level operations
    last_commit_date: HashMap<PathBuf, Option<String>>,
    // Config operations
    config_list_local: HashMap<PathBuf, Vec<(String, String)>>,
    config_remove_section_errors: HashMap<(PathBuf, String), String>,
    config_remove_section_calls: std::sync::Mutex<Vec<(PathBuf, String)>>,
    builtin_commands: Vec<String>,
    // LFS operations
    lfs_installed: bool,
    lfs_ls_files: HashMap<PathBuf, Vec<(String, char, String)>>,
    lfs_track_patterns: HashMap<PathBuf, Vec<String>>,
    lfs_prune_dry_run: HashMap<PathBuf, (usize, u64)>,
    lfs_prune_errors: HashMap<PathBuf, String>,
    lfs_prune_calls: std::sync::Mutex<Vec<PathBuf>>,
    find_large_blobs: HashMap<PathBuf, Vec<(String, u64, String)>>,
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

    pub fn with_is_branch_checked_out_error(
        mut self,
        repo: &Path,
        branch: &str,
        error: &str,
    ) -> Self {
        self.is_branch_checked_out_errors
            .insert((repo.to_path_buf(), branch.to_string()), error.to_string());
        self
    }

    pub fn with_current_branch_error(mut self, repo: &Path, error: &str) -> Self {
        self.current_branch_errors
            .insert(repo.to_path_buf(), error.to_string());
        self
    }

    pub fn with_list_remotes_error(mut self, repo: &Path, error: &str) -> Self {
        self.list_remotes_errors
            .insert(repo.to_path_buf(), error.to_string());
        self
    }

    pub fn with_list_remote_tracking_refs_error(mut self, repo: &Path, error: &str) -> Self {
        self.list_remote_tracking_refs_errors
            .insert(repo.to_path_buf(), error.to_string());
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

    /// Register a wildcard response: returned for any `files` argument on the
    /// matching `(repo, ref_spec)`. Useful for tests that don't care which
    /// files were passed.
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

    /// Register a file-specific response: returned only when `log_touching_files`
    /// is called with the exact same set of `files` (order-insensitive). Falls
    /// through to the wildcard registered via `with_log_touching_files` if no
    /// specific response matches. Tests that need to verify the caller is
    /// passing the right files must use this variant.
    pub fn with_log_touching_files_for_files(
        mut self,
        repo: &Path,
        ref_spec: &str,
        files: &[String],
        results: Vec<(String, String)>,
    ) -> Self {
        let key = normalize_file_set(files);
        self.log_touching_files_for_files
            .insert((repo.to_path_buf(), ref_spec.to_string(), key), results);
        self
    }

    pub fn with_diff_commit_on_ref(mut self, repo: &Path, commit: &str, diff: &str) -> Self {
        self.diff_commit_on_ref
            .insert((repo.to_path_buf(), commit.to_string()), diff.to_string());
        self
    }

    pub fn with_diff_working_tree_files(
        mut self,
        worktree_path: &Path,
        ref_spec: &str,
        files: Vec<String>,
    ) -> Self {
        self.diff_working_tree_files
            .insert((worktree_path.to_path_buf(), ref_spec.to_string()), files);
        self
    }

    pub fn with_diff_working_tree_files_error(
        mut self,
        worktree_path: &Path,
        ref_spec: &str,
        error: &str,
    ) -> Self {
        self.diff_working_tree_files_errors.insert(
            (worktree_path.to_path_buf(), ref_spec.to_string()),
            error.to_string(),
        );
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

    pub fn with_local_branches(mut self, repo: &Path, branches: Vec<String>) -> Self {
        self.local_branches.insert(repo.to_path_buf(), branches);
        self
    }

    pub fn with_current_branch(mut self, repo: &Path, branch: Option<&str>) -> Self {
        self.current_branch
            .insert(repo.to_path_buf(), branch.map(|s| s.to_string()));
        self
    }

    pub fn with_upstream_branch(
        mut self,
        repo: &Path,
        branch: &str,
        upstream: Option<&str>,
    ) -> Self {
        self.upstream_branch.insert(
            (repo.to_path_buf(), branch.to_string()),
            upstream.map(|s| s.to_string()),
        );
        self
    }

    pub fn with_branch_delete_error(mut self, repo: &Path, branch: &str, error: &str) -> Self {
        self.branch_delete_errors
            .insert((repo.to_path_buf(), branch.to_string()), error.to_string());
        self
    }

    pub fn with_branch_delete_safe_error(mut self, repo: &Path, branch: &str, error: &str) -> Self {
        self.branch_delete_safe_errors
            .insert((repo.to_path_buf(), branch.to_string()), error.to_string());
        self
    }

    pub fn with_delete_remote_branch_error(
        mut self,
        repo: &Path,
        remote: &str,
        branch: &str,
        error: &str,
    ) -> Self {
        self.delete_remote_branch_errors.insert(
            (repo.to_path_buf(), remote.to_string(), branch.to_string()),
            error.to_string(),
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

    pub fn with_worktree_list(
        mut self,
        repo: &Path,
        entries: Vec<(PathBuf, Option<String>)>,
    ) -> Self {
        self.worktree_list.insert(repo.to_path_buf(), entries);
        self
    }

    // --- Stash builder methods ---

    pub fn with_stash_list(mut self, repo: &Path, stashes: Vec<(String, String, String)>) -> Self {
        self.stash_list.insert(repo.to_path_buf(), stashes);
        self
    }

    pub fn with_stash_diff(mut self, repo: &Path, stash_ref: &str, diff: &str) -> Self {
        self.stash_diff.insert(
            (repo.to_path_buf(), stash_ref.to_string()),
            diff.to_string(),
        );
        self
    }

    pub fn with_stash_drop_error(mut self, repo: &Path, stash_ref: &str, error: &str) -> Self {
        self.stash_drop_errors.insert(
            (repo.to_path_buf(), stash_ref.to_string()),
            error.to_string(),
        );
        self
    }

    // --- Remote builder methods ---

    pub fn with_list_remotes(mut self, repo: &Path, remotes: Vec<String>) -> Self {
        self.list_remotes.insert(repo.to_path_buf(), remotes);
        self
    }

    pub fn with_remote_url(mut self, repo: &Path, remote: &str, url: &str) -> Self {
        self.remote_url
            .insert((repo.to_path_buf(), remote.to_string()), url.to_string());
        self
    }

    pub fn with_ls_remote_check(mut self, repo: &Path, remote: &str, reachable: bool) -> Self {
        self.ls_remote_check
            .insert((repo.to_path_buf(), remote.to_string()), reachable);
        self
    }

    pub fn with_remote_remove_error(mut self, repo: &Path, remote: &str, error: &str) -> Self {
        self.remote_remove_errors
            .insert((repo.to_path_buf(), remote.to_string()), error.to_string());
        self
    }

    pub fn with_remote_tracking_refs(mut self, repo: &Path, refs: Vec<(String, String)>) -> Self {
        self.remote_tracking_refs.insert(repo.to_path_buf(), refs);
        self
    }

    pub fn with_prune_remote_refs_result(
        mut self,
        repo: &Path,
        remote: &str,
        count: usize,
    ) -> Self {
        self.prune_remote_refs_result
            .insert((repo.to_path_buf(), remote.to_string()), count);
        self
    }

    // --- Tag builder methods ---

    pub fn with_local_tags(mut self, repo: &Path, tags: Vec<String>) -> Self {
        self.local_tags.insert(repo.to_path_buf(), tags);
        self
    }

    pub fn with_remote_tags(
        mut self,
        repo: &Path,
        remote: &str,
        tags: Vec<(String, String)>,
    ) -> Self {
        self.remote_tags
            .insert((repo.to_path_buf(), remote.to_string()), tags);
        self
    }

    pub fn with_tag_commit(mut self, repo: &Path, tag: &str, commit: &str) -> Self {
        self.tag_commit
            .insert((repo.to_path_buf(), tag.to_string()), commit.to_string());
        self
    }

    pub fn with_is_commit_reachable(mut self, repo: &Path, commit: &str, reachable: bool) -> Self {
        self.is_commit_reachable
            .insert((repo.to_path_buf(), commit.to_string()), reachable);
        self
    }

    pub fn with_tag_delete_error(mut self, repo: &Path, tag: &str, error: &str) -> Self {
        self.tag_delete_errors
            .insert((repo.to_path_buf(), tag.to_string()), error.to_string());
        self
    }

    pub fn with_tag_delete_remote_error(
        mut self,
        repo: &Path,
        remote: &str,
        tag: &str,
        error: &str,
    ) -> Self {
        self.tag_delete_remote_errors.insert(
            (repo.to_path_buf(), remote.to_string(), tag.to_string()),
            error.to_string(),
        );
        self
    }

    pub fn with_is_tag_annotated(mut self, repo: &Path, tag: &str, annotated: bool) -> Self {
        self.is_tag_annotated
            .insert((repo.to_path_buf(), tag.to_string()), annotated);
        self
    }

    pub fn with_tag_date(mut self, repo: &Path, tag: &str, date: Option<&str>) -> Self {
        self.tag_date.insert(
            (repo.to_path_buf(), tag.to_string()),
            date.map(|s| s.to_string()),
        );
        self
    }

    // --- Repo-level builder methods ---

    pub fn with_last_commit_date(mut self, repo: &Path, date: Option<&str>) -> Self {
        self.last_commit_date
            .insert(repo.to_path_buf(), date.map(|s| s.to_string()));
        // --- Config builder methods ---
        self
    }

    pub fn with_config_list_local(mut self, repo: &Path, entries: Vec<(String, String)>) -> Self {
        self.config_list_local.insert(repo.to_path_buf(), entries);
        self
    }

    pub fn with_config_remove_section_error(
        mut self,
        repo: &Path,
        section: &str,
        error: &str,
    ) -> Self {
        self.config_remove_section_errors
            .insert((repo.to_path_buf(), section.to_string()), error.to_string());
        self
    }

    pub fn with_builtin_commands(mut self, commands: Vec<String>) -> Self {
        self.builtin_commands = commands;
        self
    }

    // --- LFS builder methods ---

    pub fn with_lfs_installed(mut self, installed: bool) -> Self {
        self.lfs_installed = installed;
        self
    }

    pub fn with_lfs_ls_files(mut self, repo: &Path, files: Vec<(String, char, String)>) -> Self {
        self.lfs_ls_files.insert(repo.to_path_buf(), files);
        self
    }

    pub fn with_lfs_track_patterns(mut self, repo: &Path, patterns: Vec<String>) -> Self {
        self.lfs_track_patterns.insert(repo.to_path_buf(), patterns);
        self
    }

    pub fn with_lfs_prune_dry_run(mut self, repo: &Path, count: usize, bytes: u64) -> Self {
        self.lfs_prune_dry_run
            .insert(repo.to_path_buf(), (count, bytes));
        self
    }

    pub fn with_lfs_prune_error(mut self, repo: &Path, error: &str) -> Self {
        self.lfs_prune_errors
            .insert(repo.to_path_buf(), error.to_string());
        self
    }

    pub fn with_find_large_blobs(mut self, repo: &Path, blobs: Vec<(String, u64, String)>) -> Self {
        self.find_large_blobs.insert(repo.to_path_buf(), blobs);
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
            log_touching_files_for_files: self.log_touching_files_for_files,
            diff_commit_on_ref: self.diff_commit_on_ref,
            diff_working_tree_files: self.diff_working_tree_files,
            diff_working_tree_files_errors: self.diff_working_tree_files_errors,
            status_porcelain: self.status_porcelain,
            worktree_branch: self.worktree_branch,
            rev_parse: self.rev_parse,
            is_branch_checked_out: self.is_branch_checked_out,
            is_branch_checked_out_errors: self.is_branch_checked_out_errors,
            current_branch_errors: self.current_branch_errors,
            list_remotes_errors: self.list_remotes_errors,
            list_remote_tracking_refs_errors: self.list_remote_tracking_refs_errors,
            log_file_history: self.log_file_history,
            fetch_prune_calls: self.fetch_prune_calls,
            remove_calls: self.remove_calls,
            remove_force_calls: self.remove_force_calls,
            prune_calls: self.prune_calls,
            branch_delete_calls: self.branch_delete_calls,
            branch_delete_errors: self.branch_delete_errors,
            worktree_remove_errors: self.worktree_remove_errors,
            worktree_remove_force_errors: self.worktree_remove_force_errors,
            worktree_list: self.worktree_list,
            local_branches: self.local_branches,
            current_branch: self.current_branch,
            upstream_branch: self.upstream_branch,
            branch_delete_safe_calls: self.branch_delete_safe_calls,
            branch_delete_safe_errors: self.branch_delete_safe_errors,
            delete_remote_branch_calls: self.delete_remote_branch_calls,
            delete_remote_branch_errors: self.delete_remote_branch_errors,
            stash_list: self.stash_list,
            stash_diff: self.stash_diff,
            stash_drop_errors: self.stash_drop_errors,
            stash_drop_calls: self.stash_drop_calls,
            list_remotes: self.list_remotes,
            remote_url: self.remote_url,
            ls_remote_check: self.ls_remote_check,
            remote_remove_errors: self.remote_remove_errors,
            remote_remove_calls: self.remote_remove_calls,
            remote_tracking_refs: self.remote_tracking_refs,
            prune_remote_refs_result: self.prune_remote_refs_result,
            prune_remote_refs_calls: self.prune_remote_refs_calls,
            local_tags: self.local_tags,
            remote_tags: self.remote_tags,
            tag_commit: self.tag_commit,
            is_commit_reachable: self.is_commit_reachable,
            tag_delete_errors: self.tag_delete_errors,
            tag_delete_calls: self.tag_delete_calls,
            tag_delete_remote_errors: self.tag_delete_remote_errors,
            tag_delete_remote_calls: self.tag_delete_remote_calls,
            is_tag_annotated: self.is_tag_annotated,
            tag_date: self.tag_date,
            last_commit_date: self.last_commit_date,
            config_list_local: self.config_list_local,
            config_remove_section_errors: self.config_remove_section_errors,
            config_remove_section_calls: self.config_remove_section_calls,
            builtin_commands: self.builtin_commands,
            lfs_installed: self.lfs_installed,
            lfs_ls_files: self.lfs_ls_files,
            lfs_track_patterns: self.lfs_track_patterns,
            lfs_prune_dry_run: self.lfs_prune_dry_run,
            lfs_prune_errors: self.lfs_prune_errors,
            lfs_prune_calls: self.lfs_prune_calls,
            find_large_blobs: self.find_large_blobs,
        }
    }
}

#[allow(clippy::type_complexity)]
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
    log_touching_files_for_files: HashMap<(PathBuf, String, Vec<String>), Vec<(String, String)>>,
    diff_commit_on_ref: HashMap<(PathBuf, String), String>,
    diff_working_tree_files: HashMap<(PathBuf, String), Vec<String>>,
    diff_working_tree_files_errors: HashMap<(PathBuf, String), String>,
    status_porcelain: HashMap<PathBuf, Vec<String>>,
    worktree_branch: HashMap<PathBuf, Option<String>>,
    rev_parse: HashMap<(PathBuf, String), String>,
    is_branch_checked_out: HashMap<(PathBuf, String), bool>,
    is_branch_checked_out_errors: HashMap<(PathBuf, String), String>,
    current_branch_errors: HashMap<PathBuf, String>,
    list_remotes_errors: HashMap<PathBuf, String>,
    list_remote_tracking_refs_errors: HashMap<PathBuf, String>,
    log_file_history: HashMap<(PathBuf, String, String), Vec<(String, String)>>,
    fetch_prune_calls: std::sync::Mutex<Vec<PathBuf>>,
    remove_calls: std::sync::Mutex<Vec<(PathBuf, PathBuf)>>,
    remove_force_calls: std::sync::Mutex<Vec<(PathBuf, PathBuf)>>,
    prune_calls: std::sync::Mutex<Vec<PathBuf>>,
    branch_delete_calls: std::sync::Mutex<Vec<(PathBuf, String)>>,
    branch_delete_errors: HashMap<(PathBuf, String), String>,
    worktree_remove_errors: HashMap<PathBuf, String>,
    worktree_remove_force_errors: HashMap<PathBuf, String>,
    worktree_list: HashMap<PathBuf, Vec<(PathBuf, Option<String>)>>,
    local_branches: HashMap<PathBuf, Vec<String>>,
    current_branch: HashMap<PathBuf, Option<String>>,
    upstream_branch: HashMap<(PathBuf, String), Option<String>>,
    branch_delete_safe_calls: std::sync::Mutex<Vec<(PathBuf, String)>>,
    branch_delete_safe_errors: HashMap<(PathBuf, String), String>,
    delete_remote_branch_calls: std::sync::Mutex<Vec<(PathBuf, String, String)>>,
    delete_remote_branch_errors: HashMap<(PathBuf, String, String), String>,
    // Stash operations
    stash_list: HashMap<PathBuf, Vec<(String, String, String)>>,
    stash_diff: HashMap<(PathBuf, String), String>,
    stash_drop_errors: HashMap<(PathBuf, String), String>,
    stash_drop_calls: std::sync::Mutex<Vec<(PathBuf, String)>>,
    // Remote operations
    list_remotes: HashMap<PathBuf, Vec<String>>,
    remote_url: HashMap<(PathBuf, String), String>,
    ls_remote_check: HashMap<(PathBuf, String), bool>,
    remote_remove_errors: HashMap<(PathBuf, String), String>,
    remote_remove_calls: std::sync::Mutex<Vec<(PathBuf, String)>>,
    remote_tracking_refs: HashMap<PathBuf, Vec<(String, String)>>,
    prune_remote_refs_result: HashMap<(PathBuf, String), usize>,
    prune_remote_refs_calls: std::sync::Mutex<Vec<(PathBuf, String)>>,
    // Tag operations
    local_tags: HashMap<PathBuf, Vec<String>>,
    remote_tags: HashMap<(PathBuf, String), Vec<(String, String)>>,
    tag_commit: HashMap<(PathBuf, String), String>,
    is_commit_reachable: HashMap<(PathBuf, String), bool>,
    tag_delete_errors: HashMap<(PathBuf, String), String>,
    tag_delete_calls: std::sync::Mutex<Vec<(PathBuf, String)>>,
    tag_delete_remote_errors: HashMap<(PathBuf, String, String), String>,
    tag_delete_remote_calls: std::sync::Mutex<Vec<(PathBuf, String, String)>>,
    is_tag_annotated: HashMap<(PathBuf, String), bool>,
    tag_date: HashMap<(PathBuf, String), Option<String>>,
    // Repo-level operations
    last_commit_date: HashMap<PathBuf, Option<String>>,
    // Config operations
    config_list_local: HashMap<PathBuf, Vec<(String, String)>>,
    config_remove_section_errors: HashMap<(PathBuf, String), String>,
    config_remove_section_calls: std::sync::Mutex<Vec<(PathBuf, String)>>,
    builtin_commands: Vec<String>,
    // LFS operations
    lfs_installed: bool,
    lfs_ls_files: HashMap<PathBuf, Vec<(String, char, String)>>,
    lfs_track_patterns: HashMap<PathBuf, Vec<String>>,
    lfs_prune_dry_run: HashMap<PathBuf, (usize, u64)>,
    lfs_prune_errors: HashMap<PathBuf, String>,
    lfs_prune_calls: std::sync::Mutex<Vec<PathBuf>>,
    find_large_blobs: HashMap<PathBuf, Vec<(String, u64, String)>>,
}

impl MockGit {
    pub fn fetch_prune_calls(&self) -> Vec<PathBuf> {
        self.fetch_prune_calls.lock().unwrap().clone()
    }

    pub fn remove_calls(&self) -> Vec<(PathBuf, PathBuf)> {
        self.remove_calls.lock().unwrap().clone()
    }

    pub fn remove_force_calls(&self) -> Vec<(PathBuf, PathBuf)> {
        self.remove_force_calls.lock().unwrap().clone()
    }

    pub fn branch_delete_calls(&self) -> Vec<(PathBuf, String)> {
        self.branch_delete_calls.lock().unwrap().clone()
    }

    pub fn branch_delete_safe_calls(&self) -> Vec<(PathBuf, String)> {
        self.branch_delete_safe_calls.lock().unwrap().clone()
    }

    pub fn delete_remote_branch_calls(&self) -> Vec<(PathBuf, String, String)> {
        self.delete_remote_branch_calls.lock().unwrap().clone()
    }

    pub fn stash_drop_calls(&self) -> Vec<(PathBuf, String)> {
        self.stash_drop_calls.lock().unwrap().clone()
    }

    pub fn remote_remove_calls(&self) -> Vec<(PathBuf, String)> {
        self.remote_remove_calls.lock().unwrap().clone()
    }

    pub fn prune_remote_refs_calls(&self) -> Vec<(PathBuf, String)> {
        self.prune_remote_refs_calls.lock().unwrap().clone()
    }

    pub fn tag_delete_calls(&self) -> Vec<(PathBuf, String)> {
        self.tag_delete_calls.lock().unwrap().clone()
    }

    pub fn tag_delete_remote_calls(&self) -> Vec<(PathBuf, String, String)> {
        self.tag_delete_remote_calls.lock().unwrap().clone()
    }

    pub fn config_remove_section_calls(&self) -> Vec<(PathBuf, String)> {
        self.config_remove_section_calls.lock().unwrap().clone()
    }

    pub fn lfs_prune_calls(&self) -> Vec<PathBuf> {
        self.lfs_prune_calls.lock().unwrap().clone()
    }
}

impl GitOps for MockGit {
    fn fetch_prune(&self, repo: &Path) -> GitResult<()> {
        self.fetch_prune_calls
            .lock()
            .unwrap()
            .push(repo.to_path_buf());
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
        files: &[String],
    ) -> GitResult<Vec<(String, String)>> {
        let key = (
            repo.to_path_buf(),
            ref_spec.to_string(),
            normalize_file_set(files),
        );
        if let Some(specific) = self.log_touching_files_for_files.get(&key) {
            return Ok(specific.clone());
        }
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

    fn diff_working_tree_files(
        &self,
        worktree_path: &Path,
        ref_spec: &str,
    ) -> GitResult<Vec<String>> {
        let key = (worktree_path.to_path_buf(), ref_spec.to_string());
        if let Some(err) = self.diff_working_tree_files_errors.get(&key) {
            return Err(Error::GitCommand {
                command: format!("diff --name-only {ref_spec}"),
                message: err.clone(),
            });
        }
        Ok(self
            .diff_working_tree_files
            .get(&key)
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
            .lock()
            .unwrap()
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
            .lock()
            .unwrap()
            .push((repo.to_path_buf(), worktree_path.to_path_buf()));
        Ok(())
    }

    fn worktree_prune(&self, repo: &Path) -> GitResult<()> {
        self.prune_calls.lock().unwrap().push(repo.to_path_buf());
        Ok(())
    }

    fn worktree_list(&self, repo: &Path) -> GitResult<Vec<(PathBuf, Option<String>)>> {
        Ok(self
            .worktree_list
            .get(&repo.to_path_buf())
            .cloned()
            .unwrap_or_default())
    }

    fn branch_delete(&self, repo: &Path, branch: &str) -> GitResult<()> {
        if let Some(err) = self
            .branch_delete_errors
            .get(&(repo.to_path_buf(), branch.to_string()))
        {
            return Err(Error::GitCommand {
                command: format!("branch -D {branch}"),
                message: err.clone(),
            });
        }
        self.branch_delete_calls
            .lock()
            .unwrap()
            .push((repo.to_path_buf(), branch.to_string()));
        Ok(())
    }

    fn is_branch_checked_out(&self, repo: &Path, branch: &str) -> GitResult<bool> {
        let key = (repo.to_path_buf(), branch.to_string());
        if let Some(err) = self.is_branch_checked_out_errors.get(&key) {
            return Err(Error::GitCommand {
                command: format!("worktree list (looking for {branch})"),
                message: err.clone(),
            });
        }
        Ok(*self.is_branch_checked_out.get(&key).unwrap_or(&false))
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

    fn list_local_branches(&self, repo: &Path) -> GitResult<Vec<String>> {
        Ok(self
            .local_branches
            .get(&repo.to_path_buf())
            .cloned()
            .unwrap_or_default())
    }

    fn branch_delete_safe(&self, repo: &Path, branch: &str) -> GitResult<()> {
        if let Some(err) = self
            .branch_delete_safe_errors
            .get(&(repo.to_path_buf(), branch.to_string()))
        {
            return Err(Error::GitCommand {
                command: format!("branch -d {branch}"),
                message: err.clone(),
            });
        }
        self.branch_delete_safe_calls
            .lock()
            .unwrap()
            .push((repo.to_path_buf(), branch.to_string()));
        Ok(())
    }

    fn current_branch(&self, repo: &Path) -> GitResult<Option<String>> {
        if let Some(err) = self.current_branch_errors.get(&repo.to_path_buf()) {
            return Err(Error::GitCommand {
                command: "symbolic-ref HEAD".to_string(),
                message: err.clone(),
            });
        }
        Ok(self
            .current_branch
            .get(&repo.to_path_buf())
            .cloned()
            .unwrap_or(None))
    }

    fn upstream_branch(&self, repo: &Path, branch: &str) -> GitResult<Option<String>> {
        Ok(self
            .upstream_branch
            .get(&(repo.to_path_buf(), branch.to_string()))
            .cloned()
            .unwrap_or(None))
    }

    fn delete_remote_branch(&self, repo: &Path, remote: &str, branch: &str) -> GitResult<()> {
        if let Some(err) = self.delete_remote_branch_errors.get(&(
            repo.to_path_buf(),
            remote.to_string(),
            branch.to_string(),
        )) {
            return Err(Error::GitCommand {
                command: format!("push {remote} --delete {branch}"),
                message: err.clone(),
            });
        }
        self.delete_remote_branch_calls.lock().unwrap().push((
            repo.to_path_buf(),
            remote.to_string(),
            branch.to_string(),
        ));
        Ok(())
    }

    // --- Remote operations ---

    fn list_remotes(&self, repo: &Path) -> GitResult<Vec<String>> {
        if let Some(err) = self.list_remotes_errors.get(&repo.to_path_buf()) {
            return Err(Error::GitCommand {
                command: "remote".to_string(),
                message: err.clone(),
            });
        }
        Ok(self
            .list_remotes
            .get(&repo.to_path_buf())
            .cloned()
            .unwrap_or_default())
    }

    fn remote_url(&self, repo: &Path, remote: &str) -> GitResult<String> {
        self.remote_url
            .get(&(repo.to_path_buf(), remote.to_string()))
            .cloned()
            .ok_or_else(|| Error::GitCommand {
                command: format!("remote get-url {remote}"),
                message: "not found in mock".to_string(),
            })
    }

    fn ls_remote_check(&self, repo: &Path, remote: &str) -> GitResult<bool> {
        Ok(*self
            .ls_remote_check
            .get(&(repo.to_path_buf(), remote.to_string()))
            .unwrap_or(&false))
    }

    fn remote_remove(&self, repo: &Path, remote: &str) -> GitResult<()> {
        if let Some(err) = self
            .remote_remove_errors
            .get(&(repo.to_path_buf(), remote.to_string()))
        {
            return Err(Error::RemoteRemovalFailed {
                repo: repo.to_path_buf(),
                remote: remote.to_string(),
                reason: err.clone(),
            });
        }
        self.remote_remove_calls
            .lock()
            .unwrap()
            .push((repo.to_path_buf(), remote.to_string()));
        Ok(())
    }

    fn list_remote_tracking_refs(&self, repo: &Path) -> GitResult<Vec<(String, String)>> {
        if let Some(err) = self
            .list_remote_tracking_refs_errors
            .get(&repo.to_path_buf())
        {
            return Err(Error::GitCommand {
                command: "for-each-ref refs/remotes/".to_string(),
                message: err.clone(),
            });
        }
        Ok(self
            .remote_tracking_refs
            .get(&repo.to_path_buf())
            .cloned()
            .unwrap_or_default())
    }

    fn prune_remote_refs(&self, repo: &Path, remote: &str) -> GitResult<usize> {
        self.prune_remote_refs_calls
            .lock()
            .unwrap()
            .push((repo.to_path_buf(), remote.to_string()));
        Ok(*self
            .prune_remote_refs_result
            .get(&(repo.to_path_buf(), remote.to_string()))
            .unwrap_or(&0))
    }

    // --- Stash operations ---

    fn list_stashes(&self, repo: &Path) -> GitResult<Vec<(String, String, String)>> {
        Ok(self
            .stash_list
            .get(&repo.to_path_buf())
            .cloned()
            .unwrap_or_default())
    }

    fn stash_diff(&self, repo: &Path, stash_ref: &str) -> GitResult<String> {
        Ok(self
            .stash_diff
            .get(&(repo.to_path_buf(), stash_ref.to_string()))
            .cloned()
            .unwrap_or_default())
    }

    fn stash_drop(&self, repo: &Path, stash_ref: &str) -> GitResult<()> {
        if let Some(err) = self
            .stash_drop_errors
            .get(&(repo.to_path_buf(), stash_ref.to_string()))
        {
            return Err(Error::StashDropFailed {
                repo: repo.to_path_buf(),
                stash_ref: stash_ref.to_string(),
                reason: err.clone(),
            });
        }
        self.stash_drop_calls
            .lock()
            .unwrap()
            .push((repo.to_path_buf(), stash_ref.to_string()));
        Ok(())
    }

    // --- Tag operations ---

    fn list_local_tags(&self, repo: &Path) -> GitResult<Vec<String>> {
        Ok(self
            .local_tags
            .get(&repo.to_path_buf())
            .cloned()
            .unwrap_or_default())
    }

    fn list_remote_tags(&self, repo: &Path, remote: &str) -> GitResult<Vec<(String, String)>> {
        Ok(self
            .remote_tags
            .get(&(repo.to_path_buf(), remote.to_string()))
            .cloned()
            .unwrap_or_default())
    }

    fn tag_commit(&self, repo: &Path, tag: &str) -> GitResult<String> {
        self.tag_commit
            .get(&(repo.to_path_buf(), tag.to_string()))
            .cloned()
            .ok_or_else(|| Error::GitCommand {
                command: format!("rev-parse {tag}^{{commit}}"),
                message: "not found in mock".to_string(),
            })
    }

    fn is_commit_reachable(&self, repo: &Path, commit: &str) -> GitResult<bool> {
        Ok(*self
            .is_commit_reachable
            .get(&(repo.to_path_buf(), commit.to_string()))
            .unwrap_or(&false))
    }

    fn tag_delete(&self, repo: &Path, tag: &str) -> GitResult<()> {
        if let Some(err) = self
            .tag_delete_errors
            .get(&(repo.to_path_buf(), tag.to_string()))
        {
            return Err(Error::TagDeletionFailed {
                repo: repo.to_path_buf(),
                tag: tag.to_string(),
                reason: err.clone(),
            });
        }
        self.tag_delete_calls
            .lock()
            .unwrap()
            .push((repo.to_path_buf(), tag.to_string()));
        Ok(())
    }

    fn tag_delete_remote(&self, repo: &Path, remote: &str, tag: &str) -> GitResult<()> {
        if let Some(err) = self.tag_delete_remote_errors.get(&(
            repo.to_path_buf(),
            remote.to_string(),
            tag.to_string(),
        )) {
            return Err(Error::TagDeletionFailed {
                repo: repo.to_path_buf(),
                tag: tag.to_string(),
                reason: err.clone(),
            });
        }
        self.tag_delete_remote_calls.lock().unwrap().push((
            repo.to_path_buf(),
            remote.to_string(),
            tag.to_string(),
        ));
        Ok(())
    }

    fn is_tag_annotated(&self, repo: &Path, tag: &str) -> GitResult<bool> {
        Ok(*self
            .is_tag_annotated
            .get(&(repo.to_path_buf(), tag.to_string()))
            .unwrap_or(&false))
    }

    fn tag_date(&self, repo: &Path, tag: &str) -> GitResult<Option<String>> {
        Ok(self
            .tag_date
            .get(&(repo.to_path_buf(), tag.to_string()))
            .cloned()
            .unwrap_or(None))
    }

    // --- Repo-level operations ---

    fn last_commit_date(&self, repo: &Path) -> GitResult<Option<String>> {
        Ok(self
            .last_commit_date
            .get(&repo.to_path_buf())
            .cloned()
            .unwrap_or(None))
    }
    // --- Config operations ---

    fn config_list_local(&self, repo: &Path) -> GitResult<Vec<(String, String)>> {
        Ok(self
            .config_list_local
            .get(&repo.to_path_buf())
            .cloned()
            .unwrap_or_default())
    }

    // --- LFS operations ---

    fn lfs_installed(&self) -> GitResult<bool> {
        Ok(self.lfs_installed)
    }

    fn lfs_ls_files(&self, repo: &Path) -> GitResult<Vec<(String, char, String)>> {
        Ok(self
            .lfs_ls_files
            .get(&repo.to_path_buf())
            .cloned()
            .unwrap_or_default())
    }

    fn config_remove_section(&self, repo: &Path, section: &str) -> GitResult<()> {
        if let Some(err) = self
            .config_remove_section_errors
            .get(&(repo.to_path_buf(), section.to_string()))
        {
            return Err(Error::ConfigRemoveSectionFailed {
                repo: repo.to_path_buf(),
                section: section.to_string(),
                reason: err.clone(),
            });
        }
        self.config_remove_section_calls
            .lock()
            .unwrap()
            .push((repo.to_path_buf(), section.to_string()));
        Ok(())
    }

    fn list_builtin_commands(&self) -> GitResult<Vec<String>> {
        Ok(self.builtin_commands.clone())
    }

    fn lfs_track_patterns(&self, repo: &Path) -> GitResult<Vec<String>> {
        Ok(self
            .lfs_track_patterns
            .get(&repo.to_path_buf())
            .cloned()
            .unwrap_or_default())
    }

    fn lfs_prune_dry_run(&self, repo: &Path) -> GitResult<(usize, u64)> {
        Ok(*self
            .lfs_prune_dry_run
            .get(&repo.to_path_buf())
            .unwrap_or(&(0, 0)))
    }

    fn lfs_prune(&self, repo: &Path) -> GitResult<()> {
        if let Some(err) = self.lfs_prune_errors.get(&repo.to_path_buf()) {
            return Err(Error::LfsPruneFailed {
                repo: repo.to_path_buf(),
                reason: err.clone(),
            });
        }
        self.lfs_prune_calls
            .lock()
            .unwrap()
            .push(repo.to_path_buf());
        Ok(())
    }

    fn find_large_blobs(
        &self,
        repo: &Path,
        threshold: u64,
        _depth: usize,
    ) -> GitResult<Vec<(String, u64, String)>> {
        // Mirror RealGit: include blobs whose size is at or above the threshold (RealGit skips with `size < threshold`). Depth is not modelled here — tests register a flat blob list rather than ref trees — but the threshold filter is what catches caller bugs (passing the wrong unit, forgetting to plumb the flag, etc.).
        Ok(self
            .find_large_blobs
            .get(&repo.to_path_buf())
            .cloned()
            .unwrap_or_default()
            .into_iter()
            .filter(|(_, size, _)| *size >= threshold)
            .collect())
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

impl TestRepo {
    /// Create a local branch (without checking it out).
    pub fn create_branch(&self, name: &str) {
        git(&self.main_repo, &["branch", name]);
    }

    /// Create a branch with a commit, then merge it into main.
    pub fn create_merged_branch(&self, name: &str) {
        git(&self.main_repo, &["checkout", "-b", name]);
        let filename = format!("{name}.txt");
        self.commit_file(
            &self.main_repo,
            &filename,
            "merged content",
            &format!("work on {name}"),
        );
        git(&self.main_repo, &["checkout", "main"]);
        git(
            &self.main_repo,
            &["merge", "--no-ff", name, "-m", &format!("Merge {name}")],
        );
    }

    /// Set up a bare remote and push main to it.
    /// Returns the path to the bare repo.
    pub fn set_up_remote(&self) -> PathBuf {
        let bare = self.dir.path().join("remote.git");
        std::fs::create_dir_all(&bare).unwrap();
        git(&bare, &["init", "--bare"]);
        git(
            &self.main_repo,
            &["remote", "add", "origin", &bare.to_string_lossy()],
        );
        git(&self.main_repo, &["push", "-u", "origin", "main"]);
        bare
    }

    /// Add a remote to the repo.
    pub fn add_remote(&self, name: &str, url: &str) {
        git(&self.main_repo, &["remote", "add", name, url]);
    }

    /// Create a stash by writing a file and running `git stash`.
    pub fn create_stash(&self, filename: &str, content: &str) {
        let file = self.main_repo.join(filename);
        std::fs::write(&file, content).unwrap();
        git(&self.main_repo, &["add", filename]);
        git(&self.main_repo, &["stash"]);
    }

    /// Create a stash on a specific branch.
    pub fn create_stash_on_branch(&self, branch: &str, filename: &str, content: &str) {
        git(&self.main_repo, &["checkout", "-b", branch]);
        let file = self.main_repo.join(filename);
        std::fs::write(&file, content).unwrap();
        git(&self.main_repo, &["add", filename]);
        git(&self.main_repo, &["stash"]);
        git(&self.main_repo, &["checkout", "main"]);
    }
}

impl TestRepo {
    /// Create a lightweight tag on the current HEAD.
    pub fn create_tag(&self, name: &str) {
        git(&self.main_repo, &["tag", name]);
    }

    /// Create an annotated tag on the current HEAD.
    pub fn create_annotated_tag(&self, name: &str, message: &str) {
        git(&self.main_repo, &["tag", "-a", name, "-m", message]);
    }

    /// Push a tag to the given remote.
    pub fn push_tag(&self, remote: &str, tag: &str) {
        git(&self.main_repo, &["push", remote, tag]);
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

/// Set up a repo inside a scan directory so `discover_repos` finds it.
///
/// Returns `(tempdir, repo_path)`; the tempdir guards the lifetime of the
/// on-disk repo and must be kept alive for the duration of the test.
pub fn set_up_repo() -> (tempfile::TempDir, PathBuf) {
    let dir = tempfile::tempdir().unwrap();
    let base = dir.path().canonicalize().unwrap();

    let scan_dir = base.join("projects");
    std::fs::create_dir_all(&scan_dir).unwrap();

    let repo = scan_dir.join("my-repo");
    std::fs::create_dir_all(&repo).unwrap();
    git(&repo, &["init", "-b", "main"]);
    git(&repo, &["config", "user.email", "test@test.com"]);
    git(&repo, &["config", "user.name", "Test"]);

    std::fs::write(repo.join("README.md"), "# Test\n").unwrap();
    git(&repo, &["add", "README.md"]);
    git(&repo, &["commit", "-m", "Initial commit"]);

    (dir, repo)
}

/// Set up a repo with an `origin` remote so default-branch detection works.
///
/// Returns `(tempdir, repo_path)`; the tempdir guards the lifetime of the
/// on-disk repo and must be kept alive for the duration of the test.
pub fn set_up_repo_with_remote() -> (tempfile::TempDir, PathBuf) {
    let dir = tempfile::tempdir().unwrap();
    let base = dir.path().canonicalize().unwrap();

    // Create a bare "remote" repo
    let bare = base.join("remote.git");
    std::fs::create_dir_all(&bare).unwrap();
    git(&bare, &["init", "--bare"]);

    // Create the scan directory that holds the working repo
    let scan_dir = base.join("projects");
    std::fs::create_dir_all(&scan_dir).unwrap();

    // Create the working repo inside scan_dir
    let repo = scan_dir.join("my-repo");
    std::fs::create_dir_all(&repo).unwrap();
    git(&repo, &["init", "-b", "main"]);
    git(&repo, &["config", "user.email", "test@test.com"]);
    git(&repo, &["config", "user.name", "Test"]);

    // Add remote
    git(&repo, &["remote", "add", "origin", &bare.to_string_lossy()]);

    // Initial commit
    std::fs::write(repo.join("README.md"), "# Test\n").unwrap();
    git(&repo, &["add", "README.md"]);
    git(&repo, &["commit", "-m", "Initial commit"]);

    // Push main to remote so origin/main exists
    git(&repo, &["push", "-u", "origin", "main"]);

    (dir, repo)
}

#[cfg(test)]
mod mock_fidelity_tests {
    use super::*;
    use crate::git::GitOps;

    fn repo() -> PathBuf {
        PathBuf::from("/repos/test")
    }

    #[test]
    fn log_touching_files_specific_match_returns_registered_results() {
        let files = vec!["src/a.rs".to_string(), "src/b.rs".to_string()];
        let expected = vec![("abc1234".to_string(), "Touched a and b".to_string())];

        let git = MockGitBuilder::new()
            .with_log_touching_files_for_files(&repo(), "origin/main", &files, expected.clone())
            .build();

        let got = git
            .log_touching_files(&repo(), "origin/main", &files)
            .unwrap();
        assert_eq!(got, expected);
    }

    #[test]
    fn log_touching_files_specific_response_is_order_insensitive() {
        let registered_files = vec!["src/a.rs".to_string(), "src/b.rs".to_string()];
        let queried_files = vec!["src/b.rs".to_string(), "src/a.rs".to_string()];
        let expected = vec![("abc1234".to_string(), "Touched a and b".to_string())];

        let git = MockGitBuilder::new()
            .with_log_touching_files_for_files(
                &repo(),
                "origin/main",
                &registered_files,
                expected.clone(),
            )
            .build();

        let got = git
            .log_touching_files(&repo(), "origin/main", &queried_files)
            .unwrap();
        assert_eq!(got, expected);
    }

    #[test]
    fn log_touching_files_specific_response_does_not_match_different_files() {
        // The mock previously ignored the `files` argument, so a caller passing the wrong files would still get the registered response, hiding plumbing bugs. After the fix, a query with different files must NOT receive the file-specific canned response.
        let registered_files = vec!["src/a.rs".to_string()];
        let queried_files = vec!["src/b.rs".to_string()];
        let canned = vec![("abc1234".to_string(), "Only for a.rs".to_string())];

        let git = MockGitBuilder::new()
            .with_log_touching_files_for_files(&repo(), "origin/main", &registered_files, canned)
            .build();

        let got = git
            .log_touching_files(&repo(), "origin/main", &queried_files)
            .unwrap();
        assert!(
            got.is_empty(),
            "expected no canned response for non-matching files, got {got:?}",
        );
    }

    #[test]
    fn log_touching_files_specific_takes_priority_over_wildcard() {
        let files = vec!["src/a.rs".to_string()];
        let wildcard = vec![("wild".to_string(), "wildcard".to_string())];
        let specific = vec![("spec".to_string(), "specific".to_string())];

        let git = MockGitBuilder::new()
            .with_log_touching_files(&repo(), "origin/main", wildcard.clone())
            .with_log_touching_files_for_files(&repo(), "origin/main", &files, specific.clone())
            .build();

        // Specific files → specific response.
        let got_specific = git
            .log_touching_files(&repo(), "origin/main", &files)
            .unwrap();
        assert_eq!(got_specific, specific);

        // Different files → wildcard fallback.
        let other = vec!["src/other.rs".to_string()];
        let got_wild = git
            .log_touching_files(&repo(), "origin/main", &other)
            .unwrap();
        assert_eq!(got_wild, wildcard);
    }

    #[test]
    fn find_large_blobs_filters_by_threshold() {
        // The mock previously ignored the threshold argument, so a caller passing 100 MB while the registered blobs were 1 MB would still see them — masking plumbing bugs in the threshold flag.
        let blobs = vec![
            ("small".to_string(), 500_000, "small.bin".to_string()),
            ("large".to_string(), 5_000_000, "large.bin".to_string()),
        ];
        let git = MockGitBuilder::new()
            .with_find_large_blobs(&repo(), blobs)
            .build();

        let got = git.find_large_blobs(&repo(), 1_000_000, 1000).unwrap();
        assert_eq!(got.len(), 1);
        assert_eq!(got[0].2, "large.bin");
    }

    #[test]
    fn find_large_blobs_threshold_is_inclusive_at_boundary() {
        // RealGit skips with `size < threshold`, so a blob exactly at the threshold MUST be included. Locks the boundary semantics.
        let blobs = vec![
            ("exact".to_string(), 1_000_000, "exact.bin".to_string()),
            ("under".to_string(), 999_999, "under.bin".to_string()),
        ];
        let git = MockGitBuilder::new()
            .with_find_large_blobs(&repo(), blobs)
            .build();

        let got = git.find_large_blobs(&repo(), 1_000_000, 1000).unwrap();
        assert_eq!(got.len(), 1);
        assert_eq!(got[0].2, "exact.bin");
    }

    #[test]
    fn find_large_blobs_threshold_zero_returns_everything() {
        let blobs = vec![
            ("a".to_string(), 1, "a".to_string()),
            ("b".to_string(), 1_000_000, "b".to_string()),
        ];
        let git = MockGitBuilder::new()
            .with_find_large_blobs(&repo(), blobs.clone())
            .build();

        let got = git.find_large_blobs(&repo(), 0, 1000).unwrap();
        assert_eq!(got.len(), blobs.len());
    }
}
