use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use crate::git::{GitOps, GitResult};

/// Macro to delegate uncached GitOps methods to `self.inner`.
///
/// Each arm specifies the method name, parameter names and types, and return type.
macro_rules! delegate_git_ops {
    ($(fn $name:ident(&self $(, $param:ident: $ty:ty)*) -> $ret:ty;)*) => {
        $(
            fn $name(&self $(, $param: $ty)*) -> $ret {
                self.inner.$name($($param),*)
            }
        )*
    };
}

/// A caching wrapper around a `GitOps` implementation.
///
/// Memoizes read-only, idempotent queries that are commonly shared across
/// multiple git-tidy tools. Mutating operations and per-branch classification
/// calls pass through directly.
///
/// Designed for use by the in-process audit runner, where a single
/// `CachingGitOps` is shared across all tool scans.
pub struct CachingGitOps<'a> {
    inner: &'a dyn GitOps,
    fetched_repos: Mutex<HashSet<PathBuf>>,
    symbolic_ref_cache: Mutex<HashMap<PathBuf, Option<String>>>,
    rev_parse_verify_cache: Mutex<HashMap<(PathBuf, String), bool>>,
    local_branches_cache: Mutex<HashMap<PathBuf, Vec<String>>>,
    list_remotes_cache: Mutex<HashMap<PathBuf, Vec<String>>>,
    ls_remote_check_cache: Mutex<HashMap<(PathBuf, String), bool>>,
    builtin_commands_cache: Mutex<Option<Vec<String>>>,
    lfs_installed_cache: Mutex<Option<bool>>,
}

impl<'a> CachingGitOps<'a> {
    /// Create a new caching wrapper around the given `GitOps` implementation.
    pub fn new(inner: &'a dyn GitOps) -> Self {
        Self {
            inner,
            fetched_repos: Mutex::new(HashSet::new()),
            symbolic_ref_cache: Mutex::new(HashMap::new()),
            rev_parse_verify_cache: Mutex::new(HashMap::new()),
            local_branches_cache: Mutex::new(HashMap::new()),
            list_remotes_cache: Mutex::new(HashMap::new()),
            ls_remote_check_cache: Mutex::new(HashMap::new()),
            builtin_commands_cache: Mutex::new(None),
            lfs_installed_cache: Mutex::new(None),
        }
    }
}

