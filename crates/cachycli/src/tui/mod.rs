//! Interactive TUI: terminal setup, event loop, backend wiring.

pub mod app;
pub mod editor;
pub mod markdown;
pub mod persist;
pub mod picker;
pub mod render;
pub mod slash;
pub mod theme;

use std::io::stdout;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use anyhow::{Context, Result};
use crossterm::event::{
    Event, EventStream, KeyCode, KeyEvent, KeyEventKind, KeyModifiers, MouseButton, MouseEventKind,
};
use crossterm::execute;
use crossterm::terminal::{
    disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen,
};
use futures_util::StreamExt;
use ratatui::backend::CrosstermBackend;
use ratatui::layout::Rect;
use ratatui::Terminal;
use tokio::sync::mpsc;

use cachy_core::agent::{AgentEvent, AgentSession, ApprovalDecision, NativeAgent, TurnOptions};
use cachy_core::config::{BackendKind, Config};
use cachy_core::hooks;
use cachy_core::mcp::McpManager;
use cachy_core::models;
use cachy_core::permissions::{PermissionEngine, PermissionMode, RuleSet};
use cachy_core::session::{self, SessionStore};
use cachy_core::skills;
use cachy_core::tools::ToolRegistry;

use app::{Action, App, Item, Modal, NoticeKind, PickerState, UiMode};
use theme::Theme;

/// Options for starting the TUI.
#[derive(Default, Clone)]
pub struct TuiOptions {
    pub resume: Option<String>,
    pub resume_latest: bool,
    pub model: Option<String>,
    pub reasoning_effort: Option<String>,
    pub no_alt_screen: bool,
    pub minimal: bool,
    pub always_approve: bool,
    pub allow: Vec<String>,
    pub deny: Vec<String>,
    pub tools: Option<Vec<String>>,
    pub disallowed_tools: Vec<String>,
    pub max_turns: Option<u32>,
    pub permission_mode: Option<String>,
    pub backend_override: Option<String>,
    pub initial_prompt: Option<String>,
    /// Open the sessions dashboard on startup.
    pub open_dashboard: bool,
}

/// Shorten a path like `~/P/lipecode/cachycli`.
pub fn short_path(path: &std::path::Path) -> String {
    cachy_core::paths::display_path(path)
}

/// Locate a binary on PATH.
fn which_binary(name: &str) -> Option<std::path::PathBuf> {
    let path = std::env::var("PATH").ok()?;
    std::env::split_paths(&path)
        .map(|dir| dir.join(name))
        .find(|candidate| candidate.is_file())
}

struct Backend {
    kind: BackendKind,
    session: Arc<dyn AgentSession>,
    native: Option<Arc<NativeAgent>>,
    opencode: Option<Arc<cachy_core::providers::opencode::OpencodeBackend>>,
    acp: Option<Arc<cachy_core::acp::client::AcpBackend>>,
}

impl Backend {
    /// Backend that needs no configuration at all, used when the real one fails.
    fn placeholder(reason: String) -> Self {
        Self {
            kind: BackendKind::Native,
            session: Arc::new(NoopSession::new(reason)),
            native: None,
            opencode: None,
            acp: None,
        }
    }
}

/// Which provider should answer this session.
fn resolve_provider(config: &Arc<Config>, override_name: Option<&str>) -> BackendKind {
    if let Some(name) = override_name {
        if let Some(kind) = BackendKind::from_provider(name) {
            return kind;
        }
    }
    crate::providers::default(config)
}

/// Restores the terminal on every exit path, including panics and signals.
struct TerminalGuard {
    use_alt: bool,
    active: bool,
}

impl TerminalGuard {
    fn new(use_alt: bool) -> Self {
        Self {
            use_alt,
            active: true,
        }
    }

    fn restore(&mut self) {
        if !self.active {
            return;
        }
        self.active = false;
        let mut out = stdout();
        let _ = execute!(out, crossterm::event::PopKeyboardEnhancementFlags);
        let _ = execute!(out, crossterm::event::DisableMouseCapture);
        let _ = execute!(out, crossterm::event::DisableBracketedPaste);
        if self.use_alt {
            let _ = execute!(out, LeaveAlternateScreen);
        }
        let _ = disable_raw_mode();
        let _ = execute!(out, crossterm::cursor::Show);
    }
}

impl Drop for TerminalGuard {
    fn drop(&mut self) {
        self.restore();
    }
}

/// Anything that can leave the terminal in a strange state gets restored here.
fn install_panic_hook() {
    let previous = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let mut out = stdout();
        let _ = execute!(out, crossterm::event::PopKeyboardEnhancementFlags);
        let _ = execute!(
            out,
            crossterm::event::DisableMouseCapture,
            crossterm::event::DisableBracketedPaste
        );
        let _ = execute!(out, LeaveAlternateScreen, crossterm::cursor::Show);
        let _ = disable_raw_mode();
        previous(info);
    }));
}

/// Ctrl+C in a terminal without raw mode (or `kill`) must not leave a mess.
/// True while a child process owns the terminal (a login, an editor). Ctrl+C
/// belongs to that child then: closing the whole interface because someone
/// cancelled a sign-in prompt would lose their session.
static CHILD_OWNS_TERMINAL: std::sync::atomic::AtomicBool =
    std::sync::atomic::AtomicBool::new(false);

async fn watch_signals() {
    #[cfg(unix)]
    {
        use tokio::signal::unix::{signal, SignalKind};
        let mut term = match signal(SignalKind::terminate()) {
            Ok(sig) => sig,
            Err(_) => return,
        };
        let mut int = match signal(SignalKind::interrupt()) {
            Ok(sig) => sig,
            Err(_) => return,
        };
        loop {
            tokio::select! {
                _ = term.recv() => {}
                _ = int.recv() => {}
            }
            if CHILD_OWNS_TERMINAL.load(std::sync::atomic::Ordering::SeqCst) {
                // The child is the one that should react; keep waiting.
                continue;
            }
            break;
        }
        // Restore before exiting: the guard also runs on the way out.
        let mut out = stdout();
        let _ = execute!(out, crossterm::event::PopKeyboardEnhancementFlags);
        let _ = execute!(
            out,
            crossterm::event::DisableMouseCapture,
            crossterm::event::DisableBracketedPaste
        );
        let _ = execute!(out, LeaveAlternateScreen, crossterm::cursor::Show);
        let _ = disable_raw_mode();
        std::process::exit(130);
    }
}

