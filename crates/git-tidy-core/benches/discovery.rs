use std::fs;
use std::path::{Path, PathBuf};

use criterion::{Criterion, criterion_group, criterion_main};

fn target_dir() -> PathBuf {
    dirs::home_dir().unwrap().join("Developer")
}

// ── 1. Current implementation (verbatim from discovery.rs) ──────────────

fn discover_current(directory: &Path) -> Vec<PathBuf> {
    let directory = directory.canonicalize().unwrap();
    let entries = fs::read_dir(&directory).unwrap();
    let mut repos = Vec::new();

    for entry in entries {
        let entry = entry.unwrap();
        let entry_path = entry.path().canonicalize().unwrap_or_else(|_| entry.path());

        if !entry_path.is_dir() {
            continue;
        }

        let git_path = entry_path.join(".git");

        if !git_path.exists() {
            continue;
        }
        if !git_path.is_dir() {
            continue;
        }
        if git_path.symlink_metadata().is_ok_and(|m| m.is_symlink()) {
            continue;
        }

        repos.push(entry_path);
    }

    repos.sort();
    repos
}

// ── 2. Reduced syscalls: file_type(), single lstat, deferred canonicalize

fn discover_reduced_syscalls(directory: &Path) -> Vec<PathBuf> {
    let directory = directory.canonicalize().unwrap();
    let entries = fs::read_dir(&directory).unwrap();
    let mut repos = Vec::new();

    for entry in entries {
        let entry = entry.unwrap();

        // file_type() uses d_type from readdir — no extra stat on macOS/Linux
        if !entry.file_type().map(|ft| ft.is_dir()).unwrap_or(false) {
            continue;
        }

        let entry_path = entry.path();
        let git_path = entry_path.join(".git");

        // Single symlink_metadata call replaces exists() + is_dir() + symlink_metadata()
        let git_meta = match git_path.symlink_metadata() {
            Ok(m) => m,
            Err(_) => continue,
        };
        if git_meta.is_symlink() {
            continue;
        }
        if !git_meta.is_dir() {
            continue;
        }

        // Deferred canonicalize — only for confirmed repos
        let entry_path = entry_path.canonicalize().unwrap_or(entry_path);
        repos.push(entry_path);
    }

    repos.sort();
    repos
}

// ── 3. No canonicalize at all ───────────────────────────────────────────

fn discover_no_canonicalize(directory: &Path) -> Vec<PathBuf> {
    let entries = fs::read_dir(directory).unwrap();
    let mut repos = Vec::new();

    for entry in entries {
        let entry = entry.unwrap();

        if !entry.file_type().map(|ft| ft.is_dir()).unwrap_or(false) {
            continue;
        }

        let entry_path = entry.path();
        let git_path = entry_path.join(".git");

        let git_meta = match git_path.symlink_metadata() {
            Ok(m) => m,
            Err(_) => continue,
        };
        if git_meta.is_symlink() || !git_meta.is_dir() {
            continue;
        }

        repos.push(entry_path);
    }

    repos.sort();
    repos
}

// ── 4. Parallel stat via rayon ──────────────────────────────────────────

fn discover_rayon(directory: &Path) -> Vec<PathBuf> {
    use rayon::prelude::*;

    let directory = directory.canonicalize().unwrap();
    let entries: Vec<_> = fs::read_dir(&directory)
        .unwrap()
        .filter_map(|e| e.ok())
        .collect();

    let mut repos: Vec<PathBuf> = entries
        .par_iter()
        .filter_map(|entry| {
            if !entry.file_type().map(|ft| ft.is_dir()).unwrap_or(false) {
                return None;
            }

            let entry_path = entry.path();
            let git_path = entry_path.join(".git");

            let git_meta = git_path.symlink_metadata().ok()?;
            if git_meta.is_symlink() || !git_meta.is_dir() {
                return None;
            }

            Some(entry_path.canonicalize().unwrap_or(entry_path))
        })
        .collect();

    repos.sort();
    repos
}

// ── 5. jwalk (parallel directory walker) ────────────────────────────────

