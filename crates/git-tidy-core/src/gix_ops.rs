use std::path::{Path, PathBuf};

use gix::object::Kind;

use crate::error::Error;
use crate::git::{GitOps, GitResult, RealGit};

/// Helper to open a gix repository at the given path.
fn open_repo(repo: &Path) -> GitResult<gix::Repository> {
    gix::open(repo).map_err(|e| Error::GitCommand {
        command: format!("gix::open({})", repo.display()),
        message: e.to_string(),
    })
}

/// Format a gix time as ISO 8601 with timezone offset (matching `git log --format=%aI`).
fn format_gix_time(time: &gix::date::Time) -> String {
    let secs = time.seconds;
    let offset_secs = time.offset;
    let offset_h = offset_secs / 3600;
    let offset_m = (offset_secs.abs() % 3600) / 60;

    let ts = secs + offset_secs as i64;
    let days_since_epoch = ts.div_euclid(86400);
    let time_of_day = ts.rem_euclid(86400);

    let hours = time_of_day / 3600;
    let minutes = (time_of_day % 3600) / 60;
    let seconds = time_of_day % 60;

    // Civil date from days since Unix epoch (algorithm from Howard Hinnant)
    let z = days_since_epoch + 719468;
    let era = z.div_euclid(146097);
    let doe = z.rem_euclid(146097);
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if m <= 2 { y + 1 } else { y };

    if offset_secs == 0 {
        format!("{y:04}-{m:02}-{d:02}T{hours:02}:{minutes:02}:{seconds:02}Z")
    } else {
        let sign = if offset_secs < 0 { '-' } else { '+' };
        format!(
            "{y:04}-{m:02}-{d:02}T{hours:02}:{minutes:02}:{seconds:02}{sign}{:02}:{offset_m:02}",
            offset_h.abs()
        )
    }
}

/// Helper to run a git subprocess (for operations gix cannot handle natively).
fn run_git(repo: &Path, args: &[&str]) -> GitResult<std::process::Output> {
    std::process::Command::new("git")
        .arg("-C")
        .arg(repo)
        .args(args)
        .output()
        .map_err(|e| Error::GitCommand {
            command: format!("git -C {} {}", repo.display(), args.join(" ")),
            message: e.to_string(),
        })
}

/// Helper to run a git subprocess and return trimmed stdout on success.
fn run_git_success(repo: &Path, args: &[&str]) -> GitResult<String> {
    let output = run_git(repo, args)?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
        return Err(Error::GitCommand {
            command: format!("git -C {} {}", repo.display(), args.join(" ")),
            message: stderr,
        });
    }
    Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
}

/// Count commits reachable from `tip` but not from `base`.
/// If `base` is None, counts all commits reachable from `tip`.
fn count_commits_between(
    repo: &gix::Repository,
    base: Option<gix::ObjectId>,
    tip: gix::ObjectId,
) -> GitResult<usize> {
    use std::collections::HashSet;

    let mut count = 0usize;
    let mut base_ancestors: HashSet<gix::ObjectId> = HashSet::new();

    if let Some(base_id) = base {
        let walk = repo.rev_walk([base_id]);
        if let Ok(iter) = walk.all() {
            for info in iter.flatten() {
                base_ancestors.insert(info.id);
            }
        }
        base_ancestors.insert(base_id);
    }

    let walk = repo.rev_walk([tip]);
    if let Ok(iter) = walk.all() {
        for info in iter.flatten() {
            if !base_ancestors.contains(&info.id) {
                count += 1;
            }
        }
    }

    Ok(count)
}

/// In-process git implementation using gitoxide (gix).
///
/// Falls back to subprocess for operations gix doesn't support natively
/// (stash, worktree mutations, LFS, builtin command listing).
pub struct GixGitOps;

impl GitOps for GixGitOps {
    // All methods currently delegate to RealGit.
    // They will be replaced with gix implementations one by one.

    fn fetch_prune(&self, repo: &Path) -> GitResult<()> {
        RealGit.fetch_prune(repo)
    }

