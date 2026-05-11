//! Git project metadata helpers.
//!
//! Resolves a CWD into a `(repo_name, relative_dir, branch)` triple via a
//! single `git rev-parse` call, with caching.
//!
//! `repo_name` and `relative_dir` are immutable per CWD, so they're cached
//! forever. `branch` can change at runtime, so its cache has a TTL.
//!
//! On macOS, running `git -C <path>` inside a TCC-protected directory
//! (Documents, Desktop, etc.) triggers a system permission prompt. We bail
//! out early for those paths unless explicitly allow-listed.

use std::{
    collections::HashMap,
    path::{Path, PathBuf},
    process::Command,
    sync::{Mutex, OnceLock},
    time::{Duration, Instant},
};

/// Cached repo-name + relative-dir for a CWD. These don't change per CWD,
/// so the cache has no TTL.
struct StableGitInfo {
    repo_name: String,
    relative_dir: Option<String>,
}

/// Cached branch + when it was fetched. Branch can change at runtime, so
/// entries expire after [`BRANCH_CACHE_TTL`].
struct BranchInfo {
    branch: Option<String>,
    fetched_at: Instant,
}

/// Combined cache for stable git info, keyed by CWD string.
static STABLE_GIT_CACHE: Mutex<Option<HashMap<String, StableGitInfo>>> = Mutex::new(None);
/// Branch cache, keyed by CWD string.
static BRANCH_CACHE: Mutex<Option<HashMap<String, BranchInfo>>> = Mutex::new(None);

/// How long a branch lookup is cached before being re-fetched (5 minutes).
const BRANCH_CACHE_TTL: Duration = Duration::from_mins(5);

/// macOS TCC directories where calling `git` would trigger a permission
/// prompt.
const TCC_PROTECTED_DIRS: &[&str] =
    &["Pictures", "Desktop", "Documents", "Downloads", "Music", "Movies"];

/// Allowlist parsed from `ROOSTR_TCC_ALLOW` (comma-separated absolute paths).
///
/// Any CWD under one of these prefixes bypasses the TCC-protected check —
/// useful when the user keeps real projects under `~/Documents` or `~/Desktop`
/// and is willing to grant the one-time macOS permission prompt.
fn tcc_allow_paths() -> &'static [PathBuf] {
    static CACHE: OnceLock<Vec<PathBuf>> = OnceLock::new();
    CACHE.get_or_init(|| {
        std::env::var("ROOSTR_TCC_ALLOW")
            .ok()
            .map(|value| {
                value
                    .split(',')
                    .map(str::trim)
                    .filter(|slice| !slice.is_empty())
                    .map(PathBuf::from)
                    .collect()
            })
            .unwrap_or_default()
    })
}

/// Pure check: is `path` inside a TCC-protected dir under `home`, after
/// honoring the explicit `allow` list?
fn is_tcc_protected_with(path: &Path, home: Option<&Path>, allow: &[PathBuf]) -> bool {
    if allow.iter().any(|prefix| path.starts_with(prefix)) {
        return false;
    }
    let Some(home_path) = home else {
        return false;
    };
    TCC_PROTECTED_DIRS.iter().any(|name| path.starts_with(home_path.join(name)))
}

/// Returns true if `path` is inside a macOS TCC-protected directory.
///
/// Running `git -C <path>` inside these dirs triggers system permission
/// prompts (Photos, Desktop, Documents, Downloads, etc.) even when roostr
/// has no legitimate need to access those files.
///
/// Override via `ROOSTR_TCC_ALLOW=/abs/path1,/abs/path2`.
fn is_tcc_protected(path: &Path) -> bool {
    is_tcc_protected_with(path, dirs::home_dir().as_deref(), tcc_allow_paths())
}

/// Get the git project name, relative dir, and branch for a working
/// directory.
///
/// `repo_name` and `relative_dir` are immutable per directory — cached
/// forever to avoid repeated TCC prompts on macOS for git/canonicalize
/// syscalls. `branch` can change at runtime — refreshed every 5 minutes.
pub fn git_project_info(working_dir: &str) -> (String, Option<String>, Option<String>) {
    if !Path::new(working_dir).is_absolute() {
        let fallback = cwd_fallback_name(working_dir);
        return (fallback, None, None);
    }

    if is_tcc_protected(Path::new(working_dir)) {
        let name = cwd_fallback_name(working_dir);
        return (name, None, None);
    }

    let stable_hit = lookup_stable(working_dir);

    // Try the branch cache. If hit, also check the stable cache; if both
    // hit, we can avoid spawning git entirely.
    if let BranchHit::Found(branch) = lookup_branch(working_dir) {
        return resolve_with_branch(working_dir, stable_hit, branch);
    }

    let combined = fetch_combined_git_info(working_dir);
    let (repo_name, relative_dir) =
        stable_hit.unwrap_or_else(|| store_stable(working_dir, combined.as_ref()));
    let branch = store_branch(working_dir, combined.as_ref());
    (repo_name, relative_dir, branch)
}

