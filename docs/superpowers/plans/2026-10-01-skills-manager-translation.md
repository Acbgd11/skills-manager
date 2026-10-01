# Skills Manager 子项目丙 实施计划（双语 + 一键翻译）

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 在 Skills Manager 里加「一键翻译」：把插件技能/官方技能/插件分组说明的中文译名与中文一句话说明批量生成并缓存，界面显示双语。

**Architecture:** 在甲的分支上继续：新增纯逻辑模块（指纹、分批、容错解析）+ 可注入后端的翻译引擎 + SkillStore 上的独立缓存表（迁移 v9）+ 命令层（密钥进系统钥匙串、进度事件）+ 前端双语渲染与设置小节。只读数据一个字节都不写。

**Tech Stack:** Rust（Tauri 2、rusqlite、reqwest blocking、sha2、keyring、serde）＋ React 19 + TS + Vite + Tailwind + react-i18next。

**Spec:** `docs/superpowers/specs/2026-10-01-skills-manager-translation-design.md`

## Global Constraints

- 工作分支：`v2-translation`（从 `v1-plugin-dsh` 的 HEAD 切出）；远程 origin = 用户的 fork（Acbgd11）。
- **只读铁律**：不得写入 `plugins/` 目录任何内容；中文只进本软件自己的缓存表。
- **不新增任何 Rust/npm 依赖**（`reqwest`、`keyring`、`sha2`、`serde`、`tempfile` 均现成）。
- 密钥**只进系统钥匙串**（`keyring`），不得落明文到 DB/日志；日志不得打印密钥。
- 新增 UI 文案三语齐全（`en/zh/zh-TW`），键名一致；组件内不硬编码可见文案。
- `PathBuf::join` 组合路径；Rust `rust-version = "1.77.2"`。
- 每任务必须 commit；提交信息英文、`feat:`/`test:` 前缀。
- 甲的全部测试必须保持全绿（含既有 2 个 Windows 符号链接 1314 失败为已知基线，不得新增失败）。

## Review Focus

1. **未配置/未填模型**：点「翻译」→ 明确提示，不发起任何请求、不崩溃（Task 4 测试）。
2. **模型输出脏**：JSON 带 ```json 围栏、前后混文字、字段缺失 → 容错解析；坏批**整批不落库**（Task 2/3 测试）。
3. **接口故障**：401/超时/网络错误 → 该批失败但**后续批次继续**，汇总准确（Task 3 测试）。
4. **内容变化**：技能说明被改动 → 指纹变化 → 重新列入待翻译，旧缓存不被误用（Task 2 测试）。
5. **重复点击**：第二次点 → pending=0，**不发请求**、不花额度（Task 4 测试）。

---

### Task 1: 缓存表（迁移 v9）+ SkillStore 读写

**Files:**
- Create: `src-tauri/src/core/translation_store.rs`（记录类型 + 读写的测试）
- Modify: `src-tauri/src/core/migrations.rs`（LATEST_VERSION 8→9；match 加 `8 => migrate_v8_to_v9(conn)`；新增建表函数）
- Modify: `src-tauri/src/core/skill_store.rs`（新增 `get_translations` / `upsert_translations` / `clear_translations`）
- Modify: `src-tauri/src/core/mod.rs`（`pub mod translation_store;` 按字母序）

**Interfaces:**
- Consumes: `SkillStore::new(&PathBuf)`（现成）、`SkillStore::{get_setting,set_setting}`（现成）
- Produces:
  - `pub struct TranslationRecord { fingerprint: String, kind: String, source_name: String, zh_name: String, zh_description: String, model: String, created_at: String }`（`#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]`）
  - `SkillStore::get_translations(&self) -> anyhow::Result<std::collections::HashMap<String, TranslationRecord>>`
  - `SkillStore::upsert_translations(&self, records: &[TranslationRecord]) -> anyhow::Result<()>`
  - `SkillStore::clear_translations(&self) -> anyhow::Result<()>`
  - 表：`skill_translations(fingerprint TEXT PRIMARY KEY, kind TEXT NOT NULL, source_name TEXT NOT NULL, zh_name TEXT NOT NULL, zh_description TEXT NOT NULL, model TEXT NOT NULL, created_at TEXT NOT NULL)`

- [ ] **Step 1: 建分支**

```bash
cd /d/CloudMusic/skills-manager
git checkout v1-plugin-dsh && git pull --ff-only 2>/dev/null || true
git checkout -b v2-translation
git branch --show-current   # 预期 v2-translation
```

- [ ] **Step 2: 写失败测试**

在新建的 `core/translation_store.rs` 顶部先放类型，底部放测试：

```rust
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
}
```

并在 `core/mod.rs` 加 `pub mod translation_store;`（字母序）。

- [ ] **Step 3: 运行确认失败**

Run: `cd /d/CloudMusic/skills-manager/src-tauri && cargo test translation_store`
Expected: 编译失败（`get_translations` 不存在）

- [ ] **Step 4: 迁移 v9（migrations.rs）**

- `const LATEST_VERSION: u32 = 8;` → `9`
- `migrate_step` 的 match 增加一行：`8 => migrate_v8_to_v9(conn),`
- 文件末尾新增：

```rust
/// v9: translation cache for the bilingual UI. Independent of skill data —
/// nothing here is ever written back into agent or plugin directories.
fn migrate_v8_to_v9(conn: &Connection) -> Result<()> {
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS skill_translations (
            fingerprint    TEXT PRIMARY KEY,
            kind           TEXT NOT NULL,
            source_name    TEXT NOT NULL,
            zh_name        TEXT NOT NULL,
            zh_description TEXT NOT NULL,
            model          TEXT NOT NULL,
            created_at     TEXT NOT NULL
        );",
    )?;
    Ok(())
}
```

- [ ] **Step 5: SkillStore 读写（skill_store.rs）**

