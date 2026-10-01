//! Read-only "cross-agent skill presence": for any skill name, which other
//! installed agents also have a skill with that name, and whether their
//! content hash matches.
//!
//! This module never writes anywhere. It reuses the per-agent local-skill
//! reader (`read_linked_workspace_skills`) that `agent_workspace` already uses,
//! groups the results by skill name, and exposes a pure grouping function so
//! the logic can be exercised from an offline unit test without standing up a
//! `SkillStore`.

use std::collections::BTreeMap;
use std::sync::Arc;

use serde::Serialize;
use tauri::State;

use crate::core::{error::AppError, project_scanner::ProjectSkillInfo, skill_store::SkillStore};
use crate::core::tool_adapters::{enabled_installed_adapters, ToolAdapter};

/// One agent's entry under a given skill name.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct AgentPresenceDto {
    /// Stable agent key (e.g. "claude_code", "cursor").
    pub agent: String,
    /// Human-readable name (e.g. "Claude Code").
    pub agent_display_name: String,
    /// Content hash of this agent's copy, when one could be computed.
    pub content_hash: Option<String>,
}

/// A skill name shared across agents, plus every agent that has it.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct CrossAgentSkillDto {
    pub name: String,
    pub entries: Vec<AgentPresenceDto>,
}

/// Build the cross-agent presence index from a flat list of every installed
/// agent's local skills.
///
/// This is a pure function over `ProjectSkillInfo` so it can be unit-tested
/// without a store: the caller is responsible for collecting, per agent, the
/// records produced by `read_linked_workspace_skills`.
///
/// Skills are grouped by name (case-insensitive for grouping, but the
/// canonical case of the first occurrence is kept as the returned `name`).
/// Entries within a group are sorted by agent key for stable output.
pub fn build_presence_index(all_skills: Vec<ProjectSkillInfo>) -> Vec<CrossAgentSkillDto> {
    // Map of lowercased name -> (canonical name, agent key -> entry).
    let mut groups: BTreeMap<String, (String, BTreeMap<String, AgentPresenceDto>)> =
        BTreeMap::new();

    for skill in all_skills {
        let key = skill.name.to_lowercase();
        let entry = AgentPresenceDto {
            agent: skill.agent.clone(),
            agent_display_name: skill.agent_display_name.clone(),
            content_hash: skill.content_hash.clone(),
        };
        groups
            .entry(key.clone())
            .and_modify(|(canonical, by_agent)| {
                if canonical.is_empty() {
                    *canonical = skill.name.clone();
                }
                by_agent.insert(skill.agent.clone(), entry.clone());
            })
            .or_insert_with(|| {
                (
                    skill.name.clone(),
                    BTreeMap::from([(skill.agent.clone(), entry)]),
                )
            });
    }

    groups
        .into_values()
        .map(|(canonical, by_agent)| CrossAgentSkillDto {
            name: canonical,
            entries: by_agent.into_values().collect(),
        })
        .collect()
}

fn read_each_agent(adapter: &ToolAdapter) -> Vec<ProjectSkillInfo> {
    crate::core::project_scanner::read_linked_workspace_skills(
        &adapter.skills_dir(),
        None,
        &adapter.key,
        &adapter.display_name,
        adapter.recursive_scan,
    )
}

/// Returns the cross-agent presence index for every installed, enabled agent.
///
/// Read-only: walks each adapter's skills directory exactly like
/// `get_global_local_skills` does, but without enriching against the central
/// store (we only need name + hash + agent).
#[tauri::command]
pub async fn get_skill_agent_presence(
    store: State<'_, Arc<SkillStore>>,
) -> Result<Vec<CrossAgentSkillDto>, AppError> {
    let store = store.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        let adapters = enabled_installed_adapters(&store);
        let mut all: Vec<ProjectSkillInfo> = Vec::new();
        for adapter in &adapters {
            all.append(&mut read_each_agent(adapter));
        }
        Ok(build_presence_index(all))
    })
    .await?
}