    fn symbolic_ref_origin_head(&self, repo: &Path) -> GitResult<Option<String>> {
        let r = open_repo(repo)?;
        let reference = match r.find_reference("refs/remotes/origin/HEAD") {
            Ok(r) => r,
            Err(_) => return Ok(None),
        };
        match reference.target() {
            gix::refs::TargetRef::Symbolic(name) => {
                let full = name.as_bstr().to_string();
                let branch = full
                    .strip_prefix("refs/remotes/origin/")
                    .unwrap_or(&full)
                    .to_string();
                Ok(Some(branch))
            }
            gix::refs::TargetRef::Object(_) => Ok(None),
        }
    }

    fn rev_parse_verify(&self, repo: &Path, refspec: &str) -> GitResult<bool> {
        let r = open_repo(repo)?;
        Ok(r.rev_parse_single(refspec.as_bytes()).is_ok())
    }

    fn is_ancestor(&self, repo: &Path, branch: &str, target: &str) -> GitResult<bool> {
        let r = open_repo(repo)?;
        let branch_id = r
            .rev_parse_single(branch.as_bytes())
            .map_err(|e| Error::GitCommand {
                command: format!("gix rev-parse {branch}"),
                message: e.to_string(),
            })?
            .detach();
        let target_id = r
            .rev_parse_single(target.as_bytes())
            .map_err(|e| Error::GitCommand {
                command: format!("gix rev-parse {target}"),
                message: e.to_string(),
            })?
            .detach();

        // is_ancestor: branch is an ancestor of target iff merge_base(branch, target) == branch
        match r.merge_base(branch_id, target_id) {
            Ok(base) => Ok(base == branch_id),
            Err(_) => Ok(false),
        }
    }

    fn rev_list_left_right_count(
        &self,
        repo: &Path,
        left: &str,
        right: &str,
    ) -> GitResult<(usize, usize)> {
        let r = open_repo(repo)?;
        let left_id = r
            .rev_parse_single(left.as_bytes())
            .map_err(|e| Error::GitCommand {
                command: format!("gix rev-parse {left}"),
                message: e.to_string(),
            })?
            .detach();
        let right_id = r
            .rev_parse_single(right.as_bytes())
            .map_err(|e| Error::GitCommand {
                command: format!("gix rev-parse {right}"),
                message: e.to_string(),
            })?
            .detach();

        if left_id == right_id {
            return Ok((0, 0));
        }

        let base_id = r.merge_base(left_id, right_id).ok().map(|id| id.detach());

        // Count left-side commits (base..left)
        let left_count = count_commits_between(&r, base_id, left_id)?;
        let right_count = count_commits_between(&r, base_id, right_id)?;

        Ok((left_count, right_count))
    }

    fn log_exclusive(
        &self,
        repo: &Path,
        base: &str,
        branch: &str,
    ) -> GitResult<Vec<(String, String)>> {
        RealGit.log_exclusive(repo, base, branch)
    }

    fn log_grep(
        &self,
        repo: &Path,
        branch_or_ref: &str,
        needle: &str,
    ) -> GitResult<Vec<(String, String)>> {
        RealGit.log_grep(repo, branch_or_ref, needle)
    }

    fn diff_commit(&self, repo: &Path, commit: &str) -> GitResult<String> {
        RealGit.diff_commit(repo, commit)
    }

    fn diff_commit_files(&self, repo: &Path, commit: &str) -> GitResult<Vec<String>> {
        RealGit.diff_commit_files(repo, commit)
    }

    fn log_touching_files(
        &self,
        repo: &Path,
        ref_spec: &str,
        files: &[String],
    ) -> GitResult<Vec<(String, String)>> {
        RealGit.log_touching_files(repo, ref_spec, files)
    }

    fn diff_commit_on_ref(&self, repo: &Path, commit_hash: &str) -> GitResult<String> {
        RealGit.diff_commit_on_ref(repo, commit_hash)
    }