在 `impl SkillStore` 内（`list_audit` 附近）加：

```rust
    /// All cached translations, keyed by content fingerprint.
    pub fn get_translations(
        &self,
    ) -> Result<std::collections::HashMap<String, super::translation_store::TranslationRecord>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT fingerprint, kind, source_name, zh_name, zh_description, model, created_at
             FROM skill_translations",
        )?;
        let rows = stmt.query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                super::translation_store::TranslationRecord {
                    fingerprint: row.get(0)?,
                    kind: row.get(1)?,
                    source_name: row.get(2)?,
                    zh_name: row.get(3)?,
                    zh_description: row.get(4)?,
                    model: row.get(5)?,
                    created_at: row.get(6)?,
                },
            ))
        })?;
        let mut map = std::collections::HashMap::new();
        for row in rows {
            let (fp, rec) = row?;
            map.insert(fp, rec);
        }
        Ok(map)
    }

    /// Insert-or-replace a batch of translations. Callers only pass whole
    /// batches that parsed cleanly; a failed batch must never land here.
    pub fn upsert_translations(
        &self,
        records: &[super::translation_store::TranslationRecord],
    ) -> Result<()> {
        let mut conn = self.conn.lock().unwrap();
        let tx = conn.transaction()?;
        {
            let mut stmt = tx.prepare(
                "INSERT INTO skill_translations
                    (fingerprint, kind, source_name, zh_name, zh_description, model, created_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
                 ON CONFLICT(fingerprint) DO UPDATE SET
                    kind=excluded.kind, source_name=excluded.source_name,
                    zh_name=excluded.zh_name, zh_description=excluded.zh_description,
                    model=excluded.model, created_at=excluded.created_at",
            )?;
            for r in records {
                stmt.execute(rusqlite::params![
                    r.fingerprint, r.kind, r.source_name, r.zh_name, r.zh_description,
                    r.model, r.created_at
                ])?;
            }
        }
        tx.commit()?;
        Ok(())
    }

    pub fn clear_translations(&self) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute("DELETE FROM skill_translations", [])?;
        Ok(())
    }
```

注：`self.conn.lock().unwrap()` 与 `conn.transaction()` 需要 `let mut conn`（既有代码里事务写法照抄 `log_audit` 附近）。

- [ ] **Step 6: 运行测试确认通过 + 全量无回归**

Run: `cd /d/CloudMusic/skills-manager/src-tauri && cargo test translation_store && cargo test`
Expected: 新测试 PASS（其余不变，2 个既有 1314 失败照旧）

- [ ] **Step 7: Commit**

```bash
cd /d/CloudMusic/skills-manager
git add src-tauri/src/core/translation_store.rs src-tauri/src/core/migrations.rs src-tauri/src/core/skill_store.rs src-tauri/src/core/mod.rs
git commit -m "feat: add translation cache table and store accessors"
```

---

### Task 2: 指纹 + 分批 + 容错解析（纯逻辑）

**Files:**
- Modify: `src-tauri/src/core/translation_store.rs`（加纯函数与测试）

**Interfaces:**
- Consumes: Task 1 的 `TranslationRecord`
- Produces:
  - `pub const BATCH_SIZE: usize = 20;`
  - `pub struct TranslationInput { pub fingerprint: String, pub kind: String, pub name: String, pub description: Option<String> }`
  - `pub struct TranslationOutput { pub fingerprint: String, pub zh_name: String, pub zh_description: String }`
  - `pub fn fingerprint(kind: &str, name: &str, description: Option<&str>) -> String`
  - `pub fn plan_batches(inputs: Vec<TranslationInput>, batch_size: usize) -> Vec<Vec<TranslationInput>>`
  - `pub fn parse_translation_response(raw: &str, batch: &[TranslationInput]) -> Vec<TranslationOutput>`

- [ ] **Step 1: 写失败测试**（追加到 `translation_store.rs` 的 tests 模块）

```rust
    #[test]
    fn fingerprint_is_stable_and_content_sensitive() {
        let a = fingerprint("plugin_skill", "brainstorming", Some("Design first"));
        let b = fingerprint("plugin_skill", "brainstorming", Some("Design first"));
        assert_eq!(a, b);
        assert_ne!(a, fingerprint("plugin_skill", "brainstorming", Some("Design second")));
        assert_ne!(a, fingerprint("plugin_skill", "other", Some("Design first")));
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
```

- [ ] **Step 2: 运行确认失败**

Run: `cd /d/CloudMusic/skills-manager/src-tauri && cargo test translation_store`
Expected: 编译失败（`fingerprint` / `plan_batches` / `parse_translation_response` 不存在）

- [ ] **Step 3: 实现（追加到 `translation_store.rs` 顶部类型区之后）**

```rust
use crate::core::error::AppError; // 仅当需要错误类型时；本任务纯函数不返回 Result，可省略

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
```

- [ ] **Step 4: 运行测试确认通过**

Run: `cd /d/CloudMusic/skills-manager/src-tauri && cargo test translation_store`
Expected: 全部 PASS（Task 1 的 + 本任务 6 个）

- [ ] **Step 5: Commit**

```bash
cd /d/CloudMusic/skills-manager
git add src-tauri/src/core/translation_store.rs
git commit -m "feat: add translation fingerprints, batching and tolerant response parsing"
```

---

### Task 3: 翻译引擎（两种接口格式 + 编排 + 可注入后端）

**Files:**
- Create: `src-tauri/src/core/translator.rs`
- Modify: `src-tauri/src/core/mod.rs`（`pub mod translator;` 字母序）

