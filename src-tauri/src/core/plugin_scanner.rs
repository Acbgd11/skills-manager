//! Read-only discovery of Claude Code plugin skills and official skills.
//! Nothing in this module writes to the user's Claude configuration.

use serde::Serialize;
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

use crate::core::skill_metadata::{is_valid_skill_dir, parse_skill_md};

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct PluginSkillEntry {
    pub name: String,
    pub description: Option<String>,
    pub relative_path: String,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct PluginSkillGroup {
    pub marketplace: String,
    pub plugin: String,
    pub version: String,
    pub installed_at: Option<String>,
    pub last_updated: Option<String>,
    pub homepage: Option<String>,
    pub repository: Option<String>,
    pub author: Option<String>,
    pub description: Option<String>,
    pub skills: Vec<PluginSkillEntry>,
}

#[derive(Debug, Clone, Default)]
struct InstalledPluginInfo {
    version: Option<String>,
    installed_at: Option<String>,
    last_updated: Option<String>,
}

/// Pure function: resolve the config directory from a given env value
/// (testable without touching process-level environment variables).
pub fn claude_config_dir_from(env_value: Option<&str>) -> PathBuf {
    if let Some(value) = env_value {
        let trimmed = value.trim();
        if !trimmed.is_empty() {
            return PathBuf::from(trimmed);
        }
    }
    dirs::home_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join(".claude")
}

pub fn claude_config_dir() -> PathBuf {
    claude_config_dir_from(std::env::var("CLAUDE_CONFIG_DIR").ok().as_deref())
}

/// "2026-09-26T18:36:13.904Z" -> "2026-09-26"
fn iso_date(value: Option<&str>) -> Option<String> {
    let s = value?.trim();
    if s.len() >= 10 && s.as_bytes()[4] == b'-' && s.as_bytes()[7] == b'-' {
        Some(s[..10].to_string())
    } else {
        None
    }
}

fn read_installed_plugins(config_dir: &Path) -> HashMap<String, InstalledPluginInfo> {
    let path = config_dir.join("plugins").join("installed_plugins.json");
    let Ok(text) = fs::read_to_string(&path) else {
        return HashMap::new();
    };
    let Ok(json) = serde_json::from_str::<serde_json::Value>(&text) else {
        return HashMap::new();
    };
    let mut map = HashMap::new();
    let Some(plugins) = json.get("plugins").and_then(|v| v.as_object()) else {
        return map;
    };
    for (id, entries) in plugins {
        let Some(first) = entries.as_array().and_then(|a| a.first()) else {
            continue;
        };
        map.insert(
            id.clone(),
            InstalledPluginInfo {
                version: first.get("version").and_then(|v| v.as_str()).map(str::to_string),
                installed_at: iso_date(first.get("installedAt").and_then(|v| v.as_str())),
                last_updated: iso_date(first.get("lastUpdated").and_then(|v| v.as_str())),
            },
        );
    }
    map
}

/// Highest semver version directory inside a plugin dir; falls back to name order.
fn newest_version_dir(plugin_dir: &Path) -> Option<PathBuf> {
    let mut dirs: Vec<(Option<semver::Version>, PathBuf)> = Vec::new();
    for entry in fs::read_dir(plugin_dir).ok()? {
        let Ok(entry) = entry else { continue };
        let path = entry.path();
        if !path.is_dir() {
            continue;
        }
        let name = entry.file_name().to_string_lossy().to_string();
        if name.starts_with('.') {
            continue;
        }
        dirs.push((semver::Version::parse(&name).ok(), path));
    }
    dirs.sort_by(|a, b| match (&a.0, &b.0) {
        (Some(x), Some(y)) => x.cmp(y),
        (Some(_), None) => std::cmp::Ordering::Greater,
        (None, Some(_)) => std::cmp::Ordering::Less,
        (None, None) => a.1.cmp(&b.1),
    });
    dirs.pop().map(|(_, p)| p)
}

fn read_plugin_manifest(
    version_dir: &Path,
) -> (Option<String>, Option<String>, Option<String>, Option<String>) {
    let path = version_dir.join(".claude-plugin").join("plugin.json");
    let Ok(text) = fs::read_to_string(&path) else {
        return (None, None, None, None);
    };
    let Ok(json) = serde_json::from_str::<serde_json::Value>(&text) else {
        return (None, None, None, None);
    };
    let s = |key: &str| json.get(key).and_then(|v| v.as_str()).map(str::to_string);
    let author = json
        .get("author")
        .and_then(|a| a.get("name").and_then(|v| v.as_str()).or_else(|| a.as_str()))
        .map(str::to_string);
    (s("description"), s("homepage"), s("repository"), author)
}

fn skills_under(skills_root: &Path, relative_prefix: &Path) -> Vec<PluginSkillEntry> {
    let mut skills = Vec::new();
    let Ok(entries) = fs::read_dir(skills_root) else {
        return skills;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if !path.is_dir() || !is_valid_skill_dir(&path) {
            continue;
        }
        let meta = parse_skill_md(&path);
        let Some(relative) = relative_prefix
            .join(entry.file_name())
            .to_str()
            .map(|s| s.replace('\\', "/"))
        else {
            continue;
        };
        skills.push(PluginSkillEntry {
            name: meta
                .name
                .clone()
                .unwrap_or_else(|| entry.file_name().to_string_lossy().to_string()),
            description: meta.description.clone(),
            relative_path: relative,
        });
    }
    skills.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()));
    skills
}