    fn status_porcelain(&self, worktree_path: &Path) -> GitResult<Vec<String>> {
        RealGit.status_porcelain(worktree_path)
    }

    fn diff_working_tree_files(
        &self,
        worktree_path: &Path,
        ref_spec: &str,
    ) -> GitResult<Vec<String>> {
        RealGit.diff_working_tree_files(worktree_path, ref_spec)
    }

    fn worktree_branch(&self, worktree_path: &Path) -> GitResult<Option<String>> {
        let r = open_repo(worktree_path)?;
        match r.head_name() {
            Ok(Some(name)) => Ok(Some(name.shorten().to_string())),
            Ok(None) => Ok(None), // genuinely detached HEAD
            Err(_) => Ok(None),
        }
    }

    fn rev_parse(&self, repo: &Path, refspec: &str) -> GitResult<String> {
        let r = open_repo(repo)?;
        let id = r
            .rev_parse_single(refspec.as_bytes())
            .map_err(|e| Error::GitCommand {
                command: format!("gix rev-parse {refspec}"),
                message: e.to_string(),
            })?;
        Ok(id.to_hex().to_string())
    }

    fn worktree_remove(&self, repo: &Path, worktree_path: &Path) -> GitResult<()> {
        RealGit.worktree_remove(repo, worktree_path)
    }

    fn worktree_remove_force(&self, repo: &Path, worktree_path: &Path) -> GitResult<()> {
        RealGit.worktree_remove_force(repo, worktree_path)
    }

    fn worktree_prune(&self, repo: &Path) -> GitResult<()> {
        RealGit.worktree_prune(repo)
    }

    fn worktree_list(&self, repo: &Path) -> GitResult<Vec<(PathBuf, Option<String>)>> {
        RealGit.worktree_list(repo)
    }

    fn branch_delete(&self, repo: &Path, branch: &str) -> GitResult<()> {
        let r = open_repo(repo)?;
        let refname = format!("refs/heads/{branch}");
        let reference = r
            .find_reference(&refname)
            .map_err(|_| Error::BranchDeletionFailed {
                repo: repo.to_path_buf(),
                branch: branch.to_string(),
                reason: format!("branch '{branch}' not found"),
            })?;
        reference
            .delete()
            .map_err(|e| Error::BranchDeletionFailed {
                repo: repo.to_path_buf(),
                branch: branch.to_string(),
                reason: e.to_string(),
            })?;
        Ok(())
    }

    fn is_branch_checked_out(&self, repo: &Path, branch: &str) -> GitResult<bool> {
        RealGit.is_branch_checked_out(repo, branch)
    }

    fn list_local_branches(&self, repo: &Path) -> GitResult<Vec<String>> {
        let r = open_repo(repo)?;
        let refs = r.references().map_err(|e| Error::GitCommand {
            command: "gix references".to_string(),
            message: e.to_string(),
        })?;
        let mut branches = Vec::new();
        for reference in refs.local_branches().map_err(|e| Error::GitCommand {
            command: "gix local_branches".to_string(),
            message: e.to_string(),
        })? {
            let reference = reference.map_err(|e| Error::GitCommand {
                command: "gix reference iter".to_string(),
                message: e.to_string(),
            })?;
            let name = reference.name().shorten().to_string();
            branches.push(name);
        }
        Ok(branches)
    }

    fn branch_delete_safe(&self, repo: &Path, branch: &str) -> GitResult<()> {
        RealGit.branch_delete_safe(repo, branch)
    }

    fn current_branch(&self, repo: &Path) -> GitResult<Option<String>> {
        let r = open_repo(repo)?;
        match r.head_ref() {
            Ok(Some(reference)) => Ok(Some(reference.name().shorten().to_string())),
            Ok(None) => Ok(None), // detached HEAD
            Err(_) => Ok(None),
        }
    }

