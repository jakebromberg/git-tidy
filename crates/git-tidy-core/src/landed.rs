use std::collections::HashSet;
use std::path::Path;

use crate::error::Error;
use crate::git::GitOps;
use crate::types::{Classification, UnmatchedCommit};

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
    _verbose: bool,
) -> Result<LandedResult, Error> {
    let unique_commits = git.log_exclusive(repo, default_branch_ref, branch_ref)?;
    let total = unique_commits.len();

    if total == 0 {
        return Ok(LandedResult {
            classification: Classification::Landed {
                matched: 0,
                total: 0,
            },
            matched: 0,
            total: 0,
        });
    }

    let mut matched = 0;
    let mut unmatched = Vec::new();

    for (hash, subject) in &unique_commits {
        let short_hash = &hash[..hash.len().min(7)];

        if try_exact_subject_match(git, repo, default_branch_ref, subject)? {
            matched += 1;
            continue;
        }

        if try_fuzzy_subject_match(git, repo, default_branch_ref, subject)? {
            matched += 1;
            continue;
        }

        if try_patch_similarity(git, repo, default_branch_ref, hash)? {
            matched += 1;
            continue;
        }

        unmatched.push(UnmatchedCommit {
            short_hash: short_hash.to_string(),
            subject: subject.clone(),
        });
    }

    let classification = if matched == total {
        Classification::Landed { matched, total }
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

        let result = detect_landed(&git, &repo(), "origin/main", "feature/done", false).unwrap();
        assert_eq!(result.matched, 2);
        assert_eq!(result.total, 2);
        assert!(matches!(
            result.classification,
            Classification::Landed {
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

        let result = detect_landed(&git, &repo(), "origin/main", "feature/partial", false).unwrap();
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

        let result = detect_landed(&git, &repo(), "origin/main", "feature/none", false).unwrap();
        assert_eq!(result.matched, 0);
        assert_eq!(result.total, 1);
    }

    #[test]
    fn detect_landed_empty_branch() {
        let git = MockGitBuilder::new()
            .with_log_exclusive(&repo(), "origin/main", "feature/empty", vec![])
            .build();

        let result = detect_landed(&git, &repo(), "origin/main", "feature/empty", false).unwrap();
        assert_eq!(result.matched, 0);
        assert_eq!(result.total, 0);
        assert!(matches!(
            result.classification,
            Classification::Landed {
                matched: 0,
                total: 0
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

        let result = detect_landed(&git, &repo(), "origin/main", "feature/patch", false).unwrap();
        assert_eq!(result.matched, 1);
        assert_eq!(result.total, 1);
    }
}
