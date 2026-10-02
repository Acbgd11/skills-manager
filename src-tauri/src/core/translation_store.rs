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

/// Content hash of a whole document body. Body translations are keyed by this
/// rather than by a name/description fingerprint, so editing a skill's text
/// without renaming it still misses the cache and re-translates.
pub fn body_hash(content: &str) -> String {
    use sha2::{Digest, Sha256};
    let mut hasher = Sha256::new();
    hasher.update(content.as_bytes());
    format!("{:x}", hasher.finalize())
}

/// Prompt for translating one whole skill document. Asks for the markdown back
/// verbatim (headings, lists, code fences, links, frontmatter) with only the
/// prose in Chinese, so the result renders identically to the original.
pub fn build_body_prompt(content: &str) -> String {
    format!(
        "你是技术文档本地化助手。把下面这份 Markdown 技能文档翻译成简体中文。\n\
规则：\n\
1. 保留全部 Markdown 结构：标题层级、列表、表格、代码块、链接、图片、引用、分隔线原样不动。\n\
2. 代码块内的代码、命令、路径、变量名、配置项一律不译，保持原样。\n\
3. YAML frontmatter（--- 包裹的部分）只翻译可读的值（如 description），键名不动；\
若其中 name/键名是英文标识符则保持原样。\n\
4. 专业术语首次出现可用「中文（English）」形式。\n\
5. 只输出翻译后的完整 Markdown 文档，不要任何解释、不要用代码块把整篇包起来。\n\n\
原文：\n{content}"
    )
}

/// Strips the wrapper a model sometimes adds around a whole document. Only
/// unwraps when the *entire* reply is one fenced block — a document that itself
/// contains fenced code keeps its own fences and is returned untouched.
pub fn strip_code_fence(raw: &str) -> String {
    let text = raw.trim();
    if !text.starts_with("```") {
        return text.to_string();
    }
    let Some(first_newline) = text.find('\n') else {
        return text.to_string();
    };
    let rest = &text[first_newline + 1..];
    let trimmed = rest.trim_end();
    let Some(opener) = text[..first_newline].chars().nth(3) else {
        return text.to_string();
    };
    // The opening fence must be closing with the same language tag (if any).
    if !trimmed.ends_with("```") {
        return text.to_string();
    }
    let inner = &trimmed[..trimmed.len() - 3];
    // A `` ` `` right before the closing fence means the fence we saw was part
    // of the document's own code, not a wrapper.
    if inner.trim_end().ends_with('`') {
        return text.to_string();
    }
    let _ = opener;
    inner.trim().to_string()
}

/// Target characters per chunk. The model's output ceiling is 8000 tokens and
/// Chinese costs roughly one token per character, so 4000 leaves headroom for
/// a translation that runs longer than its source.
pub const BODY_CHUNK_CHARS: usize = 4000;

/// Split a Markdown document into chunks small enough to translate without
/// hitting the output limit.
///
/// Splits on blank lines — Markdown's block boundary — so a chunk always ends
/// between blocks, never mid-paragraph. Fenced code blocks are treated as
/// indivisible: a fence that opens is never split from its close, because a
/// half-translated code block is worse than an oversized chunk. A single block
/// longer than `max_chars` is emitted whole rather than cut.
pub fn split_markdown_chunks(content: &str, max_chars: usize) -> Vec<String> {
    if content.trim().is_empty() {
        return Vec::new();
    }
    if max_chars == 0 || content.chars().count() <= max_chars {
        return vec![content.trim().to_string()];
    }

    let mut chunks: Vec<String> = Vec::new();
    let mut current = String::new();
    let mut in_fence = false;
    // Blocks are separated by blank lines; accumulate whole blocks.
    let mut block = String::new();

    let flush_block = |block: &mut String, current: &mut String, chunks: &mut Vec<String>| {
        if block.trim().is_empty() {
            block.clear();
            return;
        }
        let would_be = current.chars().count() + if current.is_empty() { 0 } else { 2 }
            + block.trim_end().chars().count();
        if !current.is_empty() && would_be > max_chars {
            chunks.push(current.trim().to_string());
            current.clear();
        }
        if !current.is_empty() {
            current.push_str("\n\n");
        }
        current.push_str(block.trim_end());
        block.clear();
    };

    for line in content.lines() {
        let trimmed = line.trim_start();
        let is_fence = trimmed.starts_with("```") || trimmed.starts_with("~~~");
        if is_fence {
            in_fence = !in_fence;
        }
        // A blank line outside a fence ends the current block.
        if line.trim().is_empty() && !in_fence {
            flush_block(&mut block, &mut current, &mut chunks);
            continue;
        }
        if !block.is_empty() {
            block.push('\n');
        }
        block.push_str(line);
    }
    flush_block(&mut block, &mut current, &mut chunks);
    if !current.trim().is_empty() {
        chunks.push(current.trim().to_string());
    }

    chunks
}

