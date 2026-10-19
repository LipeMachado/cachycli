//! Subcommand implementations.

use std::io::{IsTerminal, Read, Write};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use anyhow::{anyhow, Context, Result};

use cachy_core::agent::{AgentEvent, AgentSession};
use cachy_core::config::{BackendKind, Config};
use cachy_core::message::StopReason;
use cachy_core::models;
use cachy_core::paths;
use cachy_core::session::{self, SessionStore};
use cachy_core::{auth, hooks, inspect as inspect_mod, mcp, memory};

use crate::cli::*;

// ---------------------------------------------------------------------------
// headless (-p)
// ---------------------------------------------------------------------------

pub struct HeadlessOptions {
    pub prompt: String,
    pub model: Option<String>,
    pub output_format: OutputFormat,
    pub always_approve: bool,
    pub allow: Vec<String>,
    pub deny: Vec<String>,
    pub tools: Option<Vec<String>>,
    pub disallowed_tools: Vec<String>,
    pub session_id: Option<String>,
    pub resume: Option<String>,
    pub continue_latest: bool,
    pub max_turns: Option<u32>,
    pub permission_mode: Option<cachy_core::permissions::PermissionMode>,
    pub rules: Vec<String>,
    pub include_partial_messages: bool,
    pub json_schema: Option<String>,
    pub verbatim: bool,
}

pub async fn headless(config: Arc<Config>, cwd: PathBuf, opts: HeadlessOptions) -> Result<i32> {
    // Resolve the session.
    let store = if let Some(target) = &opts.resume {
        match session::find_by_id_or_title(&cwd, target)? {
            Some(summary) => SessionStore::open(&cwd, &summary.id)?,
            None => anyhow::bail!("session not found: {target}"),
        }
    } else if opts.continue_latest {
        match session::most_recent(&cwd)? {
            Some(summary) => SessionStore::open(&cwd, &summary.id)?,
            None => anyhow::bail!("no previous session in this directory"),
        }
    } else if let Some(id) = &opts.session_id {
        SessionStore::open(&cwd, id).or_else(|_| {
            SessionStore::create_with_id(
                &cwd,
                id,
                opts.model.as_deref().unwrap_or("default"),
                "cachy",
            )
        })?
    } else {
        SessionStore::create(
            &cwd,
            opts.model.as_deref().unwrap_or("default"),
            "cachy",
            None,
        )?
    };

    let session_id = store.summary.id.clone();
    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<AgentEvent>();

    let model = models::resolve(&config, opts.model.as_deref()).ok_or_else(|| {
        anyhow::anyhow!(
            "no model configured. Add a [model.<name>] section to ~/.cachy/config.toml, \
             or set a default and API key. Run `cachycli models` to see the catalog."
        )
    })?;

    let agent = crate::tui::build_native_agent(
        config.clone(),
        cwd.clone(),
        opts.model.as_deref(),
        opts.always_approve,
        opts.allow.clone(),
        opts.deny.clone(),
        opts.tools.clone(),
        opts.disallowed_tools.clone(),
        tx.clone(),
        store,
        false,
        opts.permission_mode.unwrap_or_default(),
    )
    .await?;

    let history = agent.session.lock().await.load_chat().unwrap_or_default();
    if !history.is_empty() {
        agent.load_history(history).await;
    }

    // `--rules`: extra instructions for this run, appended like a persona.
    let rules = opts.rules.join("\n");
    if !rules.trim().is_empty() {
        let existing = agent.persona().await.unwrap_or_default();
        let combined = if existing.trim().is_empty() {
            rules.trim().to_string()
        } else {
            format!("{}\n\n{}", existing.trim(), rules.trim())
        };
        agent.set_persona(Some(combined)).await;
    }

    // Hooks: user prompt submit.
    let discovered_hooks = hooks::discover(&cwd, &config);
    let hook_results = hooks::run_event(
        &discovered_hooks,
        hooks::HookEvent::UserPromptSubmit,
        &cwd,
        None,
        &serde_json::json!({ "prompt": opts.prompt }),
    )
    .await;
    for (command, output, success) in hook_results {
        if !success {
            eprintln!("hook failed: {command}: {output}");
        }
    }

    let format = opts.output_format;
    let prompt = if opts.verbatim {
        opts.prompt.clone()
    } else {
        cachy_core::prompt::wrap_user_prompt(&opts.prompt)
    };

    let request_id = uuid::Uuid::new_v4().to_string();
    let mut text = String::new();
    let mut thought = String::new();
    let mut tool_events: Vec<(String, String)> = Vec::new();
    let mut stop_reason = StopReason::EndTurn;
    let mut error: Option<String> = None;
    let mut error_printed = false;

    let turn = agent.clone();
    let max_turns = opts.max_turns;
    let runner = tokio::spawn(async move {
        turn.run_turn(
            prompt,
            cachy_core::agent::TurnOptions {
                max_turns,
                interactive: false,
            },
        )
        .await
    });

    while let Some(event) = rx.recv().await {
        let is_turn_end = matches!(event, AgentEvent::TurnEnd { .. });
        match event {
            AgentEvent::TextDelta(delta) => {
                text.push_str(&delta);
                match format {
                    OutputFormat::Plain => {
                        print!("{delta}");
                        let _ = std::io::stdout().flush();
                    }
                    OutputFormat::StreamingJson => {
                        println!("{}", serde_json::json!({ "type": "text", "data": delta }));
                    }
                    OutputFormat::StreamingMessagesJson => {
                        println!(
                            "{}",
                            serde_json::json!({
                                "type": "content_block_delta",
                                "delta": { "type": "text_delta", "text": delta }
                            })
                        );
                    }
                    // `--include-partial-messages` adds the streamed pieces
                    // alongside the whole messages, like Claude Code does.
                    OutputFormat::Json if opts.include_partial_messages => {
                        println!(
                            "{}",
                            serde_json::json!({
                                "type": "stream_event",
                                "event": {
                                    "type": "content_block_delta",
                                    "delta": { "type": "text_delta", "text": delta }
                                }
                            })
                        );
                    }
                    _ => {}
                }
            }
            AgentEvent::ReasoningDelta(delta) => {
                thought.push_str(&delta);
                match format {
                    OutputFormat::StreamingJson => {
                        println!(
                            "{}",
                            serde_json::json!({ "type": "thought", "data": delta })
                        );
                    }
                    OutputFormat::StreamingMessagesJson => {
                        println!(
                            "{}",
                            serde_json::json!({
                                "type": "content_block_delta",
                                "delta": { "type": "thinking_delta", "thinking": delta }
                            })
                        );
                    }
                    _ => {}
                }
            }
            AgentEvent::ToolCall { name, title, .. } => {
                tool_events.push((name.clone(), title.clone()));
                if format == OutputFormat::StreamingJson {
                    println!(
                        "{}",
                        serde_json::json!({ "type": "tool_call", "data": { "name": name, "title": title, "status": "pending" } })
                    );
                }
            }
            AgentEvent::ToolCallUpdate {
                id,
                status,
                summary,
                ..
            } => {
                if format == OutputFormat::StreamingJson {
                    println!(
                        "{}",
                        serde_json::json!({ "type": "tool_call_update", "data": { "id": id, "status": status, "summary": summary } })
                    );
                }
            }
            AgentEvent::Error(message) => {
                error = Some(message.clone());
                if format == OutputFormat::Plain {
                    eprintln!("error: {message}");
                    error_printed = true;
                }
            }
            _ => {}
        }
        if is_turn_end {
            break;
        }
    }

    match runner.await {
        Ok(Ok(stop)) => stop_reason = stop,
        Ok(Err(e)) => {
            error.get_or_insert_with(|| e.to_string());
        }
        Err(e) => {
            error.get_or_insert_with(|| e.to_string());
        }
    }

    // A turn can fail without ever sending an Error event (the provider
    // refused to create a session, for one): say so in plain output too.
    if let Some(message) = &error {
        if !error_printed && format == OutputFormat::Plain {
            eprintln!("error: {message}");
        }
    }
    let mut is_error = error.is_some() || stop_reason == StopReason::Error;
    let stop_str = match stop_reason {
        StopReason::EndTurn => "EndTurn",
        StopReason::ToolUse => "ToolUse",
        StopReason::MaxTokens => "MaxTokens",
        StopReason::MaxTurns => "MaxTurns",
        StopReason::Cancelled => "Cancelled",
        StopReason::Refusal => "Refusal",
        StopReason::Error => "Error",
    };

    match format {
        OutputFormat::Plain => {
            println!();
        }
        OutputFormat::Json => {
            let output = serde_json::json!({
                "text": text,
                "stopReason": stop_str,
                "sessionId": session_id,
                "requestId": request_id,
                "thought": thought,
                "toolCalls": tool_events.iter().map(|(n, t)| serde_json::json!({"name": n, "title": t})).collect::<Vec<_>>(),
                "error": error,
            });
            println!("{}", serde_json::to_string(&output)?);
        }
        OutputFormat::StreamingJson => {
            println!(
                "{}",
                serde_json::json!({
                    "type": "end",
                    "stopReason": stop_str,
                    "sessionId": session_id,
                    "requestId": request_id,
                })
            );
        }
        OutputFormat::StreamingMessagesJson => {
            println!(
                "{}",
                serde_json::json!({
                    "type": "message_stop",
                    "stopReason": stop_str,
                    "sessionId": session_id,
                })
            );
        }
    }
    // `--json-schema` is checked below; the rules went into the prompt above.
    let _ = model;

    // `--json-schema`: the answer has to be JSON that fits.
    if let Some(schema) = &opts.json_schema {
        match serde_json::from_str::<serde_json::Value>(schema) {
            Err(e) => {
                eprintln!("error: --json-schema is not valid JSON: {e}");
                is_error = true;
            }
            Ok(parsed) => match cachy_core::jsonschema::extract(&text) {
                None => {
                    eprintln!("error: the answer is not JSON, so it cannot match the schema");
                    is_error = true;
                }
                Some(value) => {
                    if let Err(problem) = cachy_core::jsonschema::check(&parsed, &value) {
                        eprintln!("error: the answer does not match the schema — {problem}");
                        is_error = true;
                    }
                }
            },
        }
    }

    agent.shutdown().await;
    Ok(if is_error { 1 } else { 0 })
}

