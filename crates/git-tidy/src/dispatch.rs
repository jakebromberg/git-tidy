use std::ffi::OsString;
use std::path::PathBuf;
use std::process;

use crate::types::TOOL_SPECS;

/// Look up a binary name from a dispatch alias.
///
/// Returns the binary name (e.g., `"git-worktree-tidy"`) if the alias matches
/// any entry in [`TOOL_SPECS`], or `None` otherwise.
pub fn resolve_alias(alias: &str) -> Option<&'static str> {
    TOOL_SPECS
        .iter()
        .find(|spec| spec.aliases.contains(&alias))
        .map(|spec| spec.binary)
}

/// Attempt to dispatch to a sub-tool based on `args[1]`.
///
/// If `args[1]` matches a known alias, looks up the binary via `find_binary`.
/// On success, replaces the current process via `exec`. On binary-not-found,
/// prints an error and exits. If no alias matches, returns `()` so the caller
/// can fall through to clap parsing.
///
/// The `find_binary` parameter follows the injectable function pattern used by
/// `delete_fn` and `du_fn` elsewhere in the codebase, making the binary-not-found
/// path testable without hitting the filesystem.
#[cfg(unix)]
pub fn try_dispatch(args: &[OsString], find_binary: impl Fn(&str) -> Option<PathBuf>) {
    use std::os::unix::process::CommandExt;

    // Need at least argv[0] and argv[1]
    let Some(first_arg) = args.get(1) else {
        return;
    };
    let Some(alias_str) = first_arg.to_str() else {
        return;
    };

    // Don't intercept flags or the explicit "audit" subcommand
    if alias_str.starts_with('-') || alias_str == "audit" {
        return;
    }

    let Some(binary) = resolve_alias(alias_str) else {
        return;
    };

    let Some(path) = find_binary(binary) else {
        eprintln!(
            "error: {binary} is not installed\n\n  Install it with: cargo install --path crates/{binary}"
        );
        process::exit(1);
    };

    // Build the command: binary + remaining args (skip argv[0] and the alias)
    let remaining: Vec<_> = args.iter().skip(2).collect();
    let err = std::process::Command::new(&path).args(&remaining).exec();

    // exec() only returns on error
    eprintln!("error: failed to exec {}: {err}", path.display());
    process::exit(1);
}

/// Default dispatch using `which::which` for binary resolution.
#[cfg(unix)]
pub fn try_dispatch_default(args: &[OsString]) {
    try_dispatch(args, |b| which::which(b).ok());
}

#[cfg(test)]
mod tests {
    use super::*;

    // -- resolve_alias tests --

    #[test]
    fn resolve_alias_plural_forms() {
        assert_eq!(resolve_alias("worktrees"), Some("git-worktree-tidy"));
        assert_eq!(resolve_alias("branches"), Some("git-branch-tidy"));
        assert_eq!(resolve_alias("stashes"), Some("git-stash-tidy"));
        assert_eq!(resolve_alias("remotes"), Some("git-remote-tidy"));
        assert_eq!(resolve_alias("tags"), Some("git-tag-tidy"));
        assert_eq!(resolve_alias("repos"), Some("git-repo-tidy"));
    }

    #[test]
    fn resolve_alias_singular_forms() {
        assert_eq!(resolve_alias("worktree"), Some("git-worktree-tidy"));
        assert_eq!(resolve_alias("branch"), Some("git-branch-tidy"));
        assert_eq!(resolve_alias("stash"), Some("git-stash-tidy"));
        assert_eq!(resolve_alias("remote"), Some("git-remote-tidy"));
        assert_eq!(resolve_alias("tag"), Some("git-tag-tidy"));
        assert_eq!(resolve_alias("repo"), Some("git-repo-tidy"));
    }

    #[test]
    fn resolve_alias_single_form_tools() {
        assert_eq!(resolve_alias("config"), Some("git-config-tidy"));
        assert_eq!(resolve_alias("lfs"), Some("git-lfs-tidy"));
    }

    #[test]
    fn resolve_alias_unknown() {
        assert_eq!(resolve_alias("unknown"), None);
        assert_eq!(resolve_alias(""), None);
        assert_eq!(resolve_alias("git-branch-tidy"), None);
    }

    #[test]
    fn resolve_alias_audit_not_intercepted() {
        assert_eq!(resolve_alias("audit"), None);
    }

    // -- try_dispatch fall-through tests --

    fn os(s: &str) -> OsString {
        OsString::from(s)
    }

    fn never_find(_: &str) -> Option<PathBuf> {
        None
    }

    #[test]
    fn try_dispatch_no_args() {
        // Only argv[0], no dispatch
        let args = [os("git-tidy")];
        try_dispatch(&args, never_find);
        // If we get here, it fell through correctly
    }

    #[test]
    fn try_dispatch_unrecognized_word() {
        let args = [os("git-tidy"), os("unknown")];
        try_dispatch(&args, never_find);
    }

    #[test]
    fn try_dispatch_flag_not_intercepted() {
        let args = [os("git-tidy"), os("--json")];
        try_dispatch(&args, never_find);
    }

    #[test]
    fn try_dispatch_audit_not_intercepted() {
        let args = [os("git-tidy"), os("audit")];
        try_dispatch(&args, never_find);
    }

    #[test]
    fn try_dispatch_directory_path_not_intercepted() {
        let args = [os("git-tidy"), os("/tmp/dev")];
        try_dispatch(&args, never_find);
    }

    #[test]
    fn try_dispatch_relative_path_not_intercepted() {
        let args = [os("git-tidy"), os("./repos")];
        try_dispatch(&args, never_find);
    }

    // Note: We can't easily test the exec path or the binary-not-found exit path
    // in unit tests since they call process::exit or exec. The resolution + lookup
    // logic is tested through resolve_alias. The binary-not-found path would require
    // a subprocess test (integration test) to verify the exit code and stderr.
}
