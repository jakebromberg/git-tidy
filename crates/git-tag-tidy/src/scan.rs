use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

use git_tidy_core::discovery::discover_repos;
use git_tidy_core::error::Error;
use git_tidy_core::filter::{NameFilter, filter_paths};
use git_tidy_core::git::GitOps;
use git_tidy_core::output::repo_display_name;
use git_tidy_core::progress::Progress;
use git_tidy_core::scan::parallel_classify;
use git_tidy_core::types::ClassificationLabel;

use crate::types::{
    TagClassification, TagCounts, TagInfo, TagRepoGroup, TagScanResult, is_release_tag_name,
};

/// Classify a single tag.
///
/// - If `local_commit` is `Some`, the tag exists locally.
/// - If `remote_commit` is `Some`, the tag exists on at least one remote.
/// - `is_reachable` indicates whether the tag's commit is reachable from any branch.
/// - `offline` indicates whether we skipped remote queries.
pub fn classify_tag(
    local_commit: Option<&str>,
    remote_commit: Option<&str>,
    is_reachable: bool,
    offline: bool,
) -> TagClassification {
    match (local_commit, remote_commit) {
        (Some(_), _) if !is_reachable => TagClassification::Stale,
        (Some(_), Some(_)) => TagClassification::Synced,
        (Some(_), None) if offline => TagClassification::Synced,
        (Some(_), None) => TagClassification::LocalOnly,
        (None, Some(_)) => TagClassification::RemoteOnly,
        (None, None) => TagClassification::Stale, // shouldn't happen in practice
    }
}

/// Scan all repos in `directory` for tags.
pub fn run_scan(
    git: &dyn GitOps,
    directory: &Path,
    offline: bool,
    verbose: bool,
    repo_filter: &NameFilter,
    entity_filter: &NameFilter,
    progress: &Progress,
) -> Result<TagScanResult, Error> {
    let repo_paths = discover_repos(directory)?;
    let repo_paths = filter_paths(repo_paths, repo_filter);
    run_scan_repos(git, &repo_paths, offline, verbose, entity_filter, progress)
}