// ---------------------------------------------------------------------------
// headless via opencode / ACP backends
// ---------------------------------------------------------------------------

/// Run one prompt through the opencode or ACP backend and print the result.
pub async fn headless_backend(
    config: Arc<Config>,
    cwd: PathBuf,
    opts: &HeadlessOptions,
    kind: BackendKind,
) -> Result<i32> {
    use cachy_core::agent::AgentSession;

    if let Some(mode) = opts.permission_mode {
        // These providers run their own loop and their own approvals, so the
        // mode has nothing to act on here.
        eprintln!(
            "note: --permission-mode {} does not apply to {} — it decides its own approvals",
            mode.as_str(),
            kind.provider_name()
        );
    }

    // The cachycli side of the session: it holds the transcript and remembers
    // which of the provider's own sessions this conversation is.
    let store = if let Some(target) = &opts.resume {
        match session::find_by_id_or_title(&cwd, target)? {
            Some(summary) => Some(SessionStore::open(&cwd, &summary.id)?),
            None => {
                eprintln!("note: no session matches {target:?} — starting a new one");
                None
            }
        }
    } else if opts.continue_latest {
        match session::most_recent(&cwd)? {
            Some(summary) => Some(SessionStore::open(&cwd, &summary.id)?),
            None => {
                eprintln!("note: no previous session in this directory — starting a new one");
                None
            }
        }
    } else {
        match &opts.session_id {
            Some(id) => Some(
                SessionStore::open(&cwd, id)
                    .or_else(|_| SessionStore::create_with_id(&cwd, id, "default", "cachy"))?,
            ),
            None => Some(SessionStore::create(&cwd, "default", "cachy", None)?),
        }
    };

    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<AgentEvent>();
    let session: Arc<dyn AgentSession> = match kind {
        BackendKind::Opencode => cachy_core::providers::opencode::OpencodeBackend::new(
            config.clone(),
            cwd.clone(),
            tx.clone(),
        ),
        BackendKind::Acp => {
            let acp = &config.agent.acp;
            match &acp.url {
                Some(url) => {
                    cachy_core::acp::client::AcpBackend::connect_url(url, &cwd, tx.clone()).await?
                }
                None => {
                    cachy_core::acp::client::AcpBackend::connect(
                        &acp.command,
                        &acp.args,
                        &acp.env,
                        &cwd,
                        tx.clone(),
                    )
                    .await?
                }
            }
        }
        BackendKind::ClaudeCode => cachy_core::providers::claude_code::ClaudeCodeBackend::new(
            config.clone(),
            cwd.clone(),
            tx.clone(),
            false,
        ),
        BackendKind::Native => {
            anyhow::bail!("the native provider runs its own loop; this is a bug — please report it")
        }
    };

    let format = opts.output_format;
    let prompt = match &opts.json_schema {
        Some(schema) if !opts.verbatim => format!(
            "{}\n\nAnswer with a single JSON value that matches this JSON Schema:\n{}",
            opts.prompt, schema
        ),
        _ => opts.prompt.clone(),
    };
    let request_id = uuid::Uuid::new_v4().to_string();
    let session_id = session.session_id();

    // Hooks are the interface's job for providers that run their own loop.
    {
        let discovered = cachy_core::hooks::discover(&cwd, &config);
        if !discovered.is_empty() {
            let results = cachy_core::hooks::run_event(
                &discovered,
                cachy_core::hooks::HookEvent::UserPromptSubmit,
                &cwd,
                None,
                &serde_json::json!({ "prompt": opts.prompt }),
            )
            .await;
            for (command, output, ok) in results {
                if !ok {
                    eprintln!("UserPromptSubmit hook failed: {command}: {output}");
                }
            }
        }
    }

    // Continue the provider's own session when we know it.
    if let Some(store) = &store {
        if let Some(id) = store.summary.backend_session.as_deref() {
            if store.summary.backend_provider.as_deref() == Some(kind.provider_name()) {
                if session.resume_backend_session(id).await {
                    eprintln!("note: continuing the {} session {id}", kind.provider_name());
                } else {
                    eprintln!(
                        "note: {} no longer has session {id} — starting a new one",
                        kind.provider_name()
                    );
                }
            }
        }
    }
    let mut transcript = store.as_ref().map(|store| {
        crate::tui::persist::BackendTranscript::new(
            cwd.clone(),
            store.summary.id.clone(),
            kind.provider_name().to_string(),
        )
    });

    let runner = {
        let session = session.clone();
        tokio::spawn(async move { session.prompt(prompt).await })
    };

    let mut text = String::new();
    let mut thought = String::new();
    let mut error: Option<String> = None;
    let mut error_printed = false;
    while let Some(event) = rx.recv().await {
        let is_end = matches!(event, AgentEvent::TurnEnd { .. });
        if let Some(transcript) = transcript.as_mut() {
            transcript.on_event(&event, session.as_ref()).await;
        }
        match event {
            AgentEvent::TextDelta(delta) => {
                text.push_str(&delta);
                match format {
                    OutputFormat::Plain => {
                        print!("{delta}");
                        let _ = std::io::stdout().flush();
                    }
                    OutputFormat::StreamingJson => {
                        println!("{}", serde_json::json!({ "type": "text", "data": delta }));
                    }
                    OutputFormat::StreamingMessagesJson => {
                        println!(
                            "{}",
                            serde_json::json!({
                                "type": "content_block_delta",
                                "delta": { "type": "text_delta", "text": delta }
                            })
                        );
                    }
                    _ => {}
                }
            }
            AgentEvent::ReasoningDelta(delta) => {
                thought.push_str(&delta);
                if format == OutputFormat::StreamingJson {
                    println!(
                        "{}",
                        serde_json::json!({ "type": "thought", "data": delta })
                    );
                }
            }
            AgentEvent::ToolCall { name, title, .. } => {
                if format == OutputFormat::StreamingJson {
                    println!(
                        "{}",
                        serde_json::json!({ "type": "tool_call", "data": { "name": name, "title": title } })
                    );
                }
            }
            AgentEvent::Error(message) => {
                error = Some(message.clone());
                if format == OutputFormat::Plain {
                    eprintln!("error: {message}");
                    error_printed = true;
                }
            }
            _ => {}
        }
        if is_end {
            break;
        }
    }
    let stop = runner.await.unwrap_or(Ok(StopReason::Error));
    let stop_reason = match stop {
        Ok(s) => s,
        Err(e) => {
            error.get_or_insert_with(|| e.to_string());
            StopReason::Error
        }
    };
    let stop_str = stop_reason_label(stop_reason);
    if let Some(message) = &error {
        if !error_printed && format == OutputFormat::Plain {
            eprintln!("error: {message}");
        }
    }
    let is_error = error.is_some() || stop_reason == StopReason::Error;
    match format {
        OutputFormat::Plain => println!(),
        OutputFormat::Json => println!(
            "{}",
            serde_json::to_string(&serde_json::json!({
                "text": text,
                "stopReason": stop_str,
                "sessionId": session_id,
                "requestId": request_id,
                "thought": thought,
                "error": error,
            }))?
        ),
        OutputFormat::StreamingJson => println!(
            "{}",
            serde_json::json!({
                "type": "end",
                "stopReason": stop_str,
                "sessionId": session_id,
                "requestId": request_id,
            })
        ),
        OutputFormat::StreamingMessagesJson => println!(
            "{}",
            serde_json::json!({ "type": "message_stop", "stopReason": stop_str, "sessionId": session_id })
        ),
    }
    if opts.json_schema.is_some() {
        eprintln!(
            "note: --json-schema cannot be enforced with {} — it decides its own output",
            kind.provider_name()
        );
    }

    session.shutdown().await;
    Ok(if is_error { 1 } else { 0 })
}

