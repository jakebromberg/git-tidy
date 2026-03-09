use std::collections::{HashMap, HashSet};
use std::path::Path;
use std::sync::Mutex;

use crate::error::Error;
use crate::git::GitOps;
use crate::types::{Classification, UnmatchedCommit};

/// Thread-safe cache for per-commit landed detection results within a repo.
///
/// When multiple branches share the same commits, this cache avoids redundant
/// git subprocess calls by remembering whether each commit was already matched
/// against the default branch. Create one per repo and pass it to
/// [`detect_landed_cached`] for each branch.
///
/// Uses interior mutability (`Mutex`) so it can be shared across threads
/// for parallel branch classification.
pub struct LandedCache {
    /// Maps commit hash to whether it was matched on the default branch.
    results: Mutex<HashMap<String, bool>>,
}

impl Default for LandedCache {
    fn default() -> Self {
        Self {
            results: Mutex::new(HashMap::new()),
        }
    }
}

impl LandedCache {
    /// Create an empty cache.
    pub fn new() -> Self {
        Self::default()
    }
}

/// Options controlling how landed detection runs.
///
/// Use [`LandedOptions::default()`] for the standard full-pipeline behavior.
/// Tweak individual fields to trade accuracy for speed.
#[derive(Debug, Clone, Default)]
pub struct LandedOptions {
    /// Skip the patch similarity stage (the most expensive stage).
    /// When true, only exact and fuzzy subject matching are attempted.
    pub skip_patch_similarity: bool,

    /// Stop evaluating commits after this many consecutive unmatched commits.
    /// `None` means no limit (evaluate all commits).
    pub max_unmatched: Option<usize>,
}

/// Threshold for fuzzy subject matching (combined Jaccard + Levenshtein).
const FUZZY_THRESHOLD: f64 = 0.6;

/// Threshold for patch similarity matching.
const PATCH_SIMILARITY_THRESHOLD: f64 = 0.5;

/// Result of landed detection for a single branch.
#[derive(Debug)]
pub struct LandedResult {
    pub classification: Classification,
    pub matched: usize,
    pub total: usize,
}

/// Check if branch commits have "landed" on the default branch via rebase,
/// cherry-pick, or squash-merge.
pub fn detect_landed(
    git: &dyn GitOps,
    repo: &Path,
    default_branch_ref: &str,
    branch_ref: &str,
    verbose: bool,
    options: &LandedOptions,
) -> Result<LandedResult, Error> {
    detect_landed_cached(
        git,
        repo,
        default_branch_ref,
        branch_ref,
        verbose,
        &LandedCache::new(),
        options,
    )
}

