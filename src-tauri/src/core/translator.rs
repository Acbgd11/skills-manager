//! Translation engine: builds requests for the two supported API shapes and
//! drives batch translation through an injectable backend (fake in tests).

use serde::{Deserialize, Serialize};

use crate::core::error::AppError;
use crate::core::skillssh_api::build_http_client;
use crate::core::translation_store::{parse_translation_response, TranslationInput, TranslationOutput};

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
                "max_tokens": 8000,
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
        assert_eq!(spec.body["max_tokens"], 8000);
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

    #[test]
    fn extract_reply_text_reads_anthropic_content_blocks() {
        let body = r#"{"content":[{"type":"text","text":"hello world"}]}"#;
        assert_eq!(extract_reply_text(body), "hello world");
    }

    #[test]
    fn extract_reply_text_reads_openai_choices_message() {
        let body = r#"{"choices":[{"message":{"content":"hi there"}}]}"#;
        assert_eq!(extract_reply_text(body), "hi there");
    }

    #[test]
    fn extract_reply_text_falls_back_to_raw_body_on_unknown_shape() {
        let body = "just plain text, no JSON";
        assert_eq!(extract_reply_text(body), body);
    }

    #[test]
    fn retry_recovers_when_second_attempt_succeeds() {
        let batches = vec![vec![item(0), item(1)]];
        let backend = FakeBackend {
            replies: Mutex::new(vec![
                Err(AppError::network("transient boom")), // attempt 1 fails
                Ok(ok_reply(2)),                          // retry succeeds
            ]),
        };
        let report = run_translation(batches, &cfg(ApiFormat::OpenAi), "k", &backend, &mut |_, _| {});
        assert_eq!(report.translated.len(), 2);
        assert_eq!(report.failed_batches, 0);
    }
}