pub async fn run(mut config: Arc<Config>, cwd: PathBuf, opts: TuiOptions) -> Result<()> {
    install_panic_hook();
    // Terminal setup.
    enable_raw_mode().context("enabling raw mode (is this a terminal?)")?;
    let mut out = stdout();
    let use_alt = !opts.no_alt_screen && !opts.minimal;
    if use_alt {
        execute!(out, EnterAlternateScreen)?;
    }
    execute!(out, crossterm::event::EnableBracketedPaste)?;
    execute!(out, crossterm::event::EnableMouseCapture)?;
    // Terminals that speak the kitty keyboard protocol report modified keys
    // (Shift+Enter, Ctrl+Enter). Terminals that do not ignore the request and
    // keep sending what they always did.
    let _ = execute!(
        out,
        crossterm::event::PushKeyboardEnhancementFlags(
            crossterm::event::KeyboardEnhancementFlags::DISAMBIGUATE_ESCAPE_CODES
                | crossterm::event::KeyboardEnhancementFlags::REPORT_ALTERNATE_KEYS,
        )
    );
    let mut guard = TerminalGuard::new(use_alt);
    tokio::spawn(watch_signals());
    let backend = CrosstermBackend::new(out);
    let mut terminal = Terminal::new(backend)?;
    terminal.clear()?;

    let theme = Theme::by_name(&config.ui.theme);
    let mut app = App::new(config.clone(), cwd.clone(), theme);
    app.keys = app::Keymap::from_pairs(
        &config
            .keys
            .iter()
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect::<Vec<_>>(),
    );
    if opts.always_approve {
        app.always_approve = true;
        app.mode = UiMode::AlwaysApprove;
    }

    // Build the agent backend.
    let (agent_tx, mut agent_rx) = mpsc::unbounded_channel::<AgentEvent>();
    let initial_kind = resolve_provider(&config, opts.backend_override.as_deref());
    let mut backend =
        match build_backend(&config, &cwd, &opts, initial_kind, agent_tx.clone(), false).await {
            Ok(b) => b,
            Err(e) => {
                // Show the error in the TUI instead of dying.
                app.push_item(Item::Error {
                    text: format!("provider unavailable: {e}"),
                });
                Backend::placeholder(e.to_string())
            }
        };
    app.backend_label = backend.kind.provider_name().to_string();
    app.model_label = backend.session.model_label();
    // opencode only knows the model once a session exists, so take it from the
    // config (or the last /model choice) right away.
    if let Some(opencode) = &backend.opencode {
        if let Some(model) = config.agent.opencode.model.clone() {
            opencode.set_model_override(Some(model.clone())).await;
            app.model_label = model;
        }
    }
    fill_menu_choices(
        &mut app,
        &config,
        initial_kind,
        agent_tx.clone(),
        backend.opencode.clone(),
    );
    // A persona chosen earlier (or with --agent) applies from the first turn.
    if let Some(name) = config.agent.name.clone() {
        if !name.trim().is_empty() {
            if let Some(persona) = cachy_core::personas::find(&cwd, &config, &name) {
                if let Some(native) = &backend.native {
                    native.set_persona(Some(persona.body.clone())).await;
                }
                app.persona_name = Some(persona.name.clone());
            }
        }
    }
    // Warm the provider while the user types: the first message then does not
    // pay for starting the opencode server (2-4s).
    if let Some(opencode) = &backend.opencode {
        let warm = opencode.clone();
        let tx = agent_tx.clone();
        tokio::spawn(async move {
            match warm.ensure_server().await {
                // Working is the expected case; only trouble is worth a line.
                Ok(_) => {}
                Err(e) => {
                    let _ = tx.send(AgentEvent::Info(format!(
                        "opencode server not reachable yet ({e}) — the first message will start it"
                    )));
                }
            }
        });
    }
    app.effort = opts
        .reasoning_effort
        .clone()
        .or_else(|| config.agent.reasoning_effort.clone());
    let (used, size) = backend.session.context_usage();
    app.context_used = used;
    if size > 0 {
        app.context_size = size;
    }
    // The native loop's session *is* cachycli's; a provider that keeps its own
    // sessions has an id of its own, so the interface needs a stored session
    // for the transcript, /share, /export, /rewind and /fork.
    app.session_id = if backend.native.is_some() {
        backend.session.session_id()
    } else {
        String::new()
    };

    // `cachycli dashboard` opens the sessions view straight away.
    if opts.open_dashboard {
        let state = sessions_picker_state(&cwd);
        app.push_modal(Modal::Sessions(state));
    }

    // Resume handling.
    if let Some(target) = &opts.resume {
        match resume_session(&backend, &cwd, target).await {
            Ok((summary, messages)) => {
                let title = summary.title.clone();
                app.session_id = summary.id.clone();
                app.session_title = title.clone();
                for message in messages {
                    match message.role {
                        cachy_core::message::Role::User => {
                            let _ = message;
                        }
                        cachy_core::message::Role::Assistant if !message.content.is_empty() => {
                            app.push_item(Item::AssistantText {
                                text: message.content,
                                cache: app::RenderCache::default(),
                            });
                        }
                        _ => {}
                    }
                }
                // The native loop reloads the history into the agent; a
                // provider that keeps its own sessions is asked to continue
                // there, and the note says which of the two happened.
                let note = if backend.native.is_some() {
                    String::new()
                } else if summary.backend_session.is_some() {
                    format!(" — continuing the {} session", backend.kind.provider_name())
                } else {
                    format!(
                        " — {} starts fresh (no session id was stored)",
                        backend.kind.provider_name()
                    )
                };
                app.set_status(format!("Resumed session \"{title}\"{note}"));
            }
            Err(e) => app.set_status(format!("resume failed: {e}")),
        }
    } else if opts.resume_latest {
        if let Ok(Some(summary)) = session::most_recent(&cwd) {
            if let Ok((_, messages)) = resume_session(&backend, &cwd, &summary.id).await {
                for message in messages {
                    if message.role == cachy_core::message::Role::Assistant
                        && !message.content.is_empty()
                    {
                        app.push_item(Item::AssistantText {
                            text: message.content,
                            cache: app::RenderCache::default(),
                        });
                    }
                }
                app.session_id = summary.id.clone();
                app.session_title = summary.title.clone();
                app.set_status(format!("Resumed \"{}\"", summary.title));
            }
        } else {
            app.set_status("No previous session in this directory");
        }
    }

    // Providers that keep their own sessions still need a stored one here.
    if backend.native.is_none() && app.session_id.is_empty() {
        match SessionStore::create(&cwd, &app.model_label, "cachy", None) {
            Ok(store) => app.session_id = store.summary.id.clone(),
            Err(e) => app.set_status(format!("could not start a session: {e}")),
        }
    }

    // Hooks: session start.
    let discovered_hooks = hooks::discover(&cwd, &config);
    let _ = hooks::run_event(
        &discovered_hooks,
        hooks::HookEvent::SessionStart,
        &cwd,
        None,
        &serde_json::json!({ "cwd": cwd.to_string_lossy() }),
    )
    .await;

    let mut events = EventStream::new();
    let mut tick = tokio::time::interval(Duration::from_millis(120));
    tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);

    // Providers with sessions of their own do not write the transcript: the
    // interface does, so /export, /share, the dashboard and a later --resume
    // have something to read.
    let mut transcript = backend.native.is_none().then(|| {
        persist::BackendTranscript::new(
            cwd.clone(),
            app.session_id.clone(),
            backend.kind.provider_name().to_string(),
        )
    });

    // Kick off the initial prompt when provided.
    if let Some(prompt) = opts.initial_prompt.clone() {
        spawn_prompt(
            backend.session.clone(),
            prompt,
            Arc::new(hooks::discover(&cwd, &config)),
            cwd.clone(),
            agent_tx.clone(),
            backend.native.is_none(),
        );
    }

    let result = loop {
        // Paint before waiting for the next event, so input never feels late.
        if let Err(e) = terminal.draw(|frame| render::draw(frame, &mut app)) {
            break Err(anyhow::anyhow!("draw failed: {e}"));
        }

        // A finished turn can ring the terminal's bell or post a desktop
        // notification — after the draw, so it never lands inside a frame.
        if let Some(text) = app.pending_notification.take() {
            use std::io::Write as _;
            let mut out = std::io::stdout();
            let _ = match app.ui_notify.as_str() {
                "osc" => write!(out, "\x1b]9;{text}\x07"),
                _ => write!(out, "\x07"),
            };
            let _ = out.flush();
        }

        let action = match app.pending_action.take() {
            // A slash command asked for something (a picker, a mode change, a
            // model switch): run it through the same handler as key actions.
            Some(queued) => queued,
            None => tokio::select! {
            maybe_event = events.next() => {
                match maybe_event {
                    Some(Ok(Event::Key(key))) if key.kind != KeyEventKind::Release => {
                        handle_key(&mut app, key)
                    }
                    Some(Ok(Event::Paste(text))) => {
                        app.editor.insert_str(&text.replace('\r', ""));
                        app.update_slash_menu();
                        Action::None
                    }
                    Some(Ok(Event::Mouse(mouse))) => {
                        if app.mouse_reporting {
                            handle_mouse(&mut app, mouse)
                        } else {
                            Action::None
                        }
                    }
                    Some(Ok(Event::Resize(_, _))) => Action::None,
                    Some(Ok(_)) => Action::None,
                    Some(Err(_)) | None => Action::Quit,
                }
            }
            Some(event) = agent_rx.recv() => {
                let event = match event {
                    AgentEvent::PermissionRequest { id, tool, title, detail, options } => {
                        AgentEvent::PermissionRequest { id, tool, title, detail, options }
                    }
                    other => other,
                };
                if let Some(transcript) = transcript.as_mut() {
                    transcript.set_session(&app.session_id);
                    transcript.on_event(&event, backend.session.as_ref()).await;
                }
                app.apply_agent_event(event);
                Action::None
            }
            _ = tick.tick() => {
                app.spinner_frame = app.spinner_frame.wrapping_add(1);
                // Keep the task panel live while it is on top (~2s at 120ms).
                if matches!(app.modal(), Some(Modal::Tasks { .. })) {
                    app.tasks_refresh_ticks = app.tasks_refresh_ticks.wrapping_add(1);
                    if app.tasks_refresh_ticks >= 16 {
                        app.tasks_refresh_ticks = 0;
                        Action::RefreshTasks
                    } else {
                        Action::None
                    }
                } else {
                    app.tasks_refresh_ticks = 0;
                    Action::None
                }
            }
            },
        };

        // Backends can learn their model mid-session (Claude Code reports it on
        // the init event); keep the prompt label honest.
        if !app.busy {
            let live = backend.session.model_label();
            if !live.is_empty() && live != app.model_label && !app.provider_is_opencode {
                app.model_label = live;
            }
            // The model can set the goal itself (`set_goal`); the header follows.
            if let Some(native) = &backend.native {
                let goal = native.goal().await;
                if goal != app.goal {
                    app.goal = goal;
                }
            }
        }

        match action {
            Action::None => {}
            Action::Quit => {
                // Cancel pending work and exit.
                backend.session.cancel();
                break Ok(());
            }
            Action::ModeChange(mode) => {
                app.mode = mode;
                let permission = match mode {
                    UiMode::Normal => cachy_core::permissions::PermissionMode::Default,
                    UiMode::Plan => cachy_core::permissions::PermissionMode::Plan,
                    UiMode::AlwaysApprove => cachy_core::permissions::PermissionMode::Auto,
                    // Auto-review approves edits and read-only work but still asks
                    // before shell commands.
                    UiMode::AutoReview => cachy_core::permissions::PermissionMode::AcceptEdits,
                };
                if let Some(native) = &backend.native {
                    native.set_permission_mode(permission).await;
                    native
                        .set_always_approve(mode == UiMode::AlwaysApprove)
                        .await;
                }
                app.always_approve = mode == UiMode::AlwaysApprove;
                app.set_status(match mode.badge() {
                    Some(badge) => format!("Mode: {badge}"),
                    None => "Mode: normal".to_string(),
                });
            }
            Action::Cancel => {
                backend.session.cancel();
                app.busy = false;
                if !app.queue.is_empty() {
                    app.queue.clear();
                    app.set_status("Cancelled — queued messages dropped");
                } else {
                    app.set_status("Cancelled");
                }
            }
            Action::Send(text) => {
                if app.busy {
                    if text.trim_start().starts_with('/') {
                        let (command, args) = slash::split_command(&text);
                        if let Some(next) =
                            handle_slash(&mut app, &backend, &config, &cwd, &command, &args).await
                        {
                            app.pending_action = Some(next);
                        }
                        app.editor.clear();
                        app.slash_menu_open = false;
                    } else {
                        app.queue_prompt(text);
                    }
                } else {
                    app.busy = true;
                    app.turn_started = Some(std::time::Instant::now());
                    app.note_sent(&text);
                    // The agent emits its own UserMessage event; do not echo it
                    // here to avoid a duplicate bubble.
                    spawn_prompt(
                        backend.session.clone(),
                        text,
                        Arc::new(hooks::discover(&cwd, &config)),
                        cwd.clone(),
                        agent_tx.clone(),
                        backend.native.is_none(),
                    );
                }
            }
            Action::Approve {
                id,
                choice,
                feedback,
            } => {
                if let Some(native) = &backend.native {
                    let decision = match (&choice, &feedback) {
                        (cachy_core::permissions::ApprovalChoice::Reject, Some(f)) => {
                            ApprovalDecision::RejectWithFeedback(f.clone())
                        }
                        (c, _) => ApprovalDecision::Choice(*c),
                    };
                    if choice == cachy_core::permissions::ApprovalChoice::AllowAlways {
                        app.always_approve = true;
                    }
                    native.resolve_approval(&id, decision).await;
                } else if let Some(opencode) = &backend.opencode {
                    let reply = match choice {
                        cachy_core::permissions::ApprovalChoice::AllowAlways => "AllowAlways",
                        cachy_core::permissions::ApprovalChoice::AllowSession => "AllowSession",
                        cachy_core::permissions::ApprovalChoice::Allow => "Allow",
                        cachy_core::permissions::ApprovalChoice::Reject => "Reject",
                    };
                    if choice == cachy_core::permissions::ApprovalChoice::AllowAlways {
                        app.always_approve = true;
                    }
                    if let Err(e) = opencode.reply_permission(&id, reply).await {
                        app.set_status(format!("permission reply failed: {e}"));
                    }
                } else if let Some(acp) = &backend.acp {
                    cachy_core::acp::client::reply_acp_permission(&acp.client, &id, choice).await;
                }
            }
            Action::Slash { command, args } => {
                if let Some(next) =
                    handle_slash(&mut app, &backend, &config, &cwd, &command, &args).await
                {
                    app.pending_action = Some(next);
                }
            }
            Action::SetPersona(name) => {
                let personas = cachy_core::personas::discover(&cwd, &config);
                if name.trim().is_empty() {
                    if let Some(native) = &backend.native {
                        native.set_persona(None).await;
                    }
                    app.persona_name = None;
                    if !persist_setting(&cwd, "agent", "name", "\"\"") {
                        app.set_status("persona cleared, but the config could not be written");
                    } else {
                        app.set_status("persona cleared");
                    }
                } else {
                    match personas
                        .iter()
                        .find(|persona| persona.name.eq_ignore_ascii_case(name.trim()))
                    {
                        None => app.set_status(format!(
                            "unknown persona \"{name}\" — /personas lists the ones in .cachy/agents and .claude/agents"
                        )),
                        Some(persona) => {
                            if let Some(native) = &backend.native {
                                native.set_persona(Some(persona.body.clone())).await;
                                app.persona_name = Some(persona.name.clone());
                                if persist_setting(
                                    &cwd,
                                    "agent",
                                    "name",
                                    &format!("{:?}", persona.name),
                                ) {
                                    app.set_status(format!(
                                        "persona: {} ({})",
                                        persona.name, persona.source
                                    ));
                                } else {
                                    app.set_status(
                                        "persona applied, but the config could not be written",
                                    );
                                }
                            } else {
                                app.set_status(format!(
                                    "{} runs its own agent profiles — set [agent.opencode] agent or use the provider's own agents",
                                    backend.kind.provider_name()
                                ));
                            }
                        }
                    }
                }
            }
            Action::Rewind(turns) => {
                let turns = turns.max(1);
                let mut removed = 0usize;
                while removed < turns {
                    // Drop items back to (and including) the previous user turn.
                    let Some(index) = app
                        .items
                        .iter()
                        .rposition(|item| matches!(item, Item::User { .. }))
                    else {
                        break;
                    };
                    app.items.truncate(index);
                    removed += 1;
                }
                if removed == 0 {
                    app.set_status("nothing to rewind");
                } else {
                    // Native sessions keep their own history: truncate it too.
                    let mut truncated = false;
                    if backend.native.is_some() {
                        if let Ok(mut store) = SessionStore::open(&cwd, &app.session_id) {
                            if let Ok(messages) = store.load_chat() {
                                let keep = messages.len().saturating_sub(removed * 2);
                                if store.rewrite_chat(&messages[..keep]).is_ok() {
                                    truncated = true;
                                }
                            }
                        }
                    }
                    let rewound_backend = if backend.native.is_none() {
                        backend.session.rewind_backend_session(removed).await
                    } else {
                        false
                    };
                    app.scroll_to_bottom();
                    app.set_status(if truncated {
                        format!("rewound {removed} turn(s) — session history truncated")
                    } else if rewound_backend {
                        format!(
                            "rewound {removed} turn(s) — the {} session dropped them too",
                            backend.kind.provider_name()
                        )
                    } else {
                        format!(
                            "rewound {removed} turn(s) here; the provider session still has them (use /new for a clean one)"
                        )
                    });
                }
            }
            Action::Fork => {
                let mut forked = false;
                if backend.native.is_some() {
                    if let Ok(source) = SessionStore::open(&cwd, &app.session_id) {
                        if let Ok(messages) = source.load_chat() {
                            let model = app.model_label.clone();
                            if let Ok(mut target) =
                                SessionStore::create(&cwd, &model, "cachy", Some(&app.session_id))
                            {
                                if target.rewrite_chat(&messages).is_ok() {
                                    app.session_id = target.summary.id.clone();
                                    app.session_title = target.summary.title.clone();
                                    if let Some(native) = &backend.native {
                                        // Otherwise the loop keeps writing to
                                        // the session that was forked from.
                                        native
                                            .adopt_session(target.clone(), messages.clone())
                                            .await;
                                    }
                                    forked = true;
                                }
                            }
                        }
                    }
                }
                if forked {
                    app.push_item(Item::Notice {
                        text: format!("forked into session {}", app.session_id),
                        kind: NoticeKind::Info,
                    });
                    app.set_status("forked — this session now continues on its own");
                } else {
                    // A provider that keeps its own session: copy the transcript
                    // into a new stored session and ask the provider to branch
                    // its side too (opencode and ACP agents can).
                    backend.session.cancel();
                    let previous = app.session_id.clone();
                    let history = SessionStore::open(&cwd, &previous)
                        .ok()
                        .and_then(|store| store.load_chat().ok())
                        .unwrap_or_default();
                    let branched = backend.session.fork_backend_session().await;
                    match SessionStore::create(&cwd, &app.model_label, "cachy", Some(&previous)) {
                        Err(e) => app.set_status(format!("fork failed: {e}")),
                        Ok(mut target) => {
                            let _ = target.rewrite_chat(&history);
                            app.session_id = target.summary.id.clone();
                            app.session_title = target.summary.title.clone();
                            if let Some(transcript) = transcript.as_mut() {
                                transcript.set_session(&app.session_id);
                            }
                            app.push_item(Item::Notice {
                                text: format!("forked into session {}", app.session_id),
                                kind: NoticeKind::Info,
                            });
                            app.set_status(if branched {
                                format!(
                                    "forked — same context, a new {} session",
                                    backend.kind.provider_name()
                                )
                            } else {
                                format!(
                                    "forked — {} cannot branch, so both sessions continue the same conversation",
                                    backend.kind.provider_name()
                                )
                            });
                        }
                    }
                }
            }
            Action::SetGoal(goal) => match &backend.native {
                None => app.set_status(format!(
                    "goals need the native provider (current: {})",
                    backend.kind.provider_name()
                )),
                Some(native) => {
                    native.set_goal(goal.clone()).await;
                    app.goal = goal.clone();
                    match goal {
                        Some(text) => app.set_status(format!(
                            "goal set: {}   (/goal verify checks it with {} subagents)",
                            text.chars().take(60).collect::<String>(),
                            config.agent.goal_verifier_count
                        )),
                        None => app.set_status("goal cleared"),
                    }
                }
            },
            Action::VerifyGoal => match &backend.native {
                None => app.set_status(format!(
                    "goals need the native provider (current: {})",
                    backend.kind.provider_name()
                )),
                Some(native) if app.goal.is_none() => {
                    app.set_status("no goal set — use /goal <text>")
                }
                Some(native) => {
                    let count = config.agent.goal_verifier_count.max(1);
                    app.set_status(format!(
                        "verifying the goal with {count} subagents — watch /tasks"
                    ));
                    let native = native.clone();
                    let tx = agent_tx.clone();
                    tokio::spawn(async move {
                        let reports = native.verify_goal(count).await;
                        for line in reports {
                            let _ = tx.send(AgentEvent::Info(line));
                        }
                    });
                }
            },
            Action::RunWorkflow(name) => match cachy_core::workflows::find(&cwd, &name) {
                None => app.set_status(format!(
                    "unknown workflow \"{name}\" — /workflow lists the ones in .cachy/workflows"
                )),
                Some(workflow) => match &backend.native {
                    None => app.set_status(format!(
                        "workflows run subagents, which need the native provider (current: {})",
                        backend.kind.provider_name()
                    )),
                    Some(native) => {
                        app.set_status(format!(
                            "workflow \"{}\": {} step(s) — watch /tasks",
                            workflow.name,
                            workflow.steps.len()
                        ));
                        let native = native.clone();
                        let tx = agent_tx.clone();
                        tokio::spawn(async move {
                            let reports = native.run_workflow(workflow).await;
                            for line in reports {
                                let _ = tx.send(AgentEvent::Info(format!("workflow: {line}")));
                            }
                        });
                    }
                },
            },
            Action::JumpTo(index) => {
                let start = app.item_offsets.get(index).copied().unwrap_or(0);
                let total = app.total_lines;
                app.scroll_from_bottom = total.saturating_sub(start).min(total);
                app.set_status("jumped — Ctrl+End returns to the newest message");
            }
            Action::NewSession => {
                // Both sides start over: a new stored session here, a new one
                // on the provider, so the model does not keep the old context.
                backend.session.reset_backend_session().await;
                match SessionStore::create(&cwd, &app.model_label, "cachy", None) {
                    Ok(store) => {
                        app.session_id = store.summary.id.clone();
                        app.session_title.clear();
                        if let Some(transcript) = transcript.as_mut() {
                            transcript.set_session(&app.session_id);
                        }
                    }
                    Err(e) => app.set_status(format!("could not start a session: {e}")),
                }
                app.items.clear();
                app.context_used = 0;
                app.output_tokens = 0;
                app.push_item(Item::Notice {
                    text: "New session — context cleared. (Sessions persist to disk as you chat.)"
                        .into(),
                    kind: NoticeKind::Info,
                });
            }
            Action::Timeline => {
                let lines = app.timeline_lines();
                app.open_info("Timeline", lines);
            }
            Action::TimelineAll(filter) => {
                // "/timeline all" or "/timeline all <project|title>"
                let needle = filter.to_lowercase();
                let turns: Vec<cachy_core::activity::TurnRecord> =
                    cachy_core::activity::recent_turns(200)
                        .into_iter()
                        .filter(|turn| {
                            needle.is_empty()
                                || turn.cwd.to_lowercase().contains(&needle)
                                || turn.title.to_lowercase().contains(&needle)
                        })
                        .take(60)
                        .collect();
                let title = if filter.is_empty() {
                    "Timeline · every session".to_string()
                } else {
                    format!("Timeline · every session · {filter}")
                };
                let lines = app::activity_lines(&turns);
                app.open_info(&title, lines);
            }
            Action::Tasks | Action::RefreshTasks => {
                let mut rows: Vec<app::TaskRow> = Vec::new();
                if let Some(native) = &backend.native {
                    for task in native.tasks().list().await {
                        let status = task.status.read().await.clone();
                        let output = task.output.read().await.clone();
                        rows.push(app::TaskRow {
                            id: task.id.clone(),
                            kind: match task.kind {
                                cachy_core::tools::TaskKind::Command => "command".into(),
                                cachy_core::tools::TaskKind::Subagent => "subagent".into(),
                            },
                            description: task.description.clone(),
                            status: status.as_str().to_string(),
                            output,
                            started: task.started_at,
                            frozen: matches!(status, cachy_core::tools::TaskStatus::Running)
                                .then_some(0.0)
                                .unwrap_or_else(|| task.elapsed_secs()),
                            finished: !matches!(status, cachy_core::tools::TaskStatus::Running),
                        });
                    }
                }
                let selected = match app.modal() {
                    Some(Modal::Tasks { selected, .. }) => {
                        (*selected).min(rows.len().saturating_sub(1))
                    }
                    _ => 0,
                };
                app.close_modal();
                app.push_modal(Modal::Tasks { rows, selected });
                if backend.native.is_none() {
                    app.set_status(format!(
                        "background work runs inside the {} runtime",
                        backend.kind.provider_name()
                    ));
                }
            }
            Action::ShareSession(path) => {
                let store = SessionStore::open(&cwd, &app.session_id).or_else(|_| {
                    session::most_recent(&cwd)?
                        .ok_or_else(|| anyhow::anyhow!("no session stored yet"))
                        .and_then(|s| SessionStore::open(&cwd, &s.id))
                });
                match store.and_then(|store| {
                    let target = if path.is_empty() {
                        let name = if store.summary.title.is_empty() {
                            format!(
                                "cachy-session-{}.cachy",
                                &store.summary.id[..8.min(store.summary.id.len())]
                            )
                        } else {
                            format!("{}.cachy", slug(&store.summary.title))
                        };
                        dirs::home_dir().unwrap_or_else(|| cwd.clone()).join(name)
                    } else {
                        std::path::PathBuf::from(&path)
                    };
                    cachy_core::share::share_to_file(&store, &target)
                }) {
                    Ok(path) => app.set_status(format!(
                        "session written to {} — send it, and the other side opens it with /import",
                        path.display()
                    )),
                    Err(e) => app.set_status(format!("share failed: {e}")),
                }
            }
            Action::SharePush(url) => {
                let url = if url.is_empty() {
                    config.share.server.clone().unwrap_or_default()
                } else {
                    url
                };
                if url.is_empty() {
                    app.set_status("usage: /share push ws://host:8765 (or set [share] server)");
                } else {
                    let store = SessionStore::open(&cwd, &app.session_id).or_else(|_| {
                        session::most_recent(&cwd)?
                            .ok_or_else(|| anyhow::anyhow!("no session stored yet"))
                            .and_then(|s| SessionStore::open(&cwd, &s.id))
                    });
                    match store {
                        Err(e) => app.set_status(format!("share failed: {e}")),
                        Ok(store) => {
                            match cachy_core::share::bundle(&store)
                                .and_then(|bundle| cachy_core::share::to_json(&bundle))
                            {
                                Err(e) => app.set_status(format!("share failed: {e}")),
                                Ok(text) => match cachy_core::share_net::push(&url, &text).await {
                                    Ok(entry) => app.set_status(format!(
                                        "shared as {} — /share pull {} on the other side",
                                        entry.id, entry.id
                                    )),
                                    Err(e) => app.set_status(format!("share failed: {e}")),
                                },
                            }
                        }
                    }
                }
            }
            Action::Shares(spec) => {
                // "/shares", "/shares ws://…", "/shares remove <id> [url]"
                let mut parts = spec.split_whitespace();
                let first = parts.next().unwrap_or("");
                let removing = first == "remove";
                let id = if removing {
                    parts.next().unwrap_or("").to_string()
                } else {
                    String::new()
                };
                let url = if removing {
                    parts.next()
                } else {
                    (!first.is_empty()).then_some(first)
                }
                .map(|s| s.to_string())
                .or_else(|| config.share.server.clone())
                .unwrap_or_default();
                if url.is_empty() {
                    app.set_status(
                        "usage: /shares [url] or /shares remove <id> [url] (or set [share] server)",
                    );
                } else if removing {
                    if id.is_empty() {
                        app.set_status("usage: /shares remove <id> [url]");
                    } else {
                        match cachy_core::share_net::remove(&url, &id).await {
                            Ok(removed) => app.set_status(format!("removed {removed} from {url}")),
                            Err(e) => app.set_status(format!("remove failed: {e}")),
                        }
                    }
                } else {
                    match cachy_core::share_net::list_remote(&url).await {
                        Err(e) => app.set_status(format!("could not reach {url}: {e}")),
                        Ok(items) if items.is_empty() => app.open_info(
                            "Shared sessions",
                            vec![format!("  {url} is holding nothing yet")],
                        ),
                        Ok(items) => {
                            let mut lines = vec![
                                format!("  {url}"),
                                String::new(),
                                format!(
                                    "  {:<38} {:>8}  {:<16}  {}",
                                    "ID", "MESSAGES", "WHEN", "TITLE"
                                ),
                            ];
                            for item in &items {
                                let title = if item.title.is_empty() {
                                    "(untitled)"
                                } else {
                                    &item.title
                                };
                                lines.push(format!(
                                    "  {:<38} {:>8}  {:<16}  {}",
                                    item.id,
                                    item.messages,
                                    cachy_core::activity::when_text(&item.updated_at),
                                    title
                                ));
                            }
                            lines.push(String::new());
                            lines.push("  /share pull <id> imports one".to_string());
                            app.open_info("Shared sessions", lines);
                        }
                    }
                }
            }
            Action::SharePull(spec) => {
                let mut parts = spec.split_whitespace();
                let id = parts.next().unwrap_or("").to_string();
                let url = parts
                    .next()
                    .map(|s| s.to_string())
                    .or_else(|| config.share.server.clone())
                    .unwrap_or_default();
                if url.is_empty() {
                    app.set_status(
                        "usage: /share pull <id> ws://host:8765 (or set [share] server)",
                    );
                } else {
                    match cachy_core::share_net::fetch(&url, &id).await {
                        Err(e) => app.set_status(format!("import failed: {e}")),
                        Ok(text) => match cachy_core::share::import_into(&cwd, &text) {
                            Err(e) => app.set_status(format!("import failed: {e}")),
                            Ok(store) => {
                                let new_id = store.summary.id.clone();
                                app.set_status(format!(
                                    "imported as {new_id} — /resume {new_id} opens it"
                                ));
                            }
                        },
                    }
                }
            }
            Action::ImportSession(path) => {
                let target = if let Some(rest) = path.strip_prefix("~/") {
                    dirs::home_dir().unwrap_or_default().join(rest)
                } else {
                    std::path::PathBuf::from(&path)
                };
                match std::fs::read_to_string(&target)
                    .map_err(|e| anyhow::anyhow!("{e}"))
                    .and_then(|text| cachy_core::share::import_into(&cwd, &text))
                {
                    Err(e) => app.set_status(format!("import failed: {e}")),
                    Ok(store) => {
                        let id = store.summary.id.clone();
                        app.pending_action = Some(Action::Resume(id));
                    }
                }
            }
            Action::InstallPlugin(name) => {
                let source = config.plugins.index.clone();
                match source {
                    None => app.set_status("no plugin index configured"),
                    Some(source) => {
                        app.set_status(format!("installing {name}…"));
                        let tx = agent_tx.clone();
                        tokio::spawn(async move {
                            let message = match cachy_core::plugins::fetch_index(&source).await {
                                Err(e) => format!("index failed: {e}"),
                                Ok(entries) => match entries.iter().find(|e| e.name == name) {
                                    None => format!("\"{name}\" is no longer in the index"),
                                    Some(entry) => {
                                        match cachy_core::plugins::install_entry(entry) {
                                            Ok(path) => format!(
                                                "installed {name} into {} — restart to load it",
                                                path.display()
                                            ),
                                            Err(e) => format!("install failed: {e}"),
                                        }
                                    }
                                },
                            };
                            let _ = tx.send(AgentEvent::Info(message));
                        });
                    }
                }
            }
            Action::KillTask(id) => {
                if let Some(native) = &backend.native {
                    match native.tasks().get(&id).await {
                        Some(task) => {
                            task.cancel.cancel();
                            app.set_status(format!("cancelled {id}"));
                        }
                        None => app.set_status(format!("task {id} is gone")),
                    }
                }
                app.pending_action = Some(Action::RefreshTasks);
            }
            Action::BenchModels => {
                if app.benching.swap(true, std::sync::atomic::Ordering::SeqCst) {
                    app.set_status("a benchmark is already running");
                } else {
                    let models: Vec<String> = app
                        .opencode_model_options
                        .lock()
                        .map(|list| list.iter().map(|(id, _)| id.clone()).collect())
                        .unwrap_or_default();
                    let models: Vec<String> = if models.is_empty() {
                        Vec::new()
                    } else {
                        models.into_iter().take(8).collect()
                    };
                    if models.is_empty() {
                        app.set_status("no models to benchmark yet — open /model once");
                        app.benching
                            .store(false, std::sync::atomic::Ordering::SeqCst);
                    } else {
                        let command = config.agent.opencode.command.clone();
                        let results = app.bench_results.clone();
                        let benching = app.benching.clone();
                        let tx = agent_tx.clone();
                        app.set_status(format!("benchmarking {} models…", models.len()));
                        tokio::spawn(async move {
                            let mut measured: Vec<(String, f64)> = Vec::new();
                            for (index, model) in models.iter().enumerate() {
                                let ttft = bench_model(&command, model).await;
                                if let Some(seconds) = ttft {
                                    if let Ok(mut map) = results.lock() {
                                        map.insert(model.clone(), seconds);
                                        app_save_bench(&map);
                                    }
                                    measured.push((model.clone(), seconds));
                                    let _ = tx.send(AgentEvent::Info(format!(
                                        "bench {}/{}: {} · {:.1}s",
                                        index + 1,
                                        models.len(),
                                        model,
                                        seconds
                                    )));
                                } else {
                                    let _ = tx.send(AgentEvent::Info(format!(
                                        "bench {}/{}: {} · no answer in time",
                                        index + 1,
                                        models.len(),
                                        model
                                    )));
                                }
                            }
                            measured.sort_by(|a, b| {
                                a.1.partial_cmp(&b.1).unwrap_or(std::cmp::Ordering::Equal)
                            });
                            let fastest: Vec<String> = measured
                                .iter()
                                .take(3)
                                .map(|(id, secs)| format!("{id} ({secs:.1}s)"))
                                .collect();
                            let _ = tx.send(AgentEvent::Info(if fastest.is_empty() {
                                "bench finished: no model answered in time".to_string()
                            } else {
                                format!("fastest: {}", fastest.join(", "))
                            }));
                            benching.store(false, std::sync::atomic::Ordering::SeqCst);
                        });
                    }
                }
            }
            Action::SignInProvider(wanted) => {
                let infos = crate::providers::detect(&config);
                let wanted = wanted.clone();
                let info = infos
                    .iter()
                    .find(|p| p.id.provider_name() == wanted)
                    .cloned()
                    .or_else(|| {
                        infos
                            .iter()
                            .find(|p| p.id == BackendKind::Opencode)
                            .cloned()
                    });
                match info {
                    None => app.set_status("no provider to sign in to"),
                    Some(info) if !info.installed => app.set_status(format!(
                        "{} is not installed — {}",
                        info.label, info.install_hint
                    )),
                    Some(info) => {
                        let label = info.label;
                        let mouse = app.mouse_reporting;
                        let outcome = with_terminal_suspended(&mut terminal, mouse, || {
                            crate::providers::sign_in(&info)
                        })
                        .await;
                        match outcome {
                            Ok(message) => app.set_status(message),
                            Err(e) => app.set_status(format!("sign-in failed: {e}")),
                        }
                        // New credentials usually mean new models: re-read them.
                        app.opencode_model_options
                            .lock()
                            .map(|mut list| list.clear())
                            .ok();
                        fill_menu_choices(
                            &mut app,
                            &config,
                            backend.kind,
                            agent_tx.clone(),
                            backend.opencode.clone(),
                        );
                        let connected = crate::providers::detect(&config)
                            .iter()
                            .find(|p| p.id == info.id)
                            .map(|p| p.connected)
                            .unwrap_or(false);
                        if connected {
                            app.push_item(Item::Notice {
                                text: format!("{label} is connected — pick a model with /model"),
                                kind: NoticeKind::Info,
                            });
                        }
                    }
                }
            }
            Action::SignOut => {
                let program = "opencode".to_string();
                let args = ["auth", "logout"];
                let mouse = app.mouse_reporting;
                let outcome = with_terminal_suspended(&mut terminal, mouse, || async {
                    let status = tokio::process::Command::new(&program)
                        .args(args)
                        .status()
                        .await;
                    match status {
                        Ok(status) if status.success() => {
                            Ok(format!("{program} sign-out finished"))
                        }
                        Ok(status) => Ok(format!("{program} sign-out exited with {status}")),
                        Err(e) => Err(anyhow::anyhow!("could not run `{program}`: {e}")),
                    }
                })
                .await;
                match outcome {
                    Ok(message) => app.set_status(message),
                    Err(e) => app.set_status(format!("sign-out failed: {e}")),
                }
                app.opencode_model_options
                    .lock()
                    .map(|mut list| list.clear())
                    .ok();
                fill_menu_choices(
                    &mut app,
                    &config,
                    backend.kind,
                    agent_tx.clone(),
                    backend.opencode.clone(),
                );
            }
            Action::SendQueued => {
                if !app.busy {
                    if let Some(text) = app.queue.first().cloned() {
                        app.queue.remove(0);
                        if text.trim_start().starts_with('/') {
                            let (command, args) = slash::split_command(&text);
                            if let Some(next) =
                                handle_slash(&mut app, &backend, &config, &cwd, &command, &args)
                                    .await
                            {
                                app.pending_action = Some(next);
                            }
                        } else {
                            app.busy = true;
                            app.turn_started = Some(std::time::Instant::now());
                            app.note_sent(&text);
                            spawn_prompt(
                                backend.session.clone(),
                                text,
                                Arc::new(hooks::discover(&cwd, &config)),
                                cwd.clone(),
                                agent_tx.clone(),
                                backend.native.is_none(),
                            );
                        }
                    }
                }
            }
            Action::RefreshResumeList => {
                let state = resume_picker_state(&cwd);
                app.push_modal(Modal::Resume(state));
            }
            Action::RefreshModelList => {
                let state = model_picker_state(&config, &app);
                app.push_modal(Modal::Model(state));
            }
            Action::Resume(id) => {
                match resume_session(&backend, &cwd, &id).await {
                    Ok((summary, messages)) => {
                        // A resumed session brings its goal back.
                        if let Ok(store) = SessionStore::open(&cwd, &id) {
                            app.goal = store.summary.goal.clone();
                            if let Some(native) = &backend.native {
                                native.set_goal(store.summary.goal.clone()).await;
                            }
                        }
                        app.items.clear();
                        let continued =
                            summary.backend_session.is_some() && backend.native.is_none();
                        app.push_item(Item::Notice {
                            text: format!(
                                "Resumed session \"{}\" ({id}){}",
                                summary.title,
                                if continued {
                                    format!(
                                        " — continuing the {} session",
                                        backend.kind.provider_name()
                                    )
                                } else {
                                    String::new()
                                }
                            ),
                            kind: NoticeKind::Info,
                        });
                        for message in messages {
                            if message.role == cachy_core::message::Role::Assistant
                                && !message.content.is_empty()
                            {
                                app.push_item(Item::AssistantText {
                                    text: message.content,
                                    cache: app::RenderCache::default(),
                                });
                            }
                        }
                        app.session_title = summary.title.clone();
                    }
                    Err(e) => app.set_status(format!("resume failed: {e}")),
                }
            }
            Action::OpenSessions => {
                let state = sessions_picker_state(&cwd);
                app.push_modal(Modal::Sessions(state));
            }
            Action::SetModel(name) => {
                let name = name.trim().to_string();
                if name.is_empty() {
                    app.set_status("usage: /model <provider/model>");
                } else if let Some(opencode) = &backend.opencode {
                    // opencode models are named `provider/model`; refuse typos so
                    // the label and the next request stay meaningful.
                    let known: Vec<String> = app
                        .opencode_model_options
                        .lock()
                        .map(|list| list.iter().map(|(value, _)| value.clone()).collect())
                        .unwrap_or_default();
                    if !known.is_empty() && !known.contains(&name.to_string()) {
                        app.set_status(format!(
                            "unknown model \"{name}\" — open /model and pick one from the list"
                        ));
                        app.pending_action = None;
                    } else {
                        opencode.set_model_override(Some(name.clone())).await;
                        if !persist_setting(&cwd, "agent.opencode", "model", &format!("{name:?}")) {
                            app.set_status("model applied, but the config could not be written");
                        }
                        app.model_label = name.clone();
                        app.set_status(format!("opencode model: {name}"));
                        app.push_item(Item::Notice {
                        text: format!("opencode now uses {name}. The next message opens a fresh opencode session."),
                        kind: NoticeKind::Info,
                    });
                    }
                } else if let Some(model) = models::resolve(&config, Some(&name)) {
                    if let Some(native) = &backend.native {
                        native.set_model(model.clone()).await;
                        app.model_label = model.display().to_string();
                        app.context_size = model.context_window;
                        app.set_status(format!("Model: {}", model.display()));
                    } else {
                        app.set_status(
                            "Model switching applies to the native and opencode providers",
                        );
                    }
                } else {
                    app.set_status(format!("unknown model \"{name}\""));
                }
            }
            Action::SetProvider(name) => {
                let infos = crate::providers::detect(&config);
                let info = infos.iter().find(|p| p.id.provider_name() == name).cloned();
                match info {
                    None => app.set_status(format!("unknown provider \"{name}\"")),
                    Some(info) if !info.installed => {
                        app.set_status(format!(
                            "{} is not installed — {}",
                            info.label, info.install_hint
                        ));
                    }
                    Some(info) => {
                        let mut ready = info.connected;
                        if !ready && !info.login.is_empty() {
                            app.set_status(format!("Signing in to {}…", info.label));
                            let mouse = app.mouse_reporting;
                            let outcome = with_terminal_suspended(&mut terminal, mouse, || {
                                crate::providers::sign_in(&info)
                            })
                            .await;
                            match outcome {
                                Ok(message) => app.set_status(message),
                                Err(e) => app.set_status(format!("sign-in failed: {e}")),
                            }
                            ready = crate::providers::detect(&config)
                                .iter()
                                .find(|p| p.id == info.id)
                                .map(|p| p.connected)
                                .unwrap_or(false);
                            if !ready {
                                app.set_status(format!(
                                    "{} still looks disconnected. Try again from a shell, then /provider.",
                                    info.label
                                ));
                            }
                        }
                        if ready {
                            let kind = info.id;
                            let session_id = kind.provider_name().to_string();
                            backend.session.cancel();
                            backend.session.shutdown().await;
                            let mut next_config = (*config).clone();
                            next_config.agent.provider = Some(session_id.clone());
                            let next_config = Arc::new(next_config);
                            match build_backend(
                                &next_config,
                                &cwd,
                                &opts,
                                kind,
                                agent_tx.clone(),
                                true,
                            )
                            .await
                            {
                                Ok(next) => {
                                    backend = next;
                                    config = next_config.clone();
                                    app.backend_label = kind.provider_name().to_string();
                                    app.model_label = backend.session.model_label();
                                    fill_menu_choices(
                                        &mut app,
                                        &config,
                                        kind,
                                        agent_tx.clone(),
                                        backend.opencode.clone(),
                                    );
                                    app.context_used = 0;
                                    app.push_item(Item::Notice {
                                        text: format!(
                                            "Connected to {}. New messages go there.",
                                            info.label
                                        ),
                                        kind: NoticeKind::Info,
                                    });
                                    if !persist_setting(
                                        &cwd,
                                        "agent",
                                        "provider",
                                        &format!("{session_id:?}"),
                                    ) {
                                        app.set_status("provider connected, but the config could not be written");
                                    }
                                }
                                Err(e) => {
                                    app.push_item(Item::Error {
                                        text: format!("could not start {}: {e}", info.label),
                                    });
                                }
                            }
                        }
                    }
                }
            }
            Action::SetTheme(name) => {
                app.theme = Theme::by_name(&name);
                if persist_setting(&cwd, "ui", "theme", &format!("\"{name}\"")) {
                    app.set_status(format!("Theme: {}", app.theme.name));
                } else {
                    app.set_status("theme applied, but the config could not be written");
                }
            }
            Action::Compact => {
                if let Some(native) = &backend.native {
                    app.set_status("Compacting conversation in segments…");
                    match native.compact_segments(6).await {
                        Ok(0) => app.set_status("nothing to compact yet"),
                        Ok(segments) => app.set_status(format!(
                            "compacted {segments} earlier segment(s); the last messages stay verbatim"
                        )),
                        Err(e) => app.set_status(format!("compact failed: {e}")),
                    }
                } else {
                    app.set_status("Compaction is available on the native backend");
                }
            }
            Action::Export(path) => {
                let store = SessionStore::open(&cwd, &app.session_id).or_else(|_| {
                    session::most_recent(&cwd)?
                        .ok_or_else(|| anyhow::anyhow!("no session yet"))
                        .and_then(|s| SessionStore::open(&cwd, &s.id))
                });
                // Prefer the transcript on screen: it works for every provider
                // (opencode/ACP sessions do not live in our own store).
                let from_items = app.export_markdown();
                let markdown = if from_items.trim().lines().count() > 2 {
                    Ok(from_items)
                } else {
                    store.and_then(|s| cachy_core::export::session_markdown(&s))
                };
                match markdown {
                    Ok(markdown) if path.is_empty() => {
                        // Clipboard, the way the reference does it.
                        let chars = markdown.chars().count();
                        copy_to_clipboard(&markdown);
                        app.set_status(format!(
                            "Transcript copied ({chars} characters) — Ctrl/Cmd+V pastes it"
                        ));
                    }
                    Ok(markdown) => {
                        // `~` and missing parent directories are handled here.
                        let expanded = if let Some(rest) = path.strip_prefix("~/") {
                            match dirs::home_dir() {
                                Some(home) => home.join(rest),
                                None => std::path::PathBuf::from(&path),
                            }
                        } else {
                            std::path::PathBuf::from(&path)
                        };
                        let target = if expanded.is_absolute() {
                            expanded
                        } else {
                            cwd.join(expanded)
                        };
                        let result = (|| -> Result<()> {
                            if let Some(parent) = target.parent() {
                                std::fs::create_dir_all(parent)?;
                            }
                            std::fs::write(&target, markdown)?;
                            Ok(())
                        })();
                        match result {
                            Ok(()) => app.set_status(format!("Exported to {}", target.display())),
                            Err(e) => app.set_status(format!("export failed: {e}")),
                        }
                    }
                    Err(e) => app.set_status(format!("export failed: {e}")),
                }
            }
            Action::TrustFolder => match hooks::trust_folder(&cwd) {
                Ok(()) => app.set_status("Folder trusted for hooks"),
                Err(e) => app.set_status(format!("trust failed: {e}")),
            },
            Action::Remember(text) => {
                let scope = cachy_core::memory::Scope::Global;
                match cachy_core::memory::add_observation(&scope, &text) {
                    Ok(path) => app.set_status(format!("Saved to {}", path.display())),
                    Err(e) => app.set_status(format!("memory write failed: {e}")),
                }
            }
            Action::InitProject => {
                let path = cwd.join("AGENTS.md");
                if path.exists() {
                    app.set_status("AGENTS.md already exists");
                } else {
                    let content = "# Project instructions\n\nDescribe how agents should work in this repository.\n";
                    match std::fs::write(&path, content) {
                        Ok(()) => app.set_status("Created AGENTS.md"),
                        Err(e) => app.set_status(format!("failed to create AGENTS.md: {e}")),
                    }
                }
            }
        }
    };

    // Teardown.
    let _ = hooks::run_event(
        &discovered_hooks,
        hooks::HookEvent::SessionEnd,
        &cwd,
        None,
        &serde_json::json!({}),
    )
    .await;
    let _ = backend.session.shutdown().await;
    terminal.show_cursor().ok();
    guard.restore();
    result
}