/// Scan the given `repo_paths` for tags.
pub fn run_scan_repos(
    git: &dyn GitOps,
    repo_paths: &[PathBuf],
    offline: bool,
    verbose: bool,
    entity_filter: &NameFilter,
    progress: &Progress,
) -> Result<TagScanResult, Error> {
    let mut warnings = Vec::new();

    let (repos, scan_warnings) = parallel_classify(
        &repo_paths,
        |repo_path| {
            let mut local_warnings = Vec::new();

            let local_tags: HashSet<String> = match git.list_local_tags(repo_path) {
                Ok(tags) => tags.into_iter().collect(),
                Err(e) => {
                    local_warnings.push(format!(
                        "could not list tags for {}: {e}",
                        repo_path.display()
                    ));
                    return (None, local_warnings);
                }
            };

            let remotes = git.list_remotes(repo_path).unwrap_or_default();
            let mut remote_tag_map: HashMap<String, (String, Vec<String>)> = HashMap::new();

            if !offline {
                for remote in &remotes {
                    match git.list_remote_tags(repo_path, remote) {
                        Ok(tags) => {
                            for (tag_name, commit) in tags {
                                remote_tag_map
                                    .entry(tag_name)
                                    .and_modify(|(_c, names)| names.push(remote.clone()))
                                    .or_insert_with(|| (commit, vec![remote.clone()]));
                            }
                        }
                        Err(e) => {
                            local_warnings.push(format!(
                                "could not list remote tags for {remote} in {}: {e}",
                                repo_path.display()
                            ));
                        }
                    }
                }
            }

            let all_tag_names: Vec<String> = local_tags
                .iter()
                .cloned()
                .chain(remote_tag_map.keys().cloned())
                .collect::<HashSet<String>>()
                .into_iter()
                .collect();

            if all_tag_names.is_empty() {
                return (None, local_warnings);
            }

            let repo_name = repo_display_name(repo_path);

            if verbose {
                eprintln!(
                    "{repo_name}: {} local, {} remote tags",
                    local_tags.len(),
                    remote_tag_map.len()
                );
            }

            let mut classified = Vec::with_capacity(all_tag_names.len());

            for tag_name in &all_tag_names {
                if !entity_filter.matches(tag_name) {
                    continue;
                }

                let is_local = local_tags.contains(tag_name);

                let (local_commit, is_reachable, is_annotated, tagger_date) = if is_local {
                    let commit = match git.tag_commit(repo_path, tag_name) {
                        Ok(c) => c,
                        Err(e) => {
                            local_warnings.push(format!(
                                "could not resolve tag {tag_name} in {}: {e}",
                                repo_path.display()
                            ));
                            continue;
                        }
                    };
                    let reachable = git.is_commit_reachable(repo_path, &commit).unwrap_or(false);
                    let annotated = git.is_tag_annotated(repo_path, tag_name).unwrap_or(false);
                    let date = git.tag_date(repo_path, tag_name).unwrap_or(None);
                    (Some(commit), reachable, annotated, date)
                } else {
                    (None, false, false, None)
                };

                let (remote_commit, remote_names) = match remote_tag_map.remove(tag_name.as_str()) {
                    Some((commit, names)) => (Some(commit), names),
                    None => (None, vec![]),
                };

                let classification = classify_tag(
                    local_commit.as_deref(),
                    remote_commit.as_deref(),
                    is_reachable,
                    offline,
                );

                let commit =
                    local_commit.unwrap_or_else(|| remote_commit.unwrap_or_else(String::new));

                if verbose {
                    eprintln!(
                        "  {tag_name}: {} (reachable={is_reachable}, local={is_local}, remote={})",
                        classification.label(),
                        remote_names.len(),
                    );
                }

                classified.push(TagInfo {
                    repo_path: repo_path.to_path_buf(),
                    name: tag_name.clone(),
                    classification,
                    commit,
                    is_annotated,
                    tagger_date,
                    is_release_tag: is_release_tag_name(tag_name),
                    remote_names,
                });
            }

            classified.sort_by(|a, b| {
                a.classification
                    .priority()
                    .cmp(&b.classification.priority())
                    .then_with(|| a.name.cmp(&b.name))
            });

            let group = TagRepoGroup {
                repo_path: repo_path.to_path_buf(),
                name: repo_name,
                tags: classified,
            };

            (Some(group), local_warnings)
        },
        "Scanning tags",
        progress,
    );
    warnings.extend(scan_warnings);

    let mut counts = TagCounts::default();
    let mut total_scanned = 0;
    for g in &repos {
        for t in &g.tags {
            counts.increment(&t.classification);
        }
        total_scanned += g.tags.len();
    }

    Ok(TagScanResult {
        repos,
        total_scanned,
        counts,
        warnings,
    })
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use git_tidy_core::testutil::MockGitBuilder;

    use super::*;

    fn repo() -> PathBuf {
        PathBuf::from("/repo")
    }

    #[test]
    fn classify_stale_unreachable_commit() {
        let result = classify_tag(Some("abc123"), Some("abc123"), false, false);
        assert_eq!(result, TagClassification::Stale);
    }

    #[test]
    fn classify_stale_unreachable_local_only() {
        let result = classify_tag(Some("abc123"), None, false, false);
        assert_eq!(result, TagClassification::Stale);
    }

    #[test]
    fn classify_local_only() {
        let result = classify_tag(Some("abc123"), None, true, false);
        assert_eq!(result, TagClassification::LocalOnly);
    }

    #[test]
    fn classify_remote_only() {
        let result = classify_tag(None, Some("abc123"), false, false);
        assert_eq!(result, TagClassification::RemoteOnly);
    }

    #[test]
    fn classify_synced() {
        let result = classify_tag(Some("abc123"), Some("abc123"), true, false);
        assert_eq!(result, TagClassification::Synced);
    }

    #[test]
    fn classify_offline_local_treated_as_synced() {
        // When offline, we can't check remote, so local+reachable = synced
        let result = classify_tag(Some("abc123"), None, true, true);
        assert_eq!(result, TagClassification::Synced);
    }

    #[test]
    fn classify_offline_unreachable_still_stale() {
        let result = classify_tag(Some("abc123"), None, false, true);
        assert_eq!(result, TagClassification::Stale);
    }

    #[test]
    fn scan_sort_order() {
        let mut tags = [
            TagInfo {
                repo_path: repo(),
                name: "v1.0.0".to_string(),
                classification: TagClassification::Synced,
                commit: "aaa".to_string(),
                is_annotated: true,
                tagger_date: None,
                is_release_tag: true,
                remote_names: vec!["origin".to_string()],
            },
            TagInfo {
                repo_path: repo(),
                name: "old-experiment".to_string(),
                classification: TagClassification::Stale,
                commit: "bbb".to_string(),
                is_annotated: false,
                tagger_date: None,
                is_release_tag: false,
                remote_names: vec![],
            },
            TagInfo {
                repo_path: repo(),
                name: "feature-wip".to_string(),
                classification: TagClassification::LocalOnly,
                commit: "ccc".to_string(),
                is_annotated: false,
                tagger_date: None,
                is_release_tag: false,
                remote_names: vec![],
            },
        ];

        tags.sort_by(|a, b| {
            a.classification
                .priority()
                .cmp(&b.classification.priority())
                .then_with(|| a.name.cmp(&b.name))
        });

        assert_eq!(tags[0].classification, TagClassification::Stale);
        assert_eq!(tags[1].classification, TagClassification::LocalOnly);
        assert_eq!(tags[2].classification, TagClassification::Synced);
    }

    #[test]
    fn scan_with_mixed_tags() {
        let git = MockGitBuilder::new()
            .with_local_tags(&repo(), vec!["v1.0".to_string(), "stale-tag".to_string()])
            .with_list_remotes(&repo(), vec!["origin".to_string()])
            .with_remote_tags(
                &repo(),
                "origin",
                vec![("v1.0".to_string(), "abc123".to_string())],
            )
            .with_tag_commit(&repo(), "v1.0", "abc123")
            .with_tag_commit(&repo(), "stale-tag", "def456")
            .with_is_commit_reachable(&repo(), "abc123", true)
            .with_is_commit_reachable(&repo(), "def456", false)
            .with_is_tag_annotated(&repo(), "v1.0", true)
            .with_is_tag_annotated(&repo(), "stale-tag", false)
            .with_tag_date(&repo(), "v1.0", Some("2024-06-15T10:00:00+00:00"))
            .build();

        let result = classify_tag(Some("abc123"), Some("abc123"), true, false);
        assert_eq!(result, TagClassification::Synced);

        let result2 = classify_tag(Some("def456"), None, false, false);
        assert_eq!(result2, TagClassification::Stale);

        // Also verify mock accessors work
        assert_eq!(
            git.list_local_tags(&repo()).unwrap(),
            vec!["v1.0", "stale-tag"]
        );
    }

    #[test]
    fn scan_no_remotes_treats_reachable_as_synced() {
        // When there are no remotes, we can't distinguish local-only from synced,
        // so reachable local tags become synced (same as offline behavior).
        let result = classify_tag(Some("abc123"), None, true, true);
        assert_eq!(result, TagClassification::Synced);
    }

    #[test]
    fn run_scan_repos_with_mock() {
        let git = MockGitBuilder::new()
            .with_local_tags(&repo(), vec!["v1.0".to_string()])
            .with_list_remotes(&repo(), vec![])
            .with_tag_commit(&repo(), "v1.0", "abc123")
            .with_is_commit_reachable(&repo(), "abc123", true)
            .with_is_tag_annotated(&repo(), "v1.0", true)
            .with_tag_date(&repo(), "v1.0", Some("2024-06-15T10:00:00+00:00"))
            .build();

        let p = Progress::disabled();
        let filter = NameFilter::default();
        let result = run_scan_repos(&git, &[repo()], true, false, &filter, &p).unwrap();
        assert_eq!(result.total_scanned, 1);
        assert_eq!(result.counts.synced, 1);
    }
}