    fn upstream_branch(&self, repo: &Path, branch: &str) -> GitResult<Option<String>> {
        let r = open_repo(repo)?;
        let config = r.config_snapshot();
        let remote_key = format!("branch.{branch}.remote");
        let merge_key = format!("branch.{branch}.merge");
        let remote_name = match config.string(&remote_key) {
            Some(v) => v.to_string(),
            None => return Ok(None),
        };
        match config.string(&merge_key) {
            Some(merge_ref) => {
                let merge_str = merge_ref.to_string();
                let short = merge_str.strip_prefix("refs/heads/").unwrap_or(&merge_str);
                Ok(Some(format!("{remote_name}/{short}")))
            }
            None => Ok(None),
        }
    }

    fn delete_remote_branch(&self, repo: &Path, remote: &str, branch: &str) -> GitResult<()> {
        RealGit.delete_remote_branch(repo, remote, branch)
    }

    fn log_file_history(
        &self,
        repo: &Path,
        ref_spec: &str,
        file: &str,
    ) -> GitResult<Vec<(String, String)>> {
        RealGit.log_file_history(repo, ref_spec, file)
    }

    fn list_remotes(&self, repo: &Path) -> GitResult<Vec<String>> {
        let r = open_repo(repo)?;
        Ok(r.remote_names().iter().map(|n| n.to_string()).collect())
    }

    fn remote_url(&self, repo: &Path, remote: &str) -> GitResult<String> {
        let r = open_repo(repo)?;
        let config = r.config_snapshot();
        let key = format!("remote.{remote}.url");
        config
            .string(&key)
            .map(|v| v.to_string())
            .ok_or_else(|| Error::GitCommand {
                command: format!("gix remote_url {remote}"),
                message: format!("no url configured for remote '{remote}'"),
            })
    }

    fn ls_remote_check(&self, repo: &Path, remote: &str) -> GitResult<bool> {
        RealGit.ls_remote_check(repo, remote)
    }

    fn remote_remove(&self, repo: &Path, remote: &str) -> GitResult<()> {
        RealGit.remote_remove(repo, remote)
    }

    fn list_remote_tracking_refs(&self, repo: &Path) -> GitResult<Vec<(String, String)>> {
        let r = open_repo(repo)?;
        let refs = r.references().map_err(|e| Error::GitCommand {
            command: "gix references".to_string(),
            message: e.to_string(),
        })?;
        let mut result = Vec::new();
        let prefix_iter = refs
            .prefixed("refs/remotes/")
            .map_err(|e| Error::GitCommand {
                command: "gix prefixed refs/remotes/".to_string(),
                message: e.to_string(),
            })?;
        for reference in prefix_iter {
            let reference = reference.map_err(|e| Error::GitCommand {
                command: "gix reference iter".to_string(),
                message: e.to_string(),
            })?;
            let full = reference.name().as_bstr().to_string();
            // Match git's %(refname:short) behavior: strip "refs/remotes/" prefix,
            // then for symbolic refs like "origin/HEAD", git shortens further to
            // just the remote name if it's unambiguous.
            let short = full
                .strip_prefix("refs/remotes/")
                .unwrap_or(&full)
                .to_string();
            // Replicate git for-each-ref: HEAD refs get shortened to remote name only
            let short = if short.ends_with("/HEAD") {
                short.strip_suffix("/HEAD").unwrap_or(&short).to_string()
            } else {
                short
            };
            result.push((short, full));
        }
        Ok(result)
    }

    fn prune_remote_refs(&self, repo: &Path, remote: &str) -> GitResult<usize> {
        RealGit.prune_remote_refs(repo, remote)
    }

    fn list_stashes(&self, repo: &Path) -> GitResult<Vec<(String, String, String)>> {
        RealGit.list_stashes(repo)
    }

    fn stash_diff(&self, repo: &Path, stash_ref: &str) -> GitResult<String> {
        RealGit.stash_diff(repo, stash_ref)
    }

    fn stash_drop(&self, repo: &Path, stash_ref: &str) -> GitResult<()> {
        RealGit.stash_drop(repo, stash_ref)
    }

