//! Generic clean-loop skeleton shared by every git-tidy tool.
//!
//! Each tool's `clean.rs` reimplemented the same loop: iterate items → filter by
//! classification → dry-run-or-act → aggregate succeeded/failed/skipped. The shape
//! is identical; only the filter, the per-item action, and the wording differ.
//! [`run_clean`] owns the loop and the aggregation; the tool supplies a `decide`
//! predicate (the pure classification filter) and an `act` closure (everything
//! tool-specific: dry-run branching, IO guards, the delete/prune, and the success
//! record). See issue #118.

use std::io::Write;

use crate::error::Error;
use crate::types::{CleanResult, FailedItem};

/// Whether the `decide` pre-filter admits an item for cleanup.
///
/// This is the cheap, pure classification filter (each tool's `should_clean`).
/// It never prints and never counts more than one: a rejected item is silently
/// counted as skipped, matching every tool's current `skipped += 1; continue;`.
/// IO-dependent guards that print a warning belong in `act` (see [`Outcome::Skipped`]).
#[derive(Debug)]
pub enum Decision {
    /// Admit this item; the pipeline will call `act`.
    Clean,
    /// Reject this item; counted as skipped, `act` is not called.
    Skip,
}

/// Result of running `act` on one admitted item.
#[derive(Debug)]
pub enum Outcome<S> {
    /// Cleaned (or, in dry-run, would have been). Carries the success record.
    Cleaned(S),
    /// An IO-dependent guard rejected the item mid-flight (e.g. a dirty working
    /// tree). `act` has already printed its own warning. Counted as skipped.
    Skipped,
    /// The action failed but the loop should continue. `act` has already printed
    /// the error. Aggregated into [`CleanResult::failed`].
    Failed(FailedItem),
}

