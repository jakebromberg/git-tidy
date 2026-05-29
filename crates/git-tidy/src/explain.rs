//! Glossary of git-tidy terminology and classifications.

use std::io::Write;

/// A single glossary entry.
struct GlossaryEntry {
    term: &'static str,
    section: &'static str,
    description: &'static str,
    used_by: &'static [&'static str],
}

/// All glossary entries grouped by section.
static GLOSSARY: &[GlossaryEntry] = &[
    // Branches & Worktrees
    GlossaryEntry {
        term: "landed",
        section: "Branches & Worktrees",
        description: "Branch tip is an ancestor of the default branch (structurally merged).",
        used_by: &["branches", "worktrees"],
    },
    GlossaryEntry {
        term: "landed-stale",
        section: "Branches & Worktrees",
        description: "Branch ref was deleted (typically after a PR merge) but worktree remains. Safe to remove.",
        used_by: &["worktrees"],
    },
    GlossaryEntry {
        term: "landed-content",
        section: "Branches & Worktrees",
        description: "All branch commits matched on the default branch by content (patch heuristic).",
        used_by: &["branches", "worktrees"],
    },
    GlossaryEntry {
        term: "partial",
        section: "Branches & Worktrees",
        description: "Some but not all branch commits matched on the default branch.",
        used_by: &["branches", "worktrees"],
    },
    GlossaryEntry {
        term: "active",
        section: "Branches & Worktrees",
        description: "Branch has unique work that has not landed on the default branch. Still in use.",
        used_by: &["branches", "worktrees"],
    },
    GlossaryEntry {
        term: "local",
        section: "Branches & Worktrees",
        description: "Branch has no remote tracking branch configured.",
        used_by: &["branches", "worktrees"],
    },
    // Stashes
    GlossaryEntry {
        term: "committed",
        section: "Stashes",
        description: "Stash contents have been committed to the branch (safe to drop).",
        used_by: &["stashes"],
    },
    GlossaryEntry {
        term: "orphaned",
        section: "Stashes",
        description: "Stash references a branch that no longer exists.",
        used_by: &["stashes"],
    },
    GlossaryEntry {
        term: "aged",
        section: "Stashes",
        description: "Stash is older than the configured age threshold.",
        used_by: &["stashes"],
    },
    GlossaryEntry {
        term: "active",
        section: "Stashes",
        description: "Stash is recent and its branch still exists.",
        used_by: &["stashes"],
    },
    // Remotes
    GlossaryEntry {
        term: "unreachable",
        section: "Remotes",
        description: "Remote URL cannot be contacted (DNS failure, deleted repo, etc.).",
        used_by: &["remotes"],
    },
    GlossaryEntry {
        term: "orphaned",
        section: "Remotes",
        description: "Remote has no configured URL (leftover tracking refs only).",
        used_by: &["remotes"],
    },
    GlossaryEntry {
        term: "active",
        section: "Remotes",
        description: "Remote is reachable and in use.",
        used_by: &["remotes"],
    },
    // Tags
    GlossaryEntry {
        term: "stale",
        section: "Tags",
        description: "Tag points to a commit unreachable from any branch tip.",
        used_by: &["tags"],
    },
    GlossaryEntry {
        term: "local_only",
        section: "Tags",
        description: "Tag exists locally but not on any remote.",
        used_by: &["tags"],
    },
    GlossaryEntry {
        term: "remote_only",
        section: "Tags",
        description: "Tag exists on a remote but not locally.",
        used_by: &["tags"],
    },
    GlossaryEntry {
        term: "synced",
        section: "Tags",
        description: "Tag exists both locally and on at least one remote.",
        used_by: &["tags"],
    },
    // Repos
    GlossaryEntry {
        term: "stale",
        section: "Repos",
        description: "Repo has not been committed to within the configured stale-months threshold.",
        used_by: &["repos"],
    },
    GlossaryEntry {
        term: "orphaned",
        section: "Repos",
        description: "Repo has no configured remote (local-only, no upstream).",
        used_by: &["repos"],
    },
    GlossaryEntry {
        term: "active",
        section: "Repos",
        description: "Repo has a remote and recent commits.",
        used_by: &["repos"],
    },
    // LFS
    GlossaryEntry {
        term: "untracked",
        section: "LFS",
        description: "Large file stored directly in git instead of LFS.",
        used_by: &["lfs"],
    },
    GlossaryEntry {
        term: "missing",
        section: "LFS",
        description: "LFS pointer exists but the object is not downloaded locally.",
        used_by: &["lfs"],
    },
    GlossaryEntry {
        term: "orphaned",
        section: "LFS",
        description: "LFS object exists locally but is no longer referenced by any pointer.",
        used_by: &["lfs"],
    },
    GlossaryEntry {
        term: "healthy",
        section: "LFS",
        description: "LFS pointer and object are both present and consistent.",
        used_by: &["lfs"],
    },
    // Config
    GlossaryEntry {
        term: "orphaned_branch_config",
        section: "Config",
        description: "Git config section for a branch that no longer exists locally.",
        used_by: &["config"],
    },
    GlossaryEntry {
        term: "alias_shadows_builtin",
        section: "Config",
        description: "Git alias has the same name as a built-in git command.",
        used_by: &["config"],
    },
    // Annotations
    GlossaryEntry {
        term: "dirty",
        section: "Annotations",
        description: "Working tree has meaningful uncommitted changes.",
        used_by: &["branches", "worktrees", "repos"],
    },
    GlossaryEntry {
        term: "diverged",
        section: "Annotations",
        description: "Branch has commits both ahead of and behind the remote tracking branch.",
        used_by: &["branches", "worktrees"],
    },
    GlossaryEntry {
        term: "remote deleted",
        section: "Annotations",
        description: "The remote tracking branch has been deleted upstream.",
        used_by: &["branches", "worktrees"],
    },
    // Metrics
    GlossaryEntry {
        term: "+N/-M",
        section: "Metrics",
        description: "Commits ahead of / behind the default branch.",
        used_by: &["branches", "worktrees"],
    },
    GlossaryEntry {
        term: "N/M",
        section: "Metrics",
        description: "Landed ratio: N commits matched out of M total branch commits.",
        used_by: &["branches", "worktrees"],
    },
    GlossaryEntry {
        term: "unmatched",
        section: "Metrics",
        description: "Branch commits that could not be matched on the default branch (listed under partial).",
        used_by: &["branches", "worktrees"],
    },
];