**Interfaces:**
- Consumes: Task 2 的 `TranslationInput/TranslationOutput/plan_batches/parse_translation_response`；`core::skillssh_api::build_http_client(Option<&str>, u64) -> reqwest::blocking::Client`（现成）；`core::error::AppError`
- Produces:
  - `pub enum ApiFormat { Anthropic, OpenAi }`（`Serialize/Deserialize`，`#[serde(rename_all = "lowercase")]`）
  - `pub struct TranslationConfig { pub endpoint: String, pub model: String, pub format: ApiFormat }`
  - `pub struct HttpRequestSpec { pub url: String, pub headers: Vec<(String, String)>, pub body: serde_json::Value }`
  - `pub fn build_prompt(batch: &[TranslationInput]) -> String`
  - `pub fn build_request(cfg: &TranslationConfig, api_key: &str, prompt: &str) -> HttpRequestSpec`
  - `pub trait TranslationBackend { fn complete(&self, spec: &HttpRequestSpec) -> Result<String, AppError>; }`
  - `pub struct HttpBackend;`（实现 trait，用 `build_http_client(None, 60)`）
  - `pub struct TranslationRunReport { pub translated: Vec<TranslationOutput>, pub failed_batches: usize }`
  - `pub fn run_translation(batches: Vec<Vec<TranslationInput>>, cfg: &TranslationConfig, api_key: &str, backend: &dyn TranslationBackend, on_progress: &mut dyn FnMut(usize, usize)) -> TranslationRunReport`

