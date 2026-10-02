//! Translation command layer: settings, status, one-click translation,
//! connection test. Keys live in the OS keychain; cached translations live
//! in SQLite. Batch progress is pushed to the frontend via `translation-progress`.

use std::collections::HashMap;
use std::sync::Arc;

use serde::Serialize;
use tauri::{AppHandle, Emitter, State};

use crate::core::error::AppError;
use crate::core::plugin_scanner;
use crate::core::skill_store::SkillStore;
use crate::core::tool_adapters;
use crate::core::translation_store::{self, TranslationInput, TranslationRecord, BATCH_SIZE};
use crate::core::translator::{self, ApiFormat, HttpBackend, TranslationBackend, TranslationConfig};

const KEYRING_SERVICE: &str = "skills-manager-translation";
const KEYRING_ACCOUNT: &str = "api-key";
const DEFAULT_ENDPOINT: &str = "http://127.0.0.1:8789";

/// Held for the duration of a one-click translation run so two overlapping
/// clicks cannot both start — the UI has never offered a single entry point,
/// and duplicate runs cost real money.
static TRANSLATE_JOB_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

#[derive(Debug, Serialize)]
pub struct TranslationSettingsDto {
    pub endpoint: String,
    pub model: String,
    pub format: String,
    pub has_key: bool,
}

#[derive(Debug, Serialize)]
pub struct TranslationStatusDto {
    pub total: usize,
    pub pending: usize,
}

#[derive(Debug, Serialize)]
pub struct TranslateReportDto {
    pub translated: usize,
    pub failed_batches: usize,
    pub pending: usize,
}

fn keyring_entry() -> Result<keyring::Entry, AppError> {
    keyring::Entry::new(KEYRING_SERVICE, KEYRING_ACCOUNT)
        .map_err(|e| AppError::internal(e.to_string()))
}

/// Load the API key from the OS keychain.
///
/// `NoEntry` (no stored key) → `Ok(None)`: a legitimate "not configured yet"
/// state, surfaced to the user as a missing-key hint. Any other keychain
/// failure (service unavailable, access denied, corrupted entry) → `Err`:
/// a real infrastructure problem that must NOT be masked as "未配置", which
/// would mislead the user into re-entering a key that won't fix anything.
/// The error message carries the underlying cause but never the key itself.
fn load_api_key() -> Result<Option<String>, AppError> {
    let entry = keyring_entry()?;
    match entry.get_password() {
        Ok(v) if !v.trim().is_empty() => Ok(Some(v)),
        Ok(_) => Ok(None),
        Err(keyring::Error::NoEntry) => Ok(None),
        Err(e) => Err(AppError::internal(format!("读取翻译密钥失败：{e}"))),
    }
}

fn store_api_key(key: &str) -> Result<(), AppError> {
    keyring_entry()?
        .set_password(key)
        .map_err(|e| AppError::internal(e.to_string()))
}

fn read_config(store: &SkillStore) -> TranslationConfig {
    let endpoint = store
        .get_setting("translation_endpoint")
        .ok()
        .flatten()
        .filter(|s| !s.trim().is_empty())
        .unwrap_or_else(|| DEFAULT_ENDPOINT.to_string());
    let model = store
        .get_setting("translation_model")
        .ok()
        .flatten()
        .unwrap_or_default();
    let format = match store
        .get_setting("translation_format")
        .ok()
        .flatten()
        .as_deref()
    {
        Some("openai") => ApiFormat::OpenAi,
        _ => ApiFormat::Anthropic,
    };
    TranslationConfig {
        endpoint,
        model,
        format,
    }
}

fn validate_config(cfg: &TranslationConfig) -> Result<(), AppError> {
    if cfg.endpoint.trim().is_empty() {
        return Err(AppError::invalid_input("翻译接口地址未配置"));
    }
    if cfg.model.trim().is_empty() {
        return Err(AppError::invalid_input("翻译模型未填写"));
    }
    Ok(())
}

