use std::io::IsTerminal;

use indicatif::{ProgressBar, ProgressStyle};

/// Factory for creating progress bars that respect TTY detection.
///
/// When `enabled` is true, creates visible progress bars on stderr.
/// When `enabled` is false (non-TTY, tests, nested contexts), creates
/// hidden no-op bars so callers never need conditionals.
pub struct Progress {
    enabled: bool,
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
        }
    }

    /// Create a disabled `Progress` that always returns hidden bars.
    /// Use in tests and nested contexts (e.g., audit runner calling sub-tools).
    pub fn disabled() -> Self {
        Self { enabled: false }
    }

    /// Returns whether progress display is enabled.
    pub fn is_enabled(&self) -> bool {
        self.enabled
    }

    /// Create a progress bar with a known length (e.g., repo count).
    ///
    /// Displays: `⠋ Fetching  ████████░░░░░░  3/5`
    pub fn bar(&self, len: u64, msg: &str) -> ProgressBar {
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
        let p = Progress { enabled: true };

        let bar = p.bar(5, "Fetching");
        assert_eq!(bar.length(), Some(5));
        assert_eq!(bar.message(), "Fetching");
        bar.finish_and_clear();
    }

    #[test]
    fn spinner_sets_message() {
        let p = Progress { enabled: true };

        let spinner = p.spinner("Scanning...");
        assert_eq!(spinner.message(), "Scanning...");
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