- [ ] **Step 1: 写失败测试**（`translator.rs` 底部）

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::translation_store::TranslationInput;
    use std::sync::Mutex;

    fn item(i: usize) -> TranslationInput {
        TranslationInput {
            fingerprint: format!("fp{i}"),
            kind: "plugin_skill".into(),
            name: format!("skill-{i}"),
            description: Some(format!("desc {i}")),
        }
    }

    fn cfg(format: ApiFormat) -> TranslationConfig {
        TranslationConfig {
            endpoint: "http://127.0.0.1:8789".into(),
            model: "test-model".into(),
            format,
        }
    }

    #[test]
    fn anthropic_request_shape() {
        let spec = build_request(&cfg(ApiFormat::Anthropic), "sk-test", "prompt!");
        assert_eq!(spec.url, "http://127.0.0.1:8789/v1/messages");
        assert!(spec.headers.iter().any(|(k, v)| k == "x-api-key" && v == "sk-test"));
        assert_eq!(spec.body["model"], "test-model");
        assert_eq!(spec.body["messages"][0]["content"], "prompt!");
    }

    #[test]
    fn openai_request_shape() {
        let spec = build_request(&cfg(ApiFormat::OpenAi), "sk-test", "prompt!");
        assert_eq!(spec.url, "http://127.0.0.1:8789/v1/chat/completions");
        assert!(spec.headers.iter().any(|(k, v)| k == "Authorization" && v == "Bearer sk-test"));
        assert_eq!(spec.body["model"], "test-model");
        assert_eq!(spec.body["messages"][0]["role"], "user");
    }

    #[test]
    fn trailing_slash_endpoint_is_normalized() {
        let mut c = cfg(ApiFormat::Anthropic);
        c.endpoint = "http://127.0.0.1:8789/".into();
        assert_eq!(build_request(&c, "k", "p").url, "http://127.0.0.1:8789/v1/messages");
    }

    struct FakeBackend {
        replies: Mutex<Vec<Result<String, AppError>>>,
    }
    impl TranslationBackend for FakeBackend {
        fn complete(&self, _spec: &HttpRequestSpec) -> Result<String, AppError> {
            self.replies.lock().unwrap().remove(0)
        }
    }

    fn ok_reply(count: usize) -> String {
        let items: Vec<String> = (0..count)
            .map(|i| format!(r#"{{"i":{i},"zh_name":"甲{i}","zh_description":"说明{i}"}}"#))
            .collect();
        format!("[{}]", items.join(","))
    }

    #[test]
    fn happy_path_translates_all_batches_and_reports_progress() {
        let batches = vec![vec![item(0), item(1)], vec![item(2)]];
        let backend = FakeBackend {
            replies: Mutex::new(vec![Ok(ok_reply(2)), Ok(ok_reply(1))]),
        };
        let mut seen = Vec::new();
        let report = run_translation(
            batches,
            &cfg(ApiFormat::Anthropic),
            "k",
            &backend,
            &mut |done, total| seen.push((done, total)),
        );
        assert_eq!(report.translated.len(), 3);
        assert_eq!(report.failed_batches, 0);
        assert_eq!(seen, vec![(1, 2), (2, 2)]);
    }

    #[test]
    fn failing_batch_retries_once_then_is_counted_and_skipped() {
        let batches = vec![vec![item(0)], vec![item(1)]];
        let backend = FakeBackend {
            replies: Mutex::new(vec![
                Err(AppError::network("boom")),   // batch 1 attempt 1
                Err(AppError::network("boom")),   // batch 1 attempt 2 (retry)
                Ok(ok_reply(1)),                  // batch 2
            ]),
        };
        let report = run_translation(batches, &cfg(ApiFormat::OpenAi), "k", &backend, &mut |_, _| {});
        assert_eq!(report.translated.len(), 1);
        assert_eq!(report.translated[0].fingerprint, "fp1");
        assert_eq!(report.failed_batches, 1);
    }

    #[test]
    fn garbage_reply_retries_once_then_batch_is_dropped() {
        let batches = vec![vec![item(0)]];
        let backend = FakeBackend {
            replies: Mutex::new(vec![Ok("不是 JSON".into()), Ok("还是不是".into())]),
        };
        let report = run_translation(batches, &cfg(ApiFormat::Anthropic), "k", &backend, &mut |_, _| {});
        assert!(report.translated.is_empty());
        assert_eq!(report.failed_batches, 1);
    }
}
```

- [ ] **Step 2: 运行确认失败**

Run: `cd /d/CloudMusic/skills-manager/src-tauri && cargo test translator`
Expected: 编译失败（模块不存在）

- [ ] **Step 3: 实现 `translator.rs`**

```rust
//! Translation engine: builds requests for the two supported API shapes and
//! drives batch translation through an injectable backend (fake in tests).

use serde::{Deserialize, Serialize};

use crate::core::error::AppError;
use crate::core::skillssh_api::build_http_client;
use crate::core::translation_store::{
    parse_translation_response, TranslationInput, TranslationOutput,
};

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ApiFormat {
    Anthropic,
    OpenAi,
}

#[derive(Debug, Clone, PartialEq)]
pub struct TranslationConfig {
    pub endpoint: String,
    pub model: String,
    pub format: ApiFormat,
}

#[derive(Debug, Clone, PartialEq)]
pub struct HttpRequestSpec {
    pub url: String,
    pub headers: Vec<(String, String)>,
    pub body: serde_json::Value,
}

pub fn build_prompt(batch: &[TranslationInput]) -> String {
    let items: Vec<serde_json::Value> = batch
        .iter()
        .enumerate()
        .map(|(i, item)| {
            serde_json::json!({
                "i": i,
                "kind": item.kind,
                "name": item.name,
                "description": item.description.clone().unwrap_or_default(),
            })
        })
        .collect();
    let payload = serde_json::to_string_pretty(&items).unwrap_or_default();
    format!(
        "你是软件本地化助手。下面是一批 AI 技能的名称与英文说明，请为每一条生成简体中文译名（zh_name）\
和一句话中文说明（zh_description，不超过 40 字，说清它干什么用）。\n\
只输出 JSON 数组，元素形如 {{\"i\": 序号, \"zh_name\": \"...\", \"zh_description\": \"...\"}}，\
不要输出任何其它文字。\n\n技能清单：\n{payload}"
    )
}

pub fn build_request(cfg: &TranslationConfig, api_key: &str, prompt: &str) -> HttpRequestSpec {
    let base = cfg.endpoint.trim_end_matches('/');
    match cfg.format {
        ApiFormat::Anthropic => HttpRequestSpec {
            url: format!("{base}/v1/messages"),
            headers: vec![
                ("x-api-key".into(), api_key.into()),
                ("anthropic-version".into(), "2023-06-01".into()),
                ("content-type".into(), "application/json".into()),
            ],
            body: serde_json::json!({
                "model": cfg.model,
                "max_tokens": 8000,
                "messages": [{ "role": "user", "content": prompt }],
            }),
        },
        ApiFormat::OpenAi => HttpRequestSpec {
            url: format!("{base}/v1/chat/completions"),
            headers: vec![
                ("Authorization".into(), format!("Bearer {api_key}")),
                ("content-type".into(), "application/json".into()),
            ],
            body: serde_json::json!({
                "model": cfg.model,
                "messages": [{ "role": "user", "content": prompt }],
            }),
        },
    }
}

pub trait TranslationBackend {
    fn complete(&self, spec: &HttpRequestSpec) -> Result<String, AppError>;
}

pub struct HttpBackend;

impl TranslationBackend for HttpBackend {
    fn complete(&self, spec: &HttpRequestSpec) -> Result<String, AppError> {
        let client = build_http_client(None, 60);
        let mut req = client.post(&spec.url);
        for (k, v) in &spec.headers {
            req = req.header(k.as_str(), v.as_str());
        }
        let resp = req
            .json(&spec.body)
            .send()
            .map_err(|e| AppError::network(e.to_string()))?;
        let status = resp.status();
        let text = resp.text().map_err(|e| AppError::network(e.to_string()))?;
        if !status.is_success() {
            let snippet: String = text.chars().take(300).collect();
            return Err(AppError::network(format!("HTTP {status}: {snippet}")));
        }
        Ok(text)
    }
}

#[derive(Debug, Default, PartialEq)]
pub struct TranslationRunReport {
    pub translated: Vec<TranslationOutput>,
    pub failed_batches: usize,
}

/// Extracts the model's text from either reply shape; falls back to the raw
/// body when the shape is unexpected, so the tolerant parser still gets a shot.
pub fn extract_reply_text(body: &str) -> String {
    if let Ok(value) = serde_json::from_str::<serde_json::Value>(body) {
        if let Some(text) = value
            .get("content")
            .and_then(|c| c.as_array())
            .and_then(|arr| arr.iter().find_map(|b| b.get("text").and_then(|t| t.as_str())))
        {
            return text.to_string();
        }
        if let Some(text) = value
            .get("choices")
            .and_then(|c| c.as_array())
            .and_then(|arr| arr.first())
            .and_then(|c| c.get("message"))
            .and_then(|m| m.get("content"))
            .and_then(|t| t.as_str())
        {
            return text.to_string();
        }
    }
    body.to_string()
}

/// Runs every batch. Each batch gets two attempts; a batch that still yields no
/// parsed items is counted as failed and contributes nothing (no partial writes).
pub fn run_translation(
    batches: Vec<Vec<TranslationInput>>,
    cfg: &TranslationConfig,
    api_key: &str,
    backend: &dyn TranslationBackend,
    on_progress: &mut dyn FnMut(usize, usize),
) -> TranslationRunReport {
    let total = batches.len();
    let mut report = TranslationRunReport::default();

    for (index, batch) in batches.into_iter().enumerate() {
        let prompt = build_prompt(&batch);
        let spec = build_request(cfg, api_key, &prompt);
        let mut parsed = Vec::new();
        for _attempt in 0..2 {
            match backend.complete(&spec) {
                Ok(body) => {
                    parsed = parse_translation_response(&extract_reply_text(&body), &batch);
                    if !parsed.is_empty() {
                        break;
                    }
                }
                Err(e) => {
                    log::warn!("translation batch {} attempt failed: {}", index + 1, e);
                }
            }
        }
        if parsed.is_empty() {
            report.failed_batches += 1;
        } else {
            report.translated.extend(parsed);
        }
        on_progress(index + 1, total);
    }

    report
}
```

- [ ] **Step 4: 运行测试确认通过 + 全量**

Run: `cd /d/CloudMusic/skills-manager/src-tauri && cargo test translator && cargo test`
Expected: 全部 PASS，无回归

- [ ] **Step 5: Commit**

```bash
cd /d/CloudMusic/skills-manager
git add src-tauri/src/core/translator.rs src-tauri/src/core/mod.rs
git commit -m "feat: add the batch translation engine with injectable backend"
```

---

### Task 4: 命令层（设置 / 状态 / 翻译 / 测试连接）+ 钥匙串 + 进度事件

**Files:**
- Create: `src-tauri/src/commands/translation.rs`
- Modify: `src-tauri/src/commands/mod.rs`（`pub mod translation;` 字母序）
- Modify: `src-tauri/src/lib.rs`（注册 6 个命令）
- Modify: `src-tauri/src/core/plugin_scanner.rs`（`PluginSkillEntry`/`PluginSkillGroup` 各加 `zh_name: Option<String>`、`zh_description: Option<String>`，构造处默认 `None`）
- Modify: `src-tauri/src/commands/plugins.rs`（`get_claude_plugin_skills` 用缓存填充 zh 字段）
- Test: `commands/translation.rs` 底部 tests

**Interfaces:**
- Consumes: Task 1-3 全部产出；`core::plugin_scanner::{claude_config_dir, scan_plugin_skills, scan_official_skills}`；`core::translation_store::{fingerprint, plan_batches, TranslationInput, BATCH_SIZE}`；`core::translator::{ApiFormat, TranslationConfig, HttpBackend, run_translation}`
- Produces（命令名冻结，前端依赖）:
  - `get_translation_settings() -> { endpoint, model, format, has_key }`
  - `set_translation_settings(endpoint: String, model: String, format: String, api_key: Option<String>)`
  - `test_translation_connection() -> String`（成功返回模型回话片段；失败返回错误信息）
  - `get_translation_status() -> { total: usize, pending: usize }`
  - `translate_skills(app: AppHandle) -> { translated: usize, failed_batches: usize, pending: usize }`
  - `clear_translations()`
  - 事件：`translation-progress`，负载 `{ "done": usize, "total": usize }`
  - settings 键：`translation_endpoint`、`translation_model`、`translation_format`（值 `"anthropic"`/`"openai"`）

- [ ] **Step 1: 写失败测试**

```rust
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
    fn pending_counts_only_untranslated_items() {
        let tmp = tempfile::tempdir().unwrap();
        let store = SkillStore::new(&tmp.path().join("skills.db")).unwrap();
        let items = vec![
            input_item("plugin_skill", "a", Some("A desc")),
            input_item("plugin_skill", "b", Some("B desc")),
        ];
        assert_eq!(count_pending(&store, &items), 2);

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
        assert_eq!(count_pending(&store, &items), 1);
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
```

（`input_item(kind, name, desc)` 是测试内的小工厂，调用 `crate::core::translation_store::fingerprint` 组装 `TranslationInput`——在测试模块里写出来。）

- [ ] **Step 2: 运行确认失败**

Run: `cd /d/CloudMusic/skills-manager/src-tauri && cargo test commands::translation`
Expected: 编译失败（模块不存在）

- [ ] **Step 3: 实现 `commands/translation.rs`**

要点（完整实现，照着写）：

```rust
use std::collections::HashMap;
use serde::Serialize;
use tauri::{AppHandle, Emitter, State};
use std::sync::Arc;

use crate::core::error::AppError;
use crate::core::plugin_scanner;
use crate::core::skill_store::SkillStore;
use crate::core::translation_store::{self, TranslationInput, TranslationRecord, BATCH_SIZE};
use crate::core::translator::{self, ApiFormat, HttpBackend, TranslationConfig};

const KEYRING_SERVICE: &str = "skills-manager-translation";
const KEYRING_ACCOUNT: &str = "api-key";
const DEFAULT_ENDPOINT: &str = "http://127.0.0.1:8789";

#[derive(Debug, Serialize)]
pub struct TranslationSettingsDto { pub endpoint: String, pub model: String, pub format: String, pub has_key: bool }

#[derive(Debug, Serialize)]
pub struct TranslationStatusDto { pub total: usize, pub pending: usize }

#[derive(Debug, Serialize)]
pub struct TranslateReportDto { pub translated: usize, pub failed_batches: usize, pub pending: usize }

fn keyring_entry() -> Result<keyring::Entry, AppError> {
    keyring::Entry::new(KEYRING_SERVICE, KEYRING_ACCOUNT).map_err(|e| AppError::internal(e.to_string()))
}

fn load_api_key() -> Option<String> {
    match keyring_entry().ok()?.get_password() {
        Ok(v) if !v.trim().is_empty() => Some(v),
        _ => None,
    }
}

fn store_api_key(key: &str) -> Result<(), AppError> {
    keyring_entry()?.set_password(key).map_err(|e| AppError::internal(e.to_string()))
}

fn read_config(store: &SkillStore) -> TranslationConfig {
    let endpoint = store.get_setting("translation_endpoint").ok().flatten()
        .filter(|s| !s.trim().is_empty()).unwrap_or_else(|| DEFAULT_ENDPOINT.to_string());
    let model = store.get_setting("translation_model").ok().flatten().unwrap_or_default();
    let format = match store.get_setting("translation_format").ok().flatten().as_deref() {
        Some("openai") => ApiFormat::OpenAi,
        _ => ApiFormat::Anthropic,
    };
    TranslationConfig { endpoint, model, format }
}

fn validate_config(cfg: &TranslationConfig) -> Result<(), AppError> {
    if cfg.endpoint.trim().is_empty() { return Err(AppError::invalid_input("翻译接口地址未配置")); }
    if cfg.model.trim().is_empty() { return Err(AppError::invalid_input("翻译模型未填写")); }
    Ok(())
}

/// Every translatable read-only item: plugin skills, official skills, plugin group descriptions.
fn collect_inputs(config_dir: &std::path::Path) -> (Vec<TranslationInput>, Vec<Vec<TranslationInput>>) {
    let mut items = Vec::new();
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
            let fp = translation_store::fingerprint("plugin_skill", &skill.name, skill.description.as_deref());
            items.push(TranslationInput {
                fingerprint: fp,
                kind: "plugin_skill".into(),
                name: skill.name,
                description: skill.description,
            });
        }
    }
    for skill in plugin_scanner::scan_official_skills(config_dir) {
        let fp = translation_store::fingerprint("official_skill", &skill.name, skill.description.as_deref());
        items.push(TranslationInput {
            fingerprint: fp,
            kind: "official_skill".into(),
            name: skill.name,
            description: skill.description,
        });
    }
    let batches = translation_store::plan_batches(items.clone(), BATCH_SIZE);
    (items, batches)
}