fn handle_key(app: &mut App, key: KeyEvent) -> Action {
    let action = app.handle_key(key);
    // A rebound key is persisted immediately so it survives the next launch.
    if !app.keymap_edits.is_empty() {
        let edits = std::mem::take(&mut app.keymap_edits);
        for (name, spec) in edits {
            if !persist_setting(&app.cwd, "keys", &name, &format!("\"{spec}\"")) {
                app.set_status("could not save the new key binding to ~/.cachy/config.toml");
            }
        }
    }
    action
}

/// Leave the alternate screen so an interactive child (a sign-in flow that
/// opens the browser) can take over, then come back to the interface.
async fn with_terminal_suspended<F, Fut, R>(
    terminal: &mut Terminal<CrosstermBackend<std::io::Stdout>>,
    mouse: bool,
    run: F,
) -> Result<R>
where
    F: FnOnce() -> Fut,
    Fut: std::future::Future<Output = Result<R>>,
{
    // Everything the child must not inherit: raw mode, mouse reporting (its
    // escape sequences would be typed into the child as text), bracketed paste
    // and the extended keyboard protocol.
    disable_raw_mode().ok();
    let _ = execute!(
        terminal.backend_mut(),
        crossterm::event::DisableMouseCapture,
        crossterm::event::PopKeyboardEnhancementFlags,
        crossterm::event::DisableBracketedPaste,
        LeaveAlternateScreen
    );
    println!("\n  CachyCLI is paused for sign-in. Complete it in the window that opens.\n");
    CHILD_OWNS_TERMINAL.store(true, std::sync::atomic::Ordering::SeqCst);
    let result = run().await;
    CHILD_OWNS_TERMINAL.store(false, std::sync::atomic::Ordering::SeqCst);
    let _ = execute!(
        terminal.backend_mut(),
        EnterAlternateScreen,
        crossterm::event::EnableBracketedPaste,
        crossterm::event::PushKeyboardEnhancementFlags(
            crossterm::event::KeyboardEnhancementFlags::DISAMBIGUATE_ESCAPE_CODES
                | crossterm::event::KeyboardEnhancementFlags::REPORT_ALTERNATE_KEYS,
        )
    );
    if mouse {
        let _ = execute!(terminal.backend_mut(), crossterm::event::EnableMouseCapture);
    }
    enable_raw_mode().ok();
    terminal.clear().ok();
    result
}