/// Every translatable read-only item: plugin skills, official skills, plugin
/// group descriptions, plus each installed+enabled agent's global local skills.
/// Returns the flat item list plus the pre-planned batches.
///
/// `local_skill` entries mirror what `get_global_local_skills` reads via
/// `read_agent_local_skills` — same adapter enumeration, same recursive scan —
/// so a fingerprint computed here matches one computed there byte-for-byte.
/// Only skills carrying a non-empty description are included: the existing
/// convention for plugin/official items, and translating a name-only card adds
/// no value the user can read.
fn collect_inputs(
    store: &SkillStore,
    config_dir: &std::path::Path,
) -> (Vec<TranslationInput>, Vec<Vec<TranslationInput>>) {
    let mut items = Vec::new();
    let mut seen = std::collections::HashSet::new();
    for group in plugin_scanner::scan_plugin_skills(config_dir) {
        if let Some(desc) = group.description.as_deref().filter(|d| !d.trim().is_empty()) {
            let fp = translation_store::fingerprint("plugin_group", &group.plugin, Some(desc));
            items.push(TranslationInput {
                fingerprint: fp,
                kind: "plugin_group".into(),
                name: group.plugin.clone(),
                description: Some(desc.to_string()),
            });
        }
        for skill in group.skills {
            let fp =
                translation_store::fingerprint("plugin_skill", &skill.name, skill.description.as_deref());
            items.push(TranslationInput {
                fingerprint: fp,
                kind: "plugin_skill".into(),
                name: skill.name,
                description: skill.description,
            });
        }
    }
    for skill in plugin_scanner::scan_official_skills(config_dir) {
        let fp =
            translation_store::fingerprint("official_skill", &skill.name, skill.description.as_deref());
        items.push(TranslationInput {
            fingerprint: fp,
            kind: "official_skill".into(),
            name: skill.name,
            description: skill.description,
        });
    }
    // Each installed+enabled agent's global local skills. Read through the
    // shared `agent_workspace::read_agent_local_skills` helper — the exact
    // code path the workspace cards render — so fingerprints line up with
    // what the fill side computes, byte-for-byte.
    for adapter in tool_adapters::enabled_installed_adapters(store) {
        for skill in crate::commands::agent_workspace::read_agent_local_skills(&adapter) {
            let Some(desc) = skill.description.as_deref().filter(|d| !d.trim().is_empty()) else {
                continue;
            };
            let fp = translation_store::fingerprint("local_skill", &skill.name, Some(desc));
            items.push(TranslationInput {
                fingerprint: fp,
                kind: "local_skill".into(),
                name: skill.name,
                description: Some(desc.to_string()),
            });
        }
    }
    // The same skill can be present for several agents, or appear both as a
    // plugin skill and a local one, yielding identical fingerprints. Keeping
    // one copy per fingerprint stops the same text being paid for twice.
    let mut deduped: Vec<TranslationInput> = Vec::with_capacity(items.len());
    for item in items {
        if seen.insert(item.fingerprint.clone()) {
            deduped.push(item);
        }
    }

    let batches = translation_store::plan_batches(deduped.clone(), BATCH_SIZE);
    (deduped, batches)
}

/// Only the batches that still contain untranslated items (Review Focus #5:
/// a second click with everything cached must produce zero batches → zero calls).
fn pending_batches(
    batches: Vec<Vec<TranslationInput>>,
    cached: &HashMap<String, TranslationRecord>,
) -> Vec<Vec<TranslationInput>> {
    batches
        .into_iter()
        .map(|b| {
            b.into_iter()
                .filter(|i| !cached.contains_key(&i.fingerprint))
                .collect::<Vec<_>>()
        })
        .filter(|b| !b.is_empty())
        .collect()
}

/// How many items still need translating.
///
/// A database read failure is propagated rather than treated as an empty
/// cache: reporting "everything is pending" on a transient read error would
/// send the user to re-translate — and re-pay for — text already cached.
pub fn count_pending(
    store: &SkillStore,
    items: &[TranslationInput],
) -> Result<usize, AppError> {
    let cached = store.get_translations().map_err(AppError::db)?;
    Ok(items
        .iter()
        .filter(|i| !cached.contains_key(&i.fingerprint))
        .count())
}

#[tauri::command]
pub async fn get_translation_settings(
    store: State<'_, Arc<SkillStore>>,
) -> Result<TranslationSettingsDto, AppError> {
    let cfg = read_config(store.inner());
    Ok(TranslationSettingsDto {
        endpoint: cfg.endpoint,
        model: cfg.model,
        format: match cfg.format {
            ApiFormat::Anthropic => "anthropic".into(),
            ApiFormat::OpenAi => "openai".into(),
        },
        has_key: load_api_key()?.is_some(),
    })
}