/// Only the batches that still contain untranslated items (Review Focus #5:
/// a second click with everything cached must produce zero batches → zero calls).
fn pending_batches(
    batches: Vec<Vec<TranslationInput>>,
    cached: &HashMap<String, TranslationRecord>,
) -> Vec<Vec<TranslationInput>> {
    batches
        .into_iter()
        .map(|b| b.into_iter().filter(|i| !cached.contains_key(&i.fingerprint)).collect::<Vec<_>>())
        .filter(|b| !b.is_empty())
        .collect()
}

pub fn count_pending(store: &SkillStore, items: &[TranslationInput]) -> usize {
    let cached = store.get_translations().unwrap_or_default();
    items.iter().filter(|i| !cached.contains_key(&i.fingerprint)).count()
}

#[tauri::command]
pub async fn get_translation_settings(store: State<'_, Arc<SkillStore>>) -> Result<TranslationSettingsDto, AppError> {
    let cfg = read_config(store.inner());
    Ok(TranslationSettingsDto {
        endpoint: cfg.endpoint,
        model: cfg.model,
        format: match cfg.format { ApiFormat::Anthropic => "anthropic".into(), ApiFormat::OpenAi => "openai".into() },
        has_key: load_api_key().is_some(),
    })
}

#[tauri::command]
pub async fn set_translation_settings(
    store: State<'_, Arc<SkillStore>>,
    endpoint: String, model: String, format: String, api_key: Option<String>,
) -> Result<(), AppError> {
    let store = store.inner().clone();
    store.set_setting("translation_endpoint", endpoint.trim()).map_err(AppError::db)?;
    store.set_setting("translation_model", model.trim()).map_err(AppError::db)?;
    store.set_setting("translation_format", if format == "openai" { "openai" } else { "anthropic" })
        .map_err(AppError::db)?;
    if let Some(key) = api_key.filter(|k| !k.trim().is_empty()) {
        store_api_key(key.trim())?;
    }
    Ok(())
}

