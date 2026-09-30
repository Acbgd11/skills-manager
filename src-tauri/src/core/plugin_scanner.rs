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
    install_path: Option<String>,
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
        // Use get(..10) instead of s[..10] to avoid panicking on multi-byte
        // boundaries (a malformed UTF-8 edge at byte 10 would slice mid-char).
        Some(s.get(..10)?.to_string())
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
                install_path: first
                    .get("installPath")
                    .and_then(|v| v.as_str())
                    .map(str::to_string),
            },
        );
    }
    map
}

/// Order version directories for a plugin by priority:
/// 1. The directory pointed to by `installPath` (if it exists among the
///    candidates) comes first.
/// 2. Then semver-descending (so the highest version wins).
/// 3. Then most-recently-modified mtime descending (replaces the old
///    lexicographic fallback for git-SHA directory names that are not
///    semver-parseable).
///
/// Returns an ordered Vec so the caller can fall back to the next candidate
/// when the first has no skills (F3: don't make the whole group disappear).
fn ordered_version_dirs(plugin_dir: &Path, install_path: Option<&str>) -> Vec<PathBuf> {
    let mut entries: Vec<(Option<semver::Version>, Option<std::time::SystemTime>, PathBuf)> =
        Vec::new();
    let Ok(dir_iter) = fs::read_dir(plugin_dir) else {
        return Vec::new();
    };
    for entry in dir_iter.flatten() {
        let path = entry.path();
        if !path.is_dir() {
            continue;
        }
        let name = entry.file_name().to_string_lossy().to_string();
        if name.starts_with('.') {
            continue;
        }
        let sem = semver::Version::parse(&name).ok();
        let mtime = entry
            .metadata()
            .and_then(|m| m.modified())
            .ok();
        entries.push((sem, mtime, path));
    }

    // Resolve the installPath target's file name for comparison.
    let install_dir_name = install_path
        .and_then(|p| {
            // installPath may be absolute or relative; we compare by the final
            // path component (the version directory name).
            Path::new(p).file_name().map(|n| n.to_string_lossy().to_string())
        });

    let mut sorted: Vec<(u8, Option<semver::Version>, Option<std::time::SystemTime>, PathBuf)> =
        entries
            .into_iter()
            .map(|(sem, mtime, path)| {
                let name = path
                    .file_name()
                    .map(|n| n.to_string_lossy().to_string())
                    .unwrap_or_default();
                let is_install_target = install_dir_name
                    .as_deref()
                    .map(|target| target == name)
                    .unwrap_or(false);
                // Tuple: (rank, semver_option, mtime_option, path)
                // rank 0 = installPath target, 1 = everything else
                let rank: u8 = if is_install_target { 0 } else { 1 };
                (rank, sem, mtime, path)
            })
            .collect();

    sorted.sort_by(|a, b| {
        // rank ascending (installPath first)
        match a.0.cmp(&b.0) {
            std::cmp::Ordering::Equal => {}
            ord => return ord,
        }
        // semver descending
        match (&a.1, &b.1) {
            (Some(x), Some(y)) => y.cmp(x),
            (Some(_), None) => std::cmp::Ordering::Less,
            (None, Some(_)) => std::cmp::Ordering::Greater,
            (None, None) => std::cmp::Ordering::Equal,
        }
        .then_with(|| {
            // mtime descending
            match (&a.2, &b.2) {
                (Some(x), Some(y)) => y.cmp(x),
                (Some(_), None) => std::cmp::Ordering::Less,
                (None, Some(_)) => std::cmp::Ordering::Greater,
                (None, None) => std::cmp::Ordering::Equal,
            }
        })
    });

    sorted.into_iter().map(|(_, _, _, path)| path).collect()
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

/// Recursively discover skills under `skills_root`, bounded to `max_depth`
/// levels below the root. A directory that is itself a valid skill dir
/// (contains SKILL.md/skill.md) is recorded as a skill and **not** descended
/// into further; a non-skill directory is descended, up to the bound.
/// `relative_prefix` carries the path components from the cache root so
/// `relative_path` naturally includes intermediate layers like
/// `cache/<m>/<p>/<v>/skills/i18n/foo-zh`.
fn skills_under(skills_root: &Path, relative_prefix: &Path) -> Vec<PluginSkillEntry> {
    let mut skills = Vec::new();
    collect_skills_recursive(skills_root, relative_prefix, 0, &mut skills);
    skills.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()));
    skills
}