/// Turn terminal mouse reporting on or off at runtime.
pub fn set_mouse_capture(on: bool) {
    let mut out = std::io::stdout();
    if on {
        let _ = execute!(out, crossterm::event::EnableMouseCapture);
    } else {
        let _ = execute!(out, crossterm::event::DisableMouseCapture);
    }
}

/// Mouse: clicks go through the hit regions drawn this frame; the wheel is
/// modal-aware (selection inside pickers, scrolling inside info panels).
fn handle_mouse(app: &mut App, mouse: crossterm::event::MouseEvent) -> Action {
    use app::HitAction;
    let (column, row) = (mouse.column, mouse.row);
    let inside = |rect: Rect| {
        rect.width > 0
            && rect.height > 0
            && column >= rect.x
            && column < rect.x + rect.width
            && row >= rect.y
            && row < rect.y + rect.height
    };
    match mouse.kind {
        MouseEventKind::ScrollUp | MouseEventKind::ScrollDown => {
            let up = matches!(mouse.kind, MouseEventKind::ScrollUp);
            // The `/` menu and the `@` picker own the wheel while the pointer
            // is over them.
            if inside(app.menu_rect) {
                let key = if up { KeyCode::Up } else { KeyCode::Down };
                let _ = app.handle_key(KeyEvent::new(key, KeyModifiers::NONE));
                return Action::None;
            }
            if app.has_modal() && (inside(app.modal_rect) || !inside(app.scrollback_rect)) {
                let scrolls = matches!(
                    app.modal(),
                    Some(Modal::Info { .. }) | Some(Modal::Memory { .. })
                );
                if scrolls {
                    app.scroll_modal(if up { -3 } else { 3 });
                } else {
                    let key = if up { KeyCode::Up } else { KeyCode::Down };
                    let _ = app.handle_key(KeyEvent::new(key, KeyModifiers::NONE));
                }
                return Action::None;
            }
            if up {
                app.scroll_up(3);
            } else {
                app.scroll_down(3);
            }
            Action::None
        }
        MouseEventKind::Down(MouseButton::Left) => {
            // Dragging inside the prompt selects the input; releasing copies it.
            if !app.has_modal() && inside(app.prompt_rect) {
                let origin = app.prompt_rect.x + 4;
                let row = (row - app.prompt_rect.y - 1) as usize;
                let column = column.saturating_sub(origin) as usize;
                app.set_caret(row, column);
                let anchor = app.editor.cursor;
                app.prompt_selection = Some((anchor, anchor));
                app.selecting_prompt = true;
                app.selection = None;
                return Action::None;
            }
            // Dragging inside the scrollback selects text; releasing copies it.
            if !app.has_modal() && inside(app.scrollback_rect) {
                app.selection = Some(app::Selection {
                    anchor: (column, row),
                    focus: (column, row),
                });
                app.selecting = true;
                return Action::None;
            }
            app.clear_selection();
            let Some(hit) = app.hit_at(column, row).cloned() else {
                return Action::None;
            };
            match hit {
                HitAction::PromptCaret { row, origin_x } => {
                    let column = column.saturating_sub(origin_x) as usize;
                    app.set_caret(row, column);
                    Action::None
                }
                HitAction::ModalRow(index) => app.click_modal_row(index),
                HitAction::ModalClose => {
                    app.close_modal();
                    Action::None
                }
                HitAction::ApprovalRow(index) => app.click_approval(index),
                HitAction::StopTurn => Action::Cancel,
                HitAction::SlashRow(index) => {
                    app.slash_selected = index;
                    app.accept_slash_selection()
                }
                HitAction::FileRow(index) => {
                    app.file_selected = index;
                    app.accept_file_selection()
                }
            }
        }
        MouseEventKind::Drag(MouseButton::Left) => {
            if app.selecting_prompt {
                let rect = app.prompt_rect;
                let origin = rect.x + 4;
                let row = row.clamp(rect.y + 1, rect.y + rect.height.saturating_sub(1));
                let row = (row - rect.y - 1) as usize;
                let column = column.saturating_sub(origin) as usize;
                app.set_caret(row, column);
                let focus = app.editor.cursor;
                if let Some((anchor, _)) = app.prompt_selection {
                    app.prompt_selection = Some((anchor, focus));
                }
                return Action::None;
            }
            if app.selecting {
                let rect = app.scrollback_rect;
                let focus = (
                    column.clamp(rect.x, rect.x + rect.width.saturating_sub(1)),
                    row.clamp(rect.y, rect.y + rect.height.saturating_sub(1)),
                );
                if let Some(selection) = app.selection.as_mut() {
                    selection.focus = focus;
                }
            }
            Action::None
        }
        MouseEventKind::Up(MouseButton::Left) => {
            if app.selecting_prompt {
                app.selecting_prompt = false;
                if let Some(text) = app.prompt_selection_text() {
                    let chars = text.chars().count();
                    copy_to_clipboard(&text);
                    app.set_status(format!(
                        "Copied {chars} characters (Ctrl/Cmd+V pastes them)"
                    ));
                } else {
                    app.prompt_selection = None;
                }
                return Action::None;
            }
            if app.selecting {
                app.selecting = false;
                match app.selection {
                    Some(selection) if selection.is_empty() => app.selection = None,
                    Some(_) => {
                        if let Some(text) = app.selection_text() {
                            let chars = text.chars().count();
                            copy_to_clipboard(&text);
                            app.set_status(format!(
                                "Copied {chars} characters (Ctrl/Cmd+V pastes them)"
                            ));
                        }
                    }
                    None => {}
                }
            }
            Action::None
        }
        MouseEventKind::Moved => {
            app.hover = app.hit_at(column, row).cloned();
            Action::None
        }
        _ => Action::None,
    }
}