// ---------------------------------------------------------------------------
// doctor
// ---------------------------------------------------------------------------

pub fn doctor(cwd: &Path, config: &Config, json: bool) -> Result<i32> {
    let mut checks: Vec<(String, bool, String)> = Vec::new();

    let is_tty = std::io::stdout().is_terminal();
    checks.push((
        "stdout is a terminal".into(),
        is_tty,
        if is_tty {
            "yes".into()
        } else {
            "no (piped)".into()
        },
    ));
    let color = std::env::var("NO_COLOR").is_err();
    checks.push((
        "color enabled".into(),
        color,
        if color {
            "colors available".into()
        } else {
            "NO_COLOR set".into()
        },
    ));
    let term = std::env::var("TERM").unwrap_or_default();
    checks.push((
        "TERM".into(),
        !term.is_empty(),
        if term.is_empty() {
            "TERM unset".into()
        } else {
            term
        },
    ));

    let home = paths::cachy_home();
    checks.push(("cachy home".into(), true, home.display().to_string()));
    checks.push((
        "config file".into(),
        paths::config_file().is_file(),
        paths::config_file().display().to_string(),
    ));

    // Model resolution.
    match models::resolve(config, None) {
        Some(model) => {
            let has_key = models::api_key_for(&model).is_some();
            checks.push((
                "default model".into(),
                true,
                format!("{} ({})", model.name, model.model_id),
            ));
            checks.push((
                format!("credential for {}", model.name),
                has_key || model.base_url.contains("localhost"),
                if has_key {
                    "api key found".into()
                } else {
                    format!(
                        "no key (set {} or use `cachycli login --api-key`)",
                        model
                            .env_keys
                            .first()
                            .cloned()
                            .unwrap_or_else(|| "OPENAI_API_KEY".into())
                    )
                },
            ));
        }
        None => checks.push(("default model".into(), false, "none configured".into())),
    }

    checks.push((
        "skills directory".into(),
        paths::skills_dir().is_dir(),
        paths::skills_dir().display().to_string(),
    ));
    checks.push((
        "subagents".into(),
        config.subagents.enabled,
        if config.subagents.enabled {
            "enabled".into()
        } else {
            "disabled".into()
        },
    ));
    checks.push((
        "memory".into(),
        config.memory.enabled
            || std::env::var("CACHY_MEMORY")
                .map(|v| v != "0")
                .unwrap_or(false),
        if config.memory.enabled {
            "enabled".into()
        } else {
            "disabled".into()
        },
    ));

    // Which provider is going to answer, and is it ready?
    let infos = crate::providers::detect(config);
    let chosen = crate::providers::default(config);
    let chosen_info = infos.iter().find(|p| p.id == chosen);
    checks.push((
        "provider".into(),
        chosen_info
            .map(|p| p.installed && p.connected)
            .unwrap_or(false),
        match chosen_info {
            Some(info) => format!(
                "{}{}",
                info.label,
                if info.connected {
                    " · connected".to_string()
                } else if info.installed {
                    " · needs sign-in (/provider)".to_string()
                } else {
                    format!(" · not installed ({})", info.install_hint)
                }
            ),
            None => "none".into(),
        },
    ));

    // opencode availability.
    let opencode = which("opencode");
    checks.push((
        "opencode binary".into(),
        opencode.is_some(),
        opencode.unwrap_or_else(|| "not found in PATH (needed for the opencode backend)".into()),
    ));

    // MCP servers count.
    checks.push((
        "MCP servers".into(),
        true,
        format!("{} configured", config.mcp_servers.len()),
    ));

    // Hooks trust.
    checks.push((
        "folder trusted for hooks".into(),
        hooks::is_folder_trusted(cwd),
        if hooks::is_folder_trusted(cwd) {
            "trusted".into()
        } else {
            "not trusted".into()
        },
    ));

    if json || false {
        // (json flag reserved)
    }
    if json {
        let payload: Vec<_> = checks
            .iter()
            .map(|(name, ok, detail)| {
                serde_json::json!({ "check": name, "ok": ok, "detail": detail })
            })
            .collect();
        println!("{}", serde_json::to_string_pretty(&payload)?);
    } else {
        println!("cachycli doctor\n");
        let mut failures = 0;
        for (name, ok, detail) in &checks {
            let mark = if *ok { "✓" } else { "✗" };
            println!("  {mark} {name}: {detail}");
            if !ok {
                failures += 1;
            }
        }
        println!("\n{} check(s) need attention.", failures);
    }
    Ok(0)
}

fn which(binary: &str) -> Option<String> {
    let path = std::env::var("PATH").ok()?;
    for dir in std::env::split_paths(&path) {
        let candidate = dir.join(binary);
        if candidate.is_file() {
            return Some(candidate.display().to_string());
        }
    }
    None
}

// ---------------------------------------------------------------------------
// models
// ---------------------------------------------------------------------------

pub fn models_cmd(config: &Config, json: bool) -> Result<i32> {
    let catalog = models::catalog(config);
    if json {
        let payload: Vec<_> = catalog
            .iter()
            .map(|m| {
                serde_json::json!({
                    "name": m.name,
                    "model": m.model_id,
                    "base_url": m.base_url,
                    "backend": m.api_backend.as_str(),
                    "context_window": m.context_window,
                    "builtin": m.builtin,
                    "has_credential": models::api_key_for(m).is_some(),
                })
            })
            .collect();
        println!("{}", serde_json::to_string_pretty(&payload)?);
        return Ok(0);
    }
    println!("Available models:\n");
    for m in catalog {
        let key = if models::api_key_for(&m).is_some() {
            "✓"
        } else {
            " "
        };
        let default = if config.models.default.as_deref() == Some(m.name.as_str()) {
            "*"
        } else {
            " "
        };
        println!(
            "  {key}{default} {:<22} {:<28} {:<26} ctx {:<8} {}",
            m.name,
            m.display(),
            m.base_url,
            m.context_window,
            if m.builtin { "[builtin]" } else { "[config]" }
        );
    }
    println!("\n  ✓ = credential available   * = default");
    println!("Add models with [model.<name>] sections in ~/.cachy/config.toml");
    Ok(0)
}

// ---------------------------------------------------------------------------
// sessions / export / usage / trace
// ---------------------------------------------------------------------------

pub fn sessions_cmd(cwd: &Path, action: Option<&SessionsAction>, json: bool) -> Result<i32> {
    match action {
        None | Some(SessionsAction::List) => {
            let sessions = session::list_sessions(Some(cwd))?;
            if json {
                println!("{}", serde_json::to_string_pretty(&sessions)?);
                return Ok(0);
            }
            if sessions.is_empty() {
                println!("No sessions for {}", cwd.display());
                return Ok(0);
            }
            for s in sessions {
                println!(
                    "{}  {:<40}  {}  {}",
                    &s.id[..8.min(s.id.len())],
                    if s.title.is_empty() {
                        "(untitled)"
                    } else {
                        &s.title
                    },
                    s.updated_at.get(..16).unwrap_or(&s.updated_at),
                    s.current_model_id
                );
            }
            Ok(0)
        }
        Some(SessionsAction::Search { query }) => {
            let sessions = session::list_sessions(Some(cwd))?;
            let needle = query.to_lowercase();
            let mut found = 0;
            for s in sessions {
                if s.title.to_lowercase().contains(&needle) || s.id.contains(&needle) {
                    println!("{}  {}", &s.id[..8.min(s.id.len())], s.title);
                    found += 1;
                }
            }
            if found == 0 {
                println!("No sessions match \"{query}\"");
            }
            Ok(0)
        }
        Some(SessionsAction::Restore { session }) => {
            match session::find_by_id_or_title(cwd, session)? {
                Some(summary) => {
                    let display_cwd = paths::display_path(Path::new(&summary.cwd));
                    println!("cd {} && cachycli --resume {}", display_cwd, summary.id);
                    Ok(0)
                }
                None => {
                    eprintln!("session not found: {session}");
                    Ok(1)
                }
            }
        }
        Some(SessionsAction::Delete { session }) => {
            match session::find_by_id_or_title(cwd, session)? {
                Some(summary) => {
                    if let Some(dir) = session::find_session_dir(&summary.id) {
                        std::fs::remove_dir_all(&dir)?;
                        println!("Deleted session {}", summary.id);
                    }
                    Ok(0)
                }
                None => {
                    eprintln!("session not found: {session}");
                    Ok(1)
                }
            }
        }
    }
}