impl GitOps for CachingGitOps<'_> {
    fn fetch_prune(&self, repo: &Path) -> GitResult<()> {
        let key = repo.to_path_buf();
        if self.fetched_repos.lock().unwrap().contains(&key) {
            return Ok(());
        }
        let result = self.inner.fetch_prune(repo);
        if result.is_ok() {
            self.fetched_repos.lock().unwrap().insert(key);
        }
        result
    }

    fn symbolic_ref_origin_head(&self, repo: &Path) -> GitResult<Option<String>> {
        let key = repo.to_path_buf();
        if let Some(cached) = self.symbolic_ref_cache.lock().unwrap().get(&key) {
            return Ok(cached.clone());
        }
        let result = self.inner.symbolic_ref_origin_head(repo)?;
        self.symbolic_ref_cache
            .lock()
            .unwrap()
            .insert(key, result.clone());
        Ok(result)
    }

    fn rev_parse_verify(&self, repo: &Path, refspec: &str) -> GitResult<bool> {
        let key = (repo.to_path_buf(), refspec.to_string());
        if let Some(&cached) = self.rev_parse_verify_cache.lock().unwrap().get(&key) {
            return Ok(cached);
        }
        let result = self.inner.rev_parse_verify(repo, refspec)?;
        self.rev_parse_verify_cache
            .lock()
            .unwrap()
            .insert(key, result);
        Ok(result)
    }

    fn list_local_branches(&self, repo: &Path) -> GitResult<Vec<String>> {
        let key = repo.to_path_buf();
        if let Some(cached) = self.local_branches_cache.lock().unwrap().get(&key) {
            return Ok(cached.clone());
        }
        let result = self.inner.list_local_branches(repo)?;
        self.local_branches_cache
            .lock()
            .unwrap()
            .insert(key, result.clone());
        Ok(result)
    }

    fn list_remotes(&self, repo: &Path) -> GitResult<Vec<String>> {
        let key = repo.to_path_buf();
        if let Some(cached) = self.list_remotes_cache.lock().unwrap().get(&key) {
            return Ok(cached.clone());
        }
        let result = self.inner.list_remotes(repo)?;
        self.list_remotes_cache
            .lock()
            .unwrap()
            .insert(key, result.clone());
        Ok(result)
    }

    fn ls_remote_check(&self, repo: &Path, remote: &str) -> GitResult<bool> {
        let key = (repo.to_path_buf(), remote.to_string());
        if let Some(&cached) = self.ls_remote_check_cache.lock().unwrap().get(&key) {
            return Ok(cached);
        }
        let result = self.inner.ls_remote_check(repo, remote)?;
        self.ls_remote_check_cache
            .lock()
            .unwrap()
            .insert(key, result);
        Ok(result)
    }

    fn list_builtin_commands(&self) -> GitResult<Vec<String>> {
        if let Some(cached) = self.builtin_commands_cache.lock().unwrap().as_ref() {
            return Ok(cached.clone());
        }
        let result = self.inner.list_builtin_commands()?;
        *self.builtin_commands_cache.lock().unwrap() = Some(result.clone());
        Ok(result)
    }

    fn lfs_installed(&self) -> GitResult<bool> {
        if let Some(cached) = *self.lfs_installed_cache.lock().unwrap() {
            return Ok(cached);
        }
        let result = self.inner.lfs_installed()?;
        *self.lfs_installed_cache.lock().unwrap() = Some(result);
        Ok(result)
    }

    // All remaining methods delegate directly to the inner implementation.
    delegate_git_ops! {
        fn is_ancestor(&self, repo: &Path, branch: &str, target: &str) -> GitResult<bool>;
        fn rev_list_left_right_count(&self, repo: &Path, left: &str, right: &str) -> GitResult<(usize, usize)>;
        fn log_exclusive(&self, repo: &Path, base: &str, branch: &str) -> GitResult<Vec<(String, String)>>;
        fn log_grep(&self, repo: &Path, branch_or_ref: &str, needle: &str) -> GitResult<Vec<(String, String)>>;
        fn diff_commit(&self, repo: &Path, commit: &str) -> GitResult<String>;
        fn diff_commit_files(&self, repo: &Path, commit: &str) -> GitResult<Vec<String>>;
        fn log_touching_files(&self, repo: &Path, ref_spec: &str, files: &[String]) -> GitResult<Vec<(String, String)>>;
        fn diff_commit_on_ref(&self, repo: &Path, commit_hash: &str) -> GitResult<String>;
        fn status_porcelain(&self, worktree_path: &Path) -> GitResult<Vec<String>>;
        fn worktree_branch(&self, worktree_path: &Path) -> GitResult<Option<String>>;
        fn rev_parse(&self, repo: &Path, refspec: &str) -> GitResult<String>;
        fn worktree_remove(&self, repo: &Path, worktree_path: &Path) -> GitResult<()>;
        fn worktree_remove_force(&self, repo: &Path, worktree_path: &Path) -> GitResult<()>;
        fn worktree_prune(&self, repo: &Path) -> GitResult<()>;
        fn branch_delete(&self, repo: &Path, branch: &str) -> GitResult<()>;
        fn is_branch_checked_out(&self, repo: &Path, branch: &str) -> GitResult<bool>;
        fn branch_delete_safe(&self, repo: &Path, branch: &str) -> GitResult<()>;
        fn current_branch(&self, repo: &Path) -> GitResult<Option<String>>;
        fn upstream_branch(&self, repo: &Path, branch: &str) -> GitResult<Option<String>>;
        fn delete_remote_branch(&self, repo: &Path, remote: &str, branch: &str) -> GitResult<()>;
        fn log_file_history(&self, repo: &Path, ref_spec: &str, file: &str) -> GitResult<Vec<(String, String)>>;
        fn remote_url(&self, repo: &Path, remote: &str) -> GitResult<String>;
        fn remote_remove(&self, repo: &Path, remote: &str) -> GitResult<()>;
        fn list_remote_tracking_refs(&self, repo: &Path) -> GitResult<Vec<(String, String)>>;
        fn prune_remote_refs(&self, repo: &Path, remote: &str) -> GitResult<usize>;
        fn list_stashes(&self, repo: &Path) -> GitResult<Vec<(String, String, String)>>;
        fn stash_diff(&self, repo: &Path, stash_ref: &str) -> GitResult<String>;
        fn stash_drop(&self, repo: &Path, stash_ref: &str) -> GitResult<()>;
        fn list_local_tags(&self, repo: &Path) -> GitResult<Vec<String>>;
        fn list_remote_tags(&self, repo: &Path, remote: &str) -> GitResult<Vec<(String, String)>>;
        fn tag_commit(&self, repo: &Path, tag: &str) -> GitResult<String>;
        fn is_commit_reachable(&self, repo: &Path, commit: &str) -> GitResult<bool>;
        fn tag_delete(&self, repo: &Path, tag: &str) -> GitResult<()>;
        fn tag_delete_remote(&self, repo: &Path, remote: &str, tag: &str) -> GitResult<()>;
        fn is_tag_annotated(&self, repo: &Path, tag: &str) -> GitResult<bool>;
        fn tag_date(&self, repo: &Path, tag: &str) -> GitResult<Option<String>>;
        fn last_commit_date(&self, repo: &Path) -> GitResult<Option<String>>;
        fn config_list_local(&self, repo: &Path) -> GitResult<Vec<(String, String)>>;
        fn config_remove_section(&self, repo: &Path, section: &str) -> GitResult<()>;
        fn lfs_ls_files(&self, repo: &Path) -> GitResult<Vec<(String, char, String)>>;
        fn lfs_track_patterns(&self, repo: &Path) -> GitResult<Vec<String>>;
        fn lfs_prune_dry_run(&self, repo: &Path) -> GitResult<(usize, u64)>;
        fn lfs_prune(&self, repo: &Path) -> GitResult<()>;
        fn find_large_blobs(&self, repo: &Path, threshold: u64, depth: usize) -> GitResult<Vec<(String, u64, String)>>;
    }
}