/// Time-to-first-token for one model, driving the opencode CLI out of band so
/// the running session is untouched. `None` when it does not answer in time.
async fn bench_model(command: &str, model: &str) -> Option<f64> {
    use tokio::io::{AsyncBufReadExt, BufReader};
    let mut cmd = tokio::process::Command::new(command);
    cmd.arg("run")
        .arg("--format")
        .arg("json")
        .arg("-m")
        .arg(model)
        .arg("responda apenas: ok")
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::null())
        .stdin(std::process::Stdio::null());
    let started = std::time::Instant::now();
    let mut child = cmd.spawn().ok()?;
    let stdout = child.stdout.take()?;
    let mut lines = BufReader::new(stdout).lines();
    let deadline = std::time::Duration::from_secs(45);
    loop {
        let next = tokio::time::timeout(
            deadline.saturating_sub(started.elapsed()),
            lines.next_line(),
        )
        .await;
        match next {
            Ok(Ok(Some(line))) => {
                if line.contains("\"type\":\"text\"")
                    || line.contains("\"type\":\"reasoning\"")
                    || line.contains("\"type\":\"step_start\"")
                {
                    let seconds = started.elapsed().as_secs_f64();
                    let _ = child.kill().await;
                    return Some(seconds);
                }
            }
            _ => {
                let _ = child.kill().await;
                return None;
            }
        }
    }
}

/// Persist bench results from inside the spawned task.
fn app_save_bench(results: &std::collections::BTreeMap<String, f64>) {
    app::save_bench_results(results);
}

/// A filename-safe version of a session title.
fn slug(text: &str) -> String {
    text.to_lowercase()
        .chars()
        .map(|c| if c.is_alphanumeric() { c } else { '-' })
        .collect::<String>()
        .trim_matches('-')
        .chars()
        .take(40)
        .collect()
}

/// Full-text search across the saved sessions of this directory.
fn search_sessions(cwd: &std::path::Path, query: &str) -> PickerState {
    let needle = query.trim().to_lowercase();
    let mut items: Vec<String> = Vec::new();
    let mut values: Vec<String> = Vec::new();
    let root = cachy_core::paths::sessions_dir_for(cwd);
    let Ok(entries) = std::fs::read_dir(&root) else {
        return PickerState {
            query: String::new(),
            selected: 0,
            items: vec![format!("no sessions stored in {}", root.display())],
            values: vec![String::new()],
        };
    };
    // Newest first, capped so a big history stays quick.
    let mut dirs: Vec<std::path::PathBuf> = entries
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| path.is_dir())
        .collect();
    dirs.sort_by_key(|path| {
        std::fs::metadata(path)
            .and_then(|meta| meta.modified())
            .ok()
    });
    dirs.reverse();

    for dir in dirs.into_iter().take(200) {
        let store = match cachy_core::session::SessionStore::from_dir(dir.clone()) {
            Ok(store) => store,
            Err(_) => continue,
        };
        let id = store.summary.id.clone();
        let title = if store.summary.title.is_empty() {
            id.clone()
        } else {
            store.summary.title.clone()
        };
        let mut snippet: Option<String> = None;
        if let Ok(messages) = store.load_chat() {
            for message in &messages {
                let content = message.content.to_lowercase();
                if let Some(position) = content.find(&needle) {
                    let start = position.saturating_sub(30);
                    let slice: String = message.content.chars().skip(start).take(90).collect();
                    snippet = Some(format!(
                        "{}: …{}…",
                        message.role.as_str(),
                        slice.replace('\n', " ")
                    ));
                    break;
                }
            }
        }
        let matched_title = title.to_lowercase().contains(&needle);
        if snippet.is_none() && !matched_title {
            continue;
        }
        let line = match &snippet {
            Some(text) => format!("{title}   {text}"),
            None => format!("{title}   (title match)"),
        };
        items.push(line);
        values.push(id);
        if items.len() >= 40 {
            break;
        }
    }
    if items.is_empty() {
        items.push(format!("nothing matched \"{query}\""));
        values.push(String::new());
    }
    PickerState {
        query: String::new(),
        selected: 0,
        items,
        values,
    }
}

/// `git worktree list --porcelain` as (path, branch) pairs.
fn git_worktree_list(cwd: &std::path::Path) -> Result<Vec<(String, String)>> {
    let output = std::process::Command::new("git")
        .args(["worktree", "list", "--porcelain"])
        .current_dir(cwd)
        .output()
        .map_err(|e| anyhow::anyhow!("could not run git: {e}"))?;
    if !output.status.success() {
        anyhow::bail!("not a git repository");
    }
    let text = String::from_utf8_lossy(&output.stdout);
    let mut out: Vec<(String, String)> = Vec::new();
    let (mut path, mut branch) = (String::new(), String::new());
    for line in text.lines() {
        if let Some(rest) = line.strip_prefix("worktree ") {
            if !path.is_empty() {
                out.push((path.clone(), branch.clone()));
            }
            path = rest.trim().to_string();
            branch = "detached".into();
        } else if let Some(rest) = line.strip_prefix("branch ") {
            branch = rest.trim().trim_start_matches("refs/heads/").to_string();
        }
    }
    // The last block has no following `worktree` line to flush it.
    if !path.is_empty() {
        out.push((path, branch));
    }
    Ok(out)
}

/// Create a worktree next to the repo, on a new branch named after it.
fn git_worktree_add(cwd: &std::path::Path, name: &str) -> Result<String> {
    if name.is_empty() || name.contains(' ') {
        anyhow::bail!("use a single word as the worktree name");
    }
    let root = cachy_core::skills::git_root(cwd)
        .ok_or_else(|| anyhow::anyhow!("this directory is not inside a git repository"))?;
    let repo = root
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_else(|| "repo".into());
    let parent = root.parent().unwrap_or(&root).to_path_buf();
    let target = parent.join(format!("{repo}-{name}"));
    if target.exists() {
        anyhow::bail!("{} already exists", target.display());
    }
    let output = std::process::Command::new("git")
        .args([
            "worktree",
            "add",
            target.to_string_lossy().as_ref(),
            "-b",
            name,
        ])
        .current_dir(&root)
        .output()
        .map_err(|e| anyhow::anyhow!("could not run git: {e}"))?;
    if !output.status.success() {
        anyhow::bail!("{}", String::from_utf8_lossy(&output.stderr).trim());
    }
    Ok(target.display().to_string())
}

/// Remove a worktree by name (or by path).
fn git_worktree_remove(cwd: &std::path::Path, name: &str) -> Result<String> {
    let list = git_worktree_list(cwd)?;
    let found = list
        .iter()
        .find(|(path, branch)| branch == name || path.ends_with(name))
        .ok_or_else(|| anyhow::anyhow!("no worktree named {name}"))?;
    let output = std::process::Command::new("git")
        .args(["worktree", "remove", &found.0])
        .current_dir(cwd)
        .output()
        .map_err(|e| anyhow::anyhow!("could not run git: {e}"))?;
    if !output.status.success() {
        anyhow::bail!("{}", String::from_utf8_lossy(&output.stderr).trim());
    }
    Ok(found.0.clone())
}

/// OSC 52: puts text in the terminal's clipboard (xterm, kitty, foot, WezTerm,
/// iTerm2, tmux with `set-clipboard on`).
fn copy_to_clipboard(text: &str) {
    use base64::Engine;
    use std::io::Write;
    let encoded = base64::engine::general_purpose::STANDARD.encode(text.as_bytes());
    let mut out = std::io::stdout();
    let _ = write!(out, "\x1b]52;c;{encoded}\x07");
    let _ = out.flush();
}

/// Send a prompt. Providers that do not run their own loop also need the
/// `UserPromptSubmit` hooks to fire, so the interface runs them here — inside
/// the task, so a slow hook never freezes the UI.
fn spawn_prompt(
    session: Arc<dyn AgentSession>,
    text: String,
    hooks: Arc<Vec<cachy_core::hooks::DiscoveredHook>>,
    cwd: PathBuf,
    tx: mpsc::UnboundedSender<AgentEvent>,
    run_hooks: bool,
) {
    tokio::spawn(async move {
        if run_hooks && !hooks.is_empty() {
            let payload = serde_json::json!({ "prompt": text });
            let results = cachy_core::hooks::run_event(
                &hooks,
                cachy_core::hooks::HookEvent::UserPromptSubmit,
                &cwd,
                None,
                &payload,
            )
            .await;
            for (command, output, ok) in results {
                if !ok {
                    let _ = tx.send(AgentEvent::Info(format!(
                        "UserPromptSubmit hook failed: {command}: {output}"
                    )));
                }
            }
        }
        let _ = session.prompt(text).await;
    });
}