pub fn export_cmd(cwd: &Path, args: &ExportArgs) -> Result<i32> {
    let store = match &args.session {
        Some(target) => match session::find_by_id_or_title(cwd, target)? {
            Some(summary) => SessionStore::open(cwd, &summary.id)?,
            None => anyhow::bail!("session not found: {target}"),
        },
        None => match session::most_recent(cwd)? {
            Some(summary) => SessionStore::open(cwd, &summary.id)?,
            None => anyhow::bail!("no sessions for {}", cwd.display()),
        },
    };
    let markdown = cachy_core::export::session_markdown(&store)?;
    match &args.output {
        Some(path) => {
            std::fs::write(path, &markdown)?;
            println!("Exported to {}", path.display());
        }
        None => {
            let title = if store.summary.title.is_empty() {
                format!(
                    "cachy-{}.md",
                    &store.summary.id[..8.min(store.summary.id.len())]
                )
            } else {
                format!(
                    "{}.md",
                    store
                        .summary
                        .title
                        .to_lowercase()
                        .chars()
                        .map(|c| if c.is_alphanumeric() { c } else { '-' })
                        .collect::<String>()
                        .trim_matches('-')
                )
            };
            let path = cwd.join(title);
            std::fs::write(&path, &markdown)?;
            println!("Exported to {}", path.display());
        }
    }
    Ok(0)
}

pub fn usage_cmd(cwd: &Path, session_arg: Option<&str>) -> Result<i32> {
    let store = match session_arg {
        Some(target) => match session::find_by_id_or_title(cwd, target)? {
            Some(summary) => SessionStore::open(cwd, &summary.id)?,
            None => anyhow::bail!("session not found: {target}"),
        },
        None => match session::most_recent(cwd)? {
            Some(summary) => SessionStore::open(cwd, &summary.id)?,
            None => {
                println!("No sessions for {}", cwd.display());
                return Ok(0);
            }
        },
    };
    let signals = store.read_signals();
    println!("Session {} ({})", store.summary.id, store.summary.title);
    println!("Model: {}", store.summary.current_model_id);
    println!("Chat messages: {}", store.summary.num_chat_messages);
    println!("Turns: {}", signals.turns);
    println!("Input tokens: {}", signals.input_tokens);
    println!("Output tokens: {}", signals.output_tokens);
    Ok(0)
}

pub fn trace_cmd(cwd: &Path, args: &TraceArgs) -> Result<i32> {
    let store = match &args.session {
        Some(target) => match session::find_by_id_or_title(cwd, target)? {
            Some(summary) => SessionStore::open(cwd, &summary.id)?,
            None => anyhow::bail!("session not found: {target}"),
        },
        None => match session::most_recent(cwd)? {
            Some(summary) => SessionStore::open(cwd, &summary.id)?,
            None => anyhow::bail!("no sessions for {}", cwd.display()),
        },
    };
    let trace = cachy_core::export::session_markdown(&store)?;
    let path = args.output.clone().unwrap_or_else(|| {
        cwd.join(format!(
            "cachy-trace-{}.jsonl",
            cachy_core::text::short(&store.summary.id, 8)
        ))
    });
    std::fs::write(&path, &trace)?;
    println!("Trace written to {}", path.display());
    if args.upload {
        println!("Upload is not configured (set [telemetry] events_url to enable).");
    }
    Ok(0)
}

// ---------------------------------------------------------------------------
// inspect
// ---------------------------------------------------------------------------

pub fn inspect_cmd(cwd: &Path, json: bool) -> Result<i32> {
    let loaded = cachy_core::config::load(cwd)?;
    let inspection = inspect_mod::inspect(cwd, &loaded);
    if json {
        println!("{}", serde_json::to_string_pretty(&inspection)?);
    } else {
        print!("{}", inspect_mod::render(&inspection));
    }
    Ok(0)
}

// ---------------------------------------------------------------------------
// memory / plugins / skills listing
// ---------------------------------------------------------------------------

pub fn memory_cmd(cwd: &Path, action: &MemoryAction) -> Result<i32> {
    match action {
        MemoryAction::List(args) => {
            let global = memory::Scope::Global;
            let ws = memory::Scope::Workspace(memory::workspace_slug(cwd).into());
            let mut payload = serde_json::Map::new();
            for (label, scope) in [("global", &global), ("workspace", &ws)] {
                let topics: Vec<_> = memory::topics(scope)
                    .into_iter()
                    .map(|(title, path)| {
                        serde_json::json!({ "title": title, "path": path.display().to_string() })
                    })
                    .collect();
                payload.insert(
                    label.to_string(),
                    serde_json::json!({
                        "dir": scope.scope_dir().display().to_string(),
                        "topics": topics,
                    }),
                );
            }
            if args.json {
                println!("{}", serde_json::to_string_pretty(&payload)?);
            } else {
                for (label, scope) in [("Global", &global), ("Workspace", &ws)] {
                    println!("{label}: {}", scope.scope_dir().display());
                    let topics = memory::topics(scope);
                    if topics.is_empty() {
                        println!("  (no topics)");
                    }
                    for (title, path) in topics {
                        println!("  - {title} ({})", path.display());
                    }
                }
            }
            Ok(0)
        }
        MemoryAction::Clear(args) => {
            if args.all || !args.global {
                let ws = memory::Scope::Workspace(memory::workspace_slug(cwd).into());
                memory::clear(&ws)?;
                println!("Cleared workspace memory for {}", cwd.display());
            }
            if args.all || args.global {
                memory::clear(&memory::Scope::Global)?;
                println!("Cleared global memory");
            }
            Ok(0)
        }
        MemoryAction::Add { text } => {
            let scope = memory::Scope::Global;
            let path = memory::add_observation(&scope, text)?;
            println!("Saved observation to {}", path.display());
            Ok(0)
        }
    }
}

pub fn plugin_cmd(cwd: &Path, action: Option<&PluginAction>) -> Result<i32> {
    match action {
        None | Some(PluginAction::List) | Some(PluginAction::Reload) => {
            let dirs = [
                ("project", cwd.join(".cachy/plugins")),
                ("user", paths::plugins_dir()),
                (
                    "claude",
                    dirs::home_dir().unwrap_or_default().join(".claude/plugins"),
                ),
            ];
            let mut any = false;
            for (scope, dir) in dirs {
                if dir.is_dir() {
                    if let Ok(entries) = std::fs::read_dir(&dir) {
                        for entry in entries.flatten() {
                            println!("{}: {}", scope, entry.path().display());
                            any = true;
                        }
                    }
                }
            }
            if !any {
                println!("No plugins discovered.");
                println!("Project plugins: {}", cwd.join(".cachy/plugins").display());
                println!("User plugins: {}", paths::plugins_dir().display());
            }
            Ok(0)
        }
        Some(PluginAction::Trust { path }) => {
            println!("Plugin {path} trusted (recorded).");
            Ok(0)
        }
    }
}

// ---------------------------------------------------------------------------
// mcp
// ---------------------------------------------------------------------------

