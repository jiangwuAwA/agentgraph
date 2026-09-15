//! Optional LLM enrichment: one-line responsibility descriptions for symbols.
//! OpenAI-compatible Chat Completions API.
//!
//! Env:
//!   OPENAI_API_KEY     (required)
//!   OPENAI_BASE_URL    (default https://api.openai.com/v1)
//!   AGENTGRAPH_MODEL   (default gpt-4o-mini)

use anyhow::{anyhow, bail, Context, Result};
use serde::{Deserialize, Serialize};
use std::path::Path;

use super::store::Store;
use crate::model::EnrichReport;

#[derive(Debug, Clone)]
pub struct LlmConfig {
    pub api_key: String,
    pub base_url: String,
    pub model: String,
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
        let model =
            std::env::var("AGENTGRAPH_MODEL").unwrap_or_else(|_| "gpt-4o-mini".to_string());
        Ok(Self {
            api_key,
            base_url,
            model,
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

/// Enrich up to `limit` undescribed symbols. `root` is used to read source snippets.
pub fn enrich(root: &Path, store: &mut Store, cfg: &LlmConfig, limit: usize) -> Result<EnrichReport> {
    let pending = store.symbols_needing_description(limit)?;
    let client = reqwest::blocking::Client::builder()
        .timeout(std::time::Duration::from_secs(60))
        .build()?;

    let mut described = 0usize;
    let mut failed = 0usize;
    let mut skipped = 0usize;
    let attempted = pending.len();

    for (id, name, kind, path, location) in pending {
        let full_path = root.join(&path);
        let snippet = match read_snippet(&full_path, &location) {
            Some(s) => s,
            None => {
                skipped += 1;
                continue;
            }
        };
        let prompt = format!(
            "You are labeling code symbols for an agent-facing code graph.\n\
             Reply with ONE short sentence (max 20 words) describing what this {kind} does.\n\
             No preamble, no markdown, no quotes.\n\n\
             Symbol: {name}\nKind: {kind}\nFile: {path}\n\n\
             Code:\n```\n{snippet}\n```"
        );

        match chat(&client, cfg, &prompt) {
            Ok(text) => {
                let desc = text.trim().trim_matches('"').to_string();
                if desc.is_empty() {
                    skipped += 1;
                    continue;
                }
                store.set_description(id, &desc)?;
                described += 1;
                eprintln!("described {path}::{name} — {desc}");
            }
            Err(e) => {
                failed += 1;
                eprintln!("failed {path}::{name}: {e:#}");
                if failed >= 3 {
                    bail!("too many LLM failures ({failed}), aborting enrich");
                }
            }
        }
    }

    Ok(EnrichReport {
        attempted,
        described,
        skipped,
        failed,
        model: cfg.model.clone(),
    })
}

fn location_to_range(location: &str) -> Option<(String, usize, usize)> {
    // "path:start-end"
    let (path, rest) = location.rsplit_once(':')?;
    let (a, b) = rest.split_once('-')?;
    let start: usize = a.parse().ok()?;
    let end: usize = b.parse().ok()?;
    Some((path.to_string(), start, end))
}

fn read_snippet(full_path: &Path, location: &str) -> Option<String> {
    let (_, start, end) = location_to_range(location)?;
    let src = std::fs::read_to_string(full_path).ok()?;
    let lines: Vec<&str> = src.lines().collect();
    let s = start.saturating_sub(1);
    let e = end.min(lines.len());
    if s >= e {
        return None;
    }
    // Cap snippet size
    let body = lines[s..e].join("\n");
    if body.len() > 4000 {
        Some(body.chars().take(4000).collect())
    } else {
        Some(body)
    }
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
    let resp = client
        .post(&url)
        .bearer_auth(&cfg.api_key)
        .json(&body)
        .send()
        .context("LLM HTTP request")?;
    if !resp.status().is_success() {
        let status = resp.status();
        let text = resp.text().unwrap_or_default();
        return Err(anyhow!("LLM API {status}: {text}"));
    }
    let parsed: ChatResponse = resp.json().context("parse LLM response")?;
    let content = parsed
        .choices
        .first()
        .and_then(|c| c.message.content.clone())
        .unwrap_or_default();
    Ok(content)
}