#[tauri::command]
pub async fn set_translation_settings(
    store: State<'_, Arc<SkillStore>>,
    endpoint: String,
    model: String,
    format: String,
    api_key: Option<String>,
) -> Result<(), AppError> {
    let store = store.inner().clone();
    store
        .set_setting("translation_endpoint", endpoint.trim())
        .map_err(AppError::db)?;
    store
        .set_setting("translation_model", model.trim())
        .map_err(AppError::db)?;
    store
        .set_setting(
            "translation_format",
            if format == "openai" { "openai" } else { "anthropic" },
        )
        .map_err(AppError::db)?;
    if let Some(key) = api_key.filter(|k| !k.trim().is_empty()) {
        store_api_key(key.trim())?;
    }
    Ok(())
}

#[tauri::command]
pub async fn test_translation_connection(
    store: State<'_, Arc<SkillStore>>,
) -> Result<String, AppError> {
    let cfg = read_config(store.inner());
    validate_config(&cfg)?;
    let key = load_api_key()?
        .ok_or_else(|| AppError::invalid_input("翻译密钥未配置"))?;
    let spec = translator::build_request(&cfg, &key, "请回复：连接成功");
    let body = HttpBackend.complete(&spec)?;
    Ok(translator::extract_reply_text(&body))
}

#[tauri::command]
pub async fn get_translation_status(
    store: State<'_, Arc<SkillStore>>,
) -> Result<TranslationStatusDto, AppError> {
    let store = store.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        let (items, _) = collect_inputs(&store, &plugin_scanner::claude_config_dir());
        Ok(TranslationStatusDto {
            total: items.len(),
            pending: count_pending(&store, &items)?,
        })
    })
    .await?
}

#[tauri::command]
pub async fn translate_skills(
    app: AppHandle,
    store: State<'_, Arc<SkillStore>>,
) -> Result<TranslateReportDto, AppError> {
    let store = store.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        let cfg = read_config(&store);
        validate_config(&cfg)?;
        let key = load_api_key()?
            .ok_or_else(|| AppError::invalid_input("翻译密钥未配置"))?;

        // Two clicks (the page has always had more than one entry point) used
        // to start two identical runs, paying for the same text twice. Hold the
        // lock for the whole job; a second caller is told a run is in flight.
        let _guard = TRANSLATE_JOB_LOCK.try_lock().map_err(|_| {
            AppError::invalid_input("已有翻译任务正在进行，请等它跑完")
        })?;

        let (items, all_batches) = collect_inputs(&store, &plugin_scanner::claude_config_dir());
        let cached = store.get_translations().map_err(AppError::db)?;
        let pending = pending_batches(all_batches, &cached);

        let backend = HttpBackend;
        let mut translated_total = 0usize;
        let mut failed_batches = 0usize;
        let model = cfg.model.clone();
        let created_at = chrono::Utc::now().to_rfc3339();
        let total = pending.len();

        // Emit an initial progress event so the frontend can show the true
        // batch count before the first (potentially slow) batch returns.
        let _ = app.emit(
            "translation-progress",
            serde_json::json!({ "done": 0, "total": total }),
        );

        for (index, batch) in pending.into_iter().enumerate() {
            let done = index + 1;
            let report = translator::run_translation(
                vec![batch.clone()],
                &cfg,
                &key,
                &backend,
                &mut |_batch_done, _batch_total| {
                    // run_translation reports per-single-batch counters (always
                    // 1/1 here); re-emit with the outer pending-batch progress
                    // so the frontend sees done = batches completed so far.
                    let _ = app.emit(
                        "translation-progress",
                        serde_json::json!({ "done": done, "total": total }),
                    );
                },
            );
            failed_batches += report.failed_batches;
            if !report.translated.is_empty() {
                let records: Vec<TranslationRecord> = report
                    .translated
                    .iter()
                    .map(|o| {
                        let src = batch.iter().find(|i| i.fingerprint == o.fingerprint);
                        TranslationRecord {
                            fingerprint: o.fingerprint.clone(),
                            kind: src.map(|s| s.kind.clone()).unwrap_or_default(),
                            source_name: src.map(|s| s.name.clone()).unwrap_or_default(),
                            zh_name: o.zh_name.clone(),
                            zh_description: o.zh_description.clone(),
                            model: model.clone(),
                            created_at: created_at.clone(),
                        }
                    })
                    .collect();
                store.upsert_translations(&records).map_err(AppError::db)?;
                translated_total += records.len();
            }
        }

        let remaining = count_pending(&store, &items)?;
        Ok(TranslateReportDto {
            translated: translated_total,
            failed_batches,
            pending: remaining,
        })
    })
    .await?
}

#[tauri::command]
pub async fn clear_translations(
    store: State<'_, Arc<SkillStore>>,
) -> Result<(), AppError> {
    store.inner().clear_translations().map_err(AppError::db)
}