/// Check if branch commits have "landed" on the default branch, reusing
/// cached per-commit results from previous branches in the same repo.
///
/// When a repo has many branches that share commits (e.g., branches forked
/// from the same point), this avoids re-running the full detection pipeline
/// (exact match, fuzzy match, patch similarity) for commits already seen.
pub fn detect_landed_cached(
    git: &dyn GitOps,
    repo: &Path,
    default_branch_ref: &str,
    branch_ref: &str,
    verbose: bool,
    cache: &LandedCache,
    options: &LandedOptions,
) -> Result<LandedResult, Error> {
    let unique_commits = git.log_exclusive(repo, default_branch_ref, branch_ref)?;
    let total = unique_commits.len();

    if verbose {
        eprintln!("    landed detection: {total} unique commits on {branch_ref}");
    }

    if total == 0 {
        return Ok(LandedResult {
            classification: Classification::LandedByContent {
                matched: 0,
                total: 0,
            },
            matched: 0,
            total: 0,
        });
    }

    let mut matched = 0;
    let mut unmatched = Vec::new();
    let mut consecutive_unmatched: usize = 0;

    for (hash, subject) in &unique_commits {
        let short_hash = &hash[..hash.len().min(7)];

        // Early exit: stop evaluating after too many consecutive unmatched commits.
        if let Some(max) = options.max_unmatched
            && consecutive_unmatched >= max
        {
            if verbose {
                eprintln!("    {short_hash}: skipped (max_unmatched={max} reached)");
            }
            unmatched.push(UnmatchedCommit {
                short_hash: short_hash.to_string(),
                subject: subject.clone(),
            });
            continue;
        }

        // Check the cache for a previous result on this commit.
        // Lock briefly for lookup, then release before doing any git work.
        if let Some(&was_matched) = cache.results.lock().unwrap().get(hash) {
            if was_matched {
                matched += 1;
                consecutive_unmatched = 0;
                if verbose {
                    eprintln!("    {short_hash}: cached match \"{subject}\"");
                }
            } else {
                consecutive_unmatched += 1;
                if verbose {
                    eprintln!("    {short_hash}: cached no-match \"{subject}\"");
                }
                unmatched.push(UnmatchedCommit {
                    short_hash: short_hash.to_string(),
                    subject: subject.clone(),
                });
            }
            continue;
        }

        if try_exact_subject_match(git, repo, default_branch_ref, subject)? {
            matched += 1;
            consecutive_unmatched = 0;
            cache.results.lock().unwrap().insert(hash.clone(), true);
            if verbose {
                eprintln!("    {short_hash}: exact subject match \"{subject}\"");
            }
            continue;
        }

        if try_fuzzy_subject_match(git, repo, default_branch_ref, subject)? {
            matched += 1;
            consecutive_unmatched = 0;
            cache.results.lock().unwrap().insert(hash.clone(), true);
            if verbose {
                eprintln!("    {short_hash}: fuzzy subject match \"{subject}\"");
            }
            continue;
        }

        if !options.skip_patch_similarity
            && try_patch_similarity(git, repo, default_branch_ref, hash)?
        {
            matched += 1;
            consecutive_unmatched = 0;
            cache.results.lock().unwrap().insert(hash.clone(), true);
            if verbose {
                eprintln!("    {short_hash}: patch similarity match \"{subject}\"");
            }
            continue;
        }

        consecutive_unmatched += 1;
        cache.results.lock().unwrap().insert(hash.clone(), false);
        if verbose {
            eprintln!("    {short_hash}: no match \"{subject}\"");
        }

        unmatched.push(UnmatchedCommit {
            short_hash: short_hash.to_string(),
            subject: subject.clone(),
        });
    }

    let classification = if matched == total {
        Classification::LandedByContent { matched, total }
    } else if matched > 0 {
        Classification::LandedPartial {
            matched,
            total,
            unmatched,
        }
    } else {
        // No commits landed — not a landed classification at all.
        // Return a "zero matched" result; caller decides active vs local.
        return Ok(LandedResult {
            classification: Classification::Active, // placeholder
            matched: 0,
            total,
        });
    };

    Ok(LandedResult {
        classification,
        matched,
        total,
    })
}

/// Strip conventional commit prefixes like "feat:", "fix(scope):", etc.
pub fn strip_cc_prefix(subject: &str) -> &str {
    // Match patterns like: "feat:", "fix(scope):", "chore!:", "feat(ui)!:"
    let bytes = subject.as_bytes();
    let mut i = 0;

    // Skip the type: lowercase letters
    while i < bytes.len() && bytes[i].is_ascii_lowercase() {
        i += 1;
    }

    if i == 0 {
        return subject; // no prefix
    }

    // Optional scope in parentheses
    if i < bytes.len() && bytes[i] == b'(' {
        i += 1;
        while i < bytes.len() && bytes[i] != b')' {
            i += 1;
        }
        if i < bytes.len() {
            i += 1; // skip ')'
        }
    }

    // Optional '!'
    if i < bytes.len() && bytes[i] == b'!' {
        i += 1;
    }

    // Must have ':'
    if i < bytes.len() && bytes[i] == b':' {
        i += 1;
        // Skip whitespace after colon
        while i < bytes.len() && bytes[i] == b' ' {
            i += 1;
        }
        &subject[i..]
    } else {
        subject
    }
}

