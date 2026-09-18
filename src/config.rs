//! P2-1 — repo/project-level macro default config.
//!
//! **Primary file:** `<root>/.agentgraph/config.toml`
//! **Fallback file:** `<root>/agentgraph.toml`
//! **Env override:** `AGENTGRAPH_MACRO_DEFAULT=off|if_fresh|on`
//! **CLI explicit wins:** `--include-macro` / `--no-include-macro`
//! (graph: `--with-macro` / `--no-with-macro`).
//!
//! Field: `macro_default = "off" | "if_fresh" | "on"`.
//!
//! **Global default remains OFF.** No silent global `--with-macro`.
//! Even `"on"` still refuses when the sidecar is missing / stale / nested /
//! expanded_root missing, or when a sound window is selected (macro edges
//! are not sound-certified).
//!
//! Docs: `docs/macro-sidecar.md`, `docs/agent-recipes.md`.

use std::path::{Path, PathBuf};

/// Repo/project macro default policy (P2-1).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MacroDefaultPolicy {
    /// Never auto-include macro candidates (global product default).
    Off,
    /// Auto-include only when a fresh non-stale non-nested sidecar exists.
    IfFresh,
    /// Request include when possible; health gates still refuse bad sidecars
    /// and sound windows.
    On,
}

impl MacroDefaultPolicy {
    pub fn as_str(&self) -> &'static str {
        match self {
            MacroDefaultPolicy::Off => "off",
            MacroDefaultPolicy::IfFresh => "if_fresh",
            MacroDefaultPolicy::On => "on",
        }
    }

    /// Whether this policy requests macro include when CLI is not explicit.
    pub fn requests_include(&self) -> bool {
        matches!(self, MacroDefaultPolicy::IfFresh | MacroDefaultPolicy::On)
    }

    /// Payload reason when auto-include is **allowed** after health gates.
    pub fn auto_reason(&self) -> Option<&'static str> {
        match self {
            MacroDefaultPolicy::Off => None,
            MacroDefaultPolicy::IfFresh => Some("repo_config_if_fresh"),
            MacroDefaultPolicy::On => Some("repo_config_on"),
        }
    }
}

/// Where the effective macro_default came from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MacroConfigSource {
    /// No file + no env — product global default is OFF.
    BuiltinDefault,
    /// `AGENTGRAPH_MACRO_DEFAULT`.
    Env,
    /// Repo/workspace config file.
    File { path: PathBuf },
}

/// Effective macro_default resolution for one root/workspace.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MacroDefaultConfig {
    pub policy: MacroDefaultPolicy,
    pub source: MacroConfigSource,
}

impl Default for MacroDefaultConfig {
    fn default() -> Self {
        Self {
            policy: MacroDefaultPolicy::Off,
            source: MacroConfigSource::BuiltinDefault,
        }
    }
}

impl MacroDefaultConfig {
    pub fn source_label(&self) -> String {
        match &self.source {
            MacroConfigSource::BuiltinDefault => "builtin_default".to_string(),
            MacroConfigSource::Env => "env:AGENTGRAPH_MACRO_DEFAULT".to_string(),
            MacroConfigSource::File { path } => {
                let p = path.to_string_lossy().replace('\\', "/");
                format!("file:{p}")
            }
        }
    }

    pub fn requests_include(&self) -> bool {
        self.policy.requests_include()
    }

    pub fn auto_reason(&self) -> Option<&'static str> {
        self.policy.auto_reason()
    }
}

/// Parse a `macro_default` string value. Returns `None` when unrecognized.
pub fn parse_macro_default_policy(raw: &str) -> Option<MacroDefaultPolicy> {
    let s = raw
        .trim()
        .trim_matches(|c| c == '"' || c == '\'')
        .to_ascii_lowercase();
    match s.as_str() {
        "off" | "false" | "0" | "never" => Some(MacroDefaultPolicy::Off),
        "if_fresh" | "iffresh" | "if-fresh" => Some(MacroDefaultPolicy::IfFresh),
        "on" | "true" | "1" | "always" => Some(MacroDefaultPolicy::On),
        _ => None,
    }
}

/// Minimal top-level TOML extract for `macro_default`.
///
/// Accepts:
/// - `macro_default = "if_fresh"`
/// - keys under `[agentgraph]` / `[macro]` / `[agentgraph.macro]`
///
/// Comments (`# …`) are stripped per line. Unknown keys ignored.
pub fn extract_macro_default_from_toml(text: &str) -> Option<String> {
    let mut section = String::new();
    for line in text.lines() {
        let line = line.split('#').next().unwrap_or("").trim();
        if line.is_empty() {
            continue;
        }
        if line.starts_with('[') && line.ends_with(']') {
            section = line[1..line.len() - 1].trim().to_ascii_lowercase();
            continue;
        }
        let Some(eq) = line.find('=') else {
            continue;
        };
        let key = line[..eq].trim().to_ascii_lowercase();
        if key != "macro_default" {
            continue;
        }
        // Top-level or known macro/agentgraph sections only.
        if section.is_empty()
            || section == "agentgraph"
            || section == "macro"
            || section == "agentgraph.macro"
        {
            let val = line[eq + 1..].trim().to_string();
            return Some(val);
        }
    }
    None
}

