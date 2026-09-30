//! Read-only Tauri commands exposing plugin skills and their documents.

use serde::Serialize;
use std::path::{Path, PathBuf};
use tauri::command;

use crate::commands::projects::ProjectSkillDocumentDto;
use crate::core::error::AppError;
use crate::core::plugin_scanner;

#[derive(Debug, Serialize)]
pub struct PluginSkillsDto {
    pub groups: Vec<plugin_scanner::PluginSkillGroup>,
    pub official: Vec<plugin_scanner::PluginSkillEntry>,
    pub config_dir: String,
}

/// Resolve a plugin-relative skill path, refusing anything outside `<config>/plugins`.
fn resolve_skill_dir(config_dir: &Path, relative_path: &str) -> Result<PathBuf, AppError> {
    if relative_path.contains("..") {
        return Err(AppError::invalid_input("Invalid skill path"));
    }
    let plugins_root = config_dir.join("plugins");
    let full = plugins_root.join(relative_path);
    let canon_root = std::fs::canonicalize(&plugins_root)
        .map_err(|_| AppError::invalid_input("Plugins directory not found"))?;
    let canon_full = std::fs::canonicalize(&full)
        .map_err(|_| AppError::invalid_input("Skill directory not found"))?;
    if !canon_full.starts_with(&canon_root) {
        return Err(AppError::invalid_input("Path outside plugins directory"));
    }
    Ok(canon_full)
}

#[command]
pub async fn get_claude_plugin_skills() -> Result<PluginSkillsDto, AppError> {
    tauri::async_runtime::spawn_blocking(|| {
        let config_dir = plugin_scanner::claude_config_dir();
        let groups = plugin_scanner::scan_plugin_skills(&config_dir);
        let official = plugin_scanner::scan_official_skills(&config_dir);
        Ok(PluginSkillsDto {
            groups,
            official,
            config_dir: config_dir.to_string_lossy().to_string(),
        })
    })
    .await?
}

#[command]
pub async fn get_plugin_skill_document(
    relative_path: String,
) -> Result<ProjectSkillDocumentDto, AppError> {
    tauri::async_runtime::spawn_blocking(move || {
        let config_dir = plugin_scanner::claude_config_dir();
        let skill_dir = resolve_skill_dir(&config_dir, &relative_path)?;
        for candidate in ["SKILL.md", "skill.md", "CLAUDE.md", "README.md"] {
            let file_path = skill_dir.join(candidate);
            if file_path.is_file() {
                let content = std::fs::read_to_string(&file_path)
                    .map_err(|_| AppError::invalid_input("Skill document is not readable"))?;
                return Ok(ProjectSkillDocumentDto {
                    skill_name: relative_path.clone(),
                    filename: candidate.to_string(),
                    content,
                });
            }
        }
        Err(AppError::invalid_input("No skill document found"))
    })
    .await?
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Create a directory symlink, reporting `false` when the platform or the
    /// process privileges do not allow it (Windows without
    /// SeCreateSymbolicLinkPrivilege, i.e. error 1314).
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

    #[test]
    fn rejects_paths_outside_plugins_root() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        std::fs::create_dir_all(root.join("plugins/cache/m/p/1.0.0/skills/s")).unwrap();
        assert!(resolve_skill_dir(root, "../outside").is_err());
        assert!(resolve_skill_dir(root, "cache/m/p/1.0.0/skills/s").is_ok());
    }

    #[test]
    fn rejects_absolute_paths_outside_plugins_root() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        std::fs::create_dir_all(root.join("plugins/cache/m/p/1.0.0/skills/s")).unwrap();
        // A real directory outside `plugins/`, reached by absolute path: joining
        // an absolute path onto the plugins root discards the root entirely.
        let outside = root.join("outside");
        std::fs::create_dir_all(&outside).unwrap();
        let outside_str = outside.to_string_lossy().to_string();
        let err = resolve_skill_dir(root, &outside_str).unwrap_err();
        assert_eq!(err.message, "Path outside plugins directory");

        // The literal Windows escape (a drive-letter path replaces the base on
        // `join`): refused whether or not the path itself exists.
        assert!(resolve_skill_dir(root, "C:\\Windows").is_err());
    }

    #[test]
    fn rejects_symlink_escaping_plugins_root() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        let plugins_root = root.join("plugins");
        let outside = root.join("outside");
        std::fs::create_dir_all(&plugins_root).unwrap();
        std::fs::create_dir_all(&outside).unwrap();

        if !try_symlink_dir(&outside, &plugins_root.join("escape")) {
            // Windows without SeCreateSymbolicLinkPrivilege (the environment gap
            // behind this suite's two known symlink failures): skip, rather than
            // add a third failure that has nothing to do with this code.
            return;
        }

        let err = resolve_skill_dir(root, "escape").unwrap_err();
        assert_eq!(err.message, "Path outside plugins directory");
    }
}