async fn build_backend(
    config: &Arc<Config>,
    cwd: &Path,
    opts: &TuiOptions,
    kind: BackendKind,
    tx: mpsc::UnboundedSender<AgentEvent>,
    fresh: bool,
) -> Result<Backend> {
    let backend_kind = kind;

    match backend_kind {
        BackendKind::Opencode => {
            let backend = cachy_core::providers::opencode::OpencodeBackend::new(
                config.clone(),
                cwd.to_path_buf(),
                tx,
            );
            Ok(Backend {
                kind: BackendKind::Opencode,
                session: backend.clone(),
                native: None,
                opencode: Some(backend),
                acp: None,
            })
        }
        BackendKind::Codex | BackendKind::Gemini => {
            let dialect = if backend_kind == BackendKind::Codex {
                cachy_core::providers::cli_agent::Dialect::Codex
            } else {
                cachy_core::providers::cli_agent::Dialect::Gemini
            };
            let backend = cachy_core::providers::cli_agent::CliAgentBackend::new(
                dialect,
                config.clone(),
                cwd.to_path_buf(),
                tx,
                true,
            );
            Ok(Backend {
                kind: backend_kind,
                session: backend.clone(),
                native: None,
                opencode: None,
                acp: None,
            })
        }
        BackendKind::ClaudeCode => {
            let backend = cachy_core::providers::claude_code::ClaudeCodeBackend::new(
                config.clone(),
                cwd.to_path_buf(),
                tx,
                true,
            );
            Ok(Backend {
                kind: BackendKind::ClaudeCode,
                session: backend.clone(),
                native: None,
                opencode: None,
                acp: None,
            })
        }
        BackendKind::Acp => {
            let acp = &config.agent.acp;
            let backend = match &acp.url {
                Some(url) => cachy_core::acp::client::AcpBackend::connect_url(url, cwd, tx).await?,
                None => {
                    cachy_core::acp::client::AcpBackend::connect(
                        &acp.command,
                        &acp.args,
                        &acp.env,
                        cwd,
                        tx,
                    )
                    .await?
                }
            };
            Ok(Backend {
                kind: BackendKind::Acp,
                session: backend.clone(),
                native: None,
                opencode: None,
                acp: Some(backend),
            })
        }
        BackendKind::Native => {
            let model = models::resolve(config, opts.model.as_deref()).ok_or_else(|| {
                anyhow::anyhow!(
                    "no model configured — add a [model.<name>] section to ~/.cachy/config.toml"
                )
            })?;
            // Guide the first run when no credential is available.
            let has_key = models::api_key_for(&model).is_some()
                || model.base_url.contains("localhost")
                || model.base_url.contains("127.0.0.1");
            if !has_key {
                let env_hint = model
                    .env_keys
                    .first()
                    .cloned()
                    .unwrap_or_else(|| "OPENAI_API_KEY".into());
                let opencode_hint = if which_binary("opencode").is_some() {
                    " You have opencode installed: run `cachycli --backend opencode` to use it."
                } else {
                    ""
                };
                let _ = tx.send(AgentEvent::Info(format!(
                    "No credential for model \"{}\". Set {env_hint}, run `cachycli login --api-key <key>`, \
                     or add a [model.<name>] section.{}",
                    model.name, opencode_hint
                )));
            }
            let store =
                match (&opts.resume, fresh) {
                    (Some(id), false) => SessionStore::open(cwd, id)
                        .unwrap_or(SessionStore::create(cwd, &model.model_id, "cachy", None)?),
                    _ => SessionStore::create(cwd, &model.model_id, "cachy", None)?,
                };
            let session_id = store.summary.id.clone();
            let mcp = Arc::new(McpManager::new(config.mcp_servers.clone(), session_id));
            // Servers that are slow or broken must not hold up the interface:
            // they connect in the background (the tools are looked up per call)
            // and say something when they give up.
            {
                let mcp = mcp.clone();
                let tx = tx.clone();
                let servers = config.mcp_servers.len();
                tokio::spawn(async move {
                    mcp.initialize().await;
                    let errors = mcp.errors().await;
                    for (name, error) in &errors {
                        let _ = tx.send(AgentEvent::Info(format!(
                            "MCP server {name} is not available: {error}"
                        )));
                    }
                    if errors.is_empty() && servers > 0 {
                        let _ = tx.send(AgentEvent::Info(format!(
                            "{servers} MCP server(s) connected"
                        )));
                    }
                });
            }
            let mut tools = ToolRegistry::default_registry(config);
            let warnings = tools.filter(opts.tools.as_deref(), &opts.disallowed_tools);
            let rule_allow = opts.allow.clone();
            let engine = PermissionEngine::new(
                RuleSet::parse(&rule_allow, &opts.deny),
                opts.permission_mode
                    .as_deref()
                    .and_then(PermissionMode::parse)
                    .unwrap_or_default(),
                opts.always_approve,
            );
            let native = NativeAgent::new(
                cwd.to_path_buf(),
                config.clone(),
                model,
                store,
                mcp,
                tools,
                engine,
                tx.clone(),
                opts.max_turns.unwrap_or(100),
                true,
                0,
            )
            .await?;
            for warning in warnings {
                let _ = tx.send(AgentEvent::Info(warning));
            }
            Ok(Backend {
                kind: BackendKind::Native,
                session: native.clone(),
                native: Some(native),
                opencode: None,
                acp: None,
            })
        }
    }
}

async fn resume_session(
    backend: &Backend,
    cwd: &std::path::Path,
    id_or_title: &str,
) -> Result<(
    cachy_core::session::SessionSummary,
    Vec<cachy_core::message::ChatMessage>,
)> {
    let summary =
        if id_or_title.len() == 36 && id_or_title.chars().filter(|c| *c == '-').count() == 4 {
            session::list_sessions(Some(cwd))?
                .into_iter()
                .find(|s| s.id == id_or_title)
                .ok_or_else(|| anyhow::anyhow!("session not found: {id_or_title}"))?
        } else {
            match session::find_by_id_or_title(cwd, id_or_title)? {
                Some(found) => found,
                None => {
                    // A looser pass, but never a silent pick between several.
                    let needle = id_or_title.to_lowercase();
                    let matches: Vec<cachy_core::session::SessionSummary> =
                        session::list_sessions(Some(cwd))?
                            .into_iter()
                            .filter(|s| s.title.to_lowercase().contains(&needle))
                            .collect();
                    match matches.len() {
                        0 => anyhow::bail!("no session matches \"{id_or_title}\""),
                        1 => match matches.into_iter().next() {
                            Some(only) => only,
                            None => anyhow::bail!("no session matches \"{id_or_title}\""),
                        },
                        _ => {
                            let titles = matches
                                .iter()
                                .map(|s| {
                                    format!("{:?} ({})", s.title, cachy_core::text::short(&s.id, 8))
                                })
                                .collect::<Vec<_>>()
                                .join(", ");
                            anyhow::bail!("several sessions match \"{id_or_title}\": {titles}")
                        }
                    }
                }
            }
        };
    let store = SessionStore::open(cwd, &summary.id)?;
    let messages = store.load_chat().unwrap_or_default();
    if let Some(native) = &backend.native {
        native.load_history(messages.clone()).await;
    } else if let Some(id) = summary.backend_session.as_deref() {
        // The provider keeps its own sessions: ask it to continue that one.
        if summary.backend_provider.as_deref() == Some(backend.kind.provider_name()) {
            backend.session.resume_backend_session(id).await;
        }
    }
    Ok((summary, messages))
}

fn resume_picker_state(cwd: &std::path::Path) -> PickerState {
    let sessions = session::list_sessions(Some(cwd)).unwrap_or_default();
    let mut items = Vec::new();
    let mut values = Vec::new();
    // A bar per session, scaled to the busiest one, so the dashboard shows at a
    // glance which conversation was long.
    let widest = sessions
        .iter()
        .map(|s| s.num_messages)
        .max()
        .unwrap_or(1)
        .max(1);
    for s in sessions.iter().take(60) {
        let title = if s.title.is_empty() {
            "(untitled)"
        } else {
            &s.title
        };
        let when = s
            .last_active_at
            .get(..16)
            .unwrap_or(&s.last_active_at)
            .replace('T', " ");
        const WIDTH: usize = 12;
        let filled = ((s.num_messages as f64 / widest as f64) * WIDTH as f64).round() as usize;
        let bar = format!(
            "{}{}",
            "▇".repeat(filled.min(WIDTH)),
            "·".repeat(WIDTH - filled.min(WIDTH))
        );
        let model = if s.current_model_id.is_empty() {
            "—".to_string()
        } else {
            s.current_model_id.clone()
        };
        items.push(format!(
            "{bar}  {:>4} msgs  {title}   ({model}, {when})",
            s.num_messages
        ));
        values.push(s.id.clone());
    }
    if items.is_empty() {
        items.push("No sessions yet".into());
        values.push(String::new());
    }
    PickerState {
        query: String::new(),
        selected: 0,
        items,
        values,
    }
}

fn sessions_picker_state(cwd: &std::path::Path) -> PickerState {
    let summaries = session::list_sessions(None).unwrap_or_default();
    let widest = summaries
        .iter()
        .map(|s| s.num_messages)
        .max()
        .unwrap_or(1)
        .max(1);
    let mut all = Vec::new();
    let mut values = Vec::new();
    for summary in summaries {
        let title = if summary.title.is_empty() {
            "(untitled)"
        } else {
            summary.title.as_str()
        };
        let cwd_display = cachy_core::paths::display_path(std::path::Path::new(&summary.cwd));
        const WIDTH: usize = 12;
        let filled =
            ((summary.num_messages as f64 / widest as f64) * WIDTH as f64).round() as usize;
        let bar = format!(
            "{}{}",
            "▇".repeat(filled.min(WIDTH)),
            "·".repeat(WIDTH - filled.min(WIDTH))
        );
        all.push(format!(
            "{bar}  {:>4} msgs  {title}   {cwd_display}",
            summary.num_messages
        ));
        values.push(summary.id.clone());
    }
    let _ = cwd;
    if values.is_empty() {
        values.push(String::new());
    }
    if all.is_empty() {
        all.push("No sessions yet".into());
    }
    PickerState {
        query: String::new(),
        selected: 0,
        items: all,
        values,
    }
}

/// Choices shown by `/model`: opencode's own list when that provider is
/// active, the configured catalog otherwise.
fn model_picker_state(config: &Config, app: &App) -> PickerState {
    if app.provider_is_opencode {
        let listed: Vec<String> = app
            .opencode_model_options
            .lock()
            .map(|list| list.iter().map(|(value, _)| value.clone()).collect())
            .unwrap_or_default();
        if listed.is_empty() {
            return PickerState {
                query: String::new(),
                selected: 0,
                items: vec!["reading `opencode models`…".into()],
                values: vec![String::new()],
            };
        }
        // Models that need no credential come first once one has failed.
        let mut listed = listed;
        if app.opencode_key_expired {
            listed.sort_by_key(|id| id.starts_with("opencode-go/"));
        }
        let mut items = Vec::new();
        let mut values = Vec::new();
        let current = app.model_label.clone();
        for id in listed {
            let mut hint = app.model_hint(&id);
            if id == current {
                hint = if hint.is_empty() {
                    "· current".to_string()
                } else {
                    format!("{hint} · current")
                };
            }
            let note = if hint.is_empty() {
                String::new()
            } else {
                format!("  ({})", hint.trim_start_matches("· "))
            };
            items.push(format!("{id}{note}"));
            values.push(id);
        }
        return PickerState {
            query: String::new(),
            selected: 0,
            items,
            values,
        };
    }
    let catalog = models::catalog(config);
    let mut items = Vec::new();
    let mut values = Vec::new();
    for model in catalog {
        let key_state = if models::api_key_for(&model).is_some() {
            ""
        } else {
            "  (no API key)"
        };
        items.push(format!("{}  —  {}{key_state}", model.name, model.display()));
        values.push(model.name.clone());
    }
    PickerState {
        query: String::new(),
        selected: 0,
        items,
        values,
    }
}

/// Tell the user once why the model list is empty; repeating it on every
/// refresh is noise.
fn report_once(
    reported: &Arc<std::sync::Mutex<Option<String>>>,
    notice: &mpsc::UnboundedSender<AgentEvent>,
    reason: &str,
) {
    let message = format!("could not list opencode models ({reason}) — type one as provider/model");
    let first_time = match reported.lock() {
        Ok(mut last) => {
            let is_new = last.as_deref() != Some(message.as_str());
            if is_new {
                *last = Some(message.clone());
            }
            is_new
        }
        Err(_) => true,
    };
    if first_time {
        let _ = notice.send(AgentEvent::Info(message));
    }
}

/// Fill the `/` menu choices and, for opencode, read its model list in the
/// background (the command takes about a second).
fn fill_menu_choices(
    app: &mut App,
    config: &Arc<Config>,
    kind: BackendKind,
    notice: mpsc::UnboundedSender<AgentEvent>,
    opencode: Option<Arc<cachy_core::providers::opencode::OpencodeBackend>>,
) {
    app.provider_options = crate::providers::detect(config)
        .iter()
        .map(|info| {
            (
                info.id.provider_name().to_string(),
                format!("{} — {}", info.label, info.summary),
            )
        })
        .collect();
    app.native_model_options = models::catalog(config)
        .into_iter()
        .map(|model| (model.name.clone(), model.display().to_string()))
        .collect();
    app.provider_is_opencode = matches!(kind, BackendKind::Opencode);
    if !app.provider_is_opencode {
        return;
    }
    if let Ok(list) = app.opencode_model_options.lock() {
        if !list.is_empty() {
            return; // already read once
        }
    }
    let list = app.opencode_model_options.clone();
    let reported = app.opencode_models_error.clone();
    let command = config.agent.opencode.command.clone();
    tokio::spawn(async move {
        // The server knows the models of the providers you are signed in to,
        // and it is what the turns use. The CLI is only a fallback: on some
        // installs it fails (an unmigrated database, for one).
        // The server first, the CLI as a fallback. A failure is only worth a
        // line when both come up empty: one message, with the reason.
        let mut reason = String::new();
        if let Some(backend) = opencode {
            match backend.list_models().await {
                Ok(models) if !models.is_empty() => {
                    if let Ok(mut guard) = list.lock() {
                        *guard = models.into_iter().map(|id| (id, String::new())).collect();
                    }
                    if let Ok(mut last) = reported.lock() {
                        *last = None; // a working list clears the memory
                    }
                    return;
                }
                Ok(_) => reason = "the server listed nothing".into(),
                Err(e) => reason = format!("{e}"),
            }
        }
        let output = tokio::process::Command::new(&command)
            .arg("models")
            .output()
            .await;
        let output = match output {
            Ok(output) => output,
            Err(e) => {
                if reason.is_empty() {
                    reason = format!("{e}");
                }
                report_once(&reported, &notice, &reason);
                return;
            }
        };
        let text = String::from_utf8_lossy(&output.stdout);
        let items: Vec<(String, String)> = text
            .lines()
            .map(|line| line.trim())
            .filter(|line| !line.is_empty() && !line.starts_with('#'))
            .map(|line| (line.to_string(), String::new()))
            .collect();
        if items.is_empty() {
            if reason.is_empty() {
                reason = format!("`{command} models` printed nothing");
            }
            report_once(&reported, &notice, &reason);
            return;
        }
        if let Ok(mut guard) = list.lock() {
            *guard = items;
        }
    });
}