pub fn scan_plugin_skills(config_dir: &Path) -> Vec<PluginSkillGroup> {
    let installed = read_installed_plugins(config_dir);
    let cache_root = config_dir.join("plugins").join("cache");
    let mut groups = Vec::new();
    let Ok(markets) = fs::read_dir(&cache_root) else {
        return groups;
    };
    for market in markets.flatten() {
        let market_name = market.file_name().to_string_lossy().to_string();
        if !market.path().is_dir() || market_name.starts_with('.') {
            continue;
        }
        let Ok(plugins) = fs::read_dir(market.path()) else {
            continue;
        };
        for plugin in plugins.flatten() {
            let plugin_name = plugin.file_name().to_string_lossy().to_string();
            if !plugin.path().is_dir() || plugin_name.starts_with('.') {
                continue;
            }
            let Some(version_dir) = newest_version_dir(&plugin.path()) else {
                continue;
            };
            let version = version_dir
                .file_name()
                .map(|n| n.to_string_lossy().to_string())
                .unwrap_or_default();
            let info = installed.get(&format!("{plugin_name}@{market_name}"));
            let (description, homepage, repository, author) = read_plugin_manifest(&version_dir);
            let relative_prefix = Path::new("cache")
                .join(&market_name)
                .join(&plugin_name)
                .join(&version)
                .join("skills");
            let skills = skills_under(&version_dir.join("skills"), &relative_prefix);
            if skills.is_empty() {
                continue;
            }
            groups.push(PluginSkillGroup {
                marketplace: market_name.clone(),
                plugin: plugin_name,
                version: info
                    .and_then(|i| i.version.clone())
                    .unwrap_or(version),
                installed_at: info.and_then(|i| i.installed_at.clone()),
                last_updated: info.and_then(|i| i.last_updated.clone()),
                homepage,
                repository,
                author,
                description,
                skills,
            });
        }
    }
    groups.sort_by(|a, b| a.plugin.to_lowercase().cmp(&b.plugin.to_lowercase()));
    groups
}

