use std::path::{Path, PathBuf};

use rayon::prelude::*;
use serde::Serialize;

use crate::counts::Counts;
use crate::fetch::parallel_fetch;
use crate::git::GitOps;
use crate::output::{FlatJsonItems, IntoJsonItem, repo_display_name};
use crate::progress::Progress;

/// Run a per-repo classification function in parallel across a set of repo paths.
///
/// Handles the parallel dispatch, progress bar, and warning collection.
/// The caller provides a closure that classifies a single repo, returning
/// `(Option<G>, Vec<String>)` where `G` is the tool-specific repo group
/// and the Vec contains any warnings for that repo.
///
/// Returns `(groups, warnings)` — the non-None groups and all accumulated warnings.
pub fn parallel_classify<G: Send>(
    repo_paths: &[PathBuf],
    classify_fn: impl Fn(&Path) -> (Option<G>, Vec<String>) + Sync + Send,
    label: &str,
    progress: &Progress,
) -> (Vec<G>, Vec<String>) {
    let pb = progress.bar(repo_paths.len() as u64, label);
    let per_repo: Vec<_> = repo_paths
        .par_iter()
        .map(|repo_path| {
            let result = classify_fn(repo_path);
            pb.inc(1);
            result
        })
        .collect();
    pb.finish_and_clear();

    let mut groups = Vec::new();
    let mut warnings = Vec::new();
    for (group, local_warnings) in per_repo {
        warnings.extend(local_warnings);
        if let Some(g) = group {
            groups.push(g);
        }
    }
    (groups, warnings)
}

/// A group of classified items sharing the same repo.
///
/// The generic counterpart to each tool's hand-rolled `*RepoGroup`
/// (`TagRepoGroup`, `StashRepoGroup`, …). `items` are in the order the classifier
/// returned them — `run_pipeline` does not reorder. A tool that wants rows sorted
/// by classification priority sorts inside its own `classify_one` closure (as
/// remote-tidy does) before returning them.
#[derive(Debug, Clone, Serialize)]
pub struct RepoGroup<T> {
    /// Path to the repo.
    pub repo_path: PathBuf,
    /// Display name (directory basename).
    pub name: String,
    /// Items belonging to this repo.
    pub items: Vec<T>,
}

/// Result of a full scan, shared by every uniform scan-shaped tool.
///
/// The generic counterpart to each tool's hand-rolled `*ScanResult`
/// (`RemoteScanResult`, `TagScanResult`, …). Per-tool `--json` serializes only
/// the flat item array (via `FlatJsonItems`), never this struct whole, so the
/// `Serialize` derive here does not define any tool's public JSON.
#[derive(Debug, Clone, Serialize)]
pub struct ScanResult<T> {
    /// Items grouped by repo (empty groups omitted).
    pub repos: Vec<RepoGroup<T>>,
    /// Total items scanned across all groups.
    pub total_scanned: usize,
    /// Summary counts keyed by classification label.
    pub counts: Counts,
    /// Warnings accumulated during fetch and classification.
    pub warnings: Vec<String>,
}

/// Flatten any `ScanResult<T>` into JSON items, provided each item knows how to
/// become one (`T: IntoJsonItem`). This is the generic counterpart to each tool's
/// former hand-rolled `impl FlatJsonItems for *ScanResult`, and the reason tools
/// implement [`IntoJsonItem`] on their item type rather than `FlatJsonItems` on
/// the foreign `ScanResult<T>` (which the orphan rule forbids).
impl<T: IntoJsonItem> FlatJsonItems for ScanResult<T> {
    type JsonItem = T::JsonItem;

    fn to_json_items(&self) -> Vec<Self::JsonItem> {
        self.repos
            .iter()
            .flat_map(|g| g.items.iter())
            .map(IntoJsonItem::to_json_item)
            .collect()
    }
}

/// The one fact `run_pipeline` needs from an item to aggregate counts: its
/// classification label (the same string the tool's `label()` returns and the
/// audit runner emits as a JSON key).
pub trait Classified {
    /// Classification label used as the [`Counts`] bucket key.
    fn classification_label(&self) -> &str;
}

/// Options controlling the shared scan pipeline.
#[derive(Debug, Clone, Default)]
pub struct ScanOptions {
    /// Fetch (`git fetch --prune`) every repo before classifying. Worktree- and
    /// branch-tidy set this; the offline-by-default tools (remote, tag, stash)
    /// leave it false.
    pub fetch: bool,
}