/// Generic clean loop: iterate `items`, pre-filter via `decide`, hand admitted
/// items to `act`, and aggregate into a [`CleanResult<S>`].
///
/// `act` owns everything tool-specific: dry-run-vs-real branching, the exact
/// "would …"/"removed …" wording, any IO guard (returning [`Outcome::Skipped`]),
/// the actual delete/prune, and construction of the success record. It is handed
/// `out` per call so it can print. `act` returns `Err` only for genuinely fatal
/// errors (an `out` write failure, a hard removal failure) — those short-circuit
/// the loop. A delete that fails but should not abort the run is
/// `Ok(Outcome::Failed(..))`.
///
/// # Mapping a tool onto this seam
///
/// The loop is deliberately flat and per-item. repo-tidy has a flat scan result
/// (`Vec<RepoInfo>`) so it calls `run_clean` once over everything. The other six
/// tools group by repo (`Vec<RepoGroup<T>>`); the natural fit is to call
/// `run_clean` once per group, which keeps each repo's output contiguous and
/// leaves room for per-group work around the call:
/// - **Per-group context** — pass one group's items as `items`, and let `I` carry
///   whatever `act`'s wording needs (e.g. the group name), so the loop stays
///   oblivious to grouping.
/// - **Order-sensitive deletes** — stash must drop in descending index order.
///   Sort that group's items before the call; the loop preserves input order.
/// - **Per-group emission / aggregate extras** — lfs-tidy's per-group
///   recommendation lines, repo-tidy's `dirty_blocked`. Emit or accumulate these
///   around the call (between per-group calls, or via an `act`-captured `&mut`),
///   then wrap the returned [`CleanResult<S>`] in a tool-specific result (see
///   `RepoCleanResult`). The seam owns iterate/filter/act/aggregate, not per-group
///   output.
pub fn run_clean<I, S>(
    items: impl IntoIterator<Item = I>,
    decide: impl Fn(&I) -> Decision,
    mut act: impl FnMut(&I, &mut dyn Write) -> Result<Outcome<S>, Error>,
    out: &mut dyn Write,
) -> Result<CleanResult<S>, Error> {
    let mut succeeded = Vec::new();
    let mut failed = Vec::new();
    let mut skipped = 0usize;

    for item in items {
        match decide(&item) {
            Decision::Skip => {
                skipped += 1;
                continue;
            }
            Decision::Clean => {}
        }
        match act(&item, out)? {
            Outcome::Cleaned(s) => succeeded.push(s),
            Outcome::Skipped => skipped += 1,
            Outcome::Failed(f) => failed.push(f),
        }
    }

    Ok(CleanResult {
        succeeded,
        failed,
        skipped,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Build a `FailedItem` from a test item label.
    fn failed(name: &str) -> FailedItem {
        FailedItem {
            repo: std::path::PathBuf::from("/repo"),
            name: name.to_string(),
            reason: "boom".to_string(),
        }
    }

    #[test]
    fn decide_skip_is_counted_and_act_not_called() {
        let mut out = Vec::new();
        let result = run_clean(
            [1, 2, 3],
            |_| Decision::Skip,
            |_item, _out| -> Result<Outcome<i32>, Error> {
                panic!("act must not be called for skipped items")
            },
            &mut out,
        )
        .unwrap();

        assert_eq!(result.skipped, 3);
        assert!(result.succeeded.is_empty());
        assert!(result.failed.is_empty());
    }

    #[test]
    fn act_cleaned_collected_in_order() {
        let mut out = Vec::new();
        let result = run_clean(
            [1, 2, 3],
            |_| Decision::Clean,
            |item, _out| Ok(Outcome::Cleaned(item * 10)),
            &mut out,
        )
        .unwrap();

        assert_eq!(result.succeeded, vec![10, 20, 30]);
        assert_eq!(result.skipped, 0);
        assert!(result.failed.is_empty());
    }

    #[test]
    fn act_failed_aggregated_and_loop_continues() {
        let mut out = Vec::new();
        let result = run_clean(
            [1, 2],
            |_| Decision::Clean,
            |item, _out| {
                if *item == 1 {
                    Ok(Outcome::Failed(failed("1")))
                } else {
                    Ok(Outcome::Cleaned(*item))
                }
            },
            &mut out,
        )
        .unwrap();

        assert_eq!(result.failed.len(), 1);
        assert_eq!(result.failed[0].name, "1");
        // Item 2 was still processed after item 1 failed.
        assert_eq!(result.succeeded, vec![2]);
        // A failed item is aggregated into `failed`, never counted as skipped.
        assert_eq!(result.skipped, 0);
    }

    #[test]
    fn act_skipped_counts_alongside_decide_skips() {
        let mut out = Vec::new();
        let result = run_clean(
            [1, 2, 3, 4],
            // Reject even numbers via the pure filter.
            |item| {
                if item % 2 == 0 {
                    Decision::Skip
                } else {
                    Decision::Clean
                }
            },
            // Of the admitted odds (1, 3): skip 1 via an IO guard, clean 3.
            |item, _out| {
                if *item == 1 {
                    Ok(Outcome::Skipped)
                } else {
                    Ok(Outcome::Cleaned(*item))
                }
            },
            &mut out,
        )
        .unwrap();

        // 2 decide-skips (2, 4) + 1 act-skip (1) = 3.
        assert_eq!(result.skipped, 3);
        assert_eq!(result.succeeded, vec![3]);
    }

    #[test]
    fn act_error_short_circuits() {
        let mut out = Vec::new();
        let mut calls = 0usize;
        let result = run_clean(
            [1, 2, 3],
            |_| Decision::Clean,
            |item, _out| -> Result<Outcome<i32>, Error> {
                calls += 1;
                if *item == 2 {
                    Err(Error::DirtyBlocked)
                } else {
                    Ok(Outcome::Cleaned(*item))
                }
            },
            &mut out,
        );

        assert!(result.is_err());
        // Items 1 and 2 were processed; item 3 was not reached.
        assert_eq!(calls, 2);
    }

    #[test]
    fn act_can_write_to_out() {
        let mut out = Vec::new();
        run_clean(
            [1, 2],
            |_| Decision::Clean,
            |item, out| {
                writeln!(out, "did {item}")?;
                Ok(Outcome::Cleaned(*item))
            },
            &mut out,
        )
        .unwrap();

        let text = String::from_utf8(out).unwrap();
        assert!(text.contains("did 1"));
        assert!(text.contains("did 2"));
    }

    #[test]
    fn empty_input_yields_empty_result() {
        let mut out = Vec::new();
        let result = run_clean(
            std::iter::empty::<i32>(),
            |_| Decision::Clean,
            |item, _out| Ok(Outcome::Cleaned(*item)),
            &mut out,
        )
        .unwrap();

        assert!(result.succeeded.is_empty());
        assert!(result.failed.is_empty());
        assert_eq!(result.skipped, 0);
    }

    #[test]
    fn mixed_outcomes_route_to_distinct_buckets() {
        // A single run producing all three outcomes must keep them separate: a
        // failed item is never also counted as skipped, a cleaned item never
        // lands in `failed`, etc. The per-outcome tests above can't catch
        // cross-bucket contamination because each runs one outcome in isolation.
        let mut out = Vec::new();
        let result = run_clean(
            [1, 2, 3, 4],
            // Reject 4 via the pure filter.
            |item| {
                if *item == 4 {
                    Decision::Skip
                } else {
                    Decision::Clean
                }
            },
            // 1 -> cleaned, 2 -> act-skip (IO guard), 3 -> failed.
            |item, _out| match *item {
                1 => Ok(Outcome::Cleaned(10)),
                2 => Ok(Outcome::Skipped),
                _ => Ok(Outcome::Failed(failed("3"))),
            },
            &mut out,
        )
        .unwrap();

        assert_eq!(result.succeeded, vec![10]);
        assert_eq!(result.failed.len(), 1);
        assert_eq!(result.failed[0].name, "3");
        // 1 decide-skip (4) + 1 act-skip (2); the failed item (3) is NOT skipped.
        assert_eq!(result.skipped, 2);
    }

    #[test]
    fn act_write_failure_propagates_and_short_circuits() {
        // The documented fatal path: a `writeln!(out, ..)?` inside `act` that
        // fails must surface as `Err` and stop the loop. The other error test
        // hand-returns `Err`; this exercises the real write-failure route.
        struct FailingWriter;
        impl Write for FailingWriter {
            fn write(&mut self, _buf: &[u8]) -> std::io::Result<usize> {
                Err(std::io::Error::other("disk full"))
            }
            fn flush(&mut self) -> std::io::Result<()> {
                Ok(())
            }
        }

        let mut out = FailingWriter;
        let mut calls = 0usize;
        let result = run_clean(
            [1, 2, 3],
            |_| Decision::Clean,
            |item, out| -> Result<Outcome<i32>, Error> {
                calls += 1;
                writeln!(out, "did {item}")?;
                Ok(Outcome::Cleaned(*item))
            },
            &mut out,
        );

        assert!(result.is_err());
        // Short-circuited on the first item's failed write; 2 and 3 not reached.
        assert_eq!(calls, 1);
    }
}