/// Send one prompt to the configured endpoint and return the model's reply text
/// together with whether the output limit cut it short.
fn call_model(cfg: &TranslationConfig, prompt: &str) -> Result<(String, bool), AppError> {
    let key = load_api_key()?.ok_or_else(|| AppError::invalid_input("翻译密钥未配置"))?;
    let spec = translator::build_request(cfg, &key, prompt);
    let body = HttpBackend.complete(&spec)?;
    Ok((
        translator::extract_reply_text(&body),
        translator::reply_was_truncated(&body),
    ))
}

/// Translate one skill's whole document body. `content` is the exact text the
/// detail view is showing, so the caller and this command agree on the cache
/// key. Returns `None` when the body is empty, a chunk failed, or the model
/// was cut off — an incomplete result is never cached, because a cached body
/// is served forever and would silently lose the untranslated remainder.
///
/// Long documents are split into chunks (see `split_markdown_chunks`) and sent
/// one request each, then rejoined in order; a single oversized request used to
/// be truncated at the output limit and cached as if complete.
///
/// Read-only: the result lands in `skill_body_translations` and is never
/// written back into the skill's own file.
#[tauri::command]
pub async fn translate_skill_body(
    app: AppHandle,
    store: State<'_, Arc<SkillStore>>,
    content: String,
    source_path: String,
) -> Result<Option<String>, AppError> {
    let store = store.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        if content.trim().is_empty() {
            return Ok(None);
        }
        let hash = translation_store::body_hash(&content);
        if let Some(cached) = store.get_body_translation(&hash).map_err(AppError::db)? {
            return Ok(Some(cached));
        }
        let cfg = read_config(&store);
        validate_config(&cfg)?;

        let chunks = translation_store::split_markdown_chunks(
            &content,
            translation_store::BODY_CHUNK_CHARS,
        );
        if chunks.is_empty() {
            return Ok(None);
        }
        let total = chunks.len();

        let mut parts: Vec<String> = Vec::with_capacity(total);
        for (index, chunk) in chunks.iter().enumerate() {
            let _ = app.emit(
                "body-translation-progress",
                serde_json::json!({ "done": index, "total": total }),
            );
            let prompt = translation_store::build_body_prompt(chunk);
            let (reply, truncated) = match call_model(&cfg, &prompt) {
                Ok(v) => v,
                Err(e) => {
                    log::warn!("body translation chunk {}/{} failed: {e}", index + 1, total);
                    return Ok(None);
                }
            };
            // A truncated chunk would silently drop the tail of that section,
            // and the rejoined document would be cached as complete.
            if truncated {
                log::warn!(
                    "body translation chunk {}/{} hit the output limit; refusing to cache",
                    index + 1,
                    total
                );
                return Ok(None);
            }
            let part = translation_store::strip_code_fence(&reply);
            if part.trim().is_empty() {
                log::warn!("body translation chunk {}/{} came back empty", index + 1, total);
                return Ok(None);
            }
            parts.push(part);
        }
        let _ = app.emit(
            "body-translation-progress",
            serde_json::json!({ "done": total, "total": total }),
        );

        let translated = parts.join("\n\n");
        let created_at = chrono::Utc::now().to_rfc3339();
        store
            .upsert_body_translation(&hash, &source_path, &translated, &cfg.model, &created_at)
            .map_err(AppError::db)?;
        Ok(Some(translated))
    })
    .await?
}