async fn handle_slash(
    app: &mut App,
    backend: &Backend,
    config: &Arc<Config>,
    cwd: &std::path::Path,
    command: &str,
    args: &str,
) -> Option<Action> {
    match command {
        "/exit" | "/quit" => Some(Action::Quit),
        "/model" | "/m" => {
            if args.trim().is_empty() {
                return Some(Action::RefreshModelList);
            }
            if false {
            } else {
                return Some(Action::SetModel(args.trim().to_string()));
            }
            None
        }
        "/provider" | "/providers" | "/backend" | "/connect" => {
            let infos = crate::providers::detect(config);
            let current = config
                .agent
                .provider
                .clone()
                .unwrap_or_else(|| backend.session.model_label());
            let _ = current;
            let mut items = Vec::new();
            let mut values = Vec::new();
            for info in &infos {
                let status = if !info.installed {
                    "not installed".to_string()
                } else if info.connected {
                    if info.detail.is_empty() {
                        "connected".to_string()
                    } else {
                        format!("connected · {}", info.detail)
                    }
                } else {
                    "needs sign-in".to_string()
                };
                items.push(format!(
                    "{:<13} {:<24} {}",
                    info.label, status, info.summary
                ));
                values.push(info.id.provider_name().to_string());
            }
            let selected = values
                .iter()
                .position(|v| Some(v.as_str()) == config.agent.provider.as_deref())
                .unwrap_or(0);
            if args.trim().is_empty() {
                app.push_modal(Modal::Provider(PickerState {
                    query: String::new(),
                    selected,
                    items,
                    values,
                }));
            } else {
                return Some(Action::SetProvider(args.trim().to_string()));
            }
            None
        }
        "/new" => Some(Action::NewSession),
        "/resume" | "/load" => {
            if args.trim().is_empty() {
                let state = resume_picker_state(cwd);
                app.push_modal(Modal::Resume(state));
            } else {
                return Some(Action::Resume(args.trim().to_string()));
            }
            None
        }
        "/rename" => {
            if args.trim().is_empty() {
                app.set_status("Usage: /rename <title>");
            } else if let Err(e) = rename_session(cwd, &app.session_id, args.trim()) {
                app.set_status(format!("rename failed: {e}"));
            } else {
                app.session_title = args.trim().to_string();
                app.set_status(format!("Renamed to \"{}\"", args.trim()));
            }
            None
        }
        "/delete" => {
            if app.session_id.is_empty() {
                app.set_status("No session to delete");
            } else if let Ok(store) = SessionStore::open(cwd, &app.session_id) {
                let _ = std::fs::remove_dir_all(&store.dir);
                app.set_status("Session deleted");
            }
            None
        }
        "/session-info" => {
            let lines = vec![
                format!("Session ID: {}", app.session_id),
                format!(
                    "Title: {}",
                    if app.session_title.is_empty() {
                        "(untitled)"
                    } else {
                        &app.session_title
                    }
                ),
                format!("Working directory: {}", cwd.display()),
                format!("Backend: {}", app.backend_label),
                format!("Model: {}", app.model_label),
                format!("Mode: {}", app.mode.label()),
                format!(
                    "Context: {} / {} tokens",
                    app.context_used, app.context_size
                ),
            ];
            app.open_info("Session info", lines);
            None
        }
        "/dashboard" => Some(Action::OpenSessions),
        "/export" => {
            // No argument copies to the clipboard; a filename writes the file.
            let path = if args.trim().is_empty() {
                String::new()
            } else {
                args.trim().to_string()
            };
            Some(Action::Export(path))
        }
        "/compact" => Some(Action::Compact),
        "/context" | "/usage" => {
            let lines = vec![
                format!(
                    "Context window: {}/{} tokens ({:.1}%)",
                    app.context_used,
                    app.context_size,
                    if app.context_size > 0 {
                        app.context_used as f64 / app.context_size as f64 * 100.0
                    } else {
                        0.0
                    }
                ),
                format!("Output tokens this session: {}", app.output_tokens),
                format!("Mode: {}", app.mode.label()),
                format!(
                    "Always-approve: {}",
                    if app.always_approve { "on" } else { "off" }
                ),
            ];
            app.open_info("Context usage", lines);
            None
        }
        "/memories" | "/memory" | "/mem" => {
            let global = cachy_core::memory::Scope::Global;
            let ws = cachy_core::memory::Scope::Workspace(
                cachy_core::memory::workspace_slug(cwd).into(),
            );
            let mut lines = Vec::new();
            for (label, scope) in [("Global", &global), ("Workspace", &ws)] {
                lines.push(format!("{label}: {}", scope.scope_dir().display()));
                let topics = cachy_core::memory::topics(scope);
                if topics.is_empty() {
                    lines.push("  (no topics yet)".into());
                }
                for (title, path) in topics {
                    lines.push(format!("  - {title} ({})", path.display()));
                }
            }
            app.push_modal(Modal::Memory { lines, scroll: 0 });
            None
        }
        "/remember" => {
            if args.trim().is_empty() {
                app.set_status("Usage: /remember <text>");
                None
            } else {
                Some(Action::Remember(args.trim().to_string()))
            }
        }
        "/flush" => {
            app.set_status("Memory captures run automatically after each turn");
            None
        }
        "/skills" => {
            let discovered = skills::discover(cwd, config);
            if args.trim().is_empty() {
                let mut lines = vec![format!("Skills discovered: {}", discovered.len())];
                for skill in &discovered {
                    lines.push(format!(
                        "  - {} [{}] {}",
                        skill.name, skill.source, skill.description
                    ));
                }
                lines.push(String::new());
                lines.push("Inject one with /skills <name>".into());
                app.open_info("Skills", lines);
            } else if let Some(skill) = skills::find(&discovered, args.trim()) {
                if let Some(native) = &backend.native {
                    native.inject_skill(skill.clone()).await;
                    app.push_item(Item::Notice {
                        text: format!("Skill \"{}\" injected for the next turn", skill.name),
                        kind: NoticeKind::Info,
                    });
                } else {
                    app.set_status("Skill injection is available on the native backend");
                }
            } else {
                app.set_status(format!("unknown skill \"{}\"", args.trim()));
            }
            None
        }
        "/plugins" | "/plugin" => {
            let args = args.trim().to_string();
            if let Some(term) = args
                .strip_prefix("search ")
                .or_else(|| args.strip_prefix("find "))
            {
                let term = term.trim().to_lowercase();
                match config.plugins.index.clone() {
                    None => app.set_status(
                        "no plugin index configured — set [plugins] index to a JSON URL or file",
                    ),
                    Some(source) => match cachy_core::plugins::fetch_index(&source).await {
                        Err(e) => app.set_status(format!("index failed: {e}")),
                        Ok(entries) => {
                            let mut items: Vec<String> = Vec::new();
                            let mut values: Vec<String> = Vec::new();
                            for entry in entries {
                                if !term.is_empty()
                                    && !entry.name.to_lowercase().contains(&term)
                                    && !entry.description.to_lowercase().contains(&term)
                                {
                                    continue;
                                }
                                items.push(entry.label());
                                values.push(entry.name.clone());
                            }
                            if items.is_empty() {
                                items.push(format!("nothing matched \"{term}\""));
                                values.push(String::new());
                            }
                            app.push_modal(Modal::Plugin(PickerState {
                                query: String::new(),
                                selected: 0,
                                items,
                                values,
                            }));
                        }
                    },
                }
                return None;
            }
            if let Some(name) = args.strip_prefix("install ") {
                return Some(Action::InstallPlugin(name.trim().to_string()));
            }
            let mut lines = Vec::new();
            let dirs = [cwd.join(".cachy/plugins"), cachy_core::paths::plugins_dir()];
            for dir in dirs {
                if dir.is_dir() {
                    if let Ok(entries) = std::fs::read_dir(&dir) {
                        for entry in entries.flatten() {
                            lines.push(entry.path().display().to_string());
                        }
                    }
                }
            }
            if lines.is_empty() {
                lines.push("No plugins discovered.".into());
                lines.push(format!(
                    "Project plugin dir: {}",
                    cwd.join(".cachy/plugins").display()
                ));
                lines.push(format!(
                    "User plugin dir: {}",
                    cachy_core::paths::plugins_dir().display()
                ));
            }
            lines.push(String::new());
            lines.push(match &config.plugins.index {
                Some(source) => format!("index: {source}   (/plugin search <term> to install)"),
                None => "index: (none) — set [plugins] index to a JSON URL or file".to_string(),
            });
            app.open_info("Plugins", lines);
            None
        }
        "/mcp" => {
            let mut lines = Vec::new();
            for (name, cfg) in &config.mcp_servers {
                let transport = if cfg.url.is_some() { "http" } else { "stdio" };
                lines.push(format!(
                    "{name} ({transport}) {}",
                    if cfg.enabled { "enabled" } else { "disabled" }
                ));
            }
            if lines.is_empty() {
                lines.push("No MCP servers configured.".into());
                lines.push("Add [mcp_servers.<name>] to ~/.cachy/config.toml".into());
            }
            lines.push(String::new());
            lines.push("Tools are reached by the model through search_tool / use_tool.".into());
            app.open_info("MCP servers", lines);
            None
        }
        "/hooks-list" => {
            let discovered = hooks::discover(cwd, config);
            let mut lines = hooks::describe(&discovered);
            if lines.is_empty() {
                lines.push("No hooks loaded for this folder.".into());
                lines.push("Add .cachy/hooks/*.json and run /hooks-trust.".into());
            }
            app.open_info("Hooks", lines);
            None
        }
        "/hooks-trust" => Some(Action::TrustFolder),
        "/todos" => {
            app.show_todos = !app.show_todos;
            app.set_status(if app.show_todos {
                "Todo panel on"
            } else {
                "Todo panel off"
            });
            None
        }
        "/always-approve" | "/yolo" => {
            let value = match args.trim() {
                "on" => true,
                "off" => false,
                _ => !app.always_approve,
            };
            app.always_approve = value;
            if let Some(native) = &backend.native {
                native.set_always_approve(value).await;
            }
            app.set_status(if value {
                "Always-approve is on"
            } else {
                "Always-approve is off"
            });
            None
        }
        "/multiline" | "/ml" => {
            app.multiline = !app.multiline;
            app.editor.multiline = app.multiline;
            app.set_status(if app.multiline {
                "Multiline input on (Enter inserts a newline)"
            } else {
                "Multiline input off"
            });
            None
        }
        "/mode" => {
            let next = match args.trim().to_lowercase().as_str() {
                "normal" | "default" => UiMode::Normal,
                "plan" => UiMode::Plan,
                "auto" | "always" | "yolo" => UiMode::AlwaysApprove,
                "review" | "auto-review" => UiMode::AutoReview,
                _ => app.mode.next(),
            };
            Some(Action::ModeChange(next))
        }
        "/bench" | "/speed" => Some(Action::BenchModels),
        "/rewind" => {
            let turns = args.trim().parse::<usize>().unwrap_or(1);
            Some(Action::Rewind(turns))
        }
        "/fork" => Some(Action::Fork),
        "/personas" | "/agents" => {
            if args.trim().is_empty() {
                let state = app.persona_picker(config);
                app.push_modal(Modal::Persona(state));
            } else {
                return Some(Action::SetPersona(args.trim().to_string()));
            }
            None
        }
        "/persona" => Some(Action::SetPersona(args.trim().to_string())),
        "/timeline" => {
            let value = args.trim();
            if let Some(filter) = value.strip_prefix("all") {
                let filter = filter.trim().to_string();
                return Some(Action::TimelineAll(filter));
            }
            Some(Action::Timeline)
        }
        "/worktree" | "/worktrees" => {
            let args = args.trim().to_string();
            if args.is_empty() {
                match git_worktree_list(cwd) {
                    Ok(list) if list.is_empty() => {
                        app.open_info(
                            "Worktrees",
                            vec![
                                "  no worktrees yet".into(),
                                String::new(),
                                "  /worktree <name>          create one on a new branch".into(),
                                "  /worktree remove <name>   delete it".into(),
                            ],
                        );
                    }
                    Ok(list) => {
                        let mut lines =
                            vec!["  path                                            branch"
                                .to_string()];
                        for (path, branch) in list {
                            lines.push(format!("  {:<48} {}", path, branch));
                        }
                        lines.push(String::new());
                        lines.push(
                            "  /worktree <name> creates ../<repo>-<name> on a new branch".into(),
                        );
                        app.open_info("Worktrees", lines);
                    }
                    Err(e) => app.set_status(format!("worktree list failed: {e}")),
                }
                None
            } else if let Some(name) = args.strip_prefix("remove ") {
                match git_worktree_remove(cwd, name.trim()) {
                    Ok(path) => app.set_status(format!("removed worktree {path}")),
                    Err(e) => app.set_status(format!("worktree remove failed: {e}")),
                }
                None
            } else {
                match git_worktree_add(cwd, args.trim()) {
                    Ok(path) => {
                        app.push_item(Item::Notice {
                            text: format!(
                                "worktree created at {path} (branch {}) — work there with: cd {path} && cachycli",
                                args.trim()
                            ),
                            kind: NoticeKind::Info,
                        });
                        app.set_status(format!("worktree ready: {path}"));
                    }
                    Err(e) => app.set_status(format!("worktree add failed: {e}")),
                }
                None
            }
        }
        "/workflow" | "/workflows" => {
            if args.trim().is_empty() {
                let state = app.workflow_picker();
                app.push_modal(Modal::Workflow(state));
                None
            } else {
                Some(Action::RunWorkflow(args.trim().to_string()))
            }
        }
        "/share" => {
            let value = args.trim().to_string();
            // `/share push [url]` and `/share pull <id> [url]` talk to a server;
            // anything else is a path, as before.
            if let Some(rest) = value.strip_prefix("push") {
                if rest.is_empty() || rest.starts_with(' ') {
                    return Some(Action::SharePush(rest.trim().to_string()));
                }
            }
            if let Some(rest) = value.strip_prefix("pull") {
                if rest.is_empty() || rest.starts_with(' ') {
                    return Some(Action::SharePull(rest.trim().to_string()));
                }
            }
            Some(Action::ShareSession(value))
        }
        "/shares" => Some(Action::Shares(args.trim().to_string())),
        "/import" | "/open" => {
            if args.trim().is_empty() {
                app.set_status("usage: /import <file.cachy>");
                None
            } else {
                Some(Action::ImportSession(args.trim().to_string()))
            }
        }
        "/goal" => {
            let value = args.trim().to_string();
            if value.is_empty() {
                let lines = vec![
                    match &app.goal {
                        Some(goal) => format!("  goal: {goal}"),
                        None => "  no goal set".to_string(),
                    },
                    String::new(),
                    format!(
                        "  verifiers: {} (config: [agent] goal_verifier_count)",
                        config.agent.goal_verifier_count
                    ),
                    String::new(),
                    "  /goal <text>    set the goal".into(),
                    "  /goal verify    run the verifier subagents".into(),
                    "  /goal clear     clear it".into(),
                ];
                app.open_info("Goal", lines);
            } else if value.eq_ignore_ascii_case("clear") {
                return Some(Action::SetGoal(None));
            } else if value.eq_ignore_ascii_case("verify") {
                return Some(Action::VerifyGoal);
            } else {
                return Some(Action::SetGoal(Some(value)));
            }
            None
        }
        "/search" | "/find" => {
            if args.trim().is_empty() {
                app.set_status("usage: /search <text>");
                None
            } else {
                let state = search_sessions(cwd, args.trim());
                app.push_modal(Modal::Search(state));
                None
            }
        }
        "/jump" => {
            let state = app.jump_picker();
            app.push_modal(Modal::Jump(state));
            None
        }
        "/tasks" => Some(Action::Tasks),
        "/login" | "/signin" => {
            if args.trim().is_empty() {
                // Every provider that can be signed in to, so the choice is
                // not "whichever one happens to be current".
                let infos = crate::providers::detect(config);
                let mut items = Vec::new();
                let mut values = Vec::new();
                for info in &infos {
                    let status = if !info.installed {
                        "not installed".to_string()
                    } else if info.connected {
                        if info.detail.is_empty() {
                            "connected".to_string()
                        } else {
                            format!("connected · {}", info.detail)
                        }
                    } else if info.login.is_empty() {
                        "signs in on its own".to_string()
                    } else {
                        "needs sign-in".to_string()
                    };
                    items.push(format!(
                        "{:<13} {:<22} {}",
                        info.label, status, info.summary
                    ));
                    values.push(info.id.provider_name().to_string());
                }
                let selected = values
                    .iter()
                    .position(|v| Some(v.as_str()) == config.agent.provider.as_deref())
                    .unwrap_or(0);
                app.push_modal(Modal::SignIn(PickerState {
                    query: String::new(),
                    selected,
                    items,
                    values,
                }));
                None
            } else {
                Some(Action::SignInProvider(args.trim().to_string()))
            }
        }
        "/logout" | "/signout" => Some(Action::SignOut),
        "/palette" | "/commands" | "/cmd" => {
            app.open_palette();
            None
        }
        "/notify" => {
            let value = args.trim().to_lowercase();
            app.ui_notify = match value.as_str() {
                "off" | "bell" | "osc" => value,
                _ => match app.ui_notify.as_str() {
                    "off" => "bell".into(),
                    "bell" => "osc".into(),
                    _ => "off".into(),
                },
            };
            let how = app.ui_notify.clone();
            if persist_setting(cwd, "ui", "notify", &format!("{how:?}")) {
                app.set_status(format!(
                    "Notify when a turn ends: {how} (after {}s, [ui] notify_after)",
                    app.ui_notify_after
                ));
            } else {
                app.set_status("notify applied, but the config could not be written");
            }
            None
        }
        "/density" => {
            let value = args.trim().to_lowercase();
            app.ui_tool_density = match value.as_str() {
                "compact" | "normal" | "verbose" => value,
                _ => match app.ui_tool_density.as_str() {
                    "compact" => "normal".into(),
                    "normal" => "verbose".into(),
                    _ => "compact".into(),
                },
            };
            let density = app.ui_tool_density.clone();
            if persist_setting(cwd, "ui", "tool_density", &format!("{density:?}")) {
                app.set_status(format!("Tool call density: {density}"));
            } else {
                app.set_status("density applied, but the config could not be written");
            }
            None
        }
        "/timestamps" => {
            let value = args.trim().to_lowercase();
            app.ui_timestamps = match value.as_str() {
                "on" | "true" => true,
                "off" | "false" => false,
                _ => !app.ui_timestamps,
            };
            let state = if app.ui_timestamps { "true" } else { "false" };
            if persist_setting(cwd, "ui", "timestamps", state) {
                app.set_status(if app.ui_timestamps {
                    "Timestamps on"
                } else {
                    "Timestamps off"
                });
            } else {
                app.set_status("timestamps applied, but the config could not be written");
            }
            None
        }
        "/copy" => {
            let text = app.last_answer().unwrap_or_else(|| app.export_markdown());
            if text.trim().is_empty() {
                app.set_status("nothing to copy yet");
            } else {
                let chars = text.chars().count();
                copy_to_clipboard(&text);
                app.set_status(format!("Copied {chars} characters — Ctrl/Cmd+V pastes it"));
            }
            None
        }
        "/effort" => {
            const LEVELS: [&str; 4] = ["low", "medium", "high", "none"];
            let value = args.trim().to_lowercase();
            let chosen = if LEVELS.contains(&value.as_str()) {
                if value == "none" {
                    None
                } else {
                    Some(value)
                }
            } else {
                match app.effort.as_deref() {
                    None => Some("low".to_string()),
                    Some("low") => Some("medium".to_string()),
                    Some("medium") => Some("high".to_string()),
                    _ => None,
                }
            };
            let stored = chosen.clone().unwrap_or_else(|| "none".into());
            if persist_setting(cwd, "agent", "reasoning_effort", &format!("{stored:?}")) {
                app.effort = chosen.clone();
                app.set_status(match &chosen {
                    Some(level) => format!("Reasoning effort: {level}"),
                    None => "Reasoning effort: model default".to_string(),
                });
            } else {
                app.set_status("effort applied, but the config could not be written");
            }
            None
        }
        "/doctor" => {
            use std::io::IsTerminal;
            let is_tty = std::io::stdout().is_terminal();
            let lines = vec![
                format!(
                    "stdout is a terminal: {}",
                    if is_tty { "yes" } else { "no" }
                ),
                format!(
                    "TERM: {}",
                    std::env::var("TERM").unwrap_or_else(|_| "unset".into())
                ),
                format!("cachy home: {}", cachy_core::paths::cachy_home().display()),
                format!(
                    "config file: {}",
                    cachy_core::paths::config_file().display()
                ),
                format!("provider: {}", backend.kind.provider_name()),
                format!("model: {}", app.model_label),
                format!(
                    "sessions: {}",
                    cachy_core::paths::sessions_dir_for(cwd).display()
                ),
                format!("memory: {}", if app.memory_enabled { "on" } else { "off" }),
                format!("mouse reporting: {}", app.mouse_reporting),
                format!("theme: {}", app.theme.name),
            ];
            app.open_info("Doctor", lines);
            None
        }
        "/privacy" => {
            app.open_info(
                "Privacy",
                vec![
                    "CachyCLI has no telemetry and no analytics.".into(),
                    "Prompts and file contents go only to the provider you configured:".into(),
                    format!("  provider: {}", backend.kind.provider_name()),
                    format!("  model: {}", app.model_label),
                    String::new(),
                    "Sessions, memory and settings stay on this machine under ~/.cachy.".into(),
                    "Nothing is uploaded anywhere else; there is no account to create.".into(),
                ],
            );
            None
        }
        "/toggle-mouse-reporting" | "/mouse" => {
            app.mouse_reporting = !app.mouse_reporting;
            crate::tui::set_mouse_capture(app.mouse_reporting);
            app.set_status(if app.mouse_reporting {
                "Mouse reporting on — click and scroll work inside the interface"
            } else {
                "Mouse reporting off — the terminal selects text again"
            });
            None
        }
        "/permissions" => {
            let lines = vec![
                format!("Mode: {}", app.mode.label()),
                format!(
                    "Always-approve: {}",
                    if app.always_approve { "on" } else { "off" }
                ),
                format!("Allow rules: {}", describe_list(&config.agent.rules)),
                "Deny rules: (set with --deny at launch)".into(),
            ];
            app.open_info("Permissions", lines);
            None
        }
        "/theme" => {
            if args.trim().is_empty() {
                let themes = Theme::all();
                let items = themes.iter().map(|t| t.name.to_string()).collect();
                let values = themes
                    .iter()
                    .map(|t| t.name.to_lowercase().replace(' ', "-"))
                    .collect();
                app.push_modal(Modal::Theme(PickerState {
                    query: String::new(),
                    selected: 0,
                    items,
                    values,
                }));
            } else {
                return Some(Action::SetTheme(args.trim().to_string()));
            }
            None
        }
        "/settings" | "/config" | "/preferences" | "/prefs" => {
            app.push_modal(Modal::Settings(app::SettingsState::new()));
            None
        }
        "/shortcuts" | "/keys" => {
            app.push_modal(Modal::Shortcuts(app::ShortcutsState::new()));
            None
        }
        "/init" => Some(Action::InitProject),
        "/inspect" => {
            let loaded =
                cachy_core::config::load(cwd).unwrap_or(cachy_core::config::LoadedConfig {
                    config: (**config).clone(),
                    sources: Vec::new(),
                    project_config: None,
                });
            let inspection = cachy_core::inspect::inspect(cwd, &loaded);
            let rendered = cachy_core::inspect::render(&inspection);
            app.open_info(
                "Discovered configuration",
                rendered.lines().map(|s| s.to_string()).collect(),
            );
            None
        }
        "/help" | "/?" => {
            let mut lines = vec!["Keyboard".into()];
            lines.push(
                "  Enter send · Alt+Enter newline · Shift+Tab mode · Ctrl+x shortcuts".into(),
            );
            lines.push(String::new());
            lines.push("Slash commands".into());
            for cmd in slash::COMMANDS {
                lines.push(format!("  {:<16} {}", cmd.name, cmd.description));
            }
            app.open_info("Help", lines);
            None
        }
        _ => {
            app.set_status(format!("unknown command {command}"));
            None
        }
    }
}

