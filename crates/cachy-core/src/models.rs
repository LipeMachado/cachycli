//! Model catalog: built-in presets, custom `[model.*]` tables and resolution.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::config::{Config, ModelConfig};

/// Which wire protocol a model speaks.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum ApiBackend {
    /// OpenAI-compatible `/chat/completions`.
    #[default]
    Chat,
    /// OpenAI Responses API (`/responses`).
    Responses,
    /// Anthropic Messages API (`/v1/messages`).
    Anthropic,
}

impl ApiBackend {
    pub fn parse(s: &str) -> Self {
        match s.to_ascii_lowercase().as_str() {
            "responses" => ApiBackend::Responses,
            "messages" | "anthropic" => ApiBackend::Anthropic,
            _ => ApiBackend::Chat,
        }
    }
    pub fn as_str(&self) -> &'static str {
        match self {
            ApiBackend::Chat => "chat",
            ApiBackend::Responses => "responses",
            ApiBackend::Anthropic => "messages",
        }
    }
}

/// A fully resolved model definition.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Model {
    /// Catalog name (what the user types in `/model`).
    pub name: String,
    /// Identifier sent to the API.
    pub model_id: String,
    pub base_url: String,
    pub api_backend: ApiBackend,
    pub display_name: String,
    pub description: Option<String>,
    pub api_key: Option<String>,
    pub env_keys: Vec<String>,
    pub auth_provider: Option<String>,
    pub temperature: Option<f32>,
    pub top_p: Option<f32>,
    pub max_completion_tokens: Option<u32>,
    pub context_window: u64,
    pub context_windows: Vec<u64>,
    pub headers: BTreeMap<String, String>,
    /// Built-in presets can be overridden by user config.
    pub builtin: bool,
}

impl Model {
    pub fn display(&self) -> &str {
        if self.display_name.is_empty() {
            &self.name
        } else {
            &self.display_name
        }
    }
}

/// Built-in catalog presets. These only need an API key (env var).
pub fn builtin_models() -> Vec<Model> {
    let mut out = Vec::new();
    let mut push = |name: &str,
                    model_id: &str,
                    base_url: &str,
                    display: &str,
                    env: &[&str],
                    ctx: u64,
                    backend: ApiBackend| {
        out.push(Model {
            name: name.into(),
            model_id: model_id.into(),
            base_url: base_url.into(),
            api_backend: backend,
            display_name: display.into(),
            description: None,
            api_key: None,
            env_keys: env.iter().map(|s| s.to_string()).collect(),
            auth_provider: None,
            temperature: None,
            top_p: None,
            max_completion_tokens: None,
            context_window: ctx,
            context_windows: Vec::new(),
            headers: BTreeMap::new(),
            builtin: true,
        });
    };
    push(
        "gpt-5",
        "gpt-5",
        "https://api.openai.com/v1",
        "GPT-5 (OpenAI)",
        &["OPENAI_API_KEY"],
        400_000,
        ApiBackend::Chat,
    );
    push(
        "gpt-4o",
        "gpt-4o",
        "https://api.openai.com/v1",
        "GPT-4o (OpenAI)",
        &["OPENAI_API_KEY"],
        128_000,
        ApiBackend::Chat,
    );
    push(
        "claude-sonnet-4-5",
        "claude-sonnet-4-5",
        "https://api.anthropic.com",
        "Claude Sonnet 4.5",
        &["ANTHROPIC_API_KEY"],
        200_000,
        ApiBackend::Anthropic,
    );
    push(
        "ollama",
        "llama3.1",
        "http://localhost:11434/v1",
        "Ollama (local)",
        &[],
        128_000,
        ApiBackend::Chat,
    );
    push(
        "lmstudio",
        "local-model",
        "http://localhost:1234/v1",
        "LM Studio (local)",
        &[],
        32_000,
        ApiBackend::Chat,
    );
    push(
        "vllm",
        "default",
        "http://localhost:8000/v1",
        "vLLM (local)",
        &[],
        32_000,
        ApiBackend::Chat,
    );
    push(
        "opencode-zen",
        "opencode",
        "https://opencode.ai/zen/v1",
        "OpenCode Zen",
        &["OPENCODE_API_KEY", "OPENCODE_ZEN_API_KEY"],
        200_000,
        ApiBackend::Chat,
    );
    push(
        "grok-4",
        "grok-4",
        "https://api.x.ai/v1",
        "Grok 4 (xAI)",
        &["XAI_API_KEY"],
        256_000,
        ApiBackend::Chat,
    );
    push(
        "grok-4-mini",
        "grok-4-mini",
        "https://api.x.ai/v1",
        "Grok 4 Mini (xAI)",
        &["XAI_API_KEY"],
        256_000,
        ApiBackend::Chat,
    );
    out
}

/// Build the full catalog: builtins overridden/extended by custom models.
pub fn catalog(config: &Config) -> Vec<Model> {
    let mut map: BTreeMap<String, Model> = BTreeMap::new();
    for m in builtin_models() {
        map.insert(m.name.clone(), m);
    }
    let endpoints_base = config.endpoints.models_base_url.clone();
    for (name, cfg) in &config.custom_models {
        apply_custom(&mut map, name, cfg, endpoints_base.as_deref());
    }
    map.into_values().collect()
}