/// Section ordering for grouped output.
static SECTIONS: &[&str] = &[
    "Branches & Worktrees",
    "Stashes",
    "Remotes",
    "Tags",
    "Repos",
    "LFS",
    "Config",
    "Annotations",
    "Metrics",
];

/// Write the full glossary grouped by section headers.
pub fn write_full(out: &mut dyn Write) -> std::io::Result<()> {
    writeln!(out, "git-tidy terminology")?;

    for section in SECTIONS {
        writeln!(out)?;
        writeln!(out, "{}", section.to_uppercase())?;

        for entry in GLOSSARY.iter().filter(|e| e.section == *section) {
            writeln!(out, "  {:<24} {}", entry.term, entry.description)?;
        }
    }

    Ok(())
}

/// Write a single-term lookup. Case-insensitive. Returns every glossary entry that matches, because many terms (`orphaned`, `active`, `stale`, …) appear in multiple sections with different meanings — previously only the first was returned, silently hiding the others.
pub fn write_term(out: &mut dyn Write, term: &str) -> std::io::Result<()> {
    let lower = term.to_lowercase();
    let matches: Vec<&GlossaryEntry> = GLOSSARY
        .iter()
        .filter(|e| e.term.to_lowercase() == lower)
        .collect();

    if matches.is_empty() {
        writeln!(
            out,
            "Unknown term: \"{term}\". Run 'git tidy explain' to see all terms."
        )?;
        return Ok(());
    }

    for (i, entry) in matches.iter().enumerate() {
        if i > 0 {
            writeln!(out)?;
        }
        writeln!(
            out,
            "{} ({}): {}",
            entry.term, entry.section, entry.description
        )?;
        writeln!(
            out,
            "{}  Used by: {}",
            " ".repeat(entry.term.len() + entry.section.len() + 4),
            entry.used_by.join(", ")
        )?;
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn full_output_contains_all_sections() {
        let mut buf = Vec::new();
        write_full(&mut buf).unwrap();
        let output = String::from_utf8(buf).unwrap();

        for section in SECTIONS {
            assert!(
                output.contains(&section.to_uppercase()),
                "missing section header: {section}"
            );
        }
    }

    #[test]
    fn full_output_contains_every_term() {
        let mut buf = Vec::new();
        write_full(&mut buf).unwrap();
        let output = String::from_utf8(buf).unwrap();

        for entry in GLOSSARY {
            assert!(output.contains(entry.term), "missing term: {}", entry.term);
        }
    }

    #[test]
    fn single_term_found() {
        let mut buf = Vec::new();
        write_term(&mut buf, "partial").unwrap();
        let output = String::from_utf8(buf).unwrap();

        assert!(output.contains("partial"));
        assert!(output.contains("Some but not all"));
        assert!(output.contains("Used by:"));
        assert!(output.contains("branches"));
    }

    #[test]
    fn single_term_case_insensitive() {
        let mut buf = Vec::new();
        write_term(&mut buf, "PARTIAL").unwrap();
        let output = String::from_utf8(buf).unwrap();

        assert!(output.contains("partial"));
        assert!(output.contains("Used by:"));
    }

    #[test]
    fn multi_section_term_returns_all_matches() {
        // Regression: previously `write_term("orphaned")` returned only the first GLOSSARY entry, silently hiding the meanings of "orphaned" in the Stashes, Remotes, Repos, and LFS sections.
        let mut buf = Vec::new();
        write_term(&mut buf, "orphaned").unwrap();
        let output = String::from_utf8(buf).unwrap();

        let match_count = GLOSSARY
            .iter()
            .filter(|e| e.term.to_lowercase() == "orphaned")
            .count();
        assert!(
            match_count >= 2,
            "test premise: at least two glossary entries use the term 'orphaned'",
        );

        let section_headers = output.matches("orphaned (").count();
        assert_eq!(
            section_headers, match_count,
            "every section's entry for 'orphaned' should appear in the output, got:\n{output}",
        );
    }

    #[test]
    fn single_term_not_found() {
        let mut buf = Vec::new();
        write_term(&mut buf, "nonexistent").unwrap();
        let output = String::from_utf8(buf).unwrap();

        assert!(output.contains("Unknown term: \"nonexistent\""));
        assert!(output.contains("git tidy explain"));
    }

    #[test]
    fn single_term_hyphenated() {
        let mut buf = Vec::new();
        write_term(&mut buf, "landed-content").unwrap();
        let output = String::from_utf8(buf).unwrap();

        assert!(output.contains("landed-content"));
        assert!(output.contains("patch heuristic"));
        assert!(output.contains("Used by:"));
    }
}
