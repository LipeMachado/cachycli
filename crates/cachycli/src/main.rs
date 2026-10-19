//! cachycli entry point.

mod cli;
mod commands;
mod providers;
mod tui;
mod ws;

use std::path::PathBuf;
use std::sync::Arc;

use anyhow::{Context, Result};
use clap::Parser;

use cachy_core::config::{BackendKind, Config};
use cachy_core::paths;
use cli::{Cli, Command, LeaderAction, OutputFormat, SessionsAction};

fn main() {
    let cli = Cli::parse();
    let runtime = match tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
    {
        Ok(rt) => rt,
        Err(e) => {
            eprintln!("failed to start runtime: {e}");
            std::process::exit(1);
        }
    };
    let code = match runtime.block_on(run(cli)) {
        Ok(code) => code,
        Err(e) => {
            eprintln!("error: {e:#}");
            1
        }
    };
    std::process::exit(code);
}

async fn run(cli: Cli) -> Result<i32> {
    // Resolve the working directory (with optional worktree creation).
    let mut cwd = cli
        .cwd
        .clone()
        .unwrap_or_else(|| std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")));
    if let Ok(canonical) = cwd.canonicalize() {
        cwd = canonical;
    }
    if let Some(worktree) = &cli.worktree {
        cwd = create_worktree_cli(&cwd, worktree.as_deref(), cli.worktree_ref.as_deref())?;
    }

    paths::ensure_dirs().ok();

    // A rule nobody can parse is a guard the user thinks they have.
    for bad in cachy_core::permissions::invalid_rules(&cli.allow, &cli.deny) {
        eprintln!(
            "warning: ignoring permission rule {bad:?} — it does not look like one. \
             Use {}",
            cachy_core::permissions::RULE_PREFIXES
        );
    }

    // Load configuration and apply CLI overrides.
    let mut loaded = cachy_core::config::load(&cwd)?;
    apply_cli_overrides(&mut loaded.config, &cli);
    let config = Arc::new(loaded.config.clone());
    if let Some(path) = &cli.debug_file {
        let _ = std::fs::write(
            path,
            format!("cachycli {} debug log\n", cachy_core::VERSION),
        );
    }

    // Subcommands first.
    if let Some(command) = &cli.command {
        return run_command(&cli, command, &config, &cwd).await;
    }

    // Headless mode when a single prompt is provided (or stdin is piped).
    let stdin_piped = commands::stdin_prompt();
    let prompt = commands::prompt_from_args(&cli).or_else(|| {
        if cli.single.is_none() && cli.prompt.is_none() && stdin_piped.is_some() {
            stdin_piped
        } else {
            None
        }
    });
    if let Some(prompt) = prompt {
        let opts = commands::HeadlessOptions {
            prompt,
            model: cli.model.clone(),
            output_format: cli.output_format,
            always_approve: cli.always_approve,
            allow: cli.allow.clone(),
            deny: cli.deny.clone(),
            tools: cli.tools.as_deref().map(split_list),
            disallowed_tools: cli
                .disallowed_tools
                .as_deref()
                .map(split_list)
                .unwrap_or_default(),
            session_id: cli.session_id.clone(),
            resume: cli.resume.clone().flatten(),
            continue_latest: cli.r#continue,
            max_turns: cli.max_turns,
            permission_mode: cli.permission_mode.map(Into::into),
            rules: cli.rules.clone().map(|r| vec![r]).unwrap_or_default(),
            include_partial_messages: cli.include_partial_messages,
            json_schema: cli.json_schema.clone(),
            verbatim: cli.verbatim,
        };
        // Non-native providers run the prompt through their own agent.
        return match providers::default(&config) {
            BackendKind::Native => commands::headless(config, cwd, opts).await,
            kind => commands::headless_backend(config, cwd, &opts, kind).await,
        };
    }

    // Interactive TUI.
    let opts = tui::TuiOptions {
        resume: cli.resume.clone().flatten(),
        resume_latest: cli.r#continue,
        model: cli.model.clone(),
        reasoning_effort: cli.reasoning_effort.clone(),
        no_alt_screen: cli.no_alt_screen,
        minimal: cli.minimal,
        always_approve: cli.always_approve,
        allow: cli.allow.clone(),
        deny: cli.deny.clone(),
        tools: cli.tools.as_deref().map(split_list),
        disallowed_tools: cli
            .disallowed_tools
            .as_deref()
            .map(split_list)
            .unwrap_or_default(),
        max_turns: cli.max_turns,
        permission_mode: cli
            .permission_mode
            .map(|m| m.into())
            .map(|m: cachy_core::permissions::PermissionMode| m.as_str().to_string()),
        backend_override: cli.provider.clone(),
        initial_prompt: cli.prompt.clone(),
        open_dashboard: false,
    };
    tui::run(config, cwd, opts).await?;
    Ok(0)
}

fn apply_cli_overrides(config: &mut Config, cli: &Cli) {
    if let Some(model) = &cli.model {
        config.models.default = Some(model.clone());
    }
    if let Some(provider) = &cli.provider {
        config.agent.provider = Some(provider.clone());
    }
    if let Some(url) = &cli.opencode_url {
        config.agent.opencode.url = Some(url.clone());
    }
    if let Some(command) = &cli.acp_command {
        let parts = shlex::split(command).unwrap_or_default();
        if let Some((first, rest)) = parts.split_first() {
            config.agent.acp.command = first.clone();
            config.agent.acp.args = rest.to_vec();
            // An explicit command means "spawn this", not "connect there".
            config.agent.acp.url = None;
        }
    }
    if cli.no_subagents {
        config.subagents.enabled = false;
    }
    if cli.disable_web_search {
        config.features.web_search = false;
        config.features.web_fetch = false;
    }
    if let Some(prompt) = &cli.system_prompt_override {
        config.agent.system_prompt_override = Some(prompt.clone());
    }
    if let Some(rules) = &cli.rules {
        config.agent.rules.push(rules.clone());
    }
    if cli.minimal {
        config.ui.screen_mode = "minimal".into();
    }
    if cli.fullscreen {
        config.ui.screen_mode = "fullscreen".into();
    }
    if let Some(sandbox) = &cli.sandbox {
        if !sandbox.is_empty() && sandbox != "off" {
            config.agent.rules.push(format!(
                "Sandbox profile \"{sandbox}\" was requested. cachycli does not enforce OS-level sandboxing in this build; avoid destructive commands."
            ));
        }
    }
    if cli.debug {
        std::env::set_var("RUST_LOG", "debug");
    }
}

fn split_list(value: &str) -> Vec<String> {
    value
        .split(',')
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .collect()
}

fn create_worktree_cli(
    cwd: &std::path::Path,
    name: Option<&str>,
    reference: Option<&str>,
) -> Result<PathBuf> {
    let name = name.unwrap_or("cachy-worktree");
    let dir = paths::cachy_home().join("worktrees").join(name);
    std::fs::create_dir_all(dir.parent().unwrap_or(&dir))?;
    let mut cmd = std::process::Command::new("git");
    cmd.arg("-C")
        .arg(cwd)
        .args(["worktree", "add", "-b", name])
        .arg(&dir);
    if let Some(reference) = reference {
        cmd.arg(reference);
    }
    let output = cmd.output().context("git worktree add")?;
    if !output.status.success() {
        // The branch may already exist: retry without -b.
        let mut retry = std::process::Command::new("git");
        retry
            .arg("-C")
            .arg(cwd)
            .args(["worktree", "add"])
            .arg(&dir)
            .arg(name);
        let output = retry.output().context("git worktree add")?;
        if !output.status.success() {
            anyhow::bail!(
                "git worktree add failed: {}",
                String::from_utf8_lossy(&output.stderr).trim()
            );
        }
    }
    println!("Worktree ready at {}", dir.display());
    Ok(dir)
}

async fn run_command(
    cli: &Cli,
    command: &Command,
    config: &Arc<Config>,
    cwd: &std::path::Path,
) -> Result<i32> {
    match command {
        Command::Agent(args) => match &args.mode {
            cli::AgentMode::Stdio(stdio) => {
                cachy_core::acp::server::run_stdio(
                    config.clone(),
                    stdio.model.clone().or_else(|| cli.model.clone()),
                    stdio.always_approve || cli.always_approve,
                    cwd.to_path_buf(),
                )
                .await?;
                Ok(0)
            }
            cli::AgentMode::Serve(serve) => {
                crate::ws::serve(&serve.host, serve.port, cwd.to_path_buf(), config.clone())
                    .await?;
                Ok(0)
            }
            cli::AgentMode::Headless(headless) => match &headless.ws_url {
                Some(url) => commands::ws_relay(url).await,
                None => anyhow::bail!(
                    "provide --ws-url <ws://host:port> to bridge to a remote agent, \
                     or use `cachycli agent stdio` for local clients"
                ),
            },
        },
        Command::Acp => {
            cachy_core::acp::server::run_stdio(
                config.clone(),
                cli.model.clone(),
                cli.always_approve,
                cwd.to_path_buf(),
            )
            .await?;
            Ok(0)
        }
        Command::Clone(args) => commands::clone_cmd(args),
        Command::Completions(args) => commands::completions_cmd(&args.shell),
        Command::CursorWorker => commands::cursor_worker_cmd(),
        Command::Dashboard => {
            let opts = tui::TuiOptions {
                backend_override: cli.provider.clone(),
                model: cli.model.clone(),
                open_dashboard: true,
                ..Default::default()
            };
            tui::run(config.clone(), cwd.to_path_buf(), opts).await?;
            Ok(0)
        }
        Command::Doctor(args) => commands::doctor(cwd, config, args.json),
        Command::Du => commands::du_cmd(),
        Command::Export(args) => commands::export_cmd(cwd, args),
        Command::Inspect(args) => commands::inspect_cmd(cwd, args.json),
        Command::Leader(args) => commands::leader_cmd(args.action.as_ref()),
        Command::Login(args) => commands::login_cmd(config, args),
        Command::Logout => commands::logout_cmd(),
        Command::Mcp(args) => commands::mcp_cmd(cwd, config, args.action.as_ref()).await,
        Command::Memory(args) => commands::memory_cmd(cwd, &args.action),
        Command::Models(args) => commands::models_cmd(config, args.json),
        Command::Plugin(args) => commands::plugin_cmd(cwd, args.action.as_ref()),
        Command::Sessions(args) => commands::sessions_cmd(cwd, args.action.as_ref(), args.json),
        Command::Share(args) => commands::share_cmd(cwd, args).await,
        Command::Setup(args) => commands::setup_cmd(config, args).await,
        Command::Trace(args) => commands::trace_cmd(cwd, args),
        Command::Update(args) => commands::update_cmd(args).await,
        Command::Uninstall(args) => commands::uninstall_cmd(args),
        Command::Usage(args) => commands::usage_cmd(cwd, args.session.as_deref()),
        Command::Version => commands::version_cmd(),
        Command::Worktree(args) => commands::worktree_cmd(cwd, args.action.as_ref()),
        Command::Wrap(args) => commands::wrap_cmd(args).await,
    }
}

// Silence unused warnings for items reserved for later commands.
#[allow(dead_code)]
fn _keep_used() {
    let _ = LeaderAction::Status;
    let _ = SessionsAction::List;
    let _ = OutputFormat::Plain;
}