#[tauri::command]
pub async fn test_translation_connection(store: State<'_, Arc<SkillStore>>) -> Result<String, AppError> {
    let cfg = read_config(store.inner());
    validate_config(&cfg)?;
    let key = load_api_key().ok_or_else(|| AppError::invalid_input("翻译密钥未配置"))?;
    let spec = translator::build_request(&cfg, &key, "请回复：连接成功");
    let body = HttpBackend.complete(&spec)?;
    Ok(translator::extract_reply_text(&body))
}

#[tauri::command]
pub async fn get_translation_status(store: State<'_, Arc<SkillStore>>) -> Result<TranslationStatusDto, AppError> {
    let store = store.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        let (items, _) = collect_inputs(&plugin_scanner::claude_config_dir());
        Ok(TranslationStatusDto { total: items.len(), pending: count_pending(&store, &items) })
    }).await?
}

#[tauri::command]
pub async fn translate_skills(
    app: AppHandle, store: State<'_, Arc<SkillStore>>,
) -> Result<TranslateReportDto, AppError> {
    let store = store.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        let cfg = read_config(&store);
        validate_config(&cfg)?;
        let key = load_api_key().ok_or_else(|| AppError::invalid_input("翻译密钥未配置"))?;

        let (items, all_batches) = collect_inputs(&plugin_scanner::claude_config_dir());
        let cached = store.get_translations().unwrap_or_default();
        let pending = pending_batches(all_batches, &cached);

        let backend = HttpBackend;
        let mut translated_total = 0usize;
        let mut failed_batches = 0usize;
        let model = cfg.model.clone();
        let created_at = chrono::Utc::now().to_rfc3339();

        for batch in pending {
            let report = translator::run_translation(
                vec![batch.clone()], &cfg, &key, &backend,
                &mut |done, total| { let _ = app.emit("translation-progress", serde_json::json!({"done": done, "total": total})); },
            );
            failed_batches += report.failed_batches;
            if !report.translated.is_empty() {
                let records: Vec<TranslationRecord> = report.translated.iter().map(|o| {
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
                }).collect();
                store.upsert_translations(&records).map_err(AppError::db)?;
                translated_total += records.len();
            }
        }

        let remaining = count_pending(&store, &items);
        Ok(TranslateReportDto { translated: translated_total, failed_batches, pending: remaining })
    }).await?
}

