//! Read-only commands that open a skill's folder in the system file manager.
//!
//! Security model: the frontend never sends a filesystem path. It sends an
//! identifier — an agent key plus a skill-relative path, a plugin-relative
//! path, or a library skill id — and the backend resolves it against a known
//! skills root, then requires the canonicalized target to stay inside that
//! root. Anything else is rejected with `AppError::invalid_input`. Opening a
//! folder touches nothing on disk.

use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Arc;

use tauri::command;
use tauri::State;

use crate::commands::projects::ensure_safe_skill_relative_path;
use crate::core::error::AppError;
use crate::core::{central_repo, plugin_scanner, tool_adapters};
use crate::core::skill_store::SkillStore;

/// Resolve a skill-relative path against `root`, refusing anything that is not
/// a directory inside it.
///
/// Reuses the shared `ensure_safe_skill_relative_path` validator (from
/// `commands::projects`) to reject empty paths, `..` components, and absolute
/// paths at the component level, then canonicalizes both sides — which also
/// resolves symlinks — and requires containment. Mirrors the pattern of
/// `commands::plugins::resolve_skill_dir`. Extracted as a pure function so the
/// validation can be tested without launching a file manager.
fn resolve_reveal_target(root: &Path, relative_path: &str) -> Result<PathBuf, AppError> {
    ensure_safe_skill_relative_path(relative_path)?;
    let canon_root = std::fs::canonicalize(root)
        .map_err(|_| AppError::invalid_input("Skills root directory not found"))?;
    let full = root.join(relative_path);
    let canon_full = std::fs::canonicalize(&full)
        .map_err(|_| AppError::invalid_input("Skill folder not found"))?;
    if !canon_full.starts_with(&canon_root) {
        return Err(AppError::invalid_input("Path outside skills root directory"));
    }
    if !canon_full.is_dir() {
        return Err(AppError::invalid_input("Skill folder not found"));
    }
    Ok(canon_full)
}

/// Same containment rule for a path that is already backend-owned (the library
/// skill's stored `central_path`), so a skill id is the only input the
/// frontend supplies. The path never comes from the frontend, but it is still
/// validated against the known root before anything is opened.
fn resolve_reveal_target_within(root: &Path, path: &Path) -> Result<PathBuf, AppError> {
    let canon_root = std::fs::canonicalize(root)
        .map_err(|_| AppError::invalid_input("Skills root directory not found"))?;
    let canon_path = std::fs::canonicalize(path)
        .map_err(|_| AppError::invalid_input("Skill folder not found"))?;
    if !canon_path.starts_with(&canon_root) {
        return Err(AppError::invalid_input("Path outside skills root directory"));
    }
    if !canon_path.is_dir() {
        return Err(AppError::invalid_input("Skill folder not found"));
    }
    Ok(canon_path)
}

/// Open a folder in the system file manager. Same launcher pattern as
/// `commands::settings::open_central_repo_folder` (explorer with
/// CREATE_NO_WINDOW on Windows; explorer.exe's exit code is ignored there
/// because it returns 1 even on success).
fn open_folder_in_file_manager(path: &Path) -> Result<(), AppError> {
    #[cfg(target_os = "macos")]
    let mut cmd = Command::new("open");
    #[cfg(target_os = "windows")]
    let mut cmd = {
        let mut c = Command::new("explorer");
        use std::os::windows::process::CommandExt;
        c.creation_flags(0x08000000); // CREATE_NO_WINDOW
        c
    };
    #[cfg(target_os = "linux")]
    let mut cmd = Command::new("xdg-open");

    let status = cmd
        .arg(path)
        .status()
        .map_err(|e| AppError::io(format!("Failed to open folder: {e}")))?;

    // Windows explorer.exe returns exit code 1 even on success
    #[cfg(not(target_os = "windows"))]
    if !status.success() {
        return Err(AppError::io(format!(
            "File manager exited with status: {status}"
        )));
    }

    let _ = status;
    Ok(())
}

/// Open an agent-local skill's folder in the file manager. The agent key and
/// the skill-relative path only identify the folder; the actual path is
/// resolved here against the agent's `skills_dir()` and containment-checked.
#[command]
pub async fn reveal_skill_folder(
    store: State<'_, Arc<SkillStore>>,
    agent: String,
    relative_path: String,
) -> Result<(), AppError> {
    let store = store.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        let adapter = tool_adapters::all_tool_adapters(&store)
            .into_iter()
            .find(|adapter| adapter.key == agent)
            .ok_or_else(|| AppError::not_found(format!("Unknown agent: {agent}")))?;
        let root = adapter.skills_dir();
        let target = resolve_reveal_target(&root, &relative_path)?;
        open_folder_in_file_manager(&target)
    })
    .await?
}

/// Open a plugin or official skill's folder in the file manager. The
/// plugin-relative path (as reported by the plugin scanner, so it already
/// lives under `<claude config>/plugins`) is resolved against that root and
/// containment-checked — same shape as `get_plugin_skill_document`.
#[command]
pub async fn reveal_plugin_skill_folder(relative_path: String) -> Result<(), AppError> {
    tauri::async_runtime::spawn_blocking(move || {
        let config_dir = plugin_scanner::claude_config_dir();
        let plugins_root = config_dir.join("plugins");
        let target = resolve_reveal_target(&plugins_root, &relative_path)?;
        open_folder_in_file_manager(&target)
    })
    .await?
}