    fn list_local_tags(&self, repo: &Path) -> GitResult<Vec<String>> {
        let r = open_repo(repo)?;
        let refs = r.references().map_err(|e| Error::GitCommand {
            command: "gix references".to_string(),
            message: e.to_string(),
        })?;
        let mut tags = Vec::new();
        for reference in refs.tags().map_err(|e| Error::GitCommand {
            command: "gix tags".to_string(),
            message: e.to_string(),
        })? {
            let reference = reference.map_err(|e| Error::GitCommand {
                command: "gix tag iter".to_string(),
                message: e.to_string(),
            })?;
            let name = reference.name().shorten().to_string();
            tags.push(name);
        }
        Ok(tags)
    }

    fn list_remote_tags(&self, repo: &Path, remote: &str) -> GitResult<Vec<(String, String)>> {
        RealGit.list_remote_tags(repo, remote)
    }

    fn tag_commit(&self, repo: &Path, tag: &str) -> GitResult<String> {
        let r = open_repo(repo)?;
        let refname = format!("refs/tags/{tag}");
        let reference = r.find_reference(&refname).map_err(|e| Error::GitCommand {
            command: format!("gix find_reference {refname}"),
            message: e.to_string(),
        })?;
        let obj = reference
            .into_fully_peeled_id()
            .map_err(|e| Error::GitCommand {
                command: format!("gix peel tag {tag}"),
                message: e.to_string(),
            })?;
        Ok(obj.to_hex().to_string())
    }

    fn is_commit_reachable(&self, repo: &Path, commit: &str) -> GitResult<bool> {
        let r = open_repo(repo)?;
        let target_id = r
            .rev_parse_single(commit.as_bytes())
            .map_err(|e| Error::GitCommand {
                command: format!("gix rev-parse {commit}"),
                message: e.to_string(),
            })?
            .detach();

        // Check if any branch tip can reach this commit
        let refs = r.references().map_err(|e| Error::GitCommand {
            command: "gix references".to_string(),
            message: e.to_string(),
        })?;
        for reference in refs.local_branches().map_err(|e| Error::GitCommand {
            command: "gix local_branches".to_string(),
            message: e.to_string(),
        })? {
            let reference = match reference {
                Ok(r) => r,
                Err(_) => continue,
            };
            let tip = match reference.into_fully_peeled_id() {
                Ok(id) => id.detach(),
                Err(_) => continue,
            };
            if tip == target_id {
                return Ok(true);
            }
            // Walk from tip towards root, checking if target is reachable
            if let Ok(base) = r.merge_base(tip, target_id)
                && base == target_id
            {
                return Ok(true);
            }
        }
        Ok(false)
    }

    fn tag_delete(&self, repo: &Path, tag: &str) -> GitResult<()> {
        let r = open_repo(repo)?;
        let refname = format!("refs/tags/{tag}");
        let reference = r
            .find_reference(&refname)
            .map_err(|_| Error::TagDeletionFailed {
                repo: repo.to_path_buf(),
                tag: tag.to_string(),
                reason: format!("tag '{tag}' not found"),
            })?;
        reference.delete().map_err(|e| Error::TagDeletionFailed {
            repo: repo.to_path_buf(),
            tag: tag.to_string(),
            reason: e.to_string(),
        })?;
        Ok(())
    }

    fn tag_delete_remote(&self, repo: &Path, remote: &str, tag: &str) -> GitResult<()> {
        RealGit.tag_delete_remote(repo, remote, tag)
    }

    fn is_tag_annotated(&self, repo: &Path, tag: &str) -> GitResult<bool> {
        let r = open_repo(repo)?;
        let refname = format!("refs/tags/{tag}");
        let reference = r.find_reference(&refname).map_err(|e| Error::GitCommand {
            command: format!("gix find_reference {refname}"),
            message: e.to_string(),
        })?;
        let id = reference.id();
        let obj = id.object().map_err(|e| Error::GitCommand {
            command: format!("gix object {tag}"),
            message: e.to_string(),
        })?;
        Ok(obj.kind == Kind::Tag)
    }