/// Branch cache was already hit; complete the resolution by hitting/filling
/// the stable cache.
fn resolve_with_branch(
    working_dir: &str,
    stable_hit: Option<(String, Option<String>)>,
    branch: Option<String>,
) -> (String, Option<String>, Option<String>) {
    if let Some((repo_name, relative_dir)) = stable_hit {
        return (repo_name, relative_dir, branch);
    }
    let combined = fetch_combined_git_info(working_dir);
    let (repo_name, relative_dir) = store_stable(working_dir, combined.as_ref());
    (repo_name, relative_dir, branch)
}

/// Best-effort name when we can't or won't shell out to git.
fn cwd_fallback_name(working_dir: &str) -> String {
    Path::new(working_dir)
        .file_name()
        .map_or_else(|| working_dir.to_owned(), |name| name.to_string_lossy().into_owned())
}

/// Fetch the cached stable info for `working_dir`, if any.
fn lookup_stable(working_dir: &str) -> Option<(String, Option<String>)> {
    let guard = STABLE_GIT_CACHE.lock().ok()?;
    guard
        .as_ref()
        .and_then(|cache| cache.get(working_dir))
        .map(|info| (info.repo_name.clone(), info.relative_dir.clone()))
}

/// Outcome of a branch-cache lookup.
enum BranchHit {
    /// A non-expired cache entry was found (the branch may itself be `None`,
    /// e.g. detached HEAD).
    Found(Option<String>),
    /// No cache entry, or the entry was stale.
    Missing,
}

/// Fetch a non-expired cached branch for `working_dir`.
fn lookup_branch(working_dir: &str) -> BranchHit {
    let Ok(guard) = BRANCH_CACHE.lock() else {
        return BranchHit::Missing;
    };
    let Some(cache) = guard.as_ref() else {
        return BranchHit::Missing;
    };
    let Some(info) = cache.get(working_dir) else {
        return BranchHit::Missing;
    };
    if info.fetched_at.elapsed() >= BRANCH_CACHE_TTL {
        return BranchHit::Missing;
    }
    BranchHit::Found(info.branch.clone())
}

/// Compute and store stable repo info from a fresh `git rev-parse`.
fn store_stable(working_dir: &str, combined: Option<&CombinedGitInfo>) -> (String, Option<String>) {
    let repo_name = combined
        .and_then(|info| info.repo_name.clone())
        .unwrap_or_else(|| cwd_fallback_name(working_dir));
    let relative_dir = combined.and_then(|info| info.relative_dir.clone());

    if let Ok(mut guard) = STABLE_GIT_CACHE.lock() {
        guard.get_or_insert_with(HashMap::new).insert(
            working_dir.to_owned(),
            StableGitInfo { repo_name: repo_name.clone(), relative_dir: relative_dir.clone() },
        );
    }

    (repo_name, relative_dir)
}

/// Compute and store the branch from a fresh `git rev-parse`.
fn store_branch(working_dir: &str, combined: Option<&CombinedGitInfo>) -> Option<String> {
    let branch = combined.and_then(|info| info.branch.clone());

    if let Ok(mut guard) = BRANCH_CACHE.lock() {
        guard.get_or_insert_with(HashMap::new).insert(
            working_dir.to_owned(),
            BranchInfo { branch: branch.clone(), fetched_at: Instant::now() },
        );
    }

    branch
}

/// Output of a single `git rev-parse` call.
struct CombinedGitInfo {
    repo_name: Option<String>,
    relative_dir: Option<String>,
    branch: Option<String>,
}

/// Single `git rev-parse` call returning toplevel, common-dir, and branch.
/// Replaces three separate process spawns.
fn fetch_combined_git_info(working_dir: &str) -> Option<CombinedGitInfo> {
    let output = Command::new("git")
        .args([
            "-C",
            working_dir,
            "rev-parse",
            "--show-toplevel",
            "--git-common-dir",
            "--abbrev-ref",
            "HEAD",
        ])
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let stdout = String::from_utf8_lossy(&output.stdout);
    let lines: Vec<&str> = stdout.lines().collect();
    let toplevel = lines.first()?.trim();
    let common_dir = lines.get(1)?.trim();
    let branch_raw = lines.get(2)?.trim();

    let repo_name = derive_repo_name(working_dir, common_dir);
    let relative_dir = derive_relative_dir(working_dir, toplevel);
    let branch = if branch_raw.is_empty() || branch_raw == "HEAD" {
        None
    } else {
        Some(branch_raw.to_owned())
    };

    Some(CombinedGitInfo { repo_name, relative_dir, branch })
}