/// Candidate config files under one root, primary first.
pub fn macro_config_file_candidates(root: &Path) -> Vec<PathBuf> {
    vec![
        root.join(".agentgraph").join("config.toml"),
        root.join("agentgraph.toml"),
    ]
}

/// Load effective macro_default for a primary root (file + env overlay).
///
/// Priority (highest wins):
/// 1. env `AGENTGRAPH_MACRO_DEFAULT`
/// 2. `<root>/.agentgraph/config.toml`
/// 3. `<root>/agentgraph.toml`
/// 4. builtin `off`
///
/// Invalid env/file values fall through to the next source (never a silent
/// macro-on from a typo).
pub fn load_macro_default_config(root: &Path) -> MacroDefaultConfig {
    load_macro_default_config_for_roots(&[root.to_path_buf()])
}

/// Probe roots in order; first file that defines `macro_default` wins.
/// Env still overrides any file.
pub fn load_macro_default_config_for_roots(roots: &[PathBuf]) -> MacroDefaultConfig {
    if let Ok(raw) = std::env::var("AGENTGRAPH_MACRO_DEFAULT") {
        if let Some(policy) = parse_macro_default_policy(&raw) {
            return MacroDefaultConfig {
                policy,
                source: MacroConfigSource::Env,
            };
        }
    }
    for root in roots {
        for path in macro_config_file_candidates(root) {
            if !path.is_file() {
                continue;
            }
            let Ok(text) = std::fs::read_to_string(&path) else {
                continue;
            };
            if let Some(raw) = extract_macro_default_from_toml(&text) {
                if let Some(policy) = parse_macro_default_policy(&raw) {
                    return MacroDefaultConfig {
                        policy,
                        source: MacroConfigSource::File { path: path.clone() },
                    };
                }
            }
        }
    }
    MacroDefaultConfig::default()
}

/// Resolve whether recipes should **request** `include_macro` when the
/// operator did / did not pass an explicit CLI or MCP boolean.
///
/// Returns `(request_include, success_reason_from_config)`.
/// - `cli == Some(v)` → request `v`; success reason stays `None` (CLI explicit)
/// - `cli == None` + policy requests → request true with policy auto reason
/// - otherwise → no request
pub fn resolve_macro_include_request(
    cli: Option<bool>,
    cfg: &MacroDefaultConfig,
) -> (bool, Option<String>) {
    match cli {
        Some(true) => (true, None),
        Some(false) => (false, None),
        None => {
            if cfg.requests_include() {
                (true, cfg.auto_reason().map(|s| s.to_string()))
            } else {
                (false, None)
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builtin_default_is_off() {
        let cfg = MacroDefaultConfig::default();
        assert_eq!(cfg.policy, MacroDefaultPolicy::Off);
        assert_eq!(cfg.source_label(), "builtin_default");
        assert!(!cfg.requests_include());
        assert_eq!(cfg.auto_reason(), None);
    }

    #[test]
    fn parse_policy_aliases() {
        assert_eq!(
            parse_macro_default_policy("off"),
            Some(MacroDefaultPolicy::Off)
        );
        assert_eq!(
            parse_macro_default_policy("if_fresh"),
            Some(MacroDefaultPolicy::IfFresh)
        );
        assert_eq!(
            parse_macro_default_policy("ON"),
            Some(MacroDefaultPolicy::On)
        );
        assert_eq!(parse_macro_default_policy("nope"), None);
    }

    #[test]
    fn extract_from_toml_shapes() {
        assert_eq!(
            extract_macro_default_from_toml("macro_default = \"if_fresh\"\n"),
            Some("\"if_fresh\"".into())
        );
        assert_eq!(
            extract_macro_default_from_toml("# comment\n[agentgraph]\nmacro_default = 'on'\n"),
            Some("'on'".into())
        );
        assert_eq!(extract_macro_default_from_toml("other = 1\n"), None);
    }

    #[test]
    fn cli_explicit_wins_over_config() {
        let cfg = MacroDefaultConfig {
            policy: MacroDefaultPolicy::IfFresh,
            source: MacroConfigSource::Env,
        };
        let (req, reason) = resolve_macro_include_request(Some(true), &cfg);
        assert!(req);
        assert!(reason.is_none());
        let (req2, reason2) = resolve_macro_include_request(Some(false), &cfg);
        assert!(!req2);
        assert!(reason2.is_none());
    }

    #[test]
    fn config_if_fresh_requests_with_reason() {
        let cfg = MacroDefaultConfig {
            policy: MacroDefaultPolicy::IfFresh,
            source: MacroConfigSource::BuiltinDefault,
        };
        let (req, reason) = resolve_macro_include_request(None, &cfg);
        assert!(req);
        assert_eq!(reason.as_deref(), Some("repo_config_if_fresh"));
    }
}