fn discover_jwalk(directory: &Path) -> Vec<PathBuf> {
    use jwalk::WalkDir;

    let directory = directory.canonicalize().unwrap();

    let mut repos = Vec::new();

    for entry in WalkDir::new(&directory).min_depth(1).max_depth(1) {
        let entry = match entry {
            Ok(e) => e,
            Err(_) => continue,
        };

        if !entry.file_type().is_dir() {
            continue;
        }

        let entry_path = entry.path();
        let git_path = entry_path.join(".git");

        let git_meta = match git_path.symlink_metadata() {
            Ok(m) => m,
            Err(_) => continue,
        };
        if git_meta.is_symlink() || !git_meta.is_dir() {
            continue;
        }

        let entry_path = entry_path.canonicalize().unwrap_or(entry_path);
        repos.push(entry_path);
    }

    repos.sort();
    repos
}

// ── 6. ignore crate (ripgrep's walker) ──────────────────────────────────

fn discover_ignore(directory: &Path) -> Vec<PathBuf> {
    use ignore::WalkBuilder;

    let directory = directory.canonicalize().unwrap();

    let mut repos = Vec::new();

    let walker = WalkBuilder::new(&directory)
        .max_depth(Some(1))
        .hidden(false)
        .ignore(false)
        .git_ignore(false)
        .git_global(false)
        .git_exclude(false)
        .build();

    for entry in walker {
        let entry = match entry {
            Ok(e) => e,
            Err(_) => continue,
        };

        // Skip the root directory itself
        if entry.path() == directory {
            continue;
        }

        if !entry.file_type().map(|ft| ft.is_dir()).unwrap_or(false) {
            continue;
        }

        let entry_path = entry.into_path();
        let git_path = entry_path.join(".git");

        let git_meta = match git_path.symlink_metadata() {
            Ok(m) => m,
            Err(_) => continue,
        };
        if git_meta.is_symlink() || !git_meta.is_dir() {
            continue;
        }

        let entry_path = entry_path.canonicalize().unwrap_or(entry_path);
        repos.push(entry_path);
    }

    repos.sort();
    repos
}

// ── Benchmark group ─────────────────────────────────────────────────────

fn bench_discovery(c: &mut Criterion) {
    let dir = target_dir();
    assert!(dir.is_dir(), "~/Developer must exist");

    // Warm the filesystem cache with one pass
    let _ = discover_current(&dir);

    // Verify all implementations agree on the result count
    let expected = discover_current(&dir).len();
    assert_eq!(
        discover_reduced_syscalls(&dir).len(),
        expected,
        "reduced_syscalls mismatch"
    );
    assert_eq!(
        discover_no_canonicalize(&dir).len(),
        expected,
        "no_canonicalize mismatch"
    );
    assert_eq!(discover_rayon(&dir).len(), expected, "rayon mismatch");
    assert_eq!(discover_jwalk(&dir).len(), expected, "jwalk mismatch");
    assert_eq!(discover_ignore(&dir).len(), expected, "ignore mismatch");
    eprintln!("\n  All implementations agree: {expected} repos in ~/Developer\n");

    let mut group = c.benchmark_group("discover_repos");
    group.sample_size(200);

    group.bench_function("1_current", |b| b.iter(|| discover_current(&dir)));

    group.bench_function("2_reduced_syscalls", |b| {
        b.iter(|| discover_reduced_syscalls(&dir))
    });

    group.bench_function("3_no_canonicalize", |b| {
        b.iter(|| discover_no_canonicalize(&dir))
    });

    group.bench_function("4_rayon_parallel", |b| b.iter(|| discover_rayon(&dir)));

    group.bench_function("5_jwalk", |b| b.iter(|| discover_jwalk(&dir)));

    group.bench_function("6_ignore_crate", |b| b.iter(|| discover_ignore(&dir)));

    group.finish();
}

criterion_group!(benches, bench_discovery);
criterion_main!(benches);

mod dirs {
    use std::path::PathBuf;

    pub fn home_dir() -> Option<PathBuf> {
        std::env::var_os("HOME").map(PathBuf::from)
    }
}
