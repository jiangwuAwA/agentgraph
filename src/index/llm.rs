//! Optional LLM enrichment: one-line responsibility descriptions for symbols.
//! OpenAI-compatible Chat Completions API. Concurrent with a small worker pool.

use anyhow::{anyhow, bail, Context, Result};
use rayon::prelude::*;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::Path;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Mutex;

use super::store::Store;
use crate::model::EnrichReport;

#[derive(Debug, Clone)]
pub struct LlmConfig {
    pub api_key: String,
    pub base_url: String,
    pub model: String,
    pub concurrency: usize,
}

impl LlmConfig {
    pub fn from_env() -> Result<Self> {
        let api_key = std::env::var("OPENAI_API_KEY")
            .or_else(|_| std::env::var("AGENTGRAPH_API_KEY"))
            .context("set OPENAI_API_KEY (or AGENTGRAPH_API_KEY) to enable enrich")?;
        let base_url = std::env::var("OPENAI_BASE_URL")
            .unwrap_or_else(|_| "https://api.openai.com/v1".to_string())
            .trim_end_matches('/')
            .to_string();
        let model = std::env::var("AGENTGRAPH_MODEL").unwrap_or_else(|_| "gpt-4o-mini".to_string());
        let concurrency = std::env::var("AGENTGRAPH_ENRICH_CONCURRENCY")
            .ok()
            .and_then(|s| s.parse().ok())
            .unwrap_or(4)
            .clamp(1, 8);
        Ok(Self {
            api_key,
            base_url,
            model,
            concurrency,
        })
    }
}

#[derive(Serialize)]
struct ChatRequest {
    model: String,
    messages: Vec<ChatMessage>,
    temperature: f32,
    max_tokens: u32,
}

#[derive(Serialize)]
struct ChatMessage {
    role: String,
    content: String,
}

#[derive(Deserialize)]
struct ChatResponse {
    choices: Vec<Choice>,
}

#[derive(Deserialize)]
struct Choice {
    message: MessageBody,
}

#[derive(Deserialize)]
struct MessageBody {
    content: Option<String>,
}

struct EnrichItem {
    id: i64,
    name: String,
    kind: String,
    path: String,
    location: String,
}

pub fn enrich(
    root: &Path,
    store: &mut Store,
    cfg: &LlmConfig,
    limit: usize,
) -> Result<EnrichReport> {
    let pending = store.symbols_needing_description(limit)?;
    let items: Vec<EnrichItem> = pending
        .into_iter()
        .map(|(id, name, kind, path, location)| EnrichItem {
            id,
            name,
            kind,
            path,
            location,
        })
        .collect();
    let attempted = items.len();

    // Cache file contents once per path.
    let file_cache: Mutex<HashMap<String, Vec<String>>> = Mutex::new(HashMap::new());
    for it in &items {
        let mut cache = file_cache.lock().unwrap();
        if cache.contains_key(&it.path) {
            continue;
        }
        let full = root.join(&it.path);
        let lines = std::fs::read_to_string(&full)
            .map(|s| s.lines().map(|l| l.to_string()).collect::<Vec<_>>())
            .unwrap_or_default();
        cache.insert(it.path.clone(), lines);
    }

    let pool = rayon::ThreadPoolBuilder::new()
        .num_threads(cfg.concurrency)
        .build()?;
    let described = AtomicUsize::new(0);
    let failed = AtomicUsize::new(0);
    let skipped = AtomicUsize::new(0);
    let abort = AtomicUsize::new(0);
    let results: Mutex<Vec<(i64, String)>> = Mutex::new(Vec::new());

    pool.install(|| {
        items.par_iter().for_each(|it| {
            if abort.load(Ordering::Relaxed) > 0 {
                return;
            }
            let snippet = {
                let cache = file_cache.lock().unwrap();
                match cache.get(&it.path) {
                    Some(lines) => snippet_from_lines(lines, &it.location),
                    None => None,
                }
            };
            let Some(snippet) = snippet else {
                skipped.fetch_add(1, Ordering::Relaxed);
                return;
            };
            let prompt = format!(
                "You are labeling code symbols for an agent-facing code graph.\n\
                 Reply with ONE short sentence (max 20 words) describing what this {} does.\n\
                 No preamble, no markdown, no quotes.\n\n\
                 Symbol: {}\nKind: {}\nFile: {}\n\n\
                 Code:\n```\n{}\n```",
                it.kind, it.name, it.kind, it.path, snippet
            );
            let client = match reqwest::blocking::Client::builder()
                .timeout(std::time::Duration::from_secs(60))
                .build()
            {
                Ok(c) => c,
                Err(e) => {
                    failed.fetch_add(1, Ordering::Relaxed);
                    eprintln!("http client: {e:#}");
                    return;
                }
            };
            match chat(&client, cfg, &prompt) {
                Ok(text) => {
                    let desc = text.trim().trim_matches('"').to_string();
                    if desc.is_empty() {
                        skipped.fetch_add(1, Ordering::Relaxed);
                        return;
                    }
                    eprintln!("described {}::{} — {}", it.path, it.name, desc);
                    results.lock().unwrap().push((it.id, desc));
                    described.fetch_add(1, Ordering::Relaxed);
                }
                Err(e) => {
                    let n = failed.fetch_add(1, Ordering::Relaxed) + 1;
                    eprintln!("failed {}::{}: {e:#}", it.path, it.name);
                    if n >= 3 {
                        abort.store(1, Ordering::Relaxed);
                    }
                }
            }
        })
    });

    // Persist successful descriptions FIRST — even if we later bail on
    // too many failures, the successes must survive.
    for (id, desc) in results.into_inner().unwrap() {
        store.set_description(id, &desc)?;
    }

    if abort.load(Ordering::Relaxed) > 0 && failed.load(Ordering::Relaxed) >= 3 {
        bail!(
            "too many LLM failures ({}), aborting enrich",
            failed.load(Ordering::Relaxed)
        );
    }

    Ok(EnrichReport {
        attempted,
        described: described.load(Ordering::Relaxed),
        skipped: skipped.load(Ordering::Relaxed),
        failed: failed.load(Ordering::Relaxed),
        model: cfg.model.clone(),
    })
}