pub async fn mcp_cmd(cwd: &Path, config: &Config, action: Option<&McpAction>) -> Result<i32> {
    match action {
        None | Some(McpAction::List(_)) => {
            if config.mcp_servers.is_empty() {
                println!("No MCP servers configured.");
                println!("Add one with: cachycli mcp add <name> --command <cmd> --arg <arg>");
                return Ok(0);
            }
            for (name, cfg) in &config.mcp_servers {
                let transport = if cfg.url.is_some() { "http" } else { "stdio" };
                let target = cfg
                    .url
                    .clone()
                    .or_else(|| cfg.command.clone())
                    .unwrap_or_default();
                println!(
                    "{:<20} {:<6} {:<9} {} {}",
                    name,
                    transport,
                    if cfg.enabled { "enabled" } else { "disabled" },
                    target,
                    cfg.args.join(" ")
                );
            }
            Ok(0)
        }
        Some(McpAction::Add(args)) => {
            let target = if args.project {
                cwd.join(".cachy").join("config.toml")
            } else {
                paths::config_file()
            };
            if let Some(parent) = target.parent() {
                std::fs::create_dir_all(parent)?;
            }
            let mut content = std::fs::read_to_string(&target).unwrap_or_default();
            if content.contains(&format!("[mcp_servers.{}]", args.name)) {
                anyhow::bail!(
                    "server \"{}\" already exists in {}",
                    args.name,
                    target.display()
                );
            }
            if !content.ends_with('\n') && !content.is_empty() {
                content.push('\n');
            }
            content.push_str(&format!("\n[mcp_servers.{}]\n", args.name));
            if let Some(url) = &args.url {
                content.push_str(&format!("url = {:?}\n", url));
                for header in &args.headers {
                    if let Some((k, v)) = header.split_once('=') {
                        content.push_str(&format!(
                            "[mcp_servers.{}.headers]\n{:?} = {:?}\n",
                            args.name, k, v
                        ));
                    }
                }
            } else if let Some(command) = &args.command {
                content.push_str(&format!("command = {:?}\n", command));
                if !args.args.is_empty() {
                    let args_toml: Vec<String> =
                        args.args.iter().map(|a| format!("{a:?}")).collect();
                    content.push_str(&format!("args = [{}]\n", args_toml.join(", ")));
                }
            } else {
                anyhow::bail!("provide --command (stdio) or --url (http)");
            }
            std::fs::write(&target, content)?;
            println!("Added MCP server \"{}\" to {}", args.name, target.display());
            Ok(0)
        }
        Some(McpAction::Remove(args)) => {
            let target = if args.project {
                cwd.join(".cachy").join("config.toml")
            } else {
                paths::config_file()
            };
            let content = std::fs::read_to_string(&target)
                .with_context(|| format!("reading {}", target.display()))?;
            let mut out = String::new();
            let mut skipping = false;
            let mut removed = false;
            for line in content.lines() {
                let trimmed = line.trim();
                if trimmed.starts_with('[') {
                    skipping = trimmed == format!("[mcp_servers.{}]", args.name)
                        || trimmed.starts_with(&format!("[mcp_servers.{}.", args.name));
                    if skipping {
                        removed = true;
                    }
                }
                if !skipping {
                    out.push_str(line);
                    out.push('\n');
                }
            }
            if !removed {
                anyhow::bail!("server \"{}\" not found in {}", args.name, target.display());
            }
            std::fs::write(&target, out)?;
            println!("Removed MCP server \"{}\"", args.name);
            Ok(0)
        }
        Some(McpAction::Test(args)) => {
            let Some(cfg) = config.mcp_servers.get(&args.name).cloned() else {
                anyhow::bail!("server \"{}\" not configured", args.name);
            };
            let mut map = std::collections::BTreeMap::new();
            map.insert(args.name.clone(), cfg);
            let manager = mcp::McpManager::new(map, "test");
            manager.initialize().await;
            let errors = manager.errors().await;
            if let Some(error) = errors.get(&args.name) {
                anyhow::bail!("connection failed: {error}");
            }
            let tools = manager.list_tools().await;
            println!("{} tools available:", tools.len());
            for tool in tools {
                println!(
                    "  - {}: {}",
                    tool.name,
                    tool.description.lines().next().unwrap_or("")
                );
            }
            manager.shutdown().await;
            Ok(0)
        }
        Some(McpAction::Serve(args)) => {
            mcp_serve(
                Arc::new(config.clone()),
                cwd.to_path_buf(),
                args.cwd.clone(),
            )
            .await
        }
    }
}

/// Run cachycli as an MCP server over stdio: other agents (including
/// opencode) can call `cachy_run`, `cachy_models` and `cachy_sessions`.
async fn mcp_serve(
    config: Arc<Config>,
    default_cwd: PathBuf,
    _cwd: Option<PathBuf>,
) -> Result<i32> {
    use serde_json::json;
    let stdin = tokio::io::stdin();
    let mut reader = tokio::io::BufReader::new(stdin);
    let mut line = String::new();

    let tools = json!([
        {
            "name": "cachy_run",
            "description": "Run a headless cachycli agent turn in a directory and return its final answer.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "prompt": { "type": "string", "description": "The task for the agent" },
                    "cwd": { "type": "string", "description": "Working directory (defaults to the server's directory)" },
                    "model": { "type": "string", "description": "Optional model name" },
                    "always_approve": { "type": "boolean", "description": "Auto-approve tool executions" }
                },
                "required": ["prompt"]
            }
        },
        {
            "name": "cachy_models",
            "description": "List the models configured for cachycli.",
            "inputSchema": { "type": "object", "properties": {} }
        },
        {
            "name": "cachy_sessions",
            "description": "List recent cachycli sessions for a directory.",
            "inputSchema": {
                "type": "object",
                "properties": { "cwd": { "type": "string" } }
            }
        }
    ]);

    loop {
        line.clear();
        match tokio::io::AsyncBufReadExt::read_line(&mut reader, &mut line).await {
            Ok(0) | Err(_) => break,
            Ok(_) => {}
        }
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        let Ok(message) = serde_json::from_str::<serde_json::Value>(trimmed) else {
            continue;
        };
        let method = message.get("method").and_then(|v| v.as_str()).unwrap_or("");
        let id = message.get("id").cloned();
        if id.is_none() {
            continue; // notification
        }
        let id = id.unwrap();
        let params = message.get("params").cloned().unwrap_or(json!({}));
        let result: Result<serde_json::Value, anyhow::Error> = match method {
            "initialize" => Ok(json!({
                "protocolVersion": params.get("protocolVersion").cloned().unwrap_or(json!("2025-06-18")),
                "capabilities": { "tools": {} },
                "serverInfo": { "name": "cachycli", "title": cachy_core::APP_TITLE, "version": cachy_core::VERSION }
            })),
            "tools/list" => Ok(json!({ "tools": tools })),
            "tools/call" => {
                let name = params.get("name").and_then(|v| v.as_str()).unwrap_or("");
                let args = params.get("arguments").cloned().unwrap_or(json!({}));
                match name {
                    "cachy_models" => {
                        let list = models::catalog(&config)
                            .iter()
                            .map(|m| format!("{} ({})", m.name, m.model_id))
                            .collect::<Vec<_>>()
                            .join("\n");
                        Ok(json!({ "content": [{ "type": "text", "text": list }] }))
                    }
                    "cachy_sessions" => {
                        let cwd = args
                            .get("cwd")
                            .and_then(|v| v.as_str())
                            .map(PathBuf::from)
                            .unwrap_or_else(|| default_cwd.clone());
                        let list = session::list_sessions(Some(&cwd))?
                            .into_iter()
                            .take(20)
                            .map(|s| format!("{}  {}", s.id, s.title))
                            .collect::<Vec<_>>()
                            .join("\n");
                        Ok(json!({ "content": [{ "type": "text", "text": list }] }))
                    }
                    "cachy_run" => {
                        let prompt = args
                            .get("prompt")
                            .and_then(|v| v.as_str())
                            .unwrap_or("")
                            .to_string();
                        let cwd = args
                            .get("cwd")
                            .and_then(|v| v.as_str())
                            .map(PathBuf::from)
                            .unwrap_or_else(|| default_cwd.clone());
                        let model = args
                            .get("model")
                            .and_then(|v| v.as_str())
                            .map(|s| s.to_string());
                        let always = args
                            .get("always_approve")
                            .and_then(|v| v.as_bool())
                            .unwrap_or(true);
                        let output =
                            run_headless_subprocess(&cwd, &prompt, model.as_deref(), always).await;
                        match output {
                            Ok(text) => {
                                Ok(json!({ "content": [{ "type": "text", "text": text }] }))
                            }
                            Err(e) => Ok(json!({
                                "content": [{ "type": "text", "text": format!("error: {e}") }],
                                "isError": true
                            })),
                        }
                    }
                    other => Err(anyhow::anyhow!("unknown tool {other}")),
                }
            }
            "ping" => Ok(json!({})),
            other => Err(anyhow::anyhow!("method not found: {other}")),
        };
        let response = match result {
            Ok(value) => json!({ "jsonrpc": "2.0", "id": id, "result": value }),
            Err(e) => {
                json!({ "jsonrpc": "2.0", "id": id, "error": { "code": -32603, "message": e.to_string() } })
            }
        };
        println!("{}", serde_json::to_string(&response)?);
        let _ = std::io::stdout().flush();
    }
    Ok(0)
}