/// Try exact subject match: search the default branch for commits with matching subjects.
fn try_exact_subject_match(
    git: &dyn GitOps,
    repo: &Path,
    default_branch_ref: &str,
    subject: &str,
) -> Result<bool, Error> {
    let search_term = strip_cc_prefix(subject);
    if search_term.len() < 5 {
        return Ok(false); // too short to be meaningful
    }
    let matches = git.log_grep(repo, default_branch_ref, search_term)?;
    Ok(!matches.is_empty())
}

/// Try fuzzy subject matching using token overlap (Jaccard) and Levenshtein distance.
fn try_fuzzy_subject_match(
    git: &dyn GitOps,
    repo: &Path,
    default_branch_ref: &str,
    subject: &str,
) -> Result<bool, Error> {
    let stripped = strip_cc_prefix(subject);
    let tokens: HashSet<String> = tokenize(stripped).into_iter().collect();

    if tokens.len() < 2 {
        return Ok(false);
    }

    // Search for commits containing significant tokens
    let search_term = tokens
        .iter()
        .filter(|t| t.len() >= 4)
        .take(3)
        .cloned()
        .collect::<Vec<_>>()
        .join(" ");

    if search_term.is_empty() {
        return Ok(false);
    }

    let candidates = git.log_grep(repo, default_branch_ref, &search_term)?;

    for (_, candidate_subject) in &candidates {
        let candidate_stripped = strip_cc_prefix(candidate_subject);
        let score = combined_similarity_with_tokens(&tokens, stripped, candidate_stripped);
        if score >= FUZZY_THRESHOLD {
            return Ok(true);
        }
    }

    Ok(false)
}

/// Try patch similarity: compare diffs of branch commit vs default branch commits
/// touching the same files.
fn try_patch_similarity(
    git: &dyn GitOps,
    repo: &Path,
    default_branch_ref: &str,
    commit_hash: &str,
) -> Result<bool, Error> {
    let changed_files = git.diff_commit_files(repo, commit_hash)?;
    if changed_files.is_empty() {
        return Ok(false);
    }

    let candidates = git.log_touching_files(repo, default_branch_ref, &changed_files)?;
    if candidates.is_empty() {
        return Ok(false);
    }

    let branch_diff = git.diff_commit(repo, commit_hash)?;
    if branch_diff.is_empty() {
        return Ok(false);
    }

    for (candidate_hash, _) in &candidates {
        let candidate_diff = git.diff_commit_on_ref(repo, candidate_hash)?;
        if candidate_diff.is_empty() {
            continue;
        }
        let similarity = diff_similarity(&branch_diff, &candidate_diff);
        if similarity >= PATCH_SIMILARITY_THRESHOLD {
            return Ok(true);
        }
    }

    Ok(false)
}

/// Tokenize a string into lowercase words.
fn tokenize(s: &str) -> Vec<String> {
    s.split(|c: char| !c.is_alphanumeric())
        .filter(|w| !w.is_empty())
        .map(|w| w.to_lowercase())
        .collect()
}

/// Combined similarity score: average of Jaccard and Levenshtein.
#[cfg(test)]
fn combined_similarity(a: &str, b: &str) -> f64 {
    let tokens_a: HashSet<String> = tokenize(a).into_iter().collect();
    combined_similarity_with_tokens(&tokens_a, a, b)
}

/// Combined similarity with pre-tokenized LHS.
fn combined_similarity_with_tokens(tokens_a: &HashSet<String>, a: &str, b: &str) -> f64 {
    let jaccard = jaccard_similarity_with_tokens(tokens_a, b);
    let levenshtein = strsim::normalized_levenshtein(a, b);
    (jaccard + levenshtein) / 2.0
}