/// Run the shared scan pipeline over a set of pre-discovered repos.
///
/// Layer 2 of the scan stack: the high-level seam for the uniform tools. Given a
/// per-repo `classify_one` closure that returns this repo's classified items and
/// any per-repo warnings, the pipeline:
///
/// 1. optionally `parallel_fetch`es every repo (when `options.fetch`),
/// 2. dispatches `classify_one` across repos via [`parallel_classify`], wrapping
///    each non-empty `Vec<T>` into a [`RepoGroup`] (empty groups are dropped),
/// 3. folds [`Counts`] by [`Classified::classification_label`] and sums
///    `total_scanned`, and
/// 4. assembles the [`ScanResult`].
///
/// Discovery and the per-tool entity filter (`--match`, name filters) stay
/// *outside* this seam — in the tool's thin `run_scan` and inside `classify_one`
/// respectively — because their granularity differs per tool and the audit
/// runner discovers once and shares `repo_paths` across tools.
pub fn run_pipeline<T: Classified + Send>(
    git: &dyn GitOps,
    repo_paths: &[PathBuf],
    options: &ScanOptions,
    label: &str,
    progress: &Progress,
    classify_one: impl Fn(&Path) -> (Vec<T>, Vec<String>) + Sync + Send,
) -> ScanResult<T> {
    let mut warnings = Vec::new();

    if options.fetch {
        let path_refs: Vec<&Path> = repo_paths.iter().map(PathBuf::as_path).collect();
        warnings.extend(parallel_fetch(git, &path_refs, progress));
    }

    let (repos, classify_warnings) = parallel_classify(
        repo_paths,
        |repo_path| {
            let (items, local_warnings) = classify_one(repo_path);
            if items.is_empty() {
                return (None, local_warnings);
            }
            let group = RepoGroup {
                repo_path: repo_path.to_path_buf(),
                name: repo_display_name(repo_path),
                items,
            };
            (Some(group), local_warnings)
        },
        label,
        progress,
    );
    warnings.extend(classify_warnings);

    let mut counts = Counts::default();
    let mut total_scanned = 0;
    for group in &repos {
        for item in &group.items {
            counts.increment(item.classification_label());
        }
        total_scanned += group.items.len();
    }

    ScanResult {
        repos,
        total_scanned,
        counts,
        warnings,
    }
}

#[cfg(test)]
mod pipeline_tests {
    use std::path::{Path, PathBuf};

    use crate::progress::Progress;
    use crate::testutil::MockGitBuilder;

    use super::{Classified, RepoGroup, ScanOptions, run_pipeline};

    /// Minimal `Classified` item for exercising the pipeline without dragging in a
    /// real tool's `*Info` type.
    struct TestItem {
        label: &'static str,
    }

    impl Classified for TestItem {
        fn classification_label(&self) -> &str {
            self.label
        }
    }

    fn opts(fetch: bool) -> ScanOptions {
        ScanOptions { fetch }
    }

    #[test]
    fn folds_counts_and_total_across_repos() {
        let git = MockGitBuilder::new().build();
        let repos = vec![PathBuf::from("/a"), PathBuf::from("/b")];

        let result = run_pipeline(
            &git,
            &repos,
            &opts(false),
            "Scanning",
            &Progress::disabled(),
            |path| {
                if path == Path::new("/a") {
                    (
                        vec![TestItem { label: "active" }, TestItem { label: "stale" }],
                        vec![],
                    )
                } else {
                    (vec![TestItem { label: "active" }], vec![])
                }
            },
        );

        assert_eq!(result.total_scanned, 3);
        assert_eq!(result.counts.get("active"), 2);
        assert_eq!(result.counts.get("stale"), 1);
        assert_eq!(result.repos.len(), 2);
    }

    #[test]
    fn skips_empty_groups() {
        let git = MockGitBuilder::new().build();
        let repos = vec![PathBuf::from("/a"), PathBuf::from("/empty")];

        let result = run_pipeline(
            &git,
            &repos,
            &opts(false),
            "Scanning",
            &Progress::disabled(),
            |path| {
                if path == Path::new("/empty") {
                    (vec![], vec![])
                } else {
                    (vec![TestItem { label: "active" }], vec![])
                }
            },
        );

        assert_eq!(result.repos.len(), 1);
        assert_eq!(result.repos[0].repo_path, PathBuf::from("/a"));
        assert_eq!(result.total_scanned, 1);
    }

