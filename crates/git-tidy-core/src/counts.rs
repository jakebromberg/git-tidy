//! Generic classification counts shared by every scan-shaped tool.
//!
//! Replaces the per-tool `*Counts` structs (`ScanCounts`, `TagCounts`,
//! `RemoteCounts`, …). A `Counts` is a map keyed by classification **label** — the
//! exact strings each tool's `label()` returns, which are also the keys the audit
//! runner emits as JSON. Keying by label keeps the audit output byte-identical
//! while collapsing eight near-duplicate structs into one.
//!
//! A label with zero occurrences is simply absent (it is never inserted), so
//! [`Counts::iter`] yields only non-zero buckets — matching the audit runner's
//! historical `filter(count > 0)`. Human summary lines that need to print explicit
//! zeros use [`Counts::get`], which returns 0 for absent labels.

use std::collections::BTreeMap;

use serde::Serialize;

/// Summary counts keyed by classification label.
///
/// Serializes as the inner map (serde represents a newtype struct as its wrapped
/// value), e.g. `{"active": 2, "landed": 1}`.
#[derive(Debug, Clone, Default, Serialize)]
pub struct Counts(BTreeMap<String, usize>);

impl Counts {
    /// Increment the count for `label` by one.
    pub fn increment(&mut self, label: &str) {
        *self.0.entry(label.to_string()).or_default() += 1;
    }

    /// Count for `label`, or 0 if the label was never incremented.
    pub fn get(&self, label: &str) -> usize {
        self.0.get(label).copied().unwrap_or(0)
    }

    /// Total across all labels.
    pub fn total(&self) -> usize {
        self.0.values().sum()
    }

    /// Whether no labels have been counted.
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    /// Iterate `(label, count)` pairs in sorted key order. Only non-zero buckets
    /// are present.
    pub fn iter(&self) -> impl Iterator<Item = (&str, usize)> {
        self.0.iter().map(|(k, v)| (k.as_str(), *v))
    }
}

#[cfg(any(test, feature = "testutil"))]
impl Counts {
    /// Build a `Counts` from `(label, count)` pairs. Test helper for terse setup
    /// in place of the former `XCounts { field: n, .. }` struct literals.
    pub fn from_pairs(pairs: &[(&str, usize)]) -> Self {
        let mut c = Self::default();
        for (label, n) in pairs {
            for _ in 0..*n {
                c.increment(label);
            }
        }
        c
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn increment_accumulates_per_label() {
        let mut c = Counts::default();
        c.increment("active");
        c.increment("active");
        c.increment("stale");
        assert_eq!(c.get("active"), 2);
        assert_eq!(c.get("stale"), 1);
    }

    #[test]
    fn get_returns_zero_for_absent_label() {
        let c = Counts::default();
        assert_eq!(c.get("never-seen"), 0);
    }

    #[test]
    fn total_sums_all_buckets() {
        let mut c = Counts::default();
        c.increment("a");
        c.increment("b");
        c.increment("b");
        assert_eq!(c.total(), 3);
    }

    #[test]
    fn iter_yields_only_nonzero_buckets_in_sorted_order() {
        let mut c = Counts::default();
        c.increment("zeta");
        c.increment("alpha");
        c.increment("alpha");
        let pairs: Vec<(&str, usize)> = c.iter().collect();
        assert_eq!(pairs, vec![("alpha", 2), ("zeta", 1)]);
    }

    #[test]
    fn is_empty_tracks_presence() {
        let mut c = Counts::default();
        assert!(c.is_empty());
        c.increment("x");
        assert!(!c.is_empty());
    }

    #[test]
    fn serializes_as_inner_map() {
        let mut c = Counts::default();
        c.increment("active");
        c.increment("landed-content");
        let json = serde_json::to_string(&c).unwrap();
        assert_eq!(json, r#"{"active":1,"landed-content":1}"#);
    }
}