#[cfg(test)]
mod tests {
    use std::path::Path;
    use std::sync::Mutex;

    use super::*;

    /// A thin wrapper around MockGit that counts calls to specific methods.
    /// Only the methods we need to verify caching for are counted;
    /// all methods delegate to the inner MockGit.
    struct CountingGitOps<'a> {
        inner: &'a dyn GitOps,
        fetch_prune_count: Mutex<usize>,
        symbolic_ref_count: Mutex<usize>,
        rev_parse_verify_count: Mutex<usize>,
        list_local_branches_count: Mutex<usize>,
        list_remotes_count: Mutex<usize>,
        ls_remote_check_count: Mutex<usize>,
        list_builtin_commands_count: Mutex<usize>,
        lfs_installed_count: Mutex<usize>,
    }

    impl<'a> CountingGitOps<'a> {
        fn new(inner: &'a dyn GitOps) -> Self {
            Self {
                inner,
                fetch_prune_count: Mutex::new(0),
                symbolic_ref_count: Mutex::new(0),
                rev_parse_verify_count: Mutex::new(0),
                list_local_branches_count: Mutex::new(0),
                list_remotes_count: Mutex::new(0),
                ls_remote_check_count: Mutex::new(0),
                list_builtin_commands_count: Mutex::new(0),
                lfs_installed_count: Mutex::new(0),
            }
        }
    }

    impl GitOps for CountingGitOps<'_> {
        fn fetch_prune(&self, repo: &Path) -> GitResult<()> {
            *self.fetch_prune_count.lock().unwrap() += 1;
            self.inner.fetch_prune(repo)
        }

        fn symbolic_ref_origin_head(&self, repo: &Path) -> GitResult<Option<String>> {
            *self.symbolic_ref_count.lock().unwrap() += 1;
            self.inner.symbolic_ref_origin_head(repo)
        }

        fn rev_parse_verify(&self, repo: &Path, refspec: &str) -> GitResult<bool> {
            *self.rev_parse_verify_count.lock().unwrap() += 1;
            self.inner.rev_parse_verify(repo, refspec)
        }

        fn list_local_branches(&self, repo: &Path) -> GitResult<Vec<String>> {
            *self.list_local_branches_count.lock().unwrap() += 1;
            self.inner.list_local_branches(repo)
        }

        fn list_remotes(&self, repo: &Path) -> GitResult<Vec<String>> {
            *self.list_remotes_count.lock().unwrap() += 1;
            self.inner.list_remotes(repo)
        }

        fn ls_remote_check(&self, repo: &Path, remote: &str) -> GitResult<bool> {
            *self.ls_remote_check_count.lock().unwrap() += 1;
            self.inner.ls_remote_check(repo, remote)
        }

        fn list_builtin_commands(&self) -> GitResult<Vec<String>> {
            *self.list_builtin_commands_count.lock().unwrap() += 1;
            self.inner.list_builtin_commands()
        }

        fn lfs_installed(&self) -> GitResult<bool> {
            *self.lfs_installed_count.lock().unwrap() += 1;
            self.inner.lfs_installed()
        }

        // Remaining methods delegate directly.
        delegate_git_ops! {
            fn is_ancestor(&self, repo: &Path, branch: &str, target: &str) -> GitResult<bool>;
            fn rev_list_left_right_count(&self, repo: &Path, left: &str, right: &str) -> GitResult<(usize, usize)>;
            fn log_exclusive(&self, repo: &Path, base: &str, branch: &str) -> GitResult<Vec<(String, String)>>;
            fn log_grep(&self, repo: &Path, branch_or_ref: &str, needle: &str) -> GitResult<Vec<(String, String)>>;
            fn diff_commit(&self, repo: &Path, commit: &str) -> GitResult<String>;
            fn diff_commit_files(&self, repo: &Path, commit: &str) -> GitResult<Vec<String>>;
            fn log_touching_files(&self, repo: &Path, ref_spec: &str, files: &[String]) -> GitResult<Vec<(String, String)>>;
            fn diff_commit_on_ref(&self, repo: &Path, commit_hash: &str) -> GitResult<String>;
            fn status_porcelain(&self, worktree_path: &Path) -> GitResult<Vec<String>>;
            fn worktree_branch(&self, worktree_path: &Path) -> GitResult<Option<String>>;
            fn rev_parse(&self, repo: &Path, refspec: &str) -> GitResult<String>;
            fn worktree_remove(&self, repo: &Path, worktree_path: &Path) -> GitResult<()>;
            fn worktree_remove_force(&self, repo: &Path, worktree_path: &Path) -> GitResult<()>;
            fn worktree_prune(&self, repo: &Path) -> GitResult<()>;
            fn branch_delete(&self, repo: &Path, branch: &str) -> GitResult<()>;
            fn is_branch_checked_out(&self, repo: &Path, branch: &str) -> GitResult<bool>;
            fn branch_delete_safe(&self, repo: &Path, branch: &str) -> GitResult<()>;
            fn current_branch(&self, repo: &Path) -> GitResult<Option<String>>;
            fn upstream_branch(&self, repo: &Path, branch: &str) -> GitResult<Option<String>>;
            fn delete_remote_branch(&self, repo: &Path, remote: &str, branch: &str) -> GitResult<()>;
            fn log_file_history(&self, repo: &Path, ref_spec: &str, file: &str) -> GitResult<Vec<(String, String)>>;
            fn remote_url(&self, repo: &Path, remote: &str) -> GitResult<String>;
            fn remote_remove(&self, repo: &Path, remote: &str) -> GitResult<()>;
            fn list_remote_tracking_refs(&self, repo: &Path) -> GitResult<Vec<(String, String)>>;
            fn prune_remote_refs(&self, repo: &Path, remote: &str) -> GitResult<usize>;
            fn list_stashes(&self, repo: &Path) -> GitResult<Vec<(String, String, String)>>;
            fn stash_diff(&self, repo: &Path, stash_ref: &str) -> GitResult<String>;
            fn stash_drop(&self, repo: &Path, stash_ref: &str) -> GitResult<()>;
            fn list_local_tags(&self, repo: &Path) -> GitResult<Vec<String>>;
            fn list_remote_tags(&self, repo: &Path, remote: &str) -> GitResult<Vec<(String, String)>>;
            fn tag_commit(&self, repo: &Path, tag: &str) -> GitResult<String>;
            fn is_commit_reachable(&self, repo: &Path, commit: &str) -> GitResult<bool>;
            fn tag_delete(&self, repo: &Path, tag: &str) -> GitResult<()>;
            fn tag_delete_remote(&self, repo: &Path, remote: &str, tag: &str) -> GitResult<()>;
            fn is_tag_annotated(&self, repo: &Path, tag: &str) -> GitResult<bool>;
            fn tag_date(&self, repo: &Path, tag: &str) -> GitResult<Option<String>>;
            fn last_commit_date(&self, repo: &Path) -> GitResult<Option<String>>;
            fn config_list_local(&self, repo: &Path) -> GitResult<Vec<(String, String)>>;
            fn config_remove_section(&self, repo: &Path, section: &str) -> GitResult<()>;
            fn lfs_ls_files(&self, repo: &Path) -> GitResult<Vec<(String, char, String)>>;
            fn lfs_track_patterns(&self, repo: &Path) -> GitResult<Vec<String>>;
            fn lfs_prune_dry_run(&self, repo: &Path) -> GitResult<(usize, u64)>;
            fn lfs_prune(&self, repo: &Path) -> GitResult<()>;
            fn find_large_blobs(&self, repo: &Path, threshold: u64, depth: usize) -> GitResult<Vec<(String, u64, String)>>;
        }
    }

    use crate::testutil::MockGitBuilder;

    fn mock() -> crate::testutil::MockGit {
        MockGitBuilder::new()
            .with_symbolic_ref(Path::new("/repo"), Some("main"))
            .with_rev_parse_verify(Path::new("/repo"), "refs/remotes/origin/main", true)
            .with_local_branches(
                Path::new("/repo"),
                vec!["main".to_string(), "feature".to_string()],
            )
            .with_list_remotes(Path::new("/repo"), vec!["origin".to_string()])
            .with_ls_remote_check(Path::new("/repo"), "origin", true)
            .with_builtin_commands(vec!["add".to_string(), "commit".to_string()])
            .with_lfs_installed(true)
            .build()
    }

    #[test]
    fn fetch_prune_deduplicates() {
        let m = mock();
        let counter = CountingGitOps::new(&m);
        let caching = CachingGitOps::new(&counter);

        caching.fetch_prune(Path::new("/repo")).unwrap();
        caching.fetch_prune(Path::new("/repo")).unwrap();
        caching.fetch_prune(Path::new("/repo")).unwrap();

        assert_eq!(*counter.fetch_prune_count.lock().unwrap(), 1);
    }

    #[test]
    fn fetch_prune_different_repos() {
        let m = mock();
        let counter = CountingGitOps::new(&m);
        let caching = CachingGitOps::new(&counter);

        caching.fetch_prune(Path::new("/repo")).unwrap();
        caching.fetch_prune(Path::new("/other")).unwrap();

        assert_eq!(*counter.fetch_prune_count.lock().unwrap(), 2);
    }

    #[test]
    fn symbolic_ref_cached() {
        let m = mock();
        let counter = CountingGitOps::new(&m);
        let caching = CachingGitOps::new(&counter);

        let r1 = caching
            .symbolic_ref_origin_head(Path::new("/repo"))
            .unwrap();
        let r2 = caching
            .symbolic_ref_origin_head(Path::new("/repo"))
            .unwrap();

        assert_eq!(r1, Some("main".to_string()));
        assert_eq!(r1, r2);
        assert_eq!(*counter.symbolic_ref_count.lock().unwrap(), 1);
    }

    #[test]
    fn rev_parse_verify_cached() {
        let m = mock();
        let counter = CountingGitOps::new(&m);
        let caching = CachingGitOps::new(&counter);

        let r1 = caching
            .rev_parse_verify(Path::new("/repo"), "refs/remotes/origin/main")
            .unwrap();
        let r2 = caching
            .rev_parse_verify(Path::new("/repo"), "refs/remotes/origin/main")
            .unwrap();

        assert!(r1);
        assert_eq!(r1, r2);
        assert_eq!(*counter.rev_parse_verify_count.lock().unwrap(), 1);
    }

    #[test]
    fn rev_parse_verify_different_refspecs_not_shared() {
        let m = MockGitBuilder::new()
            .with_rev_parse_verify(Path::new("/repo"), "refs/remotes/origin/main", true)
            .with_rev_parse_verify(Path::new("/repo"), "refs/remotes/origin/develop", false)
            .build();
        let counter = CountingGitOps::new(&m);
        let caching = CachingGitOps::new(&counter);

        let r1 = caching
            .rev_parse_verify(Path::new("/repo"), "refs/remotes/origin/main")
            .unwrap();
        let r2 = caching
            .rev_parse_verify(Path::new("/repo"), "refs/remotes/origin/develop")
            .unwrap();

        assert!(r1);
        assert!(!r2);
        assert_eq!(*counter.rev_parse_verify_count.lock().unwrap(), 2);
    }

    #[test]
    fn list_local_branches_cached() {
        let m = mock();
        let counter = CountingGitOps::new(&m);
        let caching = CachingGitOps::new(&counter);

        let r1 = caching.list_local_branches(Path::new("/repo")).unwrap();
        let r2 = caching.list_local_branches(Path::new("/repo")).unwrap();

        assert_eq!(r1, vec!["main", "feature"]);
        assert_eq!(r1, r2);
        assert_eq!(*counter.list_local_branches_count.lock().unwrap(), 1);
    }

    #[test]
    fn list_remotes_cached() {
        let m = mock();
        let counter = CountingGitOps::new(&m);
        let caching = CachingGitOps::new(&counter);

        let r1 = caching.list_remotes(Path::new("/repo")).unwrap();
        let r2 = caching.list_remotes(Path::new("/repo")).unwrap();

        assert_eq!(r1, vec!["origin"]);
        assert_eq!(r1, r2);
        assert_eq!(*counter.list_remotes_count.lock().unwrap(), 1);
    }

    #[test]
    fn ls_remote_check_cached() {
        let m = mock();
        let counter = CountingGitOps::new(&m);
        let caching = CachingGitOps::new(&counter);

        let r1 = caching
            .ls_remote_check(Path::new("/repo"), "origin")
            .unwrap();
        let r2 = caching
            .ls_remote_check(Path::new("/repo"), "origin")
            .unwrap();

        assert!(r1);
        assert_eq!(r1, r2);
        assert_eq!(*counter.ls_remote_check_count.lock().unwrap(), 1);
    }

    #[test]
    fn list_builtin_commands_cached() {
        let m = mock();
        let counter = CountingGitOps::new(&m);
        let caching = CachingGitOps::new(&counter);

        let r1 = caching.list_builtin_commands().unwrap();
        let r2 = caching.list_builtin_commands().unwrap();

        assert_eq!(r1, vec!["add", "commit"]);
        assert_eq!(r1, r2);
        assert_eq!(*counter.list_builtin_commands_count.lock().unwrap(), 1);
    }

    #[test]
    fn lfs_installed_cached() {
        let m = mock();
        let counter = CountingGitOps::new(&m);
        let caching = CachingGitOps::new(&counter);

        let r1 = caching.lfs_installed().unwrap();
        let r2 = caching.lfs_installed().unwrap();

        assert!(r1);
        assert_eq!(r1, r2);
        assert_eq!(*counter.lfs_installed_count.lock().unwrap(), 1);
    }

    #[test]
    fn uncached_methods_pass_through_every_call() {
        let m = MockGitBuilder::new()
            .with_status_porcelain(Path::new("/repo"), vec!["M file.rs".to_string()])
            .build();
        let counter = CountingGitOps::new(&m);
        let caching = CachingGitOps::new(&counter);

        // status_porcelain is not cached; both calls should hit the inner impl
        let r1 = caching.status_porcelain(Path::new("/repo")).unwrap();
        let r2 = caching.status_porcelain(Path::new("/repo")).unwrap();

        assert_eq!(r1, vec!["M file.rs"]);
        assert_eq!(r1, r2);
        // No counter for status_porcelain in CountingGitOps, but we verify it
        // returns the correct result (passthrough works).
    }
}