    #[test]
    fn sets_group_name_and_path() {
        let git = MockGitBuilder::new().build();
        let repos = vec![PathBuf::from("/repos/my-project")];

        let result: super::ScanResult<TestItem> = run_pipeline(
            &git,
            &repos,
            &opts(false),
            "Scanning",
            &Progress::disabled(),
            |_| (vec![TestItem { label: "active" }], vec![]),
        );

        let group: &RepoGroup<TestItem> = &result.repos[0];
        assert_eq!(group.name, "my-project");
        assert_eq!(group.repo_path, PathBuf::from("/repos/my-project"));
        assert_eq!(group.items.len(), 1);
    }

    #[test]
    fn collects_classifier_warnings() {
        let git = MockGitBuilder::new().build();
        let repos = vec![PathBuf::from("/a")];

        let result = run_pipeline(
            &git,
            &repos,
            &opts(false),
            "Scanning",
            &Progress::disabled(),
            |_| {
                (
                    Vec::<TestItem>::new(),
                    vec!["could not read /a".to_string()],
                )
            },
        );

        assert!(
            result
                .warnings
                .iter()
                .any(|w| w.contains("could not read /a"))
        );
        assert!(result.repos.is_empty());
    }

    #[test]
    fn fetch_disabled_does_not_fetch() {
        let git = MockGitBuilder::new().build();
        let repos = vec![PathBuf::from("/a")];

        let _result = run_pipeline(
            &git,
            &repos,
            &opts(false),
            "Scanning",
            &Progress::disabled(),
            |_| (vec![TestItem { label: "active" }], vec![]),
        );

        assert!(git.fetch_prune_calls().is_empty());
    }

    #[test]
    fn fetch_enabled_fetches_each_repo() {
        let git = MockGitBuilder::new().build();
        let repos = vec![PathBuf::from("/a"), PathBuf::from("/b")];

        let _result = run_pipeline(
            &git,
            &repos,
            &opts(true),
            "Scanning",
            &Progress::disabled(),
            |_| (vec![TestItem { label: "active" }], vec![]),
        );

        let calls = git.fetch_prune_calls();
        assert_eq!(calls.len(), 2);
        assert!(calls.contains(&PathBuf::from("/a")));
        assert!(calls.contains(&PathBuf::from("/b")));
    }

    #[test]
    fn fetch_completes_before_classify() {
        // The seam's load-bearing ordering contract: when fetch is enabled, every
        // repo is fetched before ANY classify closure runs (parallel_fetch blocks
        // via thread::scope before parallel_classify dispatches). Each closure
        // observes that all fetches are already recorded — so a refactor that moved
        // the fetch after (or into) classification would fail here.
        let git = MockGitBuilder::new().build();
        let repos = vec![PathBuf::from("/a"), PathBuf::from("/b")];
        let observed_fetch_counts = std::sync::Mutex::new(Vec::new());

        let _result = run_pipeline(
            &git,
            &repos,
            &opts(true),
            "Scanning",
            &Progress::disabled(),
            |_| {
                observed_fetch_counts
                    .lock()
                    .unwrap()
                    .push(git.fetch_prune_calls().len());
                (vec![TestItem { label: "active" }], vec![])
            },
        );

        let observed = observed_fetch_counts.into_inner().unwrap();
        assert_eq!(observed.len(), 2);
        assert!(
            observed.iter().all(|&n| n == 2),
            "every classify closure should see both fetches already done, got {observed:?}"
        );
    }

    #[test]
    fn fetch_failure_warning_precedes_classify_warnings() {
        // A failed fetch surfaces a warning, and run_pipeline orders fetch warnings
        // ahead of classify warnings in the merged result.
        let git = MockGitBuilder::new()
            .with_fetch_prune_error(&PathBuf::from("/a"), "network down")
            .build();
        let repos = vec![PathBuf::from("/a")];

        let result = run_pipeline(
            &git,
            &repos,
            &opts(true),
            "Scanning",
            &Progress::disabled(),
            |_| {
                (
                    vec![TestItem { label: "active" }],
                    vec!["classify warning".to_string()],
                )
            },
        );

        assert_eq!(result.warnings.len(), 2, "warnings: {:?}", result.warnings);
        assert!(
            result.warnings[0].contains("/a") && result.warnings[0].contains("network down"),
            "first warning should be the fetch failure, got {:?}",
            result.warnings,
        );
        assert_eq!(result.warnings[1], "classify warning");
    }
}