pub fn scan_official_skills(config_dir: &Path) -> Vec<PluginSkillEntry> {
    let root = config_dir
        .join("plugins")
        .join("marketplaces")
        .join("anthropic-agent-skills")
        .join("skills");
    skills_under(&root, Path::new("marketplaces/anthropic-agent-skills/skills"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn write_skill(dir: &std::path::Path, name: &str, description: &str) {
        let skill_dir = dir.join(name);
        fs::create_dir_all(&skill_dir).unwrap();
        fs::write(
            skill_dir.join("SKILL.md"),
            format!("---\nname: {name}\ndescription: {description}\n---\n\nBody\n"),
        )
        .unwrap();
    }

    #[test]
    fn scans_plugin_skills_with_metadata_and_picks_newest_version() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        let market = root.join("plugins/cache/claude-plugins-official");
        let old = market.join("superpowers").join("5.0.7");
        let new = market.join("superpowers").join("6.4.1");
        write_skill(&old.join("skills"), "old-skill", "old");
        write_skill(&new.join("skills"), "brainstorming", "Design first");
        fs::create_dir_all(new.join(".claude-plugin")).unwrap();
        fs::write(
            new.join(".claude-plugin/plugin.json"),
            r#"{"name":"superpowers","description":"Core skills","author":{"name":"Jesse"},"homepage":"https://github.com/obra/superpowers","repository":"https://github.com/obra/superpowers"}"#,
        )
        .unwrap();
        fs::create_dir_all(root.join("plugins")).unwrap();
        fs::write(
            root.join("plugins/installed_plugins.json"),
            r#"{"version":2,"plugins":{"superpowers@claude-plugins-official":[{"scope":"user","version":"6.4.1","installedAt":"2026-09-26T18:36:13.904Z","lastUpdated":"2026-09-27T01:02:03.000Z"}]}}"#,
        )
        .unwrap();

        let groups = scan_plugin_skills(root);
        assert_eq!(groups.len(), 1);
        let g = &groups[0];
        assert_eq!(g.plugin, "superpowers");
        assert_eq!(g.version, "6.4.1");
        assert_eq!(g.installed_at.as_deref(), Some("2026-09-26"));
        assert_eq!(g.last_updated.as_deref(), Some("2026-09-27"));
        assert_eq!(g.repository.as_deref(), Some("https://github.com/obra/superpowers"));
        assert_eq!(g.skills.len(), 1, "only newest version dir is scanned");
        assert_eq!(g.skills[0].name, "brainstorming");
    }

    #[test]
    fn tolerates_missing_installed_plugins_json() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        write_skill(&root.join("plugins/cache/mkt/plug/1.0.0/skills"), "s", "d");
        let groups = scan_plugin_skills(root);
        assert_eq!(groups.len(), 1);
        assert_eq!(groups[0].installed_at, None);
        assert_eq!(groups[0].version, "1.0.0");
    }

    #[test]
    fn skips_dirs_without_skill_md_and_missing_root() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        fs::create_dir_all(root.join("plugins/cache/mkt/plug/1.0.0/skills/not-a-skill")).unwrap();
        assert!(scan_plugin_skills(root).is_empty());
        assert!(scan_plugin_skills(&root.join("does-not-exist")).is_empty());
        assert!(scan_official_skills(&root.join("does-not-exist")).is_empty());
    }

    #[test]
    fn scans_official_skills_directory() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        write_skill(
            &root.join("plugins/marketplaces/anthropic-agent-skills/skills"),
            "docx",
            "Word files",
        );
        let official = scan_official_skills(root);
        assert_eq!(official.len(), 1);
        assert_eq!(official[0].name, "docx");
    }

    #[test]
    fn tolerates_malformed_installed_plugins_json() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        write_skill(&root.join("plugins/cache/mkt/plug/1.0.0/skills"), "s", "d");
        fs::create_dir_all(root.join("plugins")).unwrap();
        fs::write(root.join("plugins/installed_plugins.json"), "{not json").unwrap();
        let groups = scan_plugin_skills(root);
        assert_eq!(groups.len(), 1);
        assert_eq!(groups[0].installed_at, None);
    }

    #[test]
    fn unreadable_skill_md_does_not_break_scan() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        let skill_dir = root.join("plugins/cache/mkt/plug/1.0.0/skills/broken");
        fs::create_dir_all(&skill_dir).unwrap();
        fs::write(skill_dir.join("SKILL.md"), [0xff, 0xfe, 0x00, 0x01]).unwrap();
        let groups = scan_plugin_skills(root);
        assert_eq!(groups.len(), 1, "scan continues");
        assert_eq!(groups[0].skills.len(), 1);
        assert_eq!(groups[0].skills[0].description, None);
    }

    #[test]
    fn claude_config_dir_from_prefers_env_value() {
        assert_eq!(
            claude_config_dir_from(Some("D:\\claude-cfg-test")),
            PathBuf::from("D:\\claude-cfg-test")
        );
    }

    #[test]
    fn claude_config_dir_from_ignores_whitespace_only_env_value() {
        assert!(
            claude_config_dir_from(Some("  "))
                .ends_with(".claude"),
            "whitespace-only env value should fall back to ~/.claude"
        );
    }

    #[test]
    fn claude_config_dir_from_ignores_empty_env_value() {
        assert!(
            claude_config_dir_from(Some("")).ends_with(".claude"),
            "empty env value should fall back to ~/.claude"
        );
    }

    #[test]
    fn claude_config_dir_from_none_falls_back_to_home() {
        assert!(
            claude_config_dir_from(None).ends_with(".claude"),
            "missing env value should fall back to ~/.claude"
        );
    }
}
