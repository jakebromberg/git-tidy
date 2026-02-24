use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Instant;

use criterion::{Criterion, criterion_group, criterion_main};

fn home_dir() -> PathBuf {
    PathBuf::from(std::env::var_os("HOME").unwrap())
}

fn target_dir() -> PathBuf {
    home_dir().join("Developer")
}

fn discover_repos(directory: &Path) -> Vec<PathBuf> {
    let directory = directory.canonicalize().unwrap();
    let mut repos = Vec::new();
    for entry in fs::read_dir(&directory).unwrap() {
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
        repos.push(entry_path.canonicalize().unwrap_or(entry_path));
    }
    repos.sort();
    repos
}

// ── Individual git subprocess calls (current approach) ──────────────────

fn git_symbolic_ref(repo: &Path) -> Option<String> {
    let out = Command::new("git")
        .args([
            "-C",
            &repo.to_string_lossy(),
            "symbolic-ref",
            "--quiet",
            "refs/remotes/origin/HEAD",
        ])
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    let full = String::from_utf8_lossy(&out.stdout).trim().to_string();
    full.strip_prefix("refs/remotes/origin/")
        .map(|s| s.to_string())
}

fn git_list_branches(repo: &Path) -> Vec<String> {
    let out = Command::new("git")
        .args([
            "-C",
            &repo.to_string_lossy(),
            "branch",
            "--list",
            "--format=%(refname:short)",
        ])
        .output()
        .unwrap();
    String::from_utf8_lossy(&out.stdout)
        .lines()
        .map(|l| l.to_string())
        .collect()
}

fn git_status(repo: &Path) -> Vec<String> {
    let out = Command::new("git")
        .args(["-C", &repo.to_string_lossy(), "status", "--porcelain"])
        .output()
        .unwrap();
    String::from_utf8_lossy(&out.stdout)
        .lines()
        .map(|l| l.to_string())
        .collect()
}

fn git_list_remotes(repo: &Path) -> Vec<String> {
    let out = Command::new("git")
        .args(["-C", &repo.to_string_lossy(), "remote"])
        .output()
        .unwrap();
    String::from_utf8_lossy(&out.stdout)
        .lines()
        .map(|l| l.to_string())
        .collect()
}

fn git_last_commit_date(repo: &Path) -> Option<String> {
    let out = Command::new("git")
        .args(["-C", &repo.to_string_lossy(), "log", "-1", "--format=%aI"])
        .output()
        .ok()?;
    let s = String::from_utf8_lossy(&out.stdout).trim().to_string();
    if s.is_empty() { None } else { Some(s) }
}

fn git_list_stashes(repo: &Path) -> Vec<String> {
    let out = Command::new("git")
        .args([
            "-C",
            &repo.to_string_lossy(),
            "stash",
            "list",
            "--format=%gd",
        ])
        .output()
        .unwrap();
    String::from_utf8_lossy(&out.stdout)
        .lines()
        .filter(|l| !l.is_empty())
        .map(|l| l.to_string())
        .collect()
}

// ── Batched git operations using for-each-ref ───────────────────────────

/// Single subprocess call to get branch count + default branch + HEAD info
fn git_batched_repo_info(repo: &Path) -> (Option<String>, Vec<String>, Option<String>) {
    // Get branches, HEAD ref, and origin/HEAD in one for-each-ref call
    let out = Command::new("git")
        .args([
            "-C",
            &repo.to_string_lossy(),
            "for-each-ref",
            "--format=%(refname:short)\t%(upstream:short)\t%(HEAD)",
            "refs/heads/",
        ])
        .output()
        .unwrap();

    let mut branches = Vec::new();
    let mut head_branch = None;

    for line in String::from_utf8_lossy(&out.stdout).lines() {
        let parts: Vec<&str> = line.split('\t').collect();
        if parts.len() >= 3 {
            branches.push(parts[0].to_string());
            if parts[2] == "*" {
                head_branch = Some(parts[0].to_string());
            }
        }
    }

    // Still need symbolic-ref for origin/HEAD
    let default = git_symbolic_ref(repo);

    (default, branches, head_branch)
}

// ── Lightweight repo info (skip heavy operations) ───────────────────────

fn classify_repo_lightweight(repo: &Path) -> (usize, usize, usize) {
    let branches = git_list_branches(repo);
    let remotes = git_list_remotes(repo);
    let stashes = git_list_stashes(repo);
    (branches.len(), remotes.len(), stashes.len())
}

fn classify_repo_batched(repo: &Path) -> (usize, usize, usize) {
    let (_, branches, _) = git_batched_repo_info(repo);
    let remotes = git_list_remotes(repo);
    let stashes = git_list_stashes(repo);
    (branches.len(), remotes.len(), stashes.len())
}

// ── Sequential vs Parallel per-repo scan ────────────────────────────────

fn scan_sequential(repos: &[PathBuf]) -> Vec<(usize, usize, usize)> {
    repos.iter().map(|r| classify_repo_lightweight(r)).collect()
}

