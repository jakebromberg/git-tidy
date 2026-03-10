use std::io::IsTerminal;

use indicatif::{MultiProgress, ProgressBar, ProgressStyle};

/// Factory for creating progress bars that respect TTY detection.
///
/// When `enabled` is true, creates visible progress bars on stderr.
/// When `enabled` is false (non-TTY, tests, nested contexts), creates
/// hidden no-op bars so callers never need conditionals.
///
/// In forwarding mode, `bar()` inserts sub-bars into an existing
/// `MultiProgress` beneath a parent spinner, providing real-time
/// per-repo progress during parallel audits.
pub struct Progress {
    enabled: bool,
    /// When set, `bar()` inserts bars into this `MultiProgress` after the
    /// given spinner instead of creating standalone bars.
    multi: Option<(MultiProgress, ProgressBar)>,
}

impl Default for Progress {
    fn default() -> Self {
        Self::new()
    }
}

impl Progress {
    /// Create a new `Progress` with TTY auto-detection on stderr.
    pub fn new() -> Self {
        Self {
            enabled: std::io::stderr().is_terminal(),
            multi: None,
        }
    }

    /// Create a disabled `Progress` that always returns hidden bars.
    /// Use in tests and nested contexts (e.g., audit runner calling sub-tools).
    pub fn disabled() -> Self {
        Self {
            enabled: false,
            multi: None,
        }
    }

    /// Create a `Progress` whose `bar()` calls insert sub-bars into an
    /// existing `MultiProgress`, positioned after the given spinner.
    ///
    /// Used by the audit runner so sub-tool repo-level progress appears
    /// beneath each tool's spinner. The sub-bar is cleared automatically
    /// when the sub-tool calls `finish_and_clear()`.
    pub fn forwarding(mp: &MultiProgress, after: &ProgressBar) -> Self {
        Self {
            enabled: true,
            multi: Some((mp.clone(), after.clone())),
        }
    }

    /// Returns whether progress display is enabled.
    pub fn is_enabled(&self) -> bool {
        self.enabled
    }

    /// Create a progress bar with a known length (e.g., repo count).
    ///
    /// In forwarding mode, the bar is inserted into the parent `MultiProgress`
    /// beneath the tool's spinner with a compact indented style.
    ///
    /// Standalone display: `⠋ Fetching  ████████░░░░░░  3/5`
    /// Forwarding display: `    ──────────····  3/12`
    pub fn bar(&self, len: u64, msg: &str) -> ProgressBar {
        if let Some((ref mp, ref after)) = self.multi {
            let pb = mp.insert_after(after, ProgressBar::new(len));
            pb.set_style(
                ProgressStyle::with_template("    {wide_bar:.dim} {pos}/{len}")
                    .unwrap()
                    .progress_chars("──·"),
            );
            return pb;
        }
        if !self.enabled {
            return ProgressBar::hidden();
        }
        let pb = ProgressBar::new(len);
        pb.set_style(
            ProgressStyle::with_template("{spinner:.cyan} {msg}  {wide_bar:.cyan} {pos}/{len}")
                .unwrap()
                .progress_chars("██░"),
        );
        pb.set_message(msg.to_string());
        pb
    }

    /// Create a `MultiProgress` container for managing multiple concurrent spinners.
    ///
    /// Returns `Some(MultiProgress)` when display is enabled, `None` when disabled.
    /// Callers should fall back to hidden bars when `None`.
    pub fn multi(&self) -> Option<MultiProgress> {
        if self.enabled {
            Some(MultiProgress::new())
        } else {
            None
        }
    }

    /// Create a spinner with a message (no known length).
    ///
    /// Displays: `⠋ [3/8] Scanning branches...`
    pub fn spinner(&self, msg: &str) -> ProgressBar {
        if !self.enabled {
            return ProgressBar::hidden();
        }
        let pb = ProgressBar::new_spinner();
        pb.set_style(ProgressStyle::with_template("{spinner:.cyan} {msg}").unwrap());
        pb.set_message(msg.to_string());
        pb.enable_steady_tick(std::time::Duration::from_millis(100));
        pb
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn disabled_returns_hidden_bar() {
        let p = Progress::disabled();
        assert!(!p.is_enabled());

        let bar = p.bar(10, "test");
        assert!(bar.is_hidden());
    }

    #[test]
    fn disabled_returns_hidden_spinner() {
        let p = Progress::disabled();
        let spinner = p.spinner("test");
        assert!(spinner.is_hidden());
    }

    #[test]
    fn bar_sets_length_and_message() {
        // Force-enable even in test (non-TTY) context.
        // Note: indicatif may still hide the bar if stderr is not a TTY,
        // so we only verify length/message, not visibility.
        let p = Progress {
            enabled: true,
            multi: None,
        };

        let bar = p.bar(5, "Fetching");
        assert_eq!(bar.length(), Some(5));
        assert_eq!(bar.message(), "Fetching");
        bar.finish_and_clear();
    }

    #[test]
    fn spinner_sets_message() {
        let p = Progress {
            enabled: true,
            multi: None,
        };

        let spinner = p.spinner("Scanning...");
        assert_eq!(spinner.message(), "Scanning...");
        spinner.finish_and_clear();
    }

    #[test]
    fn disabled_returns_none_for_multi() {
        let p = Progress::disabled();
        assert!(p.multi().is_none());
    }

    #[test]
    fn enabled_returns_some_for_multi() {
        let p = Progress {
            enabled: true,
            multi: None,
        };
        assert!(p.multi().is_some());
    }

    #[test]
    fn forwarding_bar_inserts_into_multi() {
        let mp = MultiProgress::new();
        let spinner = mp.add(ProgressBar::new_spinner());
        let p = Progress::forwarding(&mp, &spinner);

        assert!(p.is_enabled());
        let bar = p.bar(10, "Scanning");
        assert_eq!(bar.length(), Some(10));
        assert_eq!(bar.position(), 0);
        bar.inc(1);
        assert_eq!(bar.position(), 1);
        bar.finish_and_clear();
        spinner.finish_and_clear();
    }

    #[test]
    fn forwarding_is_enabled() {
        let mp = MultiProgress::new();
        let spinner = mp.add(ProgressBar::new_spinner());
        let p = Progress::forwarding(&mp, &spinner);
        assert!(p.is_enabled());
        spinner.finish_and_clear();
    }

    #[test]
    fn new_detects_tty() {
        // In test context, stderr is typically not a TTY
        let p = Progress::new();
        // We can't assert the exact value since it depends on the test runner,
        // but we can verify it doesn't panic and returns a valid Progress
        let bar = p.bar(1, "test");
        bar.finish_and_clear();
    }
}