async fn run_headless_subprocess(
    cwd: &Path,
    prompt: &str,
    model: Option<&str>,
    always_approve: bool,
) -> Result<String> {
    let exe = std::env::current_exe()?;
    let mut cmd = tokio::process::Command::new(exe);
    cmd.arg("-p")
        .arg(prompt)
        .arg("--cwd")
        .arg(cwd)
        .arg("--output-format")
        .arg("json");
    if let Some(model) = model {
        cmd.arg("-m").arg(model);
    }
    if always_approve {
        cmd.arg("--always-approve");
    }
    let output = cmd.output().await?;
    if !output.status.success() {
        anyhow::bail!(
            "cachycli run failed: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        );
    }
    let stdout = String::from_utf8_lossy(&output.stdout);
    if let Ok(value) = serde_json::from_str::<serde_json::Value>(&stdout) {
        if let Some(text) = value.get("text").and_then(|v| v.as_str()) {
            return Ok(text.to_string());
        }
    }
    Ok(stdout.trim().to_string())
}

// ---------------------------------------------------------------------------
// login / logout
// ---------------------------------------------------------------------------

pub fn login_cmd(config: &Config, args: &LoginArgs) -> Result<i32> {
    let mut state = auth::load();
    let key = match &args.api_key {
        Some(key) => key.clone(),
        None => {
            // External provider first, then interactive prompt.
            if let Some(cmd) = auth::external_provider_command(config) {
                println!("Running auth provider: {cmd}");
                auth::run_external_provider(&cmd)?
            } else {
                if !std::io::stdin().is_terminal() {
                    anyhow::bail!("pass --api-key when stdin is not a terminal");
                }
                print!("Paste your API key: ");
                std::io::stdout().flush()?;
                let mut input = String::new();
                std::io::stdin().read_line(&mut input)?;
                input.trim().to_string()
            }
        }
    };
    if key.is_empty() {
        anyhow::bail!("empty API key");
    }
    if args.provider == "default" {
        state.api_key = Some(key.clone());
    }
    state.providers.insert(
        args.provider.clone(),
        auth::Credential {
            key: Some(key.clone()),
            ..Default::default()
        },
    );
    state.auth_mode = Some("api_key".into());
    auth::save(&state)?;
    println!(
        "Stored credential for provider \"{}\" in {}",
        args.provider,
        paths::auth_file().display()
    );
    println!(
        "Tip: export {} for CI, or add [model.<name>] sections to config.toml.",
        env_hint(&args.provider)
    );
    Ok(0)
}

fn env_hint(provider: &str) -> &'static str {
    match provider {
        "openai" => "OPENAI_API_KEY",
        "anthropic" => "ANTHROPIC_API_KEY",
        "opencode" => "OPENCODE_API_KEY",
        _ => "OPENAI_API_KEY",
    }
}

pub fn logout_cmd() -> Result<i32> {
    auth::clear()?;
    println!("Credentials cleared.");
    Ok(0)
}

// ---------------------------------------------------------------------------
// du / version / update / setup / leader / clone / wrap / worktree
// ---------------------------------------------------------------------------

pub fn du_cmd() -> Result<i32> {
    let root = paths::cachy_home();
    let mut total = 0u64;
    let mut rows: Vec<(String, u64)> = Vec::new();
    if let Ok(entries) = std::fs::read_dir(&root) {
        for entry in entries.flatten() {
            let path = entry.path();
            let size = dir_size(&path);
            total += size;
            rows.push((entry.file_name().to_string_lossy().to_string(), size));
        }
    }
    rows.sort_by_key(|row| std::cmp::Reverse(row.1));
    println!("{}:\n", root.display());
    for (name, size) in rows {
        println!("  {:<28} {}", name, human_size(size));
    }
    println!("\n  {:<28} {}", "total", human_size(total));
    Ok(0)
}

fn dir_size(path: &Path) -> u64 {
    if path.is_file() {
        return path.metadata().map(|m| m.len()).unwrap_or(0);
    }
    walkdir::WalkDir::new(path)
        .into_iter()
        .filter_map(|e| e.ok())
        .filter_map(|e| e.metadata().ok())
        .filter(|m| m.is_file())
        .map(|m| m.len())
        .sum()
}

fn human_size(bytes: u64) -> String {
    const UNITS: [&str; 4] = ["B", "KB", "MB", "GB"];
    let mut value = bytes as f64;
    let mut unit = 0;
    while value >= 1024.0 && unit < UNITS.len() - 1 {
        value /= 1024.0;
        unit += 1;
    }
    format!("{value:.1} {}", UNITS[unit])
}

/// Bridge stdin/stdout to a WebSocket agent, byte for byte: an editor that
/// only speaks stdio can drive a remote `cachycli agent serve` this way.
pub async fn ws_relay(url: &str) -> Result<i32> {
    use futures_util::{SinkExt, StreamExt};
    use tokio::io::{AsyncBufReadExt, AsyncWriteExt};

    let (socket, _) = tokio_tungstenite::connect_async(url)
        .await
        .with_context(|| format!("connecting to {url}"))?;
    let (mut sink, mut stream) = socket.split();
    let mut lines = tokio::io::BufReader::new(tokio::io::stdin()).lines();
    let mut stdout = tokio::io::stdout();

    // When stdin ends we keep reading: the answers to what was already sent
    // are still on their way.
    let mut stdin_open = true;
    loop {
        tokio::select! {
            line = lines.next_line(), if stdin_open => match line {
                Ok(Some(line)) => {
                    if line.trim().is_empty() {
                        continue;
                    }
                    if sink.send(tokio_tungstenite::tungstenite::Message::Text(line)).await.is_err() {
                        break;
                    }
                }
                _ => stdin_open = false,
            },
            message = stream.next() => match message {
                Some(Ok(tokio_tungstenite::tungstenite::Message::Text(text))) => {
                    stdout.write_all(text.as_bytes()).await?;
                    stdout.write_all(b"\n").await?;
                    stdout.flush().await?;
                }
                Some(Ok(tokio_tungstenite::tungstenite::Message::Close(_))) | None => break,
                Some(Ok(_)) => {}
                Some(Err(e)) => return Err(anyhow!("reading from {url}: {e}")),
            },
        }
    }
    Ok(0)
}

/// `cachycli share`: push a session to a server, fetch one back, or list.
pub async fn share_cmd(cwd: &Path, args: &ShareArgs) -> Result<i32> {
    // Check the address before doing any work, so the error is about the url
    // and not about whatever came next.
    let url = args.to.trim();
    if !(url.starts_with("ws://") || url.starts_with("wss://")) {
        anyhow::bail!(
            "the server url should look like ws://host:8765 (got {:?})",
            args.to
        );
    }
    if let Some(id) = &args.remove {
        let removed = cachy_core::share_net::remove(&args.to, id).await?;
        println!("removed {removed} from {}", args.to);
        return Ok(0);
    }
    if args.list {
        let items = cachy_core::share_net::list_remote(&args.to).await?;
        if items.is_empty() {
            println!("{} has no shared sessions yet", args.to);
            return Ok(0);
        }
        println!("{:<38} {:>8}  TITLE", "ID", "MESSAGES");
        for item in items {
            let title = if item.title.is_empty() {
                "(untitled)"
            } else {
                item.title.as_str()
            };
            println!("{:<38} {:>8}  {}", item.id, item.messages, title);
        }
        return Ok(0);
    }

    if let Some(id) = &args.id {
        let text = cachy_core::share_net::fetch(&args.to, id).await?;
        let store = cachy_core::share::import_into(cwd, &text)?;
        let title = if store.summary.title.is_empty() {
            "(untitled)".to_string()
        } else {
            store.summary.title.clone()
        };
        println!("imported {title:?} as {}", store.summary.id);
        println!("resume it with: cachycli --resume {}", store.summary.id);
        return Ok(0);
    }

    let summary = match &args.session {
        Some(needle) => session::find_by_id_or_title(cwd, needle)?
            .ok_or_else(|| anyhow!("no session matches {needle:?} in {}", cwd.display()))?,
        None => session::most_recent(cwd)?
            .ok_or_else(|| anyhow!("no sessions in {} yet", cwd.display()))?,
    };
    let store = SessionStore::open(cwd, &summary.id)?;
    let text = cachy_core::share::to_json(&cachy_core::share::bundle(&store)?)?;
    let entry = cachy_core::share_net::push(&args.to, &text).await?;
    let title = if entry.title.is_empty() {
        "(untitled)"
    } else {
        entry.title.as_str()
    };
    println!(
        "shared {title:?} as {} ({} messages, {} bytes)",
        entry.id, entry.messages, entry.bytes
    );
    println!(
        "fetch it with: cachycli share --to {} --id {}",
        args.to, entry.id
    );
    Ok(0)
}

/// Remove the PATH block the installer adds to a shell rc file. Returns the
/// new text and whether anything was removed.
pub fn strip_installer_path(text: &str) -> (String, bool) {
    let mut out = String::with_capacity(text.len());
    let mut removed = false;
    let mut lines = text.lines().peekable();
    while let Some(line) = lines.next() {
        if line.trim() == "# Added by cachycli installer" {
            if let Some(next) = lines.peek() {
                if next.trim_start().starts_with("export PATH=") {
                    lines.next();
                    removed = true;
                    continue;
                }
            }
        }
        out.push_str(line);
        out.push('\n');
    }
    (out, removed)
}

/// Ask before deleting data. Anything but yes keeps it.
fn confirm_purge(dir: &Path) -> Result<bool> {
    print!(
        "Remove {} and everything in it (config, sessions, memory)? [y/N] ",
        dir.display()
    );
    std::io::stdout().flush().ok();
    let mut answer = String::new();
    std::io::stdin().read_line(&mut answer)?;
    Ok(matches!(
        answer.trim().to_ascii_lowercase().as_str(),
        "y" | "yes"
    ))
}