#[cfg(test)]
mod tests {
    use super::{build_presence_index, AgentPresenceDto, CrossAgentSkillDto};
    use crate::core::project_scanner::ProjectSkillInfo;

    fn info(
        name: &str,
        agent: &str,
        display: &str,
        hash: Option<&str>,
    ) -> ProjectSkillInfo {
        ProjectSkillInfo {
            name: name.to_string(),
            dir_name: name.to_string(),
            relative_path: name.to_string(),
            description: None,
            path: format!("/tmp/{agent}/{name}"),
            files: Vec::new(),
            enabled: true,
            agent: agent.to_string(),
            agent_display_name: display.to_string(),
            tags: Vec::new(),
            in_center: false,
            sync_status: "project_only".to_string(),
            center_skill_id: None,
            last_modified_at: None,
            content_hash: hash.map(|h| h.to_string()),
        }
    }

    #[test]
    fn groups_by_name_case_insensitive_and_sorts_entries_by_agent() {
        let skills = vec![
            info("Web Search", "cursor", "Cursor", Some("hash-a")),
            info("web search", "claude_code", "Claude Code", Some("hash-a")),
            info("git", "cursor", "Cursor", Some("hash-g")),
        ];

        let index = build_presence_index(skills);

        assert_eq!(
            index,
            vec![
                CrossAgentSkillDto {
                    name: "git".to_string(),
                    entries: vec![AgentPresenceDto {
                        agent: "cursor".to_string(),
                        agent_display_name: "Cursor".to_string(),
                        content_hash: Some("hash-g".to_string()),
                    }],
                },
                CrossAgentSkillDto {
                    // canonical case is the first occurrence ("Web Search")
                    name: "Web Search".to_string(),
                    entries: vec![
                        AgentPresenceDto {
                            agent: "claude_code".to_string(),
                            agent_display_name: "Claude Code".to_string(),
                            content_hash: Some("hash-a".to_string()),
                        },
                        AgentPresenceDto {
                            agent: "cursor".to_string(),
                            agent_display_name: "Cursor".to_string(),
                            content_hash: Some("hash-a".to_string()),
                        },
                    ],
                },
            ]
        );
    }

    #[test]
    fn same_name_different_hash_keeps_both_agents_with_their_hashes() {
        let skills = vec![
            info("Brainstorm", "claude_code", "Claude Code", Some("h-cc")),
            info("brainstorm", "codex", "Codex", Some("h-cdx-different")),
        ];

        let index = build_presence_index(skills);

        assert_eq!(index.len(), 1);
        let group = &index[0];
        assert_eq!(group.name, "Brainstorm");
        assert_eq!(group.entries.len(), 2);

        let cc = group
            .entries
            .iter()
            .find(|e| e.agent == "claude_code")
            .unwrap();
        assert_eq!(cc.content_hash.as_deref(), Some("h-cc"));

        let cdx = group.entries.iter().find(|e| e.agent == "codex").unwrap();
        assert_eq!(cdx.content_hash.as_deref(), Some("h-cdx-different"));
    }

    #[test]
    fn missing_hash_is_preserved_as_none() {
        let skills = vec![
            info("Draft", "claude_code", "Claude Code", None),
            info("draft", "cursor", "Cursor", Some("h")),
        ];

        let index = build_presence_index(skills);

        assert_eq!(index.len(), 1);
        let cc = index[0]
            .entries
            .iter()
            .find(|e| e.agent == "claude_code")
            .unwrap();
        assert!(cc.content_hash.is_none());
    }

    #[test]
    fn duplicate_same_agent_collapses_to_single_entry_keeping_last() {
        // A recursive scan could surface the same canonical dir twice; the
        // grouping must not double-count an agent for one name.
        let skills = vec![
            info("Solo", "cursor", "Cursor", Some("h1")),
            info("Solo", "cursor", "Cursor", Some("h2")),
        ];

        let index = build_presence_index(skills);

        assert_eq!(index.len(), 1);
        assert_eq!(index[0].entries.len(), 1);
        // The last write wins for the entry, but it is still a single row.
        assert_eq!(index[0].entries[0].agent, "cursor");
    }
}