fn describe_list(items: &[String]) -> String {
    if items.is_empty() {
        "(none)".into()
    } else {
        items.join(", ")
    }
}

fn rename_session(cwd: &std::path::Path, session_id: &str, title: &str) -> Result<()> {
    let mut store = SessionStore::open(cwd, session_id)?;
    store.rename(title)?;
    Ok(())
}

/// Persist a single `[section] key = value` into the user config file.
///
/// Line-based so comments survive, but it replaces an existing key instead of
/// appending a second one (duplicate keys make the file invalid TOML).
pub fn persist_setting(_cwd: &std::path::Path, section: &str, key: &str, value: &str) -> bool {
    let path = cachy_core::paths::config_file();
    let existing = std::fs::read_to_string(&path).unwrap_or_default();
    let header = format!("[{section}]");
    let mut lines: Vec<String> = existing.lines().map(|l| l.to_string()).collect();

    let header_index = lines.iter().position(|l| l.trim() == header);
    match header_index {
        Some(index) => {
            // Find the key inside this section (stop at the next header).
            let mut replaced = false;
            let mut insert_at = index + 1;
            let mut i = index + 1;
            while i < lines.len() {
                let trimmed = lines[i].trim().to_string();
                if trimmed.starts_with('[') && trimmed.ends_with(']') {
                    break;
                }
                if !trimmed.starts_with('#') {
                    if let Some((name, _)) = trimmed.split_once('=') {
                        if name.trim() == key {
                            lines[i] = format!("{key} = {value}");
                            replaced = true;
                            break;
                        }
                    }
                }
                insert_at = i + 1;
                i += 1;
            }
            if !replaced {
                lines.insert(insert_at, format!("{key} = {value}"));
            }
        }
        None => {
            if !lines.is_empty() && !lines.last().map(|l| l.is_empty()).unwrap_or(true) {
                lines.push(String::new());
            }
            lines.push(header);
            lines.push(format!("{key} = {value}"));
        }
    }

    let mut contents = lines.join("\n");
    contents.push('\n');
    if toml::from_str::<cachy_core::config::Config>(&contents).is_err() {
        // Never write a file our own parser cannot read.
        return false;
    }
    std::fs::write(&path, contents).is_ok()
}

/// A session used when the backend could not be constructed.
struct NoopSession {
    reason: String,
}

impl NoopSession {
    fn new(reason: String) -> Self {
        Self { reason }
    }
}

#[async_trait::async_trait]
impl AgentSession for NoopSession {
    async fn prompt(&self, _text: String) -> Result<cachy_core::message::StopReason> {
        Err(anyhow::anyhow!("{}", self.reason))
    }
    fn cancel(&self) {}
    fn session_id(&self) -> String {
        String::new()
    }
    fn model_label(&self) -> String {
        "no model".into()
    }
    fn context_usage(&self) -> (u64, u64) {
        (0, 0)
    }
    async fn shutdown(&self) {}
}

// Keep the TurnOptions import used (headless parity helpers live in commands).
#[allow(dead_code)]
fn _unused(_: TurnOptions) {}

/// Helper used by headless mode to build a native agent outside the TUI.
#[allow(clippy::too_many_arguments)] // a constructor: every field has to come in
pub async fn build_native_agent(
    config: Arc<Config>,
    cwd: PathBuf,
    model_name: Option<&str>,
    always_approve: bool,
    allow: Vec<String>,
    deny: Vec<String>,
    tools_filter: Option<Vec<String>>,
    disallowed: Vec<String>,
    tx: mpsc::UnboundedSender<AgentEvent>,
    session: SessionStore,
    interactive: bool,
    permission_mode: cachy_core::permissions::PermissionMode,
) -> Result<Arc<NativeAgent>> {
    let model = models::resolve(&config, model_name).ok_or_else(|| {
        anyhow::anyhow!(
            "no model configured — add a [model.<name>] section to ~/.cachy/config.toml"
        )
    })?;
    let session_id = session.summary.id.clone();
    let mcp = Arc::new(McpManager::new(config.mcp_servers.clone(), session_id));
    mcp.initialize().await;
    let mut registry = ToolRegistry::default_registry(&config);
    let warnings = registry.filter(tools_filter.as_deref(), &disallowed);
    let engine = PermissionEngine::new(
        RuleSet::parse(&allow, &deny),
        permission_mode,
        always_approve,
    );
    let agent = NativeAgent::new(
        cwd,
        config,
        model,
        session,
        mcp,
        registry,
        engine,
        tx,
        u32::MAX,
        interactive,
        0,
    )
    .await?;
    for warning in warnings {
        eprintln!("{warning}");
    }
    Ok(agent)
}