fn apply_custom(
    map: &mut BTreeMap<String, Model>,
    name: &str,
    cfg: &ModelConfig,
    endpoints_base: Option<&str>,
) {
    let entry = map.entry(name.to_string()).or_insert_with(|| Model {
        name: name.to_string(),
        model_id: name.to_string(),
        base_url: endpoints_base
            .unwrap_or("https://api.openai.com/v1")
            .to_string(),
        api_backend: ApiBackend::Chat,
        display_name: name.to_string(),
        description: None,
        api_key: None,
        env_keys: Vec::new(),
        auth_provider: None,
        temperature: None,
        top_p: None,
        max_completion_tokens: None,
        context_window: 128_000,
        context_windows: Vec::new(),
        headers: BTreeMap::new(),
        builtin: false,
    });
    entry.model_id = cfg.model.clone().unwrap_or_else(|| name.to_string());
    if let Some(url) = &cfg.base_url {
        entry.base_url = url.clone();
    }
    if let Some(backend) = &cfg.api_backend {
        entry.api_backend = ApiBackend::parse(backend);
    }
    if let Some(display) = &cfg.name {
        entry.display_name = display.clone();
    }
    if let Some(desc) = &cfg.description {
        entry.description = Some(desc.clone());
    }
    if let Some(key) = &cfg.api_key {
        entry.api_key = Some(key.clone());
    }
    if let Some(env) = &cfg.env_key {
        entry.env_keys = env.clone();
    }
    if let Some(p) = cfg.auth_provider.clone() {
        entry.auth_provider = Some(p);
    }
    entry.temperature = cfg.temperature.or(entry.temperature);
    entry.top_p = cfg.top_p.or(entry.top_p);
    entry.max_completion_tokens = cfg.max_completion_tokens.or(entry.max_completion_tokens);
    if let Some(cw) = cfg.context_window {
        entry.context_window = cw;
    }
    if !cfg.context_windows.is_empty() {
        entry.context_windows = cfg.context_windows.clone();
        if cfg.context_window.is_none() {
            entry.context_window = cfg.context_windows[0];
        }
    }
    entry.headers.extend(cfg.headers.clone());
    entry.builtin = false;
}

/// Resolve a model name to a `Model`, applying the default when `None`.
pub fn resolve(config: &Config, name: Option<&str>) -> Option<Model> {
    let catalog = catalog(config);
    let target = name
        .map(|s| s.to_string())
        .or_else(|| config.models.default.clone());
    if let Some(target) = target {
        if let Some(m) = catalog.iter().find(|m| m.name == target) {
            return Some(m.clone());
        }
        // Unknown name: treat it as a raw model id against the default endpoint.
        let base = config
            .endpoints
            .models_base_url
            .clone()
            .unwrap_or_else(|| "https://api.openai.com/v1".into());
        return Some(Model {
            name: target.clone(),
            model_id: target,
            base_url: base,
            api_backend: ApiBackend::Chat,
            display_name: String::new(),
            description: None,
            api_key: None,
            env_keys: vec!["OPENAI_API_KEY".into()],
            auth_provider: None,
            temperature: None,
            top_p: None,
            max_completion_tokens: None,
            context_window: 128_000,
            context_windows: Vec::new(),
            headers: BTreeMap::new(),
            builtin: false,
        });
    }
    // No default: pick the first model with an available credential, else the
    // first builtin.
    let with_key = catalog.iter().find(|m| {
        m.api_key.is_some()
            || m.env_keys
                .iter()
                .any(|k| std::env::var(k).map(|v| !v.is_empty()).unwrap_or(false))
    });
    with_key.cloned().or_else(|| catalog.first().cloned())
}

/// The resolved API key for a model (config key first, then env vars).
pub fn api_key_for(model: &Model) -> Option<String> {
    if let Some(k) = &model.api_key {
        if !k.is_empty() {
            return Some(k.clone());
        }
    }
    for env in &model.env_keys {
        if let Ok(v) = std::env::var(env) {
            if !v.is_empty() {
                return Some(v);
            }
        }
    }
    // OpenCode Zen shared key fallback.
    if model.base_url.contains("opencode.ai/zen") {
        if let Ok(v) = std::env::var("OPENCODE_API_KEY") {
            if !v.is_empty() {
                return Some(v);
            }
        }
    }
    None
}

/// Header list for a model request (auth + custom headers).
pub fn auth_headers(model: &Model) -> Vec<(String, String)> {
    let mut headers = Vec::new();
    if let Some(key) = api_key_for(model) {
        match model.api_backend {
            ApiBackend::Anthropic => {
                headers.push(("x-api-key".into(), key.clone()));
                headers.push(("anthropic-version".into(), "2023-06-01".into()));
            }
            _ => headers.push(("authorization".into(), format!("Bearer {key}"))),
        }
    }
    for (k, v) in &model.headers {
        headers.push((k.clone(), v.clone()));
    }
    headers
}