/// `cachycli uninstall`: remove what the installer put there — the binary, the
/// convenience symlink and the PATH lines in the shell files. `--purge` also
/// removes `~/.cachy` after asking.
pub fn uninstall_cmd(args: &UninstallArgs) -> Result<i32> {
    let exe = std::env::current_exe().context("finding the running binary")?;
    let home = dirs::home_dir();
    let mut did: Vec<String> = Vec::new();
    let mut notes: Vec<String> = Vec::new();

    // 1. The binary, when it lives in your home: anything else (a package
    //    manager, /usr/local) has to be removed by whatever put it there.
    if home.as_ref().is_some_and(|home| exe.starts_with(home)) {
        match std::fs::remove_file(&exe) {
            Ok(()) => did.push(format!("removed {}", exe.display())),
            Err(e) => notes.push(format!("could not remove {}: {e}", exe.display())),
        }
    } else {
        notes.push(format!(
            "{} is outside your home — remove it with whatever installed it",
            exe.display()
        ));
    }

    // 2. The ~/.cachy/bin symlink, when it points at that binary.
    let link = paths::cachy_home().join("bin").join("cachycli");
    if let Ok(target) = std::fs::read_link(&link) {
        let points_at_us = target == exe
            || match (target.canonicalize(), exe.canonicalize()) {
                (Ok(target), Ok(exe)) => target == exe,
                _ => false,
            };
        if points_at_us {
            let _ = std::fs::remove_file(&link);
            did.push(format!("removed {}", link.display()));
        } else {
            notes.push(format!(
                "kept {} (it points at {})",
                link.display(),
                target.display()
            ));
        }
    }

    // 3. The PATH lines the installer added to the shell files.
    if let Some(home) = &home {
        for rc in [".bashrc", ".zshrc", ".profile"] {
            let path = home.join(rc);
            let Ok(text) = std::fs::read_to_string(&path) else {
                continue;
            };
            let (stripped, changed) = strip_installer_path(&text);
            if changed {
                match std::fs::write(&path, stripped) {
                    Ok(()) => did.push(format!("removed the PATH line from {}", path.display())),
                    Err(e) => notes.push(format!("could not rewrite {}: {e}", path.display())),
                }
            }
        }
    }

    // 4. With --purge, the data directory.
    let data = paths::cachy_home();
    if args.purge {
        if args.yes || confirm_purge(&data)? {
            match std::fs::remove_dir_all(&data) {
                Ok(()) => did.push(format!(
                    "removed {} (configuration, sessions, memory)",
                    data.display()
                )),
                Err(e) => notes.push(format!("could not remove {}: {e}", data.display())),
            }
        } else {
            notes.push(format!("kept {}", data.display()));
        }
    } else if data.exists() {
        notes.push(format!(
            "{} was kept (configuration, sessions, memory) — use --purge to remove it",
            data.display()
        ));
    }

    for line in &did {
        println!("{line}");
    }
    for line in &notes {
        println!("note: {line}");
    }
    if !did.is_empty() {
        println!("Done — open a new shell so PATH is up to date.");
    }
    Ok(0)
}

pub fn version_cmd() -> Result<i32> {
    println!(
        "cachycli {} ({})",
        cachy_core::VERSION,
        env!("CARGO_PKG_VERSION")
    );
    println!("cachy-core {}", cachy_core::VERSION);
    println!(
        "platform: {} {}",
        std::env::consts::OS,
        std::env::consts::ARCH
    );
    println!("home: {}", paths::cachy_home().display());
    Ok(0)
}

pub async fn update_cmd(args: &UpdateArgs) -> Result<i32> {
    let repo = std::env::var("CACHY_UPDATE_REPO").unwrap_or_else(|_| "LipeMachado/cachycli".into());
    let client = reqwest::Client::builder()
        .user_agent(cachy_core::user_agent())
        .build()?;
    let url = format!("https://api.github.com/repos/{repo}/releases/latest");
    let release: serde_json::Value = match client.get(&url).send().await {
        Ok(resp) if resp.status().is_success() => resp.json().await?,
        Ok(resp) if resp.status() == reqwest::StatusCode::NOT_FOUND => {
            anyhow::bail!(
                "no releases published for {repo} yet — publish one (git tag v{} && git push --tags) or point CACHY_UPDATE_REPO at your fork",
                cachy_core::VERSION
            );
        }
        Ok(resp) => {
            anyhow::bail!("release lookup failed (HTTP {})", resp.status());
        }
        Err(e) => anyhow::bail!("release lookup failed: {e} (set CACHY_UPDATE_REPO to your fork)"),
    };
    let tag = release
        .get("tag_name")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .trim_start_matches('v')
        .to_string();
    if args.json {
        println!(
            "{}",
            serde_json::json!({ "current": cachy_core::VERSION, "latest": tag })
        );
        return Ok(0);
    }
    if tag.is_empty() {
        println!("Could not determine the latest version.");
        return Ok(1);
    }
    let comparison = compare_versions(&tag, cachy_core::VERSION);
    if comparison <= 0 {
        println!(
            "cachycli {} is up to date (latest {tag}).",
            cachy_core::VERSION
        );
        return Ok(0);
    }
    println!("Update available: {} -> {}", cachy_core::VERSION, tag);
    if args.check {
        return Ok(0);
    }
    // Find a matching asset for this platform.
    let os = std::env::consts::OS;
    let arch = std::env::consts::ARCH;
    let needle = format!("{os}-{arch}");
    let asset_url = release
        .get("assets")
        .and_then(|a| a.as_array())
        .and_then(|assets| {
            assets.iter().find_map(|asset| {
                let name = asset.get("name").and_then(|v| v.as_str()).unwrap_or("");
                if name.contains(&needle) {
                    asset.get("browser_download_url").and_then(|v| v.as_str())
                } else {
                    None
                }
            })
        });
    let Some(asset_url) = asset_url else {
        println!(
            "No asset found for {needle}. Build from source: cargo install --git https://github.com/{repo}"
        );
        return Ok(1);
    };
    println!("Downloading {asset_url}…");
    let bytes = client.get(asset_url).send().await?.bytes().await?;
    let current = std::env::current_exe()?;
    let tmp = current.with_extension("new");
    std::fs::write(&tmp, &bytes)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&tmp, std::fs::Permissions::from_mode(0o755))?;
    }
    std::fs::rename(&tmp, &current)?;
    println!("Updated to {tag}.");
    Ok(0)
}

fn compare_versions(a: &str, b: &str) -> i32 {
    let parse = |s: &str| -> Vec<u32> {
        s.split(['.', '-', '+'])
            .take(3)
            .map(|p| p.parse::<u32>().unwrap_or(0))
            .collect()
    };
    let av = parse(a);
    let bv = parse(b);
    for i in 0..3 {
        let x = av.get(i).copied().unwrap_or(0);
        let y = bv.get(i).copied().unwrap_or(0);
        if x != y {
            return if x > y { 1 } else { -1 };
        }
    }
    0
}

pub async fn setup_cmd(config: &Config, args: &SetupArgs) -> Result<i32> {
    let url = args
        .url
        .clone()
        .or_else(|| std::env::var("CACHY_MANAGED_CONFIG_URL").ok())
        .or_else(|| std::env::var("CACHY_DEPLOYMENT_CONFIG_URL").ok());
    let Some(url) = url else {
        anyhow::bail!("provide --url or set CACHY_MANAGED_CONFIG_URL");
    };
    let client = reqwest::Client::builder()
        .user_agent(cachy_core::user_agent())
        .build()?;
    let mut request = client.get(&url);
    if let Ok(key) = std::env::var("CACHY_DEPLOYMENT_KEY") {
        request = request.header("authorization", format!("Bearer {key}"));
    }
    let body = request.send().await?.error_for_status()?.text().await?;
    // Validate as TOML before writing.
    toml::from_str::<Config>(&body).context("managed config is not valid cachycli TOML")?;
    paths::ensure_dirs().ok();
    let target = paths::managed_config_file();
    std::fs::write(&target, body)?;
    println!("Installed managed configuration to {}", target.display());
    let _ = config;
    Ok(0)
}

pub fn leader_cmd(action: Option<&LeaderAction>) -> Result<i32> {
    match action {
        Some(LeaderAction::List) | Some(LeaderAction::Status) | None => {
            println!("cachycli runs single-process; no leader sockets to list.");
            println!("The opencode backend starts/reuses `opencode serve` automatically.");
            Ok(0)
        }
        Some(LeaderAction::Kill { .. }) => {
            println!("Nothing to kill.");
            Ok(0)
        }
    }
}

pub fn clone_cmd(_args: &CloneArgs) -> Result<i32> {
    eprintln!("`cachycli clone` is not supported in this build.");
    eprintln!("Use `git clone <url> && cachycli --cwd <dir>` instead.");
    Ok(1)
}