/// Maximum depth below `skills/` root for recursive discovery. The real-world
/// nesting observed is `skills/i18n/<skill>/SKILL.md` (2 levels), so 3 covers
/// that plus one more without an unbounded walk.
const SKILL_SCAN_MAX_DEPTH: usize = 3;

fn collect_skills_recursive(
    dir: &Path,
    relative_prefix: &Path,
    depth: usize,
    out: &mut Vec<PluginSkillEntry>,
) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if !path.is_dir() {
            continue;
        }
        if is_valid_skill_dir(&path) {
            // This directory is a skill — record it and do not descend further.
            let meta = parse_skill_md(&path);
            let Some(relative) = relative_prefix
                .join(entry.file_name())
                .to_str()
                .map(|s| s.replace('\\', "/"))
            else {
                continue;
            };
            out.push(PluginSkillEntry {
                name: meta
                    .name
                    .clone()
                    .unwrap_or_else(|| entry.file_name().to_string_lossy().to_string()),
                description: meta.description.clone(),
                relative_path: relative,
            });
        } else if depth < SKILL_SCAN_MAX_DEPTH {
            // Not a skill directory — descend into it, carrying the deeper
            // relative prefix so the final path includes the intermediate layer.
            if let Some(child_prefix) = relative_prefix.join(entry.file_name()).to_str() {
                let child_prefix = PathBuf::from(child_prefix);
                collect_skills_recursive(&path, &child_prefix, depth + 1, out);
            }
        }
    }
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
            let info = installed.get(&format!("{plugin_name}@{market_name}"));
            let candidates =
                ordered_version_dirs(&plugin.path(), info.and_then(|i| i.install_path.as_deref()));
            if candidates.is_empty() {
                continue;
            }
            // Try each candidate version directory in priority order. If the
            // first has no skills (e.g. a just-installed version whose skills
            // directory is empty), fall back to the next instead of making the
            // whole plugin group disappear (F3).
            let mut chosen: Option<(
                PathBuf,
                String,
                Vec<PluginSkillEntry>,
            )> = None;
            for version_dir in &candidates {
                let version = version_dir
                    .file_name()
                    .map(|n| n.to_string_lossy().to_string())
                    .unwrap_or_default();
                let relative_prefix = Path::new("cache")
                    .join(&market_name)
                    .join(&plugin_name)
                    .join(&version)
                    .join("skills");
                let skills = skills_under(&version_dir.join("skills"), &relative_prefix);
                if !skills.is_empty() {
                    chosen = Some((version_dir.clone(), version, skills));
                    break;
                }
            }
            let Some((version_dir, version, skills)) = chosen else {
                // All candidates empty — still skip, but this is now only when
                // every version dir genuinely has no skills.
                continue;
            };
            let (description, homepage, repository, author) = read_plugin_manifest(&version_dir);
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

    // ── F2: nested skill discovery ──

    #[test]
    fn discovers_nested_skills_in_subdirectory() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        // skills/i18n/foo-zh/SKILL.md — one level of nesting under skills/
        // write_skill creates <dir>/<name>/SKILL.md, so we pass the i18n parent.
        write_skill(
            &root.join("plugins/cache/mkt/plug/1.0.0/skills/i18n"),
            "foo-zh",
            "Chinese",
        );

        let groups = scan_plugin_skills(root);
        assert_eq!(groups.len(), 1);
        assert_eq!(groups[0].skills.len(), 1, "nested skill discovered");
        assert_eq!(groups[0].skills[0].name, "foo-zh");
        assert!(
            groups[0].skills[0]
                .relative_path
                .ends_with("skills/i18n/foo-zh"),
            "relative_path includes intermediate layer: {}",
            groups[0].skills[0].relative_path
        );
    }

    #[test]
    fn does_not_descend_into_skill_directory_that_is_itself_a_skill() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        // A skill dir that contains a nested SKILL.md child — the parent is
        // the skill; the child should NOT be separately listed.
        let skill_dir = root.join("plugins/cache/mkt/plug/1.0.0/skills/parent");
        fs::create_dir_all(&skill_dir).unwrap();
        fs::write(
            skill_dir.join("SKILL.md"),
            "---\nname: parent\ndescription: top\n---\n\nBody\n",
        )
        .unwrap();
        fs::create_dir_all(skill_dir.join("child")).unwrap();
        fs::write(
            skill_dir.join("child/SKILL.md"),
            "---\nname: child\ndescription: nested\n---\n\nBody\n",
        )
        .unwrap();

        let groups = scan_plugin_skills(root);
        assert_eq!(groups.len(), 1);
        assert_eq!(groups[0].skills.len(), 1, "parent recorded, child not double-counted");
        assert_eq!(groups[0].skills[0].name, "parent");
    }

    #[test]
    fn deep_non_skill_directory_produces_no_entry() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        // A directory 2 levels deep with no SKILL.md — no entries, no panic.
        fs::create_dir_all(root.join("plugins/cache/mkt/plug/1.0.0/skills/a/b/c")).unwrap();
        let groups = scan_plugin_skills(root);
        assert!(groups.is_empty(), "no skills → no groups");
    }

    // ── F3: version directory selection ──

    #[test]
    fn install_path_directs_version_selection_over_semver() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        // Two SHA-named dirs: "aaa111" (lexicographically smaller) and "zzz999".
        // installPath points to "aaa111" — it should be chosen despite "zzz999"
        // being lexicographically larger (the old bug).
        let small = root.join("plugins/cache/mkt/plug/aaa111/skills");
        let big = root.join("plugins/cache/mkt/plug/zzz999/skills");
        write_skill(&small, "real-skill", "from small dir");
        write_skill(&big, "other-skill", "from big dir");

        // Sleep so the "small" dir is also older (mtime) to make sure it's
        // installPath, not mtime, that drives selection.
        std::thread::sleep(std::time::Duration::from_millis(20));
        std::fs::create_dir_all(root.join("plugins")).unwrap();
        fs::write(
            root.join("plugins/installed_plugins.json"),
            r#"{"version":2,"plugins":{"plug@mkt":[{"scope":"user","version":"aaa111","installPath":"aaa111"}]}}"#,
        )
        .unwrap();

        let groups = scan_plugin_skills(root);
        assert_eq!(groups.len(), 1);
        assert_eq!(groups[0].skills.len(), 1);
        assert_eq!(groups[0].skills[0].name, "real-skill");
    }

    #[test]
    fn falls_back_to_next_version_dir_when_first_is_empty() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        // newest (by mtime) has no skills; older has skills → should pick older
        let newer = root.join("plugins/cache/mkt/plug/2.0.0/skills");
        let older = root.join("plugins/cache/mkt/plug/1.0.0/skills");
        fs::create_dir_all(&newer).unwrap(); // empty skills dir
        write_skill(&older, "survivor", "from old dir");
        // Make the newer dir actually newer in mtime
        std::thread::sleep(std::time::Duration::from_millis(20));
        std::fs::write(newer.join(".touch"), "").unwrap();

        let groups = scan_plugin_skills(root);
        assert_eq!(groups.len(), 1, "group does not disappear when newest is empty");
        assert_eq!(groups[0].skills.len(), 1);
        assert_eq!(groups[0].skills[0].name, "survivor");
    }

    #[test]
    fn picks_semver_highest_when_no_install_path() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        let v1 = root.join("plugins/cache/mkt/plug/1.0.0/skills");
        let v2 = root.join("plugins/cache/mkt/plug/2.0.0/skills");
        write_skill(&v1, "old", "v1");
        write_skill(&v2, "new", "v2");
        // No installPath in JSON → semver wins: 2.0.0 > 1.0.0
        fs::create_dir_all(root.join("plugins")).unwrap();
        fs::write(
            root.join("plugins/installed_plugins.json"),
            r#"{"version":2,"plugins":{"plug@mkt":[{"scope":"user","version":"2.0.0"}]}}"#,
        )
        .unwrap();

        let groups = scan_plugin_skills(root);
        assert_eq!(groups.len(), 1);
        assert_eq!(groups[0].skills.len(), 1);
        assert_eq!(groups[0].skills[0].name, "new");
    }
}