fn scan_parallel(repos: &[PathBuf]) -> Vec<(usize, usize, usize)> {
    use rayon::prelude::*;
    repos
        .par_iter()
        .map(|r| classify_repo_lightweight(r))
        .collect()
}

fn scan_sequential_batched(repos: &[PathBuf]) -> Vec<(usize, usize, usize)> {
    repos.iter().map(|r| classify_repo_batched(r)).collect()
}

fn scan_parallel_batched(repos: &[PathBuf]) -> Vec<(usize, usize, usize)> {
    use rayon::prelude::*;
    repos.par_iter().map(|r| classify_repo_batched(r)).collect()
}

// ── Full repo-tidy-like classification (heavier) ────────────────────────

fn classify_full(repo: &Path) -> (usize, usize, bool) {
    let branches = git_list_branches(repo);
    let remotes = git_list_remotes(repo);
    let status = git_status(repo);
    let _date = git_last_commit_date(repo);
    let is_dirty = !status.is_empty();
    (branches.len(), remotes.len(), is_dirty)
}

fn scan_full_sequential(repos: &[PathBuf]) -> Vec<(usize, usize, bool)> {
    repos.iter().map(|r| classify_full(r)).collect()
}

fn scan_full_parallel(repos: &[PathBuf]) -> Vec<(usize, usize, bool)> {
    use rayon::prelude::*;
    repos.par_iter().map(|r| classify_full(r)).collect()
}

// ── Benchmarks ──────────────────────────────────────────────────────────

fn bench_pipeline(c: &mut Criterion) {
    let dir = target_dir();
    assert!(dir.is_dir(), "~/Developer must exist");

    let repos = discover_repos(&dir);
    eprintln!("\n  Found {} repos in ~/Developer", repos.len());

    // ── Phase timing (not criterion — just raw wall-clock) ──────────
    eprintln!("\n  === Phase timing (single pass) ===");

    let t0 = Instant::now();
    let _ = discover_repos(&dir);
    let t_discover = t0.elapsed();
    eprintln!("  Discovery:                 {:>8.2?}", t_discover);

    let t0 = Instant::now();
    let _ = scan_sequential(&repos);
    let t_seq = t0.elapsed();
    eprintln!("  Sequential scan (light):   {:>8.2?}", t_seq);

    let t0 = Instant::now();
    let _ = scan_parallel(&repos);
    let t_par = t0.elapsed();
    eprintln!("  Parallel scan (light):     {:>8.2?}", t_par);

    let t0 = Instant::now();
    let _ = scan_full_sequential(&repos);
    let t_full_seq = t0.elapsed();
    eprintln!("  Sequential scan (full):    {:>8.2?}", t_full_seq);

    let t0 = Instant::now();
    let _ = scan_full_parallel(&repos);
    let t_full_par = t0.elapsed();
    eprintln!("  Parallel scan (full):      {:>8.2?}", t_full_par);

    let t0 = Instant::now();
    let _ = scan_sequential_batched(&repos);
    let t_batch_seq = t0.elapsed();
    eprintln!("  Sequential batched:        {:>8.2?}", t_batch_seq);

    let t0 = Instant::now();
    let _ = scan_parallel_batched(&repos);
    let t_batch_par = t0.elapsed();
    eprintln!("  Parallel batched:          {:>8.2?}", t_batch_par);

    // Per-subprocess cost estimate
    let t0 = Instant::now();
    for _ in 0..100 {
        let _ = Command::new("git").arg("--version").output();
    }
    let t_spawn = t0.elapsed() / 100;
    eprintln!("  Avg subprocess spawn:      {:>8.2?}", t_spawn);

    eprintln!();

    // ── Criterion groups ────────────────────────────────────────────

    {
        let mut group = c.benchmark_group("scan_light");
        group.sample_size(30);

        group.bench_function("sequential", |b| b.iter(|| scan_sequential(&repos)));

        group.bench_function("parallel_rayon", |b| b.iter(|| scan_parallel(&repos)));

        group.bench_function("sequential_batched", |b| {
            b.iter(|| scan_sequential_batched(&repos))
        });

        group.bench_function("parallel_batched", |b| {
            b.iter(|| scan_parallel_batched(&repos))
        });

        group.finish();
    }

    {
        let mut group = c.benchmark_group("scan_full");
        group.sample_size(20);

        group.bench_function("sequential", |b| b.iter(|| scan_full_sequential(&repos)));

        group.bench_function("parallel_rayon", |b| b.iter(|| scan_full_parallel(&repos)));

        group.finish();
    }

    // Single-subprocess overhead
    {
        let mut group = c.benchmark_group("subprocess_overhead");
        group.sample_size(100);

        group.bench_function("git_version", |b| {
            b.iter(|| Command::new("git").arg("--version").output().unwrap())
        });

        group.bench_function("git_branch_list_single_repo", |b| {
            b.iter(|| git_list_branches(&repos[0]))
        });

        group.bench_function("git_for_each_ref_single_repo", |b| {
            b.iter(|| git_batched_repo_info(&repos[0]))
        });

        group.finish();
    }
}

criterion_group!(benches, bench_pipeline);
criterion_main!(benches);