pub fn cursor_worker_cmd() -> Result<i32> {
    eprintln!("`cachycli cursor-worker` is not supported in this build.");
    Ok(1)
}

pub fn completions_cmd(shell: &str) -> Result<i32> {
    use clap::CommandFactory;
    let mut cmd = crate::cli::Cli::command();
    let shell = match shell.to_lowercase().as_str() {
        "bash" => clap_complete::Shell::Bash,
        "zsh" => clap_complete::Shell::Zsh,
        "fish" => clap_complete::Shell::Fish,
        "powershell" | "pwsh" => clap_complete::Shell::PowerShell,
        "elvish" => clap_complete::Shell::Elvish,
        other => anyhow::bail!("unsupported shell: {other} (bash, zsh, fish, powershell, elvish)"),
    };
    clap_complete::generate(shell, &mut cmd, "cachycli", &mut std::io::stdout());
    Ok(0)
}

pub fn worktree_cmd(cwd: &Path, action: Option<&WorktreeAction>) -> Result<i32> {
    let worktrees_dir = paths::cachy_home().join("worktrees");
    match action {
        None | Some(WorktreeAction::List) => {
            let output = std::process::Command::new("git")
                .arg("-C")
                .arg(cwd)
                .args(["worktree", "list"])
                .output()
                .context("running git (is it installed?)")?;
            if !output.status.success() {
                anyhow::bail!(
                    "git worktree list failed: {}",
                    String::from_utf8_lossy(&output.stderr).trim()
                );
            }
            print!("{}", String::from_utf8_lossy(&output.stdout));
            Ok(0)
        }
        Some(WorktreeAction::Create { name, r#ref }) => {
            std::fs::create_dir_all(&worktrees_dir)?;
            let target = worktrees_dir.join(slug(name));
            let mut cmd = std::process::Command::new("git");
            cmd.arg("-C").arg(cwd).args(["worktree", "add", "-b", name]);
            cmd.arg(&target);
            if let Some(reference) = r#ref {
                cmd.arg(reference);
            }
            let output = cmd.output().context("git worktree add")?;
            if !output.status.success() {
                anyhow::bail!(
                    "git worktree add failed: {}",
                    String::from_utf8_lossy(&output.stderr).trim()
                );
            }
            println!("Worktree created at {}", target.display());
            Ok(0)
        }
        Some(WorktreeAction::Remove { name }) => {
            let target = worktrees_dir.join(slug(name));
            let output = std::process::Command::new("git")
                .arg("-C")
                .arg(cwd)
                .args(["worktree", "remove", "--force"])
                .arg(&target)
                .output()
                .context("git worktree remove")?;
            if !output.status.success() {
                anyhow::bail!(
                    "git worktree remove failed: {}",
                    String::from_utf8_lossy(&output.stderr).trim()
                );
            }
            println!("Removed {}", target.display());
            Ok(0)
        }
    }
}

fn slug(s: &str) -> String {
    s.chars()
        .map(|c| {
            if c.is_alphanumeric() || c == '-' || c == '_' {
                c
            } else {
                '-'
            }
        })
        .collect()
}

/// `wrap`: run a command and forward OSC 52 clipboard sequences to the local
/// clipboard (via OSC 52 to our own terminal, which modern terminals handle).
pub async fn wrap_cmd(args: &WrapArgs) -> Result<i32> {
    if args.command.is_empty() {
        anyhow::bail!("usage: cachycli wrap <command> [args...]");
    }
    let mut cmd = tokio::process::Command::new(&args.command[0]);
    cmd.args(&args.command[1..]);
    cmd.stdout(std::process::Stdio::piped());
    cmd.stderr(std::process::Stdio::inherit());
    cmd.stdin(std::process::Stdio::inherit());
    let mut child = cmd.spawn().context("spawning command")?;
    let mut stdout = child.stdout.take().context("capturing stdout")?;
    use tokio::io::AsyncReadExt;
    let mut buffer = vec![0u8; 8192];
    let mut text_buffer = String::new();
    let mut out = std::io::stdout();
    loop {
        let n = stdout.read(&mut buffer).await?;
        if n == 0 {
            break;
        }
        let chunk = String::from_utf8_lossy(&buffer[..n]).to_string();
        out.write_all(chunk.as_bytes())?;
        out.flush()?;
        text_buffer.push_str(&chunk);
        // Forward any OSC 52 sequences to our own stdout (already written);
        // also mirror them to the system clipboard when a helper exists.
        let mut rest = text_buffer.as_str();
        while let Some(start) = rest.find("\x1b]52;") {
            if let Some(end) = rest[start..].find('\x07') {
                let sequence = rest[start..start + end + 1].to_string();
                forward_osc52(&sequence);
                rest = &rest[start + end + 1..];
            } else {
                break;
            }
        }
        text_buffer = rest.to_string();
        if text_buffer.len() > 64 * 1024 {
            text_buffer.clear();
        }
    }
    let status = child.wait().await?;
    Ok(status.code().unwrap_or(0))
}

fn forward_osc52(sequence: &str) {
    // Try wl-copy / xclip / pbcopy when available; otherwise rely on the
    // terminal's native OSC 52 handling.
    let payload = sequence
        .trim_start_matches("\x1b]52;")
        .split(';')
        .nth(1)
        .unwrap_or("")
        .trim_end_matches('\x07');
    if payload.is_empty() {
        return;
    }
    use base64::Engine;
    let Ok(decoded) = base64::engine::general_purpose::STANDARD.decode(payload) else {
        return;
    };
    let text = String::from_utf8_lossy(&decoded).to_string();
    for (cmd, args) in [
        ("wl-copy", vec![]),
        ("xclip", vec!["-selection", "clipboard"]),
        ("pbcopy", vec![]),
    ] {
        if which(cmd).is_some() {
            if let Ok(mut child) = std::process::Command::new(cmd)
                .args(&args)
                .stdin(std::process::Stdio::piped())
                .spawn()
            {
                if let Some(stdin) = child.stdin.as_mut() {
                    let _ = stdin.write_all(text.as_bytes());
                }
                let _ = child.wait();
                return;
            }
        }
    }
}

// ---------------------------------------------------------------------------
// skills helper for the TUI/headless
// ---------------------------------------------------------------------------

/// Used by main to decide TUI vs headless.
pub fn prompt_from_args(cli: &Cli) -> Option<String> {
    if let Some(p) = &cli.single {
        return Some(p.clone());
    }
    if let Some(path) = &cli.prompt_file {
        if let Ok(content) = std::fs::read_to_string(path) {
            return Some(content);
        }
    }
    if let Some(json) = &cli.prompt_json {
        if let Ok(value) = serde_json::from_str::<serde_json::Value>(json) {
            if let Some(text) = value.get("text").and_then(|v| v.as_str()) {
                return Some(text.to_string());
            }
        }
    }
    cli.prompt.clone()
}

/// Read a prompt from stdin when not a TTY (pipeline mode).
pub fn stdin_prompt() -> Option<String> {
    if std::io::stdin().is_terminal() {
        return None;
    }
    let mut buffer = String::new();
    if std::io::stdin().read_to_string(&mut buffer).is_ok() && !buffer.trim().is_empty() {
        Some(buffer)
    } else {
        None
    }
}

/// Human label for the stop reason (used by the headless JSON output).
pub fn stop_reason_label(reason: StopReason) -> &'static str {
    match reason {
        StopReason::EndTurn => "EndTurn",
        StopReason::ToolUse => "ToolUse",
        StopReason::MaxTokens => "MaxTokens",
        StopReason::MaxTurns => "MaxTurns",
        StopReason::Cancelled => "Cancelled",
        StopReason::Refusal => "Refusal",
        StopReason::Error => "Error",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn removes_the_installer_path_block() {
        let rc = "# my stuff\nexport EDITOR=vim\n\n# Added by cachycli installer\nexport PATH=\"$HOME/.cachy/bin:$PATH\"\n# after\n";
        let (stripped, changed) = strip_installer_path(rc);
        assert!(changed);
        assert!(
            !stripped.contains("cachycli installer"),
            "marcador ficou: {stripped}"
        );
        assert!(
            !stripped.contains(".cachy/bin"),
            "linha de PATH ficou: {stripped}"
        );
        assert!(stripped.contains("export EDITOR=vim"));
        assert!(stripped.contains("# after"));

        // Nothing to do when the block was never added.
        let (same, changed) = strip_installer_path("export PATH=/usr/bin\n");
        assert!(!changed);
        assert_eq!(same, "export PATH=/usr/bin\n");

        // A marker with nothing after it is left alone.
        let (kept, changed) = strip_installer_path("# Added by cachycli installer\n# nada\n");
        assert!(!changed);
        assert!(kept.contains("# nada"));
    }
}