/// Jaccard similarity: intersection / union of token sets.
#[cfg(test)]
fn jaccard_similarity(a: &str, b: &str) -> f64 {
    let tokens_a: HashSet<String> = tokenize(a).into_iter().collect();
    jaccard_similarity_with_tokens(&tokens_a, b)
}

/// Jaccard similarity with pre-tokenized LHS.
fn jaccard_similarity_with_tokens(tokens_a: &HashSet<String>, b: &str) -> f64 {
    let tokens_b: HashSet<String> = tokenize(b).into_iter().collect();

    if tokens_a.is_empty() && tokens_b.is_empty() {
        return 1.0;
    }

    let intersection = tokens_a.intersection(&tokens_b).count();
    let union = tokens_a.union(&tokens_b).count();

    if union == 0 {
        return 0.0;
    }

    intersection as f64 / union as f64
}

/// Simple diff similarity: compare the added/removed lines as token sets.
pub fn diff_similarity(diff_a: &str, diff_b: &str) -> f64 {
    let lines_a: HashSet<&str> = diff_a
        .lines()
        .filter(|l| l.starts_with('+') || l.starts_with('-'))
        .filter(|l| !l.starts_with("+++") && !l.starts_with("---"))
        .collect();

    let lines_b: HashSet<&str> = diff_b
        .lines()
        .filter(|l| l.starts_with('+') || l.starts_with('-'))
        .filter(|l| !l.starts_with("+++") && !l.starts_with("---"))
        .collect();

    if lines_a.is_empty() && lines_b.is_empty() {
        return 1.0;
    }

    let intersection = lines_a.intersection(&lines_b).count();
    let union = lines_a.union(&lines_b).count();

    if union == 0 {
        return 0.0;
    }

    intersection as f64 / union as f64
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::*;
    use crate::testutil::MockGitBuilder;

    fn repo() -> PathBuf {
        PathBuf::from("/repo")
    }

    #[test]
    fn strip_cc_prefix_feat() {
        assert_eq!(strip_cc_prefix("feat: add login"), "add login");
    }

    #[test]
    fn strip_cc_prefix_fix_with_scope() {
        assert_eq!(
            strip_cc_prefix("fix(auth): handle null token"),
            "handle null token"
        );
    }

    #[test]
    fn strip_cc_prefix_breaking() {
        assert_eq!(strip_cc_prefix("feat!: breaking change"), "breaking change");
    }

    #[test]
    fn strip_cc_prefix_none() {
        assert_eq!(strip_cc_prefix("Add login feature"), "Add login feature");
    }

    #[test]
    fn strip_cc_prefix_chore_scope_bang() {
        assert_eq!(
            strip_cc_prefix("chore(deps)!: bump version"),
            "bump version"
        );
    }

    #[test]
    fn tokenize_basic() {
        let tokens = tokenize("Fix HTTP response splitting");
        assert_eq!(tokens, vec!["fix", "http", "response", "splitting"]);
    }

    #[test]
    fn tokenize_with_special_chars() {
        let tokens = tokenize("feat(ui): add-button_component");
        assert_eq!(tokens, vec!["feat", "ui", "add", "button", "component"]);
    }

    #[test]
    fn jaccard_identical() {
        assert!((jaccard_similarity("add login feature", "add login feature") - 1.0).abs() < 0.01);
    }

    #[test]
    fn jaccard_similar() {
        let score = jaccard_similarity(
            "Sanitize returnURL in LoginServlet",
            "Fix HTTP response splitting in LoginServlet",
        );
        // Some overlap: "in", "LoginServlet"
        assert!(score > 0.1);
        assert!(score < 1.0);
    }

    #[test]
    fn jaccard_disjoint() {
        let score = jaccard_similarity("add button", "remove database");
        assert!(score < 0.01);
    }

    #[test]
    fn combined_similarity_same_meaning() {
        let score = combined_similarity(
            "Sanitize returnURL in LoginServlet endSession redirect",
            "Fix HTTP response splitting in LoginServlet endSession redirect",
        );
        // Should be reasonably high due to shared tokens
        assert!(score > 0.4, "score was {score}");
    }

    #[test]
    fn diff_similarity_identical() {
        let diff = "+line1\n-line2\n+line3\n";
        assert!((diff_similarity(diff, diff) - 1.0).abs() < 0.01);
    }

    #[test]
    fn diff_similarity_different() {
        let diff_a = "+aaa\n-bbb\n";
        let diff_b = "+xxx\n-yyy\n";
        assert!(diff_similarity(diff_a, diff_b) < 0.01);
    }

    #[test]
    fn detect_landed_all_matched() {
        let git = MockGitBuilder::new()
            .with_log_exclusive(
                &repo(),
                "origin/main",
                "feature/done",
                vec![
                    ("aaa1111".into(), "feat: add widget".into()),
                    ("bbb2222".into(), "fix: widget alignment".into()),
                ],
            )
            .with_log_grep(
                &repo(),
                "origin/main",
                "add widget",
                vec![("ccc".into(), "feat: add widget".into())],
            )
            .with_log_grep(
                &repo(),
                "origin/main",
                "widget alignment",
                vec![("ddd".into(), "fix: widget alignment".into())],
            )
            .build();

        let result = detect_landed(&git, &repo(), "origin/main", "feature/done", false, &LandedOptions::default()).unwrap();
        assert_eq!(result.matched, 2);
        assert_eq!(result.total, 2);
        assert!(matches!(
            result.classification,
            Classification::LandedByContent {
                matched: 2,
                total: 2
            }
        ));
    }

    #[test]
    fn detect_landed_partial() {
        let git = MockGitBuilder::new()
            .with_log_exclusive(
                &repo(),
                "origin/main",
                "feature/partial",
                vec![
                    ("aaa1111".into(), "feat: add widget".into()),
                    ("bbb2222".into(), "Add unique feature X".into()),
                ],
            )
            .with_log_grep(
                &repo(),
                "origin/main",
                "add widget",
                vec![("ccc".into(), "feat: add widget".into())],
            )
            // "Add unique feature X" — no exact match, no fuzzy match, no patch match
            .build();

        let result = detect_landed(&git, &repo(), "origin/main", "feature/partial", false, &LandedOptions::default()).unwrap();
        assert_eq!(result.matched, 1);
        assert_eq!(result.total, 2);
        match &result.classification {
            Classification::LandedPartial {
                matched: 1,
                total: 2,
                unmatched,
            } => {
                assert_eq!(unmatched.len(), 1);
                assert_eq!(unmatched[0].subject, "Add unique feature X");
            }
            other => panic!("expected LandedPartial, got {other:?}"),
        }
    }

    #[test]
    fn detect_landed_none_matched() {
        let git = MockGitBuilder::new()
            .with_log_exclusive(
                &repo(),
                "origin/main",
                "feature/none",
                vec![("aaa1111".into(), "Totally unique work".into())],
            )
            .build();

        let result = detect_landed(&git, &repo(), "origin/main", "feature/none", false, &LandedOptions::default()).unwrap();
        assert_eq!(result.matched, 0);
        assert_eq!(result.total, 1);
    }

    #[test]
    fn detect_landed_empty_branch() {
        let git = MockGitBuilder::new()
            .with_log_exclusive(&repo(), "origin/main", "feature/empty", vec![])
            .build();

        let result = detect_landed(&git, &repo(), "origin/main", "feature/empty", false, &LandedOptions::default()).unwrap();
        assert_eq!(result.matched, 0);
        assert_eq!(result.total, 0);
        assert!(matches!(
            result.classification,
            Classification::LandedByContent {
                matched: 0,
                total: 0
            }
        ));
    }

    #[test]
    fn detect_landed_cached_skips_redundant_commits() {
        // Two branches share commit "aaa1111" which doesn't match.
        // The second branch should use the cached result.
        let git = MockGitBuilder::new()
            // Branch 1: has aaa1111 (unmatched) and bbb2222 (matched)
            .with_log_exclusive(
                &repo(),
                "origin/main",
                "feature/branch1",
                vec![
                    ("aaa1111".into(), "Unique unmatched work".into()),
                    ("bbb2222".into(), "feat: add widget".into()),
                ],
            )
            .with_log_grep(
                &repo(),
                "origin/main",
                "add widget",
                vec![("ccc".into(), "feat: add widget".into())],
            )
            // Branch 2: shares aaa1111 with branch1, plus has ddd4444 (matched)
            .with_log_exclusive(
                &repo(),
                "origin/main",
                "feature/branch2",
                vec![
                    ("aaa1111".into(), "Unique unmatched work".into()),
                    ("ddd4444".into(), "fix: widget alignment".into()),
                ],
            )
            .with_log_grep(
                &repo(),
                "origin/main",
                "widget alignment",
                vec![("eee".into(), "fix: widget alignment".into())],
            )
            .build();

        let cache = LandedCache::new();

        // First branch: aaa1111 runs full pipeline (no exact, no fuzzy, no patch → unmatched)
        let r1 = detect_landed_cached(
            &git,
            &repo(),
            "origin/main",
            "feature/branch1",
            false,
            &cache,
            &LandedOptions::default(),
        )
        .unwrap();
        assert_eq!(r1.matched, 1); // bbb2222 matched
        assert_eq!(r1.total, 2);
        assert_eq!(cache.results.lock().unwrap().len(), 2); // both commits cached

        // Second branch: aaa1111 should be served from cache (no git calls),
        // ddd4444 runs the pipeline fresh
        let r2 = detect_landed_cached(
            &git,
            &repo(),
            "origin/main",
            "feature/branch2",
            false,
            &cache,
            &LandedOptions::default(),
        )
        .unwrap();
        assert_eq!(r2.matched, 1); // ddd4444 matched
        assert_eq!(r2.total, 2);
        assert_eq!(cache.results.lock().unwrap().len(), 3); // aaa1111, bbb2222, ddd4444
    }

    #[test]
    fn detect_landed_cached_reuses_matched_commits() {
        // Both branches have the same matched commit — second should hit cache
        let git = MockGitBuilder::new()
            .with_log_exclusive(
                &repo(),
                "origin/main",
                "feature/branch1",
                vec![("aaa1111".into(), "feat: add widget".into())],
            )
            .with_log_grep(
                &repo(),
                "origin/main",
                "add widget",
                vec![("ccc".into(), "feat: add widget".into())],
            )
            .with_log_exclusive(
                &repo(),
                "origin/main",
                "feature/branch2",
                vec![("aaa1111".into(), "feat: add widget".into())],
            )
            // No log_grep needed for branch2 — cache should handle it
            .build();

        let cache = LandedCache::new();

        let r1 = detect_landed_cached(
            &git,
            &repo(),
            "origin/main",
            "feature/branch1",
            false,
            &cache,
            &LandedOptions::default(),
        )
        .unwrap();
        assert_eq!(r1.matched, 1);
        assert!(matches!(
            r1.classification,
            Classification::LandedByContent {
                matched: 1,
                total: 1,
            }
        ));

        // Second branch: aaa1111 is in cache as matched — no git calls needed
        let r2 = detect_landed_cached(
            &git,
            &repo(),
            "origin/main",
            "feature/branch2",
            false,
            &cache,
            &LandedOptions::default(),
        )
        .unwrap();
        assert_eq!(r2.matched, 1);
        assert!(matches!(
            r2.classification,
            Classification::LandedByContent {
                matched: 1,
                total: 1,
            }
        ));
    }

    #[test]
    fn detect_landed_patch_similarity() {
        let diff_content = "+added line\n-removed line\n";
        let git = MockGitBuilder::new()
            .with_log_exclusive(
                &repo(),
                "origin/main",
                "feature/patch",
                vec![("aaa1111".into(), "Completely different subject".into())],
            )
            // No exact match, no fuzzy match
            // But patch similarity should match
            .with_diff_commit_files(&repo(), "aaa1111", vec!["src/main.rs".into()])
            .with_log_touching_files(
                &repo(),
                "origin/main",
                vec![("bbb2222".into(), "Some other commit".into())],
            )
            .with_diff_commit(&repo(), "aaa1111", diff_content)
            .with_diff_commit_on_ref(&repo(), "bbb2222", diff_content) // identical patch
            .build();

        let result = detect_landed(&git, &repo(), "origin/main", "feature/patch", false, &LandedOptions::default()).unwrap();
        assert_eq!(result.matched, 1);
        assert_eq!(result.total, 1);
    }

    #[test]
    fn skip_patch_similarity_skips_patch_stage() {
        // Same setup as detect_landed_patch_similarity, but with skip_patch_similarity=true.
        // Without patch similarity, the commit should not match.
        let diff_content = "+added line\n-removed line\n";
        let git = MockGitBuilder::new()
            .with_log_exclusive(
                &repo(),
                "origin/main",
                "feature/patch",
                vec![("aaa1111".into(), "Completely different subject".into())],
            )
            .with_diff_commit_files(&repo(), "aaa1111", vec!["src/main.rs".into()])
            .with_log_touching_files(
                &repo(),
                "origin/main",
                vec![("bbb2222".into(), "Some other commit".into())],
            )
            .with_diff_commit(&repo(), "aaa1111", diff_content)
            .with_diff_commit_on_ref(&repo(), "bbb2222", diff_content)
            .build();

        let options = LandedOptions {
            skip_patch_similarity: true,
            ..Default::default()
        };
        let result = detect_landed(&git, &repo(), "origin/main", "feature/patch", false, &options).unwrap();
        assert_eq!(result.matched, 0);
        assert_eq!(result.total, 1);
    }

    #[test]
    fn max_unmatched_stops_evaluation() {
        // Three commits, none match. With max_unmatched=2, only the first two
        // should run the full pipeline; the third should be skipped.
        let git = MockGitBuilder::new()
            .with_log_exclusive(
                &repo(),
                "origin/main",
                "feature/many",
                vec![
                    ("aaa1111".into(), "Unique work one".into()),
                    ("bbb2222".into(), "Unique work two".into()),
                    ("ccc3333".into(), "Unique work three".into()),
                ],
            )
            // No matches configured — all three fail exact/fuzzy/patch
            .build();

        let options = LandedOptions {
            max_unmatched: Some(2),
            ..Default::default()
        };
        let result = detect_landed(&git, &repo(), "origin/main", "feature/many", false, &options).unwrap();
        assert_eq!(result.matched, 0);
        assert_eq!(result.total, 3);
    }

    #[test]
    fn max_unmatched_resets_on_match() {
        // Four commits: first unmatched, second matches, third unmatched, fourth unmatched.
        // With max_unmatched=2, all should be evaluated because the match resets the counter.
        let git = MockGitBuilder::new()
            .with_log_exclusive(
                &repo(),
                "origin/main",
                "feature/mixed",
                vec![
                    ("aaa1111".into(), "Unique work".into()),
                    ("bbb2222".into(), "feat: add widget".into()),
                    ("ccc3333".into(), "More unique work".into()),
                    ("ddd4444".into(), "Even more unique".into()),
                ],
            )
            .with_log_grep(
                &repo(),
                "origin/main",
                "add widget",
                vec![("eee".into(), "feat: add widget".into())],
            )
            .build();

        let options = LandedOptions {
            max_unmatched: Some(2),
            ..Default::default()
        };
        let result = detect_landed(&git, &repo(), "origin/main", "feature/mixed", false, &options).unwrap();
        assert_eq!(result.matched, 1);
        assert_eq!(result.total, 4);
    }
}