    fn tag_date(&self, repo: &Path, tag: &str) -> GitResult<Option<String>> {
        let r = open_repo(repo)?;
        let refname = format!("refs/tags/{tag}");
        let reference = r.find_reference(&refname).map_err(|e| Error::GitCommand {
            command: format!("gix find_reference {refname}"),
            message: e.to_string(),
        })?;
        let id = reference.id();
        let obj = id.object().map_err(|e| Error::GitCommand {
            command: format!("gix object {tag}"),
            message: e.to_string(),
        })?;
        if obj.kind == Kind::Tag {
            let tag_obj = obj.into_tag();
            match tag_obj.tagger() {
                Ok(Some(sig)) => match sig.time() {
                    Ok(t) => Ok(Some(format_gix_time(&t))),
                    Err(_) => Ok(None),
                },
                _ => Ok(None),
            }
        } else {
            let commit = obj.into_commit();
            match commit.time() {
                Ok(t) => Ok(Some(format_gix_time(&t))),
                Err(_) => Ok(None),
            }
        }
    }

    fn last_commit_date(&self, repo: &Path) -> GitResult<Option<String>> {
        let r = open_repo(repo)?;
        match r.head_commit() {
            Ok(commit) => {
                let author = commit.author().map_err(|e| Error::GitCommand {
                    command: "gix head_commit author".to_string(),
                    message: e.to_string(),
                })?;
                let time = author.time().map_err(|e| Error::GitCommand {
                    command: "gix author time".to_string(),
                    message: e.to_string(),
                })?;
                Ok(Some(format_gix_time(&time)))
            }
            Err(_) => Ok(None),
        }
    }

    fn config_list_local(&self, repo: &Path) -> GitResult<Vec<(String, String)>> {
        let r = open_repo(repo)?;
        let config = r.config_snapshot();
        let mut result = Vec::new();
        for section in config.sections() {
            if section.meta().source != gix::config::Source::Local {
                continue;
            }
            let section_name = section.header().name().to_string();
            let subsection = section.header().subsection_name().map(|s| s.to_string());
            let body = section.body();
            for vn in body.value_names() {
                let key_str = vn.to_string();
                if let Some(value) = body.value(&key_str) {
                    let full_key = match &subsection {
                        Some(sub) => format!("{section_name}.{sub}.{key_str}"),
                        None => format!("{section_name}.{key_str}"),
                    };
                    result.push((full_key, value.to_string()));
                }
            }
        }
        Ok(result)
    }

    fn config_remove_section(&self, repo: &Path, section: &str) -> GitResult<()> {
        run_git_success(repo, &["config", "--remove-section", section]).map_err(|_| {
            Error::ConfigRemoveSectionFailed {
                repo: repo.to_path_buf(),
                section: section.to_string(),
                reason: "git config --remove-section failed".to_string(),
            }
        })?;
        Ok(())
    }

    fn list_builtin_commands(&self) -> GitResult<Vec<String>> {
        RealGit.list_builtin_commands()
    }

    fn lfs_installed(&self) -> GitResult<bool> {
        RealGit.lfs_installed()
    }

    fn lfs_ls_files(&self, repo: &Path) -> GitResult<Vec<(String, char, String)>> {
        RealGit.lfs_ls_files(repo)
    }

    fn lfs_track_patterns(&self, repo: &Path) -> GitResult<Vec<String>> {
        RealGit.lfs_track_patterns(repo)
    }

    fn lfs_prune_dry_run(&self, repo: &Path) -> GitResult<(usize, u64)> {
        RealGit.lfs_prune_dry_run(repo)
    }

    fn lfs_prune(&self, repo: &Path) -> GitResult<()> {
        RealGit.lfs_prune(repo)
    }

    fn find_large_blobs(
        &self,
        repo: &Path,
        threshold: u64,
        depth: usize,
    ) -> GitResult<Vec<(String, u64, String)>> {
        RealGit.find_large_blobs(repo, threshold, depth)
    }
}