#[tauri::command]
pub async fn clear_translations(store: State<'_, Arc<SkillStore>>) -> Result<(), AppError> {
    store.inner().clear_translations().map_err(AppError::db)
}
```

`collect_inputs` 的完整实现（不要留 `unimplemented!`）：

```rust
fn collect_inputs(config_dir: &std::path::Path) -> (Vec<TranslationInput>, Vec<Vec<TranslationInput>>) {
    let mut items = Vec::new();
    for group in plugin_scanner::scan_plugin_skills(config_dir) {
        if let Some(desc) = group.description.as_deref().filter(|d| !d.trim().is_empty()) {
            let fp = translation_store::fingerprint("plugin_group", &group.plugin, Some(desc));
            items.push(TranslationInput { fingerprint: fp, kind: "plugin_group".into(), name: group.plugin.clone(), description: Some(desc.to_string()) });
        }
        for skill in group.skills {
            let fp = translation_store::fingerprint("plugin_skill", &skill.name, skill.description.as_deref());
            items.push(TranslationInput { fingerprint: fp, kind: "plugin_skill".into(), name: skill.name, description: skill.description });
        }
    }
    for skill in plugin_scanner::scan_official_skills(config_dir) {
        let fp = translation_store::fingerprint("official_skill", &skill.name, skill.description.as_deref());
        items.push(TranslationInput { fingerprint: fp, kind: "official_skill".into(), name: skill.name, description: skill.description });
    }
    let batches = translation_store::plan_batches(items.clone(), BATCH_SIZE);
    (items, batches)
}
```

- [ ] **Step 4: 扫描结果带上中文（plugin_scanner.rs + commands/plugins.rs）**

- `PluginSkillEntry` 与 `PluginSkillGroup` 各加两个字段：
```rust
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub zh_name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub zh_description: Option<String>,
```
  并在 `plugin_scanner.rs` 的构造处（`skills_under` / `scan_plugin_skills` 内）补 `zh_name: None, zh_description: None`。
- `commands/plugins.rs::get_claude_plugin_skills`：拿到扫描结果后按指纹查缓存并填充：
```rust
        let cached = store.get_translations().unwrap_or_default();
        for group in &mut groups {
            if let Some(d) = group.description.as_deref() {
                let fp = translation_store::fingerprint("plugin_group", &group.plugin, Some(d));
                if let Some(rec) = cached.get(&fp) { group.zh_name = Some(rec.zh_name.clone()); group.zh_description = Some(rec.zh_description.clone()); }
            }
            for skill in &mut group.skills {
                let fp = translation_store::fingerprint("plugin_skill", &skill.name, skill.description.as_deref());
                if let Some(rec) = cached.get(&fp) { skill.zh_name = Some(rec.zh_name.clone()); skill.zh_description = Some(rec.zh_description.clone()); }
            }
        }
        for skill in &mut official {
            let fp = translation_store::fingerprint("official_skill", &skill.name, skill.description.as_deref());
            if let Some(rec) = cached.get(&fp) { skill.zh_name = Some(rec.zh_name.clone()); skill.zh_description = Some(rec.zh_description.clone()); }
        }
```
  （该命令因此需要 `State<Arc<SkillStore>>` 参数——与既有命令同款。）

- [ ] **Step 5: 注册命令（lib.rs）**

在 `generate_handler!` 中 `commands::plugins::*` 之后加：
```rust
            commands::translation::get_translation_settings,
            commands::translation::set_translation_settings,
            commands::translation::test_translation_connection,
            commands::translation::get_translation_status,
            commands::translation::translate_skills,
            commands::translation::clear_translations,
```

- [ ] **Step 6: 测试 + 全量**

Run: `cd /d/CloudMusic/skills-manager/src-tauri && cargo test commands::translation && cargo test`
Expected: 新测试 PASS（注意：`refuses_without_model_or_endpoint` 与 `pending_counts_only_untranslated_items` 为纯逻辑测试，不触网、不触钥匙串）

- [ ] **Step 7: Commit**

```bash
cd /d/CloudMusic/skills-manager
git add src-tauri/src/commands/translation.rs src-tauri/src/commands/mod.rs src-tauri/src/lib.rs src-tauri/src/core/plugin_scanner.rs src-tauri/src/commands/plugins.rs
git commit -m "feat: expose translation settings, status and one-click translation commands"
```

---

### Task 5: 前端（双语渲染 + 一键翻译按钮 + 设置小节 + 三语文案）

**Files:**
- Modify: `src/lib/tauri.ts`（类型 + 6 个 API 封装；`PluginSkillEntry`/`PluginSkillGroup` 加 `zh_name?: string | null`、`zh_description?: string | null`）
- Modify: `src/components/PluginSkillsSection.tsx`（双语渲染 + 按钮/计数/进度/汇总）
- Modify: `src/views/Settings.tsx`（「AI 翻译」小节）
- Modify: `src/i18n/{en,zh,zh-TW}.json`（新命名空间 `translation`）

**Interfaces:**
- Consumes: Task 4 的命令与事件；`@tauri-apps/api/event` 的 `listen`（组件里照 `install-progress` 的既有用法）；`useNavigate`（跳到 `/settings`）
- Produces: 无（叶子）

- [ ] **Step 1: `src/lib/tauri.ts` 追加**

```ts
// ── Translation (bilingual UI) ──

export interface TranslationSettings {
  endpoint: string;
  model: string;
  format: "anthropic" | "openai";
  has_key: boolean;
}

export interface TranslationStatus {
  total: number;
  pending: number;
}

export interface TranslateReport {
  translated: number;
  failed_batches: number;
  pending: number;
}

