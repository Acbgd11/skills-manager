//! Read-only Tauri commands exposing plugin skills and their documents.

use serde::Serialize;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tauri::command;
use tauri::State;

use crate::commands::projects::{ensure_safe_skill_relative_path, ProjectSkillDocumentDto};
use crate::core::error::AppError;
use crate::core::plugin_scanner;
use crate::core::skill_store::SkillStore;
use crate::core::translation_store;

#[derive(Debug, Serialize)]
pub struct PluginSkillsDto {
    pub groups: Vec<plugin_scanner::PluginSkillGroup>,
    pub official: Vec<plugin_scanner::PluginSkillEntry>,
    pub config_dir: String,
}

/// Resolve a plugin-relative skill path, refusing anything outside `<config>/plugins`.
///
/// Reuses the shared `ensure_safe_skill_relative_path` validator (from
/// `commands::projects`) which rejects empty paths, `..` components, and
/// absolute paths at the component level — stricter and more correct than the
/// old ad-hoc `contains("..")` check, and consistent with the project-skill
/// commands. The canonicalize containment check is retained.
fn resolve_skill_dir(config_dir: &Path, relative_path: &str) -> Result<PathBuf, AppError> {
    ensure_safe_skill_relative_path(relative_path)?;
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
pub async fn get_claude_plugin_skills(
    store: State<'_, Arc<SkillStore>>,
) -> Result<PluginSkillsDto, AppError> {
    let store = store.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        let config_dir = plugin_scanner::claude_config_dir();
        let mut groups = plugin_scanner::scan_plugin_skills(&config_dir);
        let mut official = plugin_scanner::scan_official_skills(&config_dir);

        // Fill zh_name/zh_description from the translation cache (by fingerprint).
        let cached = store.get_translations().unwrap_or_default();
        for group in &mut groups {
            if let Some(d) = group.description.as_deref() {
                let fp = translation_store::fingerprint("plugin_group", &group.plugin, Some(d));
                if let Some(rec) = cached.get(&fp) {
                    group.zh_name = Some(rec.zh_name.clone());
                    group.zh_description = Some(rec.zh_description.clone());
                }
            }
            for skill in &mut group.skills {
                let fp =
                    translation_store::fingerprint("plugin_skill", &skill.name, skill.description.as_deref());
                if let Some(rec) = cached.get(&fp) {
                    skill.zh_name = Some(rec.zh_name.clone());
                    skill.zh_description = Some(rec.zh_description.clone());
                }
            }
        }
        for skill in &mut official {
            let fp =
                translation_store::fingerprint("official_skill", &skill.name, skill.description.as_deref());
            if let Some(rec) = cached.get(&fp) {
                skill.zh_name = Some(rec.zh_name.clone());
                skill.zh_description = Some(rec.zh_description.clone());
            }
        }

        Ok(PluginSkillsDto {
            groups,
            official,
            config_dir: config_dir.to_string_lossy().to_string(),
        })
    })
    .await?
}

/// Read a skill document from the resolved skill directory, with file-level
/// symlink containment. Extracted as a synchronous helper so it can be tested
/// without the Tauri async runtime.
fn read_skill_document_inner(
    config_dir: &Path,
    relative_path: &str,
) -> Result<ProjectSkillDocumentDto, AppError> {
    let skill_dir = resolve_skill_dir(config_dir, relative_path)?;
    let plugins_root = config_dir.join("plugins");
    let canon_plugins = std::fs::canonicalize(&plugins_root)
        .map_err(|_| AppError::invalid_input("Plugins directory not found"))?;
    for candidate in ["SKILL.md", "skill.md", "CLAUDE.md", "README.md"] {
        let file_path = skill_dir.join(candidate);
        if !file_path.exists() {
            continue;
        }
        // File-level symlink check: if the candidate is a symlink, resolve
        // it and require the target to remain inside <config>/plugins.
        // Mirrors the pattern in agent_workspace::get_global_local_skill_document.
        if let Ok(meta) = std::fs::symlink_metadata(&file_path) {
            if meta.file_type().is_symlink() {
                let resolved = match std::fs::canonicalize(&file_path) {
                    Ok(path) => path,
                    Err(_) => continue,
                };
                if !resolved.starts_with(&canon_plugins) {
                    continue;
                }
            }
        }
        if file_path.is_file() {
            let content = std::fs::read_to_string(&file_path)
                .map_err(|_| AppError::invalid_input("Skill document is not readable"))?;
            return Ok(ProjectSkillDocumentDto {
                skill_name: relative_path.to_string(),
                filename: candidate.to_string(),
                content,
            });
        }
    }
    Err(AppError::invalid_input("No skill document found"))
}

#[command]
pub async fn get_plugin_skill_document(
    relative_path: String,
) -> Result<ProjectSkillDocumentDto, AppError> {
    let relative_path = relative_path.clone();
    tauri::async_runtime::spawn_blocking(move || {
        let config_dir = plugin_scanner::claude_config_dir();
        read_skill_document_inner(&config_dir, &relative_path)
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
        // A real directory outside `plugins/`, reached by absolute path.
        // ensure_safe_skill_relative_path now rejects it at the component
        // level (an absolute path has a Prefix/RootDir component, not Normal)
        // before the canonicalize containment check runs.
        let outside = root.join("outside");
        std::fs::create_dir_all(&outside).unwrap();
        let outside_str = outside.to_string_lossy().to_string();
        let err = resolve_skill_dir(root, &outside_str).unwrap_err();
        assert_eq!(err.message, "Invalid skill directory path");

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

    #[test]
    fn rejects_empty_relative_path() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        std::fs::create_dir_all(root.join("plugins")).unwrap();
        // Empty string is now rejected by ensure_safe_skill_relative_path
        // (the old contains("..") check let it through).
        assert!(resolve_skill_dir(root, "").is_err());
    }

    #[test]
    fn rejects_dotdot_path_component() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        std::fs::create_dir_all(root.join("plugins/cache/m/p/1.0.0/skills/s")).unwrap();
        // A path containing ".." as a component is rejected.
        assert!(resolve_skill_dir(root, "cache/m/p/1.0.0/skills/../s").is_err());
    }

    #[test]
    fn rejects_file_level_symlink_pointing_outside_plugins() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        let skill_dir = root.join("plugins/cache/m/p/1.0.0/skills/s");
        std::fs::create_dir_all(&skill_dir).unwrap();

        // A SKILL.md that is a symlink pointing outside <config>/plugins.
        let outside = root.join("outside/SKILL.md");
        std::fs::create_dir_all(outside.parent().unwrap()).unwrap();
        std::fs::write(&outside, "secret").unwrap();

        #[cfg(unix)]
        {
            std::os::unix::fs::symlink(&outside, skill_dir.join("SKILL.md")).unwrap();
        }
        #[cfg(windows)]
        {
            if !std::os::windows::fs::symlink_file(&outside, skill_dir.join("SKILL.md")).is_ok() {
                // Windows without symlink privilege: skip, like the existing
                // 1314 skips. The guard code is still exercised on CI platforms
                // that do support symlinks.
                return;
            }
        }
        #[cfg(not(any(unix, windows)))]
        {
            return;
        }

        // The document helper should skip the symlink and return not-found
        // rather than reading the outside file.
        let result = read_skill_document_inner(root, "cache/m/p/1.0.0/skills/s");
        assert!(result.is_err(), "symlink pointing outside plugins must be skipped");
    }
}