/// Derive the repo name from `--git-common-dir`. Stable across worktrees.
fn derive_repo_name(working_dir: &str, common_dir: &str) -> Option<String> {
    let common_path = if Path::new(common_dir).is_absolute() {
        PathBuf::from(common_dir)
    } else {
        PathBuf::from(working_dir).join(common_dir)
    };
    let repo_root = if common_path.file_name().is_some_and(|name| name == ".git") {
        common_path.parent().map(Path::to_path_buf)
    } else {
        Some(common_path)
    };
    repo_root.and_then(|path| path.file_name().map(|name| name.to_string_lossy().into_owned()))
}

/// Derive the relative path of `working_dir` inside `toplevel`.
fn derive_relative_dir(working_dir: &str, toplevel: &str) -> Option<String> {
    let cwd_path = Path::new(working_dir);
    let top_path = Path::new(toplevel);
    let relative = cwd_path
        .strip_prefix(top_path)
        .map(Path::to_path_buf)
        .or_else(|_| {
            let cwd_resolved =
                cwd_path.canonicalize().unwrap_or_else(|_| PathBuf::from(working_dir));
            let top_resolved = top_path.canonicalize().unwrap_or_else(|_| PathBuf::from(toplevel));
            cwd_resolved.strip_prefix(&top_resolved).map(Path::to_path_buf)
        })
        .unwrap_or_default();
    if relative.as_os_str().is_empty() || relative == Path::new(".") {
        None
    } else {
        Some(relative.display().to_string())
    }
}

/// Decode an encoded project directory name back to a path.
///
/// `-Users-gavra-repos-yaba` -> `/Users/gavra/repos/yaba`
///
/// This is a best-effort reverse of the encoding (ambiguous for `.` and `_`).
pub fn decode_project_path(project_dir: &Path) -> String {
    let name =
        project_dir.file_name().map(|stem| stem.to_string_lossy().into_owned()).unwrap_or_default();

    // The encoded name replaces `/` with `-`, so the first char is always `-`.
    // Convert back: leading `-` becomes `/`, internal `-` becomes `/`.
    if name.starts_with('-') {
        name.replacen('-', "/", 1).replace('-', "/")
    } else {
        name
    }
}

#[cfg(test)]
mod tests {
    use std::path::{Path, PathBuf};

    use super::is_tcc_protected_with;

    /// All TCC-protected dirs under home are detected.
    ///
    /// # Panics
    /// Panics on assertion failure.
    #[test]
    fn tcc_protected_detects_known_dirs() {
        let home = Path::new("/Users/test");
        let no_allow: Vec<PathBuf> = vec![];
        assert!(is_tcc_protected_with(
            &home.join("Pictures").join("Photos Library.photoslibrary"),
            Some(home),
            &no_allow,
        ));
        assert!(is_tcc_protected_with(&home.join("Desktop").join("a"), Some(home), &no_allow,));
        assert!(
            is_tcc_protected_with(&home.join("Documents").join("work"), Some(home), &no_allow,)
        );
        assert!(is_tcc_protected_with(&home.join("Downloads"), Some(home), &no_allow,));
        assert!(!is_tcc_protected_with(&home.join("dev").join("project"), Some(home), &no_allow,));
        assert!(!is_tcc_protected_with(Path::new("/tmp/x"), Some(home), &no_allow,));
    }

    /// Explicit allowlist overrides TCC protection.
    ///
    /// # Panics
    /// Panics on assertion failure.
    #[test]
    fn tcc_allow_list_overrides_protection() {
        let home = Path::new("/Users/test");
        let allow = vec![home.join("Documents").join("code")];
        assert!(!is_tcc_protected_with(
            &home.join("Documents").join("code").join("project"),
            Some(home),
            &allow,
        ));
        assert!(is_tcc_protected_with(
            &home.join("Documents").join("personal"),
            Some(home),
            &allow,
        ));
    }

    /// When `home` is absent, no path is protected.
    ///
    /// # Panics
    /// Panics on assertion failure.
    #[test]
    fn tcc_protected_no_home_returns_false() {
        let allow: Vec<PathBuf> = vec![];
        assert!(!is_tcc_protected_with(Path::new("/anywhere"), None, &allow));
    }
}