/// Whether a translated body is already cached for this exact content.
#[tauri::command]
pub async fn get_cached_body_translation(
    store: State<'_, Arc<SkillStore>>,
    content: String,
) -> Result<Option<String>, AppError> {
    let store = store.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        if content.trim().is_empty() {
            return Ok(None);
        }
        let hash = translation_store::body_hash(&content);
        store.get_body_translation(&hash).map_err(AppError::db)
    })
    .await?
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::skill_store::SkillStore;
    use crate::core::translation_store::fingerprint;

    fn input_item(kind: &str, name: &str, description: Option<&str>) -> TranslationInput {
        TranslationInput {
            fingerprint: fingerprint(kind, name, description),
            kind: kind.into(),
            name: name.into(),
            description: description.map(str::to_string),
        }
    }

    #[test]
    fn identical_items_collapse_to_one_batch_entry() {
        // The same skill installed for two agents shares a fingerprint; sending
        // it twice would pay twice for identical text.
        let items = vec![
            input_item("local_skill", "brainstorming", Some("Design first")),
            input_item("local_skill", "brainstorming", Some("Design first")),
            input_item("local_skill", "other", Some("Design first")),
        ];
        let mut seen = std::collections::HashSet::new();
        let deduped: Vec<_> = items
            .into_iter()
            .filter(|i| seen.insert(i.fingerprint.clone()))
            .collect();
        assert_eq!(deduped.len(), 2);
        assert_eq!(deduped[0].name, "brainstorming");
        assert_eq!(deduped[1].name, "other");
    }

    #[test]
    fn a_second_translate_run_is_refused_while_one_holds_the_lock() {
        let first = TRANSLATE_JOB_LOCK.try_lock();
        assert!(first.is_ok(), "first acquisition should succeed");
        let second = TRANSLATE_JOB_LOCK.try_lock();
        assert!(second.is_err(), "second concurrent run must be refused");
        drop(first);
        assert!(
            TRANSLATE_JOB_LOCK.try_lock().is_ok(),
            "lock must be released with the guard"
        );
    }

    #[test]
    fn pending_counts_only_untranslated_items() {
        let tmp = tempfile::tempdir().unwrap();
        let store = SkillStore::new(&tmp.path().join("skills.db")).unwrap();
        let items = vec![
            input_item("plugin_skill", "a", Some("A desc")),
            input_item("plugin_skill", "b", Some("B desc")),
        ];
        assert_eq!(count_pending(&store, &items).unwrap(), 2);

        let done = crate::core::translation_store::TranslationRecord {
            fingerprint: items[0].fingerprint.clone(),
            kind: items[0].kind.clone(),
            source_name: items[0].name.clone(),
            zh_name: "甲".into(),
            zh_description: "一".into(),
            model: "m".into(),
            created_at: "2026-10-01T00:00:00Z".into(),
        };
        store.upsert_translations(&[done]).unwrap();
        assert_eq!(count_pending(&store, &items).unwrap(), 1);
    }

    #[test]
    fn pending_counts_local_skill_items_one_cached_one_not() {
        // Extension scope: local_skill entries flow through the same pending
        // counter. One cached, one not → exactly one pending.
        let tmp = tempfile::tempdir().unwrap();
        let store = SkillStore::new(&tmp.path().join("skills.db")).unwrap();
        let items = vec![
            input_item("local_skill", "codex-skill", Some("Codex local skill")),
            input_item("local_skill", "ds-skill", Some("DeepSeek local skill")),
        ];
        assert_eq!(count_pending(&store, &items).unwrap(), 2);

        let done = crate::core::translation_store::TranslationRecord {
            fingerprint: items[0].fingerprint.clone(),
            kind: items[0].kind.clone(),
            source_name: items[0].name.clone(),
            zh_name: "代码技能".into(),
            zh_description: "Codex 本地技能".into(),
            model: "m".into(),
            created_at: "2026-10-01T00:00:00Z".into(),
        };
        store.upsert_translations(&[done]).unwrap();
        assert_eq!(count_pending(&store, &items).unwrap(), 1);
    }

    #[test]
    fn refuses_without_model_or_endpoint() {
        let cfg = crate::core::translator::TranslationConfig {
            endpoint: "http://127.0.0.1:8789".into(),
            model: "".into(),
            format: crate::core::translator::ApiFormat::Anthropic,
        };
        assert!(validate_config(&cfg).is_err());
    }

    #[test]
    fn second_click_with_everything_cached_produces_no_batches() {
        // Review Focus #5: nothing pending → zero batches → zero API calls.
        let tmp = tempfile::tempdir().unwrap();
        let store = SkillStore::new(&tmp.path().join("skills.db")).unwrap();
        let items = vec![
            input_item("plugin_skill", "a", Some("A desc")),
            input_item("official_skill", "docx", Some("Word")),
        ];
        let batches = crate::core::translation_store::plan_batches(items.clone(), BATCH_SIZE);

        let records: Vec<_> = items
            .iter()
            .map(|i| crate::core::translation_store::TranslationRecord {
                fingerprint: i.fingerprint.clone(),
                kind: i.kind.clone(),
                source_name: i.name.clone(),
                zh_name: "甲".into(),
                zh_description: "一".into(),
                model: "m".into(),
                created_at: "2026-10-01T00:00:00Z".into(),
            })
            .collect();
        store.upsert_translations(&records).unwrap();

        let cached = store.get_translations().unwrap();
        assert!(pending_batches(batches, &cached).is_empty());
    }
}