/// Open a library (central) skill's folder in the file manager. The frontend
/// sends only the skill id; the stored `central_path` is resolved here and
/// required to sit inside the central repo's skills root.
#[command]
pub async fn reveal_managed_skill_folder(
    store: State<'_, Arc<SkillStore>>,
    skill_id: String,
) -> Result<(), AppError> {
    let store = store.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        let record = store
            .get_skill_by_id(&skill_id)
            .map_err(AppError::db)?
            .ok_or_else(|| AppError::not_found("Skill not found"))?;
        let root = central_repo::skills_dir();
        let target = resolve_reveal_target_within(&root, Path::new(&record.central_path))?;
        open_folder_in_file_manager(&target)
    })
    .await?
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Create a directory symlink, reporting `false` when the platform or the
    /// process privileges do not allow it (Windows without
    /// SeCreateSymbolicLinkPrivilege, i.e. error 1314). Same helper pattern as
    /// `commands::plugins::tests`.
    fn try_symlink_dir(target: &Path, link: &Path) -> bool {
        #[cfg(unix)]
        {
            std::os::unix::fs::symlink(target, link).is_ok()
        }
        #[cfg(windows)]
        {
            std::os::windows::fs::symlink_dir(target, link).is_ok()
        }
        #[cfg(not(any(unix, windows)))]
        {
            let _ = (target, link);
            false
        }
    }

    fn make_skill_root(base: &Path) -> PathBuf {
        let root = base.join("skills");
        std::fs::create_dir_all(root.join("category/my-skill")).unwrap();
        root
    }

    #[test]
    fn resolver_accepts_nested_path_inside_root() {
        let tmp = tempfile::tempdir().unwrap();
        let root = make_skill_root(tmp.path());
        let target = resolve_reveal_target(&root, "category/my-skill").unwrap();
        assert_eq!(target, std::fs::canonicalize(root.join("category/my-skill")).unwrap());
    }

    #[test]
    fn resolver_rejects_empty_relative_path() {
        let tmp = tempfile::tempdir().unwrap();
        let root = make_skill_root(tmp.path());
        assert!(resolve_reveal_target(&root, "").is_err());
        assert!(resolve_reveal_target(&root, "   ").is_err());
    }

    #[test]
    fn resolver_rejects_dotdot_escape() {
        let tmp = tempfile::tempdir().unwrap();
        let root = make_skill_root(tmp.path());
        std::fs::create_dir_all(tmp.path().join("outside")).unwrap();
        assert!(resolve_reveal_target(&root, "../outside").is_err());
        // A `..` buried mid-path is rejected at the component level too.
        assert!(resolve_reveal_target(&root, "category/../my-skill").is_err());
    }

    #[test]
    fn resolver_rejects_absolute_paths() {
        let tmp = tempfile::tempdir().unwrap();
        let root = make_skill_root(tmp.path());
        // A real directory outside the root, reached by absolute path.
        let outside = tmp.path().join("outside");
        std::fs::create_dir_all(&outside).unwrap();
        let outside_str = outside.to_string_lossy().to_string();
        assert!(resolve_reveal_target(&root, &outside_str).is_err());
        // The literal Windows escape: a drive-letter path replaces the base on
        // `join`, so it must be refused whether or not the target exists.
        assert!(resolve_reveal_target(&root, "C:\\Windows").is_err());
    }

    #[test]
    fn resolver_rejects_missing_target_and_missing_root() {
        let tmp = tempfile::tempdir().unwrap();
        let root = make_skill_root(tmp.path());
        // Target inside the root that does not exist.
        assert!(resolve_reveal_target(&root, "category/nope").is_err());
        // The root itself missing.
        assert!(resolve_reveal_target(&tmp.path().join("missing-root"), "my-skill").is_err());
    }

    #[test]
    fn resolver_rejects_file_target() {
        let tmp = tempfile::tempdir().unwrap();
        let root = make_skill_root(tmp.path());
        std::fs::write(root.join("category/plain.txt"), "x").unwrap();
        assert!(resolve_reveal_target(&root, "category/plain.txt").is_err());
    }

    #[test]
    fn resolver_rejects_symlink_escaping_root() {
        let tmp = tempfile::tempdir().unwrap();
        let root = make_skill_root(tmp.path());
        let outside = tmp.path().join("outside/real-skill");
        std::fs::create_dir_all(&outside).unwrap();

        if !try_symlink_dir(&outside, &root.join("link")) {
            // Windows without SeCreateSymbolicLinkPrivilege (the environment
            // gap behind this suite's two known symlink failures): skip,
            // rather than add a third failure that has nothing to do with
            // this code.
            return;
        }

        // canonicalize resolves the link, so the containment check must
        // reject a target that only looks inside the root.
        assert!(resolve_reveal_target(&root, "link").is_err());
    }

    #[test]
    fn within_resolver_rejects_path_outside_root() {
        let tmp = tempfile::tempdir().unwrap();
        let root = make_skill_root(tmp.path());
        let outside = tmp.path().join("outside");
        std::fs::create_dir_all(&outside).unwrap();

        assert!(resolve_reveal_target_within(&root, &outside).is_err());
        // Inside the root is accepted, even with an unnormalized spelling.
        let inside = root.join("category").join("my-skill");
        assert!(resolve_reveal_target_within(&root, &inside).is_ok());
    }
}