export const getTranslationSettings = () => invoke<TranslationSettings>("get_translation_settings");
export const setTranslationSettings = (
  endpoint: string,
  model: string,
  format: "anthropic" | "openai",
  apiKey?: string
) => invoke<void>("set_translation_settings", { endpoint, model, format, apiKey });
export const testTranslationConnection = () => invoke<string>("test_translation_connection");
export const getTranslationStatus = () => invoke<TranslationStatus>("get_translation_status");
export const translateSkills = () => invoke<TranslateReport>("translate_skills");
export const clearTranslations = () => invoke<void>("clear_translations");
```

- [ ] **Step 2: 三语文案（三个 JSON 各加顶层 `translation`）**

`zh.json`：
```json
  "translation": {
    "translate": "一键翻译",
    "pendingBadge": "{{count}} 条待翻译",
    "translating": "翻译中…（第 {{done}}/{{total}} 批）",
    "translatedAll": "已全部翻译",
    "goToSettings": "去设置翻译接口",
    "resultSummary": "翻译完成：成功 {{ok}} 条，失败 {{failed}} 批",
    "noPending": "没有需要翻译的",
    "sectionTitle": "AI 翻译",
    "sectionHint": "钥匙存在系统凭据库；翻译结果只存在本软件自己的缓存里，不会写进插件目录。",
    "endpoint": "接口地址",
    "model": "模型名",
    "modelHint": "填入你的接口所支持的模型名",
    "format": "接口格式",
    "formatAnthropic": "Anthropic 格式",
    "formatOpenAi": "OpenAI 兼容",
    "apiKey": "密钥",
    "apiKeyConfigured": "已配置（留空则不改）",
    "save": "保存",
    "saved": "已保存",
    "testConnection": "测试连接",
    "testing": "测试中…",
    "emptyModel": "请先填写模型名",
    "clearCache": "清空翻译缓存",
    "clearCacheDone": "已清空，可重新翻译"
  },
```
`en.json`（同键名，英文文案；例：`"translate": "Translate"`, `"pendingBadge": "{{count}} to translate"`, `"translatedAll": "All translated"`, `"goToSettings": "Set up translation"`, `"noPending": "Nothing to translate"`, `"sectionTitle": "AI translation"`, `"testConnection": "Test connection"`, …）。`zh-TW.json` 用繁体对应文案。

- [ ] **Step 3: `PluginSkillsSection.tsx` 改动**

- 挂载时并行取 `getTranslationStatus()` 与 `listen("translation-progress", ...)`；卸载时 `unlisten()`。
- 块头右侧：`pending > 0` 时显示按钮 `t("translation.translate")` + 徽标 `t("translation.pendingBadge", { count: pending })`；`total > 0 && pending === 0` 时显示 `t("translation.translatedAll")`；`has_key === false || model === ""` 时按钮文案换成 `t("translation.goToSettings")` 且点击 `navigate("/settings")`。
- 点击翻译：按钮进入 `t("translation.translating", {done, total})` 状态（由事件驱动），完成后 `toast.success(t("translation.resultSummary", { ok, failed: failed_batches }))` 或 `t("translation.noPending")`，并重新拉取技能列表（父组件已有加载函数；组件内触发一次 `getClaudePluginSkills()` 刷新自身数据）。
- 每条技能行渲染：
```tsx
{entry.zh_name ? (
  <div className="mt-0.5 text-[12px] text-secondary">
    {entry.zh_name}
    {entry.zh_description ? <span className="text-muted"> — {entry.zh_description}</span> : null}
  </div>
) : null}
```
  分组行同理渲染 `group.zh_description`（在既有英文描述下方）。
- **只读铁律**：新增的只有按钮/文本，不得引入任何编辑/删除/同步入口。

- [ ] **Step 4: `Settings.tsx` 新增「AI 翻译」小节**

放在既有「GitHub 备份」小节之后；字段与按钮照本文件既有表单控件写法（同一套 className）。行为：
- 进入页面时 `getTranslationSettings()` 预填 endpoint/model/format，密钥框占位符显示 `apiKeyConfigured`（已配置时）；
- 「保存」→ `setTranslationSettings(...)` → toast `saved`；
- 「测试连接」→ 未填模型先提示 `emptyModel`；否则 `testTranslationConnection()`，成功 `toast.success(reply 前 80 字)`，失败 `toast.error(getErrorMessage(e, ...))`；
- 「清空翻译缓存」→ 二次确认后 `clearTranslations()` → toast `clearCacheDone`。

- [ ] **Step 5: 验证**

Run:
```bash
cd /d/CloudMusic/skills-manager
npm run build
npm run lint
```
Expected: 均零错误

- [ ] **Step 6: Commit**

```bash
cd /d/CloudMusic/skills-manager
git add src/lib/tauri.ts src/components/PluginSkillsSection.tsx src/views/Settings.tsx src/i18n/en.json src/i18n/zh.json src/i18n/zh-TW.json
git commit -m "feat: bilingual skill display with one-click translation"
```

---

### Task 6: 构建 + 安装 + 用户验收 + 推送

**Files:** 无代码改动（发现问题回对应任务修）

**Interfaces:**
- Consumes: Task 1-5 全部产出
- Produces: 安装包 + `v2-translation` 推送到 fork

- [ ] **Step 1: 出安装包**

```bash
cd /d/CloudMusic/skills-manager
HTTPS_PROXY=http://127.0.0.1:7897 npm run tauri:build
```
（后台跑 + 轮询；updater 签名步骤 exit 1 属预期，以产物存在为准）

- [ ] **Step 2: 安装新构建**

先按精确进程名结束 `skills-manager`，再运行新安装器（`/S` 或 `Start-Process -Wait`），启动应用。

- [ ] **Step 3: 用户验收（规格书第 3 节 7 行表）**

1. 设置页「测试连接」成功
2. 插件页点「一键翻译」→ 出现中文 → 待翻译归 0
3. 再点一次 → 提示「没有需要翻译的」
4. 升级/新装插件 → 提示有 N 条待翻译，点击只翻新增
5. 重启软件 → 中文仍在
6. `plugins/` 零写入、仍无编辑/删除入口
7. 甲的功能与既有功能照常

- [ ] **Step 4: 推送**

```bash
cd /d/CloudMusic/skills-manager
cp "D:/CloudMusic/docs/superpowers/specs/2026-10-01-skills-manager-translation-design.md" docs/superpowers/specs/
cp "D:/CloudMusic/docs/superpowers/plans/2026-10-01-skills-manager-translation.md" docs/superpowers/plans/
git add -f docs/superpowers
git commit -m "docs: add the translation design spec and implementation plan"
git push -u origin v2-translation
```