//! Translation cache: records plus (in `skill_store`) their persistence.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TranslationRecord {
    pub fingerprint: String,
    pub kind: String,
    pub source_name: String,
    pub zh_name: String,
    pub zh_description: String,
    pub model: String,
    pub created_at: String,
}

/// Items per API call. 20 keeps prompts small and failures cheap to retry.
pub const BATCH_SIZE: usize = 20;

#[derive(Debug, Clone, PartialEq)]
pub struct TranslationInput {
    pub fingerprint: String,
    pub kind: String,
    pub name: String,
    pub description: Option<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct TranslationOutput {
    pub fingerprint: String,
    pub zh_name: String,
    pub zh_description: String,
}

/// Content fingerprint: any change to the source text yields a new fingerprint,
/// so a cached translation is only reused while the source is byte-identical.
pub fn fingerprint(kind: &str, name: &str, description: Option<&str>) -> String {
    use sha2::{Digest, Sha256};
    let mut hasher = Sha256::new();
    hasher.update(kind.as_bytes());
    hasher.update(b"\x00");
    hasher.update(name.as_bytes());
    hasher.update(b"\x00");
    hasher.update(description.unwrap_or("").as_bytes());
    format!("{:x}", hasher.finalize())
}

pub fn plan_batches(inputs: Vec<TranslationInput>, batch_size: usize) -> Vec<Vec<TranslationInput>> {
    if batch_size == 0 {
        return Vec::new();
    }
    inputs.chunks(batch_size).map(|c| c.to_vec()).collect()
}

/// Tolerant parser for the model's reply: accepts a bare JSON array, one wrapped
/// in ```json fences, or an array embedded in surrounding prose. Items are
/// matched back to the batch by their `i` index; malformed or out-of-range
/// items are skipped rather than failing the batch.
pub fn parse_translation_response(raw: &str, batch: &[TranslationInput]) -> Vec<TranslationOutput> {
    let Some(array_text) = extract_json_array(raw) else {
        return Vec::new();
    };
    let Ok(value) = serde_json::from_str::<serde_json::Value>(&array_text) else {
        return Vec::new();
    };
    let Some(items) = value.as_array() else {
        return Vec::new();
    };

    let mut out = Vec::new();
    for item in items {
        let Some(index) = item.get("i").and_then(|v| v.as_u64()) else {
            continue;
        };
        let Some(source) = batch.get(index as usize) else {
            continue;
        };
        let Some(zh_name) = item.get("zh_name").and_then(|v| v.as_str()) else {
            continue;
        };
        let Some(zh_description) = item.get("zh_description").and_then(|v| v.as_str()) else {
            continue;
        };
        if zh_name.trim().is_empty() || zh_description.trim().is_empty() {
            continue;
        }
        out.push(TranslationOutput {
            fingerprint: source.fingerprint.clone(),
            zh_name: zh_name.trim().to_string(),
            zh_description: zh_description.trim().to_string(),
        });
    }
    out
}

/// First top-level `[...]` span in the text, respecting the fences models add.
fn extract_json_array(raw: &str) -> Option<String> {
    let start = raw.find('[')?;
    let mut depth = 0usize;
    let mut in_string = false;
    let mut escape = false;
    for (offset, ch) in raw[start..].char_indices() {
        if in_string {
            if escape {
                escape = false;
            } else if ch == '\\' {
                escape = true;
            } else if ch == '"' {
                in_string = false;
            }
            continue;
        }
        match ch {
            '"' => in_string = true,
            '[' => depth += 1,
            ']' => {
                depth -= 1;
                if depth == 0 {
                    return Some(raw[start..start + offset + ch.len_utf8()].to_string());
                }
            }
            _ => {}
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::skill_store::SkillStore;

    fn record(fp: &str) -> TranslationRecord {
        TranslationRecord {
            fingerprint: fp.into(),
            kind: "plugin_skill".into(),
            source_name: "brainstorming".into(),
            zh_name: "头脑风暴".into(),
            zh_description: "写代码前的需求与设计探索".into(),
            model: "test-model".into(),
            created_at: "2026-10-01T00:00:00Z".into(),
        }
    }

    #[test]
    fn upsert_get_clear_roundtrip() {
        let tmp = tempfile::tempdir().unwrap();
        let store = SkillStore::new(&tmp.path().join("skills.db")).unwrap();

        assert!(store.get_translations().unwrap().is_empty());

        store.upsert_translations(&[record("fp1"), record("fp2")]).unwrap();
        let map = store.get_translations().unwrap();
        assert_eq!(map.len(), 2);
        assert_eq!(map["fp1"].zh_name, "头脑风暴");

        // Upsert overwrites by fingerprint
        let mut updated = record("fp1");
        updated.zh_name = "脑暴".into();
        store.upsert_translations(&[updated]).unwrap();
        assert_eq!(store.get_translations().unwrap()["fp1"].zh_name, "脑暴");
        assert_eq!(store.get_translations().unwrap().len(), 2);

        store.clear_translations().unwrap();
        assert!(store.get_translations().unwrap().is_empty());
    }

    #[test]
    fn fingerprint_is_stable_and_content_sensitive() {
        let a = fingerprint("plugin_skill", "brainstorming", Some("Design first"));
        let b = fingerprint("plugin_skill", "brainstorming", Some("Design first"));
        assert_eq!(a, b);
        assert_ne!(a, fingerprint("plugin_skill", "brainstorming", Some("Design second")));
        assert_ne!(a, fingerprint("plugin_skill", "other", Some("Design first")));
        assert_ne!(a, fingerprint("official_skill", "brainstorming", Some("Design first")));
        assert_eq!(a.len(), 64); // sha256 hex
    }

    fn input(i: usize) -> TranslationInput {
        TranslationInput {
            fingerprint: format!("fp{i}"),
            kind: "plugin_skill".into(),
            name: format!("skill-{i}"),
            description: Some(format!("desc {i}")),
        }
    }

    #[test]
    fn plan_batches_splits_every_twenty() {
        let items: Vec<_> = (0..195).map(input).collect();
        let batches = plan_batches(items, BATCH_SIZE);
        assert_eq!(batches.len(), 10);
        assert_eq!(batches[0].len(), 20);
        assert_eq!(batches[9].len(), 15);
        assert!(plan_batches(vec![], BATCH_SIZE).is_empty());
    }

    #[test]
    fn parses_plain_json_array() {
        let batch = vec![input(0), input(1)];
        let raw = r#"[{"i":0,"zh_name":"甲","zh_description":"第一"},{"i":1,"zh_name":"乙","zh_description":"第二"}]"#;
        let out = parse_translation_response(raw, &batch);
        assert_eq!(out.len(), 2);
        assert_eq!(out[0].fingerprint, "fp0");
        assert_eq!(out[1].zh_name, "乙");
    }

    #[test]
    fn parses_fenced_json_with_surrounding_prose() {
        let batch = vec![input(0)];
        let raw = "好的，这是结果：\n```json\n[{\"i\":0,\"zh_name\":\"甲\",\"zh_description\":\"第一\"}]\n```\n以上。";
        let out = parse_translation_response(raw, &batch);
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].fingerprint, "fp0");
    }

    #[test]
    fn parses_bracket_inside_string_value() {
        let batch = vec![input(0)];
        let raw = r#"[{"i":0,"zh_name":"a]b","zh_description":"含右括号的说明"}]"#;
        let out = parse_translation_response(raw, &batch);
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].zh_name, "a]b");
    }

    #[test]
    fn skips_invalid_items_but_keeps_valid_ones() {
        let batch = vec![input(0), input(1), input(2)];
        // index 1 missing zh_name, index 9 out of range, index 2 fine
        let raw = r#"[{"i":0,"zh_name":"甲","zh_description":"一"},{"i":1,"zh_description":"缺名"},{"i":9,"zh_name":"X","zh_description":"Y"},{"i":2,"zh_name":"丙","zh_description":"三"}]"#;
        let out = parse_translation_response(raw, &batch);
        let fps: Vec<_> = out.iter().map(|o| o.fingerprint.as_str()).collect();
        assert_eq!(fps, vec!["fp0", "fp2"]);
    }

    #[test]
    fn returns_empty_on_garbage() {
        let batch = vec![input(0)];
        assert!(parse_translation_response("完全不是 JSON", &batch).is_empty());
        assert!(parse_translation_response("", &batch).is_empty());
    }
}