fn snippet_from_lines(lines: &[String], location: &str) -> Option<String> {
    let (_, start, end) = location_to_range(location)?;
    let s = start.saturating_sub(1);
    let e = end.min(lines.len());
    if s >= e {
        return None;
    }
    let body = lines[s..e].join("\n");
    if body.len() > 4000 {
        Some(body.chars().take(4000).collect())
    } else {
        Some(body)
    }
}

fn location_to_range(location: &str) -> Option<(String, usize, usize)> {
    let (path, rest) = location.rsplit_once(':')?;
    let (a, b) = rest.split_once('-')?;
    let start: usize = a.parse().ok()?;
    let end: usize = b.parse().ok()?;
    Some((path.to_string(), start, end))
}

fn chat(client: &reqwest::blocking::Client, cfg: &LlmConfig, prompt: &str) -> Result<String> {
    let url = format!("{}/chat/completions", cfg.base_url);
    let body = ChatRequest {
        model: cfg.model.clone(),
        messages: vec![
            ChatMessage {
                role: "system".into(),
                content: "You write terse, accurate code-symbol labels.".into(),
            },
            ChatMessage {
                role: "user".into(),
                content: prompt.to_string(),
            },
        ],
        temperature: 0.1,
        max_tokens: 64,
    };
    let mut last_err = None;
    for attempt in 0..3 {
        let resp = client
            .post(&url)
            .bearer_auth(&cfg.api_key)
            .json(&body)
            .send();
        let resp = match resp {
            Ok(r) => r,
            Err(e) => {
                last_err = Some(anyhow!("LLM HTTP request: {e}"));
                std::thread::sleep(std::time::Duration::from_millis(200 * (attempt + 1)));
                continue;
            }
        };
        if resp.status().is_server_error() {
            let status = resp.status();
            last_err = Some(anyhow!("LLM API {status}"));
            std::thread::sleep(std::time::Duration::from_millis(200 * (attempt + 1)));
            continue;
        }
        if !resp.status().is_success() {
            let status = resp.status();
            let text = resp.text().unwrap_or_default();
            return Err(anyhow!("LLM API {status}: {text}"));
        }
        let parsed: ChatResponse = resp.json().context("parse LLM response")?;
        return Ok(parsed
            .choices
            .first()
            .and_then(|c| c.message.content.clone())
            .unwrap_or_default());
    }
    Err(last_err.unwrap_or_else(|| anyhow!("LLM request failed")))
}