/// Where a chunk sits in the whole document, for the progress message.
pub fn chunk_count(content: &str, max_chars: usize) -> usize {
    split_markdown_chunks(content, max_chars).len()
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
    fn body_translation_roundtrip_and_clear() {
        let tmp = tempfile::tempdir().unwrap();
        let store = SkillStore::new(&tmp.path().join("skills.db")).unwrap();
        let hash = body_hash("# Title\n\nBody.");

        assert!(store.get_body_translation(&hash).unwrap().is_none());

        store
            .upsert_body_translation(&hash, "SKILL.md", "# 标题\n\n正文。", "m1", "2026-10-02T00:00:00Z")
            .unwrap();
        assert_eq!(
            store.get_body_translation(&hash).unwrap().as_deref(),
            Some("# 标题\n\n正文。")
        );

        // Upsert overwrites the same content hash in place.
        store
            .upsert_body_translation(&hash, "SKILL.md", "# 标题二", "m2", "2026-10-02T01:00:00Z")
            .unwrap();
        assert_eq!(
            store.get_body_translation(&hash).unwrap().as_deref(),
            Some("# 标题二")
        );

        // A different body is a different key and stays missing.
        assert!(store
            .get_body_translation(&body_hash("something else"))
            .unwrap()
            .is_none());

        // Clearing the name/description cache clears bodies too.
        store.clear_translations().unwrap();
        assert!(store.get_body_translation(&hash).unwrap().is_none());
    }

    #[test]
    fn pruning_keeps_entries_whose_source_cannot_be_found() {
        // The pruner must not delete on a guess: an unknown source path (or a
        // test dir with no central library) leaves the cache untouched.
        let tmp = tempfile::tempdir().unwrap();
        let store = SkillStore::new(&tmp.path().join("skills.db")).unwrap();
        let hash = body_hash("some document");
        store
            .upsert_body_translation(&hash, "no-such-skill", "短", "m", "2026-10-03T00:00:00Z")
            .unwrap();

        let dropped = store.drop_truncated_body_translations(0.8).unwrap();
        assert_eq!(dropped, 0);
        assert!(store.get_body_translation(&hash).unwrap().is_some());
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

    #[test]
    fn body_hash_is_stable_and_content_sensitive() {
        assert_eq!(body_hash("# Title\ntext"), body_hash("# Title\ntext"));
        assert_ne!(body_hash("# Title\ntext"), body_hash("# Title\ntext!"));
        assert_ne!(body_hash(""), body_hash("\n"));
    }

    #[test]
    fn strip_code_fence_unwraps_a_whole_document_wrapper() {
        let raw = "```markdown\n# 标题\n\n正文。\n```";
        assert_eq!(strip_code_fence(raw), "# 标题\n\n正文。");
    }

    #[test]
    fn strip_code_fence_keeps_documents_that_contain_code() {
        // The reply is not a single wrapper: it carries its own fenced block.
        let raw = "# 标题\n\n```bash\nls -la\n```\n";
        assert_eq!(strip_code_fence(raw), raw.trim());
    }

    #[test]
    fn strip_code_fence_leaves_unfenced_text_untouched() {
        assert_eq!(strip_code_fence("  纯文本  "), "纯文本");
        assert_eq!(strip_code_fence("```\nunclosed"), "```\nunclosed");
    }

    #[test]
    fn body_prompt_carries_rules_and_original_text() {
        let prompt = build_body_prompt("# Title\n\nBody text.");
        assert!(prompt.contains("# Title\n\nBody text."));
        assert!(prompt.contains("Markdown"));
    }

    #[test]
    fn chunker_keeps_short_documents_whole() {
        let short = "# Title\n\nOne paragraph.";
        assert_eq!(split_markdown_chunks(short, 4000), vec![short.to_string()]);
        assert!(split_markdown_chunks("   ", 4000).is_empty());
        assert!(split_markdown_chunks("", 4000).is_empty());
    }

    #[test]
    fn chunker_splits_between_blocks_not_inside_them() {
        // Each block ~60 chars; max 100 forces a split, but only at blank lines.
        let content = "aaaaaaaaaa bbbbbbbbbb cccccccccc dddddddddd eeeeeeeeee\n\n\
                       ffffffffff gggggggggg hhhhhhhhhh iiiiiiiiii jjjjjjjjjj\n\n\
                       kkkkkkkkkk llllllllll mmmmmmmmmm nnnnnnnnnn oooooooooo";
        let chunks = split_markdown_chunks(content, 100);
        assert!(chunks.len() >= 3, "expected several chunks, got {chunks:?}");
        for c in &chunks {
            // No chunk may start or end mid-sentence for these whole blocks.
            assert!(!c.trim().is_empty());
        }
        // Reassembling restores every block in order.
        let joined = chunks.join("\n\n");
        for token in ["aaaaaaaaaa", "jjjjjjjjjj", "oooooooooo"] {
            assert!(joined.contains(token), "lost {token}");
        }
    }

    #[test]
    fn chunker_never_splits_inside_a_fenced_code_block() {
        // The fence is longer than the limit on its own; it must stay in one piece.
        let code = format!("```rust\n{}\n```", "let x = 1;\n".repeat(40));
        let content = format!("# Title\n\nbefore\n\n{code}\n\nafter");
        let chunks = split_markdown_chunks(&content, 50);

        let with_fence: Vec<_> = chunks.iter().filter(|c| c.contains("```")).collect();
        assert_eq!(with_fence.len(), 1, "fence was split across chunks: {chunks:#?}");
        let fenced = with_fence[0];
        assert_eq!(fenced.matches("```").count(), 2, "fence opened without closing: {fenced}");
        assert!(fenced.contains("let x = 1;"));
    }

    #[test]
    fn chunker_emits_an_oversized_block_whole() {
        let huge = "x".repeat(500);
        let content = format!("{huge}\n\nsmall");
        let chunks = split_markdown_chunks(&content, 100);
        assert!(chunks.iter().any(|c| c.chars().count() >= 500));
    }

    #[test]
    fn chunker_handles_an_unclosed_fence() {
        // Malformed input must still terminate and not panic.
        let content = format!("# T\n\n```\n{}\n\nmore text here", "line\n".repeat(60));
        let chunks = split_markdown_chunks(&content, 60);
        assert!(!chunks.is_empty());
        let joined = chunks.join("\n\n");
        assert!(joined.contains("more text here"));
    }

    #[test]
    fn chunker_chunk_count_matches_split() {
        let content = format!("{}\n\n{}", "a".repeat(60), "b".repeat(60));
        assert_eq!(
            chunk_count(&content, 100),
            split_markdown_chunks(&content, 100).len()
        );
    }
}
