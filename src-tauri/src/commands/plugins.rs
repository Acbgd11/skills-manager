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

    #[test]
    fn rejects_paths_outside_plugins_root() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        std::fs::create_dir_all(root.join("plugins/cache/m/p/1.0.0/skills/s")).unwrap();
        assert!(resolve_skill_dir(root, "../outside").is_err());
        assert!(resolve_skill_dir(root, "cache/m/p/1.0.0/skills/s").is_ok());
    }
}