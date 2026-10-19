//! TUI application state: conversation items, modals, editors and key handling.

use std::path::PathBuf;
use std::sync::Arc;

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::layout::Rect;
use ratatui::text::Line;

use cachy_core::agent::AgentEvent;
use cachy_core::config::{BackendKind, Config};
use cachy_core::permissions::ApprovalChoice;
use cachy_core::session::PlanEntry;
use cachy_core::tools::TodoItem;

use super::editor::Editor;
use super::picker::FileIndex;
use super::slash;
use super::theme::Theme;

/// One block in the conversation scrollback.
pub enum Item {
    Welcome,
    User {
        text: String,
        ts: String,
    },
    AssistantText {
        text: String,
        cache: RenderCache,
    },
    Reasoning {
        text: String,
        expanded: bool,
        cache: RenderCache,
        seconds: Option<f64>,
    },
    ToolCall(ToolCallItem),
    Diff {
        path: String,
        diff: String,
        cache: RenderCache,
    },
    Todos {
        todos: Vec<TodoItem>,
        cache: RenderCache,
    },
    Notice {
        text: String,
        kind: NoticeKind,
    },
    TurnSummary {
        text: String,
    },
    Error {
        text: String,
    },
}

pub struct ToolCallItem {
    pub id: String,
    pub name: String,
    pub title: String,
    pub kind: String,
    pub arguments: serde_json::Value,
    pub status: String,
    pub summary: Option<String>,
    pub meta: Option<serde_json::Value>,
    pub started: std::time::Instant,
    pub elapsed: Option<f64>,
    /// Rendered output lines from the summary/meta.
    pub cache: RenderCache,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NoticeKind {
    Info,
    Warning,
    Error,
}

/// A clickable rectangle registered during rendering.
#[derive(Debug, Clone)]
pub struct HitRegion {
    pub area: ratatui::layout::Rect,
    pub action: HitAction,
}

#[derive(Debug, Clone, PartialEq)]
pub enum HitAction {
    /// Select row `index` of the top modal.
    ModalRow(usize),
    /// Close the top modal.
    ModalClose,
    /// Move the prompt caret to this column on the given input row.
    PromptCaret { row: usize, origin_x: u16 },
    /// Choose slash command at index.
    SlashRow(usize),
    /// Choose file candidate at index.
    FileRow(usize),
    /// Approval option index.
    ApprovalRow(usize),
    /// The [stop] control of the running-turn strip.
    StopTurn,
}

/// A text selection over the scrollback, in screen coordinates.
#[derive(Debug, Clone, Copy)]
pub struct Selection {
    pub anchor: (u16, u16),
    pub focus: (u16, u16),
}

impl Selection {
    /// (start, end) with start before end in reading order.
    pub fn normalized(&self) -> ((u16, u16), (u16, u16)) {
        let (a, b) = (self.anchor, self.focus);
        if (a.1, a.0) <= (b.1, b.0) {
            (a, b)
        } else {
            (b, a)
        }
    }

    pub fn is_empty(&self) -> bool {
        self.anchor == self.focus
    }
}

/// One row of the `/tasks` panel.
#[derive(Debug, Clone)]
pub struct TaskRow {
    pub id: String,
    pub kind: String,
    pub description: String,
    pub status: String,
    pub output: String,
    /// When the task started, so the panel can show live elapsed time.
    pub started: std::time::Instant,
    /// Duration for finished tasks (running ones use `started`).
    pub frozen: f64,
    pub finished: bool,
}

/// Cached markdown rendering keyed by (width, theme id).
#[derive(Default)]
pub struct RenderCache {
    pub key: Option<(u16, &'static str)>,
    pub lines: Vec<Line<'static>>,
}

impl RenderCache {
    pub fn is_valid(&self, width: u16, theme: &Theme) -> bool {
        self.key
            .map(|(w, t)| w == width && t == theme.name)
            .unwrap_or(false)
    }
    pub fn store(&mut self, width: u16, theme: &Theme, lines: Vec<Line<'static>>) {
        self.key = Some((width, theme.name));
        self.lines = lines;
    }
}

/// Rebindable shortcuts. Keys are stored as `ctrl+x`, `shift+enter`, `f2`.
#[derive(Debug, Clone)]
pub struct Keymap {
    bindings: std::collections::BTreeMap<String, String>,
}

pub const KEY_ACTIONS: &[(&str, &str, &str)] = &[
    ("send", "Enter", "Send the message"),
    ("newline", "shift+enter", "Insert a newline"),
    ("cancel", "ctrl+c", "Cancel the turn / clear the input"),
    ("quit", "ctrl+q", "Quit (with confirmation)"),
    ("always_approve", "ctrl+o", "Toggle always-approve"),
    ("todos", "ctrl+t", "Toggle the plan panel"),
    ("resume", "ctrl+r", "Resume a session"),
    ("palette", "ctrl+p", "Command palette"),
    ("shortcuts", "ctrl+x", "Keyboard shortcuts"),
    ("settings", "f2", "Settings"),
    ("multiline", "ctrl+m", "Toggle multi-line input"),
    ("undo", "ctrl+u", "Undo the input"),
    ("redo", "ctrl+y", "Redo the input"),
];

impl Default for Keymap {
    fn default() -> Self {
        let mut bindings = std::collections::BTreeMap::new();
        for (action, key, _) in KEY_ACTIONS {
            bindings.insert((*action).to_string(), (*key).to_string());
        }
        // Extra defaults the reference also accepts.
        bindings.insert("cancel".into(), "ctrl+c,esc".into());
        bindings.insert("newline".into(), "shift+enter,alt+enter,ctrl+enter".into());
        bindings.insert("quit".into(), "ctrl+q,ctrl+d".into());
        bindings.insert("shortcuts".into(), "ctrl+x,ctrl+.".into());
        bindings.insert("settings".into(), "f2,ctrl+,".into());
        Self { bindings }
    }
}

impl Keymap {
    pub fn from_pairs(pairs: &[(String, String)]) -> Self {
        let mut map = Self::default();
        for (action, keys) in pairs {
            map.bindings.insert(action.clone(), keys.clone());
        }
        map
    }

    pub fn keys_for(&self, action: &str) -> String {
        self.bindings.get(action).cloned().unwrap_or_default()
    }

    pub fn set(&mut self, action: &str, keys: &str) {
        self.bindings.insert(action.to_string(), keys.to_string());
    }

    /// Factory default for one action (used by the reset shortcut).
    pub fn default_value(action: &str) -> String {
        match action {
            "newline" => "shift+enter,alt+enter,ctrl+enter".to_string(),
            "quit" => "ctrl+q,ctrl+d".to_string(),
            "shortcuts" => "ctrl+x,ctrl+.".to_string(),
            "settings" => "f2,ctrl+,".to_string(),
            other => KEY_ACTIONS
                .iter()
                .find(|(a, _, _)| *a == other)
                .map(|(_, key, _)| (*key).to_string())
                .unwrap_or_default(),
        }
    }

    /// Which action (if any) this key event triggers.
    pub fn resolve(&self, key: &KeyEvent) -> Option<String> {
        let needed = format_key(key);
        // "enter" from the numpad or with modifiers all normalise here.
        self.bindings
            .iter()
            .find(|(_, keys)| {
                keys.split(',')
                    .any(|k| k.trim().eq_ignore_ascii_case(&needed))
            })
            .map(|(action, _)| action.clone())
    }
}

/// `ctrl+x`, `shift+enter`, `alt+enter`, `f2`, `a`.
pub fn format_key(key: &KeyEvent) -> String {
    let mut parts: Vec<String> = Vec::new();
    if key.modifiers.contains(KeyModifiers::CONTROL) {
        parts.push("ctrl".into());
    }
    if key.modifiers.contains(KeyModifiers::ALT) {
        parts.push("alt".into());
    }
    if key.modifiers.contains(KeyModifiers::SHIFT) {
        parts.push("shift".into());
    }
    let name = match key.code {
        KeyCode::Char(' ') => "space".to_string(),
        KeyCode::Char(c) => c.to_string(),
        KeyCode::Enter => "enter".to_string(),
        KeyCode::Esc => "esc".to_string(),
        KeyCode::Tab => "tab".to_string(),
        KeyCode::BackTab => "backtab".to_string(),
        KeyCode::Backspace => "backspace".to_string(),
        KeyCode::Delete => "delete".to_string(),
        KeyCode::Up => "up".to_string(),
        KeyCode::Down => "down".to_string(),
        KeyCode::Left => "left".to_string(),
        KeyCode::Right => "right".to_string(),
        KeyCode::Home => "home".to_string(),
        KeyCode::End => "end".to_string(),
        KeyCode::PageUp => "pageup".to_string(),
        KeyCode::PageDown => "pagedown".to_string(),
        KeyCode::F(n) => format!("f{n}"),
        other => format!("{other:?}").to_lowercase(),
    };
    parts.push(name);
    parts.join("+")
}

/// Pending approval prompt state.
pub struct ApprovalState {
    pub id: String,
    pub tool: String,
    pub title: String,
    pub detail: String,
    pub options: Vec<String>,
    pub selected: usize,
    /// Free-text feedback when rejecting.
    pub feedback: String,
}

/// Modal dialogs.
pub enum Modal {
    Quit,
    Palette(PaletteState),
    Shortcuts(ShortcutsState),
    Settings(SettingsState),
    Resume(PickerState),
    Model(PickerState),
    Provider(PickerState),
    /// Choosing which provider to sign in to (`/login`).
    SignIn(PickerState),
    Theme(PickerState),
    /// Agent profiles (`/personas`).
    Persona(PickerState),
    /// Turns to scroll back to (`/jump`).
    Jump(PickerState),
    /// Workflows to run (`/workflow`).
    Workflow(PickerState),
    /// Sessions matching a search (`/search`).
    Search(PickerState),
    /// Background tasks and subagents, selectable (`/tasks`).
    Tasks {
        rows: Vec<TaskRow>,
        selected: usize,
    },
    /// Plugins from the configured index (`/plugin search`).
    Plugin(PickerState),
    Sessions(PickerState),
    Info {
        title: String,
        lines: Vec<String>,
        scroll: u16,
    },
    /// Offered when the configured model cannot be used (expired credential):
    /// Enter switches to a model that works, Esc keeps things as they are.
    ModelFallback {
        model: String,
        reason: String,
    },
    Memory {
        lines: Vec<String>,
        scroll: u16,
    },
}

pub struct PaletteState {
    pub query: String,
    pub selected: usize,
    pub items: Vec<PaletteItem>,
}

pub struct PaletteItem {
    pub label: String,
    pub shortcut: String,
    pub category: String,
    pub command: String,
}

pub struct PickerState {
    pub query: String,
    pub selected: usize,
    pub items: Vec<String>,
    /// Values parallel to `items` (session ids, model names, theme names).
    pub values: Vec<String>,
}

pub struct ShortcutsState {
    pub selected: usize,
    pub expanded: Vec<usize>,
    pub search: String,
    pub searching: bool,
}

impl ShortcutsState {
    pub fn new() -> Self {
        Self {
            selected: 0,
            expanded: vec![0],
            search: String::new(),
            searching: false,
        }
    }
}

impl Default for ShortcutsState {
    fn default() -> Self {
        Self::new()
    }
}

pub struct SettingsState {
    pub selected: usize,
    /// `/` activates this field; typing filters the list.
    pub search: String,
    pub searching: bool,
}

impl SettingsState {
    pub fn new() -> Self {
        Self {
            selected: 0,
            search: String::new(),
            searching: false,
        }
    }
}

impl Default for SettingsState {
    fn default() -> Self {
        Self::new()
    }
}

/// Overall UI mode shown in the bottom-left and cycled by Shift+Tab.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UiMode {
    Normal,
    Plan,
    AlwaysApprove,
    AutoReview,
}

impl UiMode {
    pub fn label(&self) -> &'static str {
        match self {
            UiMode::Normal => "Normal",
            UiMode::Plan => "Plan",
            UiMode::AlwaysApprove => "Always-approve",
            UiMode::AutoReview => "Auto-review",
        }
    }

    /// Text shown next to the model name (nothing in normal mode).
    pub fn badge(&self) -> Option<&'static str> {
        match self {
            UiMode::Normal => None,
            UiMode::Plan => Some("plan mode"),
            UiMode::AlwaysApprove => Some("always-approve"),
            UiMode::AutoReview => Some("auto-review"),
        }
    }

    pub fn next(&self) -> Self {
        match self {
            UiMode::Normal => UiMode::Plan,
            UiMode::Plan => UiMode::AlwaysApprove,
            UiMode::AlwaysApprove => UiMode::AutoReview,
            UiMode::AutoReview => UiMode::Normal,
        }
    }
}

/// Action requested by a key press, executed by the event loop.
pub enum Action {
    None,
    Send(String),
    /// Send the next message typed while a turn was running.
    SendQueued,
    /// Time every model and remember the results (`/bench`).
    BenchModels,
    /// Drop the last N turns (`/rewind`).
    Rewind(usize),
    /// Turn-by-turn view of the session (`/timeline`).
    Timeline,
    /// Background commands and subagents (`/tasks`).
    Tasks,
    /// Copy the session into a new one (`/fork`).
    Fork,
    /// Apply an agent profile (`/personas`, `/persona <name>`).
    SetPersona(String),
    /// Scroll the transcript to an item (`/jump`).
    JumpTo(usize),
    /// Run a workflow by name.
    RunWorkflow(String),
    /// Set or clear the session goal.
    SetGoal(Option<String>),
    /// Kill a background task.
    KillTask(String),
    /// Install a plugin from the index.
    InstallPlugin(String),
    /// Write the session to a portable bundle (`/share`).
    ShareSession(String),
    /// Push this session to a server (empty url: `[share] server`).
    SharePush(String),
    /// Fetch a shared session ("<id> [url]"; empty id: the newest).
    SharePull(String),
    /// The same timeline across every session, optionally filtered.
    TimelineAll(String),
    /// What a server is holding ("[remove <id>] [url]").
    Shares(String),
    /// Start a new conversation, on both sides.
    NewSession,
    /// Sign in to a specific provider (`/login <name>` or the picker).
    SignInProvider(String),
    /// Open a bundle (`/import`).
    ImportSession(String),
    /// Re-read the task list.
    RefreshTasks,
    /// Verify the goal with subagents.
    VerifyGoal,
    /// Sign out of the current provider.
    SignOut,
    Slash {
        command: String,
        args: String,
    },
    Quit,
    Cancel,
    Approve {
        id: String,
        choice: ApprovalChoice,
        feedback: Option<String>,
    },
    Resume(String),
    RefreshResumeList,
    RefreshModelList,
    OpenSessions,
    ModeChange(UiMode),
    SetTheme(String),
    SetModel(String),
    SetProvider(String),
    Compact,
    Export(String),
    TrustFolder,
    Remember(String),
    InitProject,
}

pub struct App {
    pub theme: Theme,
    /// Action produced by a slash command, handled by the main loop so nothing
    /// is silently dropped.
    pub pending_action: Option<Action>,
    pub cwd: PathBuf,
    pub editor: Editor,
    pub items: Vec<Item>,
    /// Scroll offset in lines from the bottom (0 = pinned to bottom).
    pub scroll_from_bottom: usize,
    pub total_lines: usize,
    pub viewport_lines: usize,
    /// Modal stack: the last entry is the one on screen (Esc pops one level).
    pub modals: Vec<Modal>,
    /// Clickable regions of the current frame, filled while drawing.
    pub hits: Vec<HitRegion>,
    pub approval: Option<ApprovalState>,
    pub slash_menu_open: bool,
    pub slash_selected: usize,
    pub file_picker_open: bool,
    pub file_picker_query: String,
    pub file_index: FileIndex,
    pub file_selected: usize,
    pub file_candidates: Vec<String>,
    pub todos: Vec<TodoItem>,
    pub plan: Vec<PlanEntry>,
    pub show_todos: bool,
    pub mode: UiMode,
    pub multiline: bool,
    pub always_approve: bool,
    /// True while the terminal sends mouse events to us.
    pub mouse_reporting: bool,
    /// Set when the user pressed Ctrl+C once with an empty prompt.
    pub quit_armed: Option<std::time::Instant>,
    /// Rebindable shortcuts.
    pub keys: Keymap,
    /// Action waiting for its new key.
    pub capturing: Option<String>,
    /// Keymap changes made in this session (persisted by the event loop).
    pub keymap_edits: Vec<(String, String)>,
    /// Title of the tool currently running (for the status strip).
    pub running_tool: Option<String>,
    /// Hit action under the pointer (hover highlight).
    pub hover: Option<HitAction>,
    /// Area of the scrollback in the last frame (for mouse selection).
    pub scrollback_rect: Rect,
    /// Plain text of the scrollback rows shown in the last frame.
    pub scrollback_text: Vec<String>,
    /// Area of the `/` menu or `@` picker in the last frame.
    pub menu_rect: Rect,
    /// Area of the top modal in the last frame.
    pub modal_rect: Rect,
    /// Drag selection over the scrollback.
    pub selection: Option<Selection>,
    /// True while the left button is held after starting a selection.
    pub selecting: bool,
    /// Selection inside the prompt box (byte offsets into the editor text).
    pub prompt_selection: Option<(usize, usize)>,
    /// True while the prompt selection is being dragged.
    pub selecting_prompt: bool,
    /// Prompt box area from the last frame.
    pub prompt_rect: Rect,
    /// Options for the argument being typed (`/theme ` → themes).
    pub arg_options: Vec<(String, String)>,
    /// Providers offered by `/provider `.
    pub provider_options: Vec<(String, String)>,
    /// Models offered by `/model ` for the native backend.
    pub native_model_options: Vec<(String, String)>,
    /// Models reported by `opencode models`, filled in the background.
    pub opencode_model_options: std::sync::Arc<std::sync::Mutex<Vec<(String, String)>>>,
    /// Measured time-to-first-token per model (`/bench`), model -> seconds.
    pub bench_results: std::sync::Arc<std::sync::Mutex<std::collections::BTreeMap<String, f64>>>,
    /// True while a benchmark run is going.
    pub benching: std::sync::Arc<std::sync::atomic::AtomicBool>,
    /// True when the active provider is opencode (models come from there).
    pub provider_is_opencode: bool,
    /// Providers you are signed in to, in the order the picker shows them.
    pub connected_providers: Vec<BackendKind>,
    /// The built-in catalog entries that have a key, as `(value, hint)`.
    pub native_ready_options: Vec<(String, String)>,
    /// A model chosen from another provider, applied once the switch is done.
    pub pending_model: Option<String>,
    /// The last model-list failure we told the user about, so a repeated one
    /// stays quiet.
    pub opencode_models_error: Arc<std::sync::Mutex<Option<String>>>,
    /// Set when opencode reported an expired credential in this session, so the
    /// model menu can warn instead of letting the same mistake happen twice.
    pub opencode_key_expired: bool,
    /// Messages typed while a turn is running (sent as soon as it ends).
    pub queue: Vec<String>,
    /// Active agent profile name, when one is set.
    pub persona_name: Option<String>,
    /// What this session is working towards (`/goal`).
    pub goal: Option<String>,
    /// Subagent limits shown by `/tasks` (concurrency, depth).
    pub ui_task_limits: (usize, usize),
    /// Ticks since the last `/tasks` refresh (keeps the panel live).
    pub tasks_refresh_ticks: u32,
    /// First rendered line of each item in the last frame (`/jump`).
    pub item_offsets: Vec<usize>,
    /// Live interface settings (mirrored from the config, editable in /settings).
    pub ui_timestamps: bool,
    pub ui_thinking: bool,
    pub ui_tool_density: String,
    pub ui_thoughts_width: u16,
    /// How to notify when a long turn ends (`off`, `bell`, `osc`).
    pub ui_notify: String,
    pub ui_notify_after: u64,
    /// Set by a finished turn; the event loop writes it after the next draw.
    pub pending_notification: Option<String>,
    pub memory_enabled: bool,
    /// Token counter for the running turn.
    pub stream_tokens: u64,
    pub model_label: String,
    /// Reasoning effort shown next to the model, e.g. "high".
    pub effort: Option<String>,
    pub backend_label: String,
    pub context_used: u64,
    pub context_size: u64,
    pub output_tokens: u64,
    pub session_id: String,
    pub session_title: String,
    pub busy: bool,
    pub streaming_text: Option<String>,
    /// Reasoning streamed live (shown dim while the model thinks).
    pub streaming_reasoning: Option<String>,
    /// When the current thinking phase started (for "Thought for Xs").
    pub reasoning_started: Option<std::time::Instant>,
    pub spinner_frame: usize,
    pub turn_started: Option<std::time::Instant>,
    /// When the first token of the running turn arrived (time-to-first-token).
    pub first_token_at: Option<std::time::Instant>,
    pub last_worked: Option<f64>,
    pub status_message: Option<(String, std::time::Instant)>,
}

impl App {
    pub fn new(config: Arc<Config>, cwd: PathBuf, theme: Theme) -> Self {
        let file_index = FileIndex::new(cwd.clone());
        Self {
            theme,
            pending_action: None,
            cwd,
            editor: Editor::new(),
            items: vec![Item::Welcome],
            scroll_from_bottom: 0,
            total_lines: 0,
            viewport_lines: 0,
            modals: Vec::new(),
            hits: Vec::new(),
            approval: None,
            slash_menu_open: false,
            slash_selected: 0,
            file_picker_open: false,
            file_picker_query: String::new(),
            file_index,
            file_selected: 0,
            file_candidates: Vec::new(),
            todos: Vec::new(),
            plan: Vec::new(),
            show_todos: false,
            mode: UiMode::Normal,
            multiline: config.ui.screen_mode == "multiline",
            always_approve: false,
            mouse_reporting: true,
            quit_armed: None,
            keys: Keymap::default(),
            capturing: None,
            keymap_edits: Vec::new(),
            running_tool: None,
            hover: None,
            scrollback_rect: Rect::default(),
            scrollback_text: Vec::new(),
            menu_rect: Rect::default(),
            modal_rect: Rect::default(),
            selection: None,
            selecting: false,
            prompt_selection: None,
            selecting_prompt: false,
            prompt_rect: Rect::default(),
            arg_options: Vec::new(),
            provider_options: Vec::new(),
            native_model_options: Vec::new(),
            opencode_model_options: std::sync::Arc::new(std::sync::Mutex::new(Vec::new())),
            bench_results: std::sync::Arc::new(std::sync::Mutex::new(load_bench_results())),
            benching: std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false)),
            provider_is_opencode: false,
            connected_providers: Vec::new(),
            native_ready_options: Vec::new(),
            pending_model: None,
            opencode_models_error: Arc::new(std::sync::Mutex::new(None)),
            opencode_key_expired: false,
            queue: Vec::new(),
            persona_name: config
                .agent
                .name
                .clone()
                .filter(|name| !name.trim().is_empty()),
            goal: None,
            ui_task_limits: (config.subagents.max_concurrent, config.subagents.max_depth),
            tasks_refresh_ticks: 0,
            item_offsets: Vec::new(),
            ui_timestamps: config.ui.timestamps,
            ui_thinking: config.ui.show_thinking,
            ui_tool_density: config.ui.tool_density.clone(),
            ui_thoughts_width: config.ui.max_thoughts_width,
            ui_notify: config.ui.notify.clone(),
            ui_notify_after: config.ui.notify_after,
            pending_notification: None,
            memory_enabled: config.memory.enabled,
            stream_tokens: 0,
            model_label: "model".into(),
            effort: None,
            backend_label: "native".into(),
            context_used: 0,
            context_size: config
                .models
                .default
                .as_ref()
                .and_then(|name| cachy_core::models::resolve(&config, Some(name)))
                .map(|m| m.context_window)
                .unwrap_or(256_000),
            output_tokens: 0,
            session_id: String::new(),
            session_title: String::new(),
            busy: false,
            streaming_text: None,
            streaming_reasoning: None,
            reasoning_started: None,
            spinner_frame: 0,
            turn_started: None,
            first_token_at: None,
            last_worked: None,
            status_message: None,
        }
    }

    /// Look up the clickable region under a terminal coordinate (last wins).
    pub fn hit_at(&self, column: u16, row: u16) -> Option<&HitAction> {
        self.hits
            .iter()
            .rev()
            .find(|hit| {
                column >= hit.area.x
                    && column < hit.area.x + hit.area.width
                    && row >= hit.area.y
                    && row < hit.area.y + hit.area.height
            })
            .map(|hit| &hit.action)
    }

    pub fn clear_hits(&mut self) {
        self.hits.clear();
    }

    pub fn register_hit(&mut self, area: ratatui::layout::Rect, action: HitAction) {
        self.hits.push(HitRegion { area, action });
    }

    /// Pointer moved the caret inside the prompt box.
    pub fn set_caret(&mut self, row: usize, column: usize) {
        let lines: Vec<&str> = self.editor.text.split('\n').collect();
        if row >= lines.len() {
            return;
        }
        let mut offset = 0usize;
        for line in lines.iter().take(row) {
            offset += line.len() + 1;
        }
        let line = lines[row];
        let mut column = column.min(line.chars().count());
        let mut byte = 0usize;
        for (i, ch) in line.char_indices() {
            if i >= column {
                break;
            }
            byte = i + ch.len_utf8();
        }
        let _ = &mut column;
        self.editor.cursor = offset + byte.min(line.len());
    }

    /// Window of list rows that keeps `selected` visible.
    pub fn visible_window(selected: usize, total: usize, viewport: usize) -> (usize, usize) {
        if viewport == 0 || total <= viewport {
            return (0, total);
        }
        let mut start = selected.saturating_sub(viewport / 2);
        if start + viewport > total {
            start = total - viewport;
        }
        (start, viewport)
    }

    /// Scroll the top modal when it is a text panel.
    pub fn scroll_modal(&mut self, delta: i32) {
        // The list modals need their own length to clamp the selection.
        let shortcut_len = match self.modals.last() {
            Some(Modal::Shortcuts(state)) => {
                shortcut_rows(&state.search, &state.expanded, &self.keys).len()
            }
            _ => 0,
        };
        let settings_len = match self.modals.last() {
            Some(Modal::Settings(state)) => settings_items_filtered(&state.search, self).len(),
            _ => 0,
        };
        if let Some(modal) = self.modals.last_mut() {
            let move_selection = |selected: &mut usize, len: usize| {
                let max = len.saturating_sub(1) as i32;
                *selected = (*selected as i32 + delta).clamp(0, max.max(0)) as usize;
            };
            match modal {
                Modal::Info { scroll, lines, .. } => {
                    let max = lines.len().saturating_sub(1) as i32;
                    *scroll = (*scroll as i32 + delta).clamp(0, max.max(0)) as u16;
                }
                Modal::Memory { scroll, lines } => {
                    let max = lines.len().saturating_sub(1) as i32;
                    *scroll = (*scroll as i32 + delta).clamp(0, max.max(0)) as u16;
                }
                Modal::Palette(state) => move_selection(&mut state.selected, state.items.len()),
                Modal::Shortcuts(state) => move_selection(&mut state.selected, shortcut_len),
                Modal::Settings(state) => move_selection(&mut state.selected, settings_len),
                Modal::Resume(state)
                | Modal::Sessions(state)
                | Modal::Model(state)
                | Modal::Provider(state)
                | Modal::SignIn(state)
                | Modal::Theme(state)
                | Modal::Persona(state)
                | Modal::Jump(state)
                | Modal::Workflow(state)
                | Modal::Search(state)
                | Modal::Plugin(state) => move_selection(&mut state.selected, state.items.len()),
                Modal::Tasks { rows, selected } => move_selection(selected, rows.len()),
                Modal::Quit | Modal::ModelFallback { .. } => {}
            }
        }
    }

    /// Click on a modal row: select it and activate it.
    pub fn click_modal_row(&mut self, index: usize) -> Action {
        let shortcut_len = match self.modals.last() {
            Some(Modal::Shortcuts(state)) => {
                shortcut_rows(&state.search, &state.expanded, &self.keys)
                    .len()
                    .saturating_sub(1)
            }
            _ => 0,
        };
        if let Some(modal) = self.modals.last_mut() {
            match modal {
                Modal::Palette(state) => state.selected = index,
                Modal::Shortcuts(state) => state.selected = index.min(shortcut_len),
                Modal::Settings(state) => state.selected = index,
                Modal::Resume(state)
                | Modal::Sessions(state)
                | Modal::Model(state)
                | Modal::Provider(state)
                | Modal::SignIn(state)
                | Modal::Theme(state)
                | Modal::Persona(state)
                | Modal::Jump(state)
                | Modal::Workflow(state)
                | Modal::Search(state)
                | Modal::Plugin(state) => state.selected = index,
                Modal::Tasks { rows, selected } => {
                    *selected = index.min(rows.len().saturating_sub(1))
                }
                Modal::ModelFallback { .. } => {}
                Modal::Info { scroll, .. } => {
                    *scroll = index as u16;
                    return Action::None;
                }
                Modal::Memory { scroll, .. } => {
                    *scroll = index as u16;
                    return Action::None;
                }
                Modal::Quit => return Action::None,
            }
        }
        self.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE))
    }

    /// Click on an approval option: select it, or confirm when already selected.
    pub fn click_approval(&mut self, index: usize) -> Action {
        let already = self
            .approval
            .as_ref()
            .map(|a| a.selected == index)
            .unwrap_or(false);
        if let Some(approval) = self.approval.as_mut() {
            approval.selected = index;
        }
        if already {
            return self.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
        }
        Action::None
    }

    /// Picker contents for `/personas`: `none` first, then every profile.
    pub fn persona_picker(&self, config: &Config) -> PickerState {
        let personas = cachy_core::personas::discover(&self.cwd, config);
        let mut items = vec!["none — use the default behaviour".to_string()];
        let mut values = vec![String::new()];
        for persona in &personas {
            items.push(format!("{}   ({})", persona.label(), persona.source));
            values.push(persona.name.clone());
        }
        PickerState {
            query: String::new(),
            selected: 0,
            items,
            values,
        }
    }

    /// Picker contents for `/workflow`.
    pub fn workflow_picker(&self) -> PickerState {
        let workflows = cachy_core::workflows::discover(&self.cwd);
        let mut items: Vec<String> = Vec::new();
        let mut values: Vec<String> = Vec::new();
        for workflow in &workflows {
            items.push(format!("{}   ({})", workflow.label(), workflow.source));
            values.push(workflow.name.clone());
        }
        if items.is_empty() {
            items.push("no workflows yet — add .cachy/workflows/<name>.md".to_string());
            values.push(String::new());
        }
        PickerState {
            query: String::new(),
            selected: 0,
            items,
            values,
        }
    }

    /// Picker contents for `/jump`: every user turn with its position.
    pub fn jump_picker(&self) -> PickerState {
        let mut items: Vec<String> = Vec::new();
        let mut values: Vec<String> = Vec::new();
        let mut index = 0usize;
        for (position, item) in self.items.iter().enumerate() {
            if let Item::User { text, ts } = item {
                index += 1;
                let first = text.lines().next().unwrap_or("");
                items.push(format!("{index:>3}  {first}   ({ts})"));
                values.push(position.to_string());
            }
        }
        if items.is_empty() {
            items.push("no turns yet".to_string());
            values.push(String::new());
        }
        PickerState {
            query: String::new(),
            selected: items.len().saturating_sub(1),
            items,
            values,
        }
    }

    /// Turn-by-turn view of the session: a bar scaled to each turn's duration,
    /// the message, how many tool calls it took and the totals.
    pub fn timeline_lines(&self) -> Vec<String> {
        struct Turn {
            label: String,
            seconds: f64,
            tools: usize,
            summary: String,
        }
        let mut turns: Vec<Turn> = Vec::new();
        let mut pending: Option<(String, usize)> = None;
        let mut tools = 0usize;
        for item in &self.items {
            match item {
                Item::User { text, ts } => {
                    if let Some((label, tools)) = pending.take() {
                        turns.push(Turn {
                            label,
                            seconds: 0.0,
                            tools,
                            summary: String::new(),
                        });
                    }
                    tools = 0;
                    let first = text.lines().next().unwrap_or("");
                    pending = Some((format!("{first}   ({ts})"), 0));
                }
                Item::ToolCall(_) => {
                    tools += 1;
                    if let Some((_, count)) = pending.as_mut() {
                        *count = tools;
                    }
                }
                Item::TurnSummary { text } => {
                    let seconds = text
                        .split("for ")
                        .nth(1)
                        .and_then(|rest| rest.split([' ', '·']).next())
                        .and_then(|value| {
                            let value = value.trim_end_matches('s');
                            value.parse::<f64>().ok()
                        })
                        .unwrap_or(0.0);
                    let (label, tools) = pending
                        .take()
                        .unwrap_or_else(|| ("(no message)".to_string(), 0));
                    turns.push(Turn {
                        label,
                        seconds,
                        tools,
                        summary: text.clone(),
                    });
                }
                _ => {}
            }
        }
        if let Some((label, tools)) = pending.take() {
            turns.push(Turn {
                label,
                seconds: 0.0,
                tools,
                summary: String::new(),
            });
        }
        if turns.is_empty() {
            return vec!["  no turns yet".to_string()];
        }
        let slowest = turns
            .iter()
            .map(|turn| turn.seconds)
            .fold(0.0_f64, f64::max)
            .max(0.1);
        let total: f64 = turns.iter().map(|turn| turn.seconds).sum();
        let mut lines = vec![format!(
            "  {} turns · {:.0}s total · slowest {:.0}s",
            turns.len(),
            total,
            slowest
        )];
        lines.push(String::new());
        for (index, turn) in turns.iter().enumerate() {
            const WIDTH: usize = 24;
            let filled = ((turn.seconds / slowest) * WIDTH as f64).round() as usize;
            let bar = format!(
                "{}{}",
                "▇".repeat(filled.min(WIDTH)),
                "·".repeat(WIDTH - filled.min(WIDTH))
            );
            lines.push(format!(
                "  {:>3}  {bar}  {:>4.0}s  {}",
                index + 1,
                turn.seconds,
                turn.label
            ));
            let tools = if turn.tools > 0 {
                format!(" · {} tool calls", turn.tools)
            } else {
                String::new()
            };
            if !turn.summary.is_empty() {
                lines.push(format!("          {}{tools}", turn.summary));
            } else if !tools.is_empty() {
                lines.push(format!("         {tools}"));
            }
        }
        lines
    }

    /// Newest assistant answer, for `/copy`.
    pub fn last_answer(&self) -> Option<String> {
        self.items.iter().rev().find_map(|item| match item {
            Item::AssistantText { text, .. } if !text.trim().is_empty() => Some(text.clone()),
            _ => None,
        })
    }

    /// Markdown transcript of what is on screen, for `/export`.
    pub fn export_markdown(&self) -> String {
        let mut out = String::new();
        out.push_str("# CachyCLI session\n\n");
        if !self.session_title.is_empty() {
            out.push_str(&format!("- Title: {}\n", self.session_title));
        }
        if !self.model_label.is_empty() {
            out.push_str(&format!("- Model: `{}`\n", self.model_label));
        }
        out.push_str(&format!("- Directory: `{}`\n\n---\n\n", self.cwd.display()));
        for item in &self.items {
            match item {
                Item::User { text, .. } => out.push_str(&format!("## User\n\n{text}\n\n")),
                Item::AssistantText { text, .. } => {
                    out.push_str(&format!("## Assistant\n\n{text}\n\n"))
                }
                Item::Reasoning { text, .. } => out.push_str(&format!(
                    "<details><summary>Thinking</summary>\n\n{text}\n\n</details>\n\n"
                )),
                Item::ToolCall(call) => {
                    out.push_str(&format!("- **{}** ({})", call.title, call.status));
                    if let Some(summary) = &call.summary {
                        out.push_str(&format!("\n\n```\n{summary}\n```\n"));
                    }
                    out.push('\n');
                }
                Item::Diff { path, diff, .. } => {
                    out.push_str(&format!("```diff\n# {path}\n{diff}\n```\n\n"))
                }
                Item::Todos { todos, .. } => {
                    for todo in todos {
                        out.push_str(&format!("- [{}] {}\n", todo.status, todo.content));
                    }
                    out.push('\n');
                }
                Item::Notice { text, .. } => out.push_str(&format!("> {text}\n\n")),
                Item::Error { text } => out.push_str(&format!("> **error:** {text}\n\n")),
                Item::TurnSummary { text } => out.push_str(&format!("_{text}_\n\n")),
                Item::Welcome => {}
            }
        }
        out
    }

    /// Text covered by the current selection (rows joined with newlines).
    pub fn selection_text(&self) -> Option<String> {
        let selection = self.selection?;
        let rect = self.scrollback_rect;
        if rect.width == 0 || self.scrollback_text.is_empty() {
            return None;
        }
        let (start, end) = selection.normalized();
        let mut rows: Vec<String> = Vec::new();
        for row in start.1..=end.1 {
            if row < rect.y || row >= rect.y + rect.height {
                continue;
            }
            let index = (row - rect.y) as usize;
            let Some(text) = self.scrollback_text.get(index) else {
                continue;
            };
            let chars: Vec<char> = text.chars().collect();
            let from = if row == start.1 {
                start.0.saturating_sub(rect.x) as usize
            } else {
                0
            };
            let to = if row == end.1 {
                (end.0.saturating_sub(rect.x) as usize + 1).min(chars.len())
            } else {
                chars.len()
            };
            let from = from.min(chars.len());
            let to = to.max(from);
            rows.push(
                chars[from..to]
                    .iter()
                    .collect::<String>()
                    .trim_end()
                    .to_string(),
            );
        }
        while rows.last().map(|r| r.is_empty()).unwrap_or(false) {
            rows.pop();
        }
        let joined = rows.join("\n");
        if joined.trim().is_empty() {
            None
        } else {
            Some(joined)
        }
    }

    pub fn clear_selection(&mut self) {
        self.selection = None;
        self.selecting = false;
        self.prompt_selection = None;
        self.selecting_prompt = false;
    }

    /// Text covered by the prompt selection.
    pub fn prompt_selection_text(&self) -> Option<String> {
        let (start, end) = self.prompt_selection?;
        let (start, end) = if start <= end {
            (start, end)
        } else {
            (end, start)
        };
        let text: Vec<char> = self.editor.text.chars().collect();
        if start >= text.len() || start == end {
            return None;
        }
        let slice: String = text[start..end.min(text.len())].iter().collect();
        if slice.trim().is_empty() {
            None
        } else {
            Some(slice)
        }
    }

    /// Called by the event loop right before a prompt leaves for the model.
    /// The message itself shows up in the transcript (the agent echoes it), so
    /// nothing is duplicated above the prompt box.
    pub fn note_sent(&mut self, _text: &str) {
        self.busy = true;
        self.stream_tokens = 0;
        self.first_token_at = None;
    }

    /// Accept the highlighted slash command (mouse or Tab).
    pub fn accept_slash_selection(&mut self) -> Action {
        self.accept_slash_candidate()
    }

    /// Accept the highlighted file candidate (mouse or Tab).
    pub fn accept_file_selection(&mut self) -> Action {
        self.accept_file_candidate()
    }

    /// The modal currently on screen.
    pub fn modal(&self) -> Option<&Modal> {
        self.modals.last()
    }

    /// True when a modal is open.
    pub fn has_modal(&self) -> bool {
        !self.modals.is_empty()
    }

    pub fn push_modal(&mut self, modal: Modal) {
        self.modals.push(modal);
    }

    pub fn close_modal(&mut self) {
        self.modals.pop();
    }

    fn take_modal(&mut self) -> Option<Modal> {
        self.modals.pop()
    }

    /// Temporarily remove the top modal (rendering needs `&mut App` while the
    /// modal itself is borrowed immutably).
    pub fn take_modal_for_render(&mut self) -> Option<Modal> {
        self.modals.pop()
    }

    pub fn set_status(&mut self, message: impl Into<String>) {
        self.status_message = Some((message.into(), std::time::Instant::now()));
    }

    pub fn clear_welcome(&mut self) {
        if matches!(self.items.first(), Some(Item::Welcome)) {
            self.items.remove(0);
        }
    }

    pub fn push_item(&mut self, item: Item) {
        self.clear_welcome();
        self.items.push(item);
        self.scroll_from_bottom = 0;
    }

    pub fn scroll_up(&mut self, lines: usize) {
        self.clear_selection();
        let max = self.total_lines.saturating_sub(self.viewport_lines);
        self.scroll_from_bottom = (self.scroll_from_bottom + lines).min(max);
    }

    pub fn scroll_down(&mut self, lines: usize) {
        self.clear_selection();
        self.scroll_from_bottom = self.scroll_from_bottom.saturating_sub(lines);
    }

    pub fn scroll_to_bottom(&mut self) {
        self.scroll_from_bottom = 0;
    }

    /// Apply an agent event to the UI state.
    pub fn apply_agent_event(&mut self, event: AgentEvent) {
        match event {
            AgentEvent::Started { session_id, model } => {
                self.session_id = session_id;
                self.model_label = model;
            }
            AgentEvent::UserMessage { text } => {
                self.push_item(Item::User {
                    text,
                    ts: chrono::Local::now().format("%-I:%M %p").to_string(),
                });
            }
            AgentEvent::TextDelta(delta) => {
                self.busy = true;
                if self.first_token_at.is_none() {
                    self.first_token_at = Some(std::time::Instant::now());
                }
                // Thinking is over once the answer starts: keep it as a block.
                if let Some(reasoning) = self.streaming_reasoning.take() {
                    if !reasoning.trim().is_empty() && self.ui_thinking {
                        let seconds = self
                            .reasoning_started
                            .take()
                            .map(|at| at.elapsed().as_secs_f64());
                        self.push_item(Item::Reasoning {
                            text: reasoning,
                            expanded: false,
                            cache: RenderCache::default(),
                            seconds,
                        });
                    }
                }
                match self.streaming_text.as_mut() {
                    Some(text) => text.push_str(&delta),
                    None => self.streaming_text = Some(delta),
                }
            }
            AgentEvent::ReasoningDelta(delta) => {
                self.busy = true;
                if self.first_token_at.is_none() {
                    self.first_token_at = Some(std::time::Instant::now());
                }
                if self.reasoning_started.is_none() {
                    self.reasoning_started = Some(std::time::Instant::now());
                }
                match self.streaming_reasoning.as_mut() {
                    Some(text) => text.push_str(&delta),
                    None => self.streaming_reasoning = Some(delta),
                }
            }
            AgentEvent::MessageComplete { text } => {
                if !text.trim().is_empty() {
                    self.push_item(Item::AssistantText {
                        text,
                        cache: RenderCache::default(),
                    });
                }
                self.streaming_text = None;
            }
            AgentEvent::ReasoningComplete { text } => {
                if !text.trim().is_empty() && self.ui_thinking {
                    let seconds = self
                        .reasoning_started
                        .take()
                        .map(|at| at.elapsed().as_secs_f64());
                    self.push_item(Item::Reasoning {
                        text,
                        expanded: false,
                        cache: RenderCache::default(),
                        seconds,
                    });
                }
            }
            AgentEvent::ToolCall {
                id,
                name,
                title,
                kind,
                arguments,
            } => {
                self.busy = true;
                if self.first_token_at.is_none() {
                    self.first_token_at = Some(std::time::Instant::now());
                }
                self.running_tool = Some(if title.is_empty() {
                    name.clone()
                } else {
                    title.clone()
                });
                // Collapse prior completed tool calls into a single summary is
                // intentionally not done: each call stays visible, matching the
                // streaming layout.
                self.push_item(Item::ToolCall(ToolCallItem {
                    id,
                    name,
                    title,
                    kind,
                    arguments,
                    status: "pending".into(),
                    summary: None,
                    meta: None,
                    started: std::time::Instant::now(),
                    elapsed: None,
                    cache: RenderCache::default(),
                }));
            }
            AgentEvent::ToolCallUpdate {
                id,
                status,
                title,
                summary,
                meta,
            } => {
                if status != "in_progress" && status != "running" {
                    self.running_tool = None;
                }
                let mut pending_diff: Option<(String, String)> = None;
                if let Some(Item::ToolCall(call)) = self
                    .items
                    .iter_mut()
                    .rev()
                    .find(|item| matches!(item, Item::ToolCall(c) if c.id == id))
                {
                    if status == "completed" || status == "failed" || status == "killed" {
                        call.elapsed = Some(call.started.elapsed().as_secs_f64());
                    }
                    call.status = status;
                    if title.is_some() {
                        call.title = title.unwrap_or_default();
                    }
                    if summary.is_some() {
                        call.summary = summary;
                    }
                    if meta.is_some() {
                        call.meta = meta;
                    }
                    call.cache = RenderCache::default();
                    // Show diffs from edit tools as their own block.
                    if let Some(meta) = &call.meta {
                        if let Some(diff) = meta.get("diff").and_then(|v| v.as_str()) {
                            let path = meta
                                .get("path")
                                .and_then(|v| v.as_str())
                                .unwrap_or("file")
                                .to_string();
                            pending_diff = Some((path, diff.to_string()));
                        }
                    }
                }
                if let Some((path, diff)) = pending_diff {
                    self.items.push(Item::Diff {
                        path,
                        diff,
                        cache: RenderCache::default(),
                    });
                }
            }
            AgentEvent::PermissionRequest {
                id,
                tool,
                title,
                detail,
                options,
            } => {
                self.approval = Some(ApprovalState {
                    id,
                    tool,
                    title,
                    detail,
                    options,
                    selected: 2.min(3),
                    feedback: String::new(),
                });
            }
            AgentEvent::Todos(todos) => {
                self.todos = todos.clone();
                if !todos.is_empty() {
                    self.items.push(Item::Todos {
                        todos,
                        cache: RenderCache::default(),
                    });
                }
            }
            AgentEvent::Plan(plan) => {
                self.plan = plan;
            }
            AgentEvent::Usage {
                context_used,
                context_size,
                output_tokens,
                ..
            } => {
                self.stream_tokens = output_tokens;
                self.context_used = context_used;
                if context_size > 0 {
                    self.context_size = context_size;
                }
                self.output_tokens = output_tokens;
            }
            AgentEvent::TurnEnd {
                stop_reason,
                duration_secs,
            } => {
                self.busy = false;
                self.running_tool = None;
                self.stream_tokens = 0;
                let turn_started = self.turn_started;
                // Backends that only stream deltas (opencode CLI, ACP) never
                // emit a message-complete event; persist the streamed text.
                if let Some(text) = self.streaming_text.take() {
                    if !text.trim().is_empty() {
                        self.push_item(Item::AssistantText {
                            text,
                            cache: RenderCache::default(),
                        });
                    }
                }
                if let Some(reasoning) = self.streaming_reasoning.take() {
                    if !reasoning.trim().is_empty() && self.ui_thinking {
                        let seconds = self
                            .reasoning_started
                            .take()
                            .map(|at| at.elapsed().as_secs_f64());
                        self.push_item(Item::Reasoning {
                            text: reasoning,
                            expanded: false,
                            cache: RenderCache::default(),
                            seconds,
                        });
                    }
                }
                self.turn_started = None;
                // A message typed during the turn goes out now (Enter queues,
                // exactly like the reference's "· N queued" behaviour).
                if !self.queue.is_empty() {
                    self.pending_action = Some(Action::SendQueued);
                }
                self.last_worked = Some(duration_secs);
                let ttft = match (self.first_token_at, turn_started) {
                    (Some(first), Some(started)) => {
                        let secs = first.duration_since(started).as_secs_f64();
                        (secs > 0.5).then_some(secs)
                    }
                    _ => None,
                };
                let label = match stop_reason {
                    cachy_core::message::StopReason::Cancelled => "Stopped",
                    cachy_core::message::StopReason::MaxTokens => "Stopped (max tokens)",
                    cachy_core::message::StopReason::MaxTurns => "Stopped (max turns)",
                    cachy_core::message::StopReason::Refusal => "Stopped (refusal)",
                    cachy_core::message::StopReason::Error => "Stopped (error)",
                    _ => "Worked",
                };
                let summary = match ttft {
                    Some(secs) => format!(
                        "{label} for {} · first token {}",
                        format_duration(duration_secs),
                        format_duration(secs)
                    ),
                    None => format!("{label} for {}", format_duration(duration_secs)),
                };
                self.first_token_at = None;
                // A turn you walked away from is worth a bell; one you
                // cancelled yourself is not.
                if !matches!(stop_reason, cachy_core::message::StopReason::Cancelled) {
                    self.maybe_notify(duration_secs, &summary);
                }
                self.push_item(Item::TurnSummary { text: summary });
            }
            AgentEvent::BackgroundFinished {
                task_id,
                description,
                status,
            } => {
                self.push_item(Item::Notice {
                    text: format!("background task {task_id} ({description}) {status}"),
                    kind: NoticeKind::Info,
                });
            }
            AgentEvent::Info(message) => {
                self.set_status(message.clone());
                // Messages coming from providers carry their own severity words.
                let lower = message.to_lowercase();
                let kind = if lower.contains("error") || lower.contains("failed") {
                    NoticeKind::Error
                } else if lower.contains("warning")
                    || lower.contains("not installed")
                    || lower.contains("needs")
                {
                    NoticeKind::Warning
                } else {
                    NoticeKind::Info
                };
                self.push_item(Item::Notice {
                    text: message,
                    kind,
                });
            }
            AgentEvent::Error(message) => {
                self.busy = false;
                let expired = {
                    let lower = message.to_lowercase();
                    lower.contains("api_key_expired")
                        || lower.contains("api key has expired")
                        || lower.contains("authentication_error")
                };
                self.push_item(Item::Error { text: message });
                if expired && self.provider_is_opencode {
                    self.opencode_key_expired = true;
                    // Offer the model that works without a key, so a dead
                    // credential never leaves the user stuck.
                    let current = self.model_label.clone();
                    // Prefer the ones measured to work (beam-free is broken
                    // upstream), then any other free model, then big-pickle.
                    const PREFERRED: [&str; 2] = [
                        "opencode/space-bunny-free",
                        "opencode/deepseek-v4.1-flash-free",
                    ];
                    let free = self
                        .opencode_model_options
                        .lock()
                        .map(|list| {
                            let ids: Vec<String> = list.iter().map(|(id, _)| id.clone()).collect();
                            PREFERRED
                                .iter()
                                .find(|want| ids.iter().any(|id| id == *want))
                                .map(|want| (*want).to_string())
                                .or_else(|| {
                                    ids.iter()
                                        .find(|id| {
                                            id.ends_with("-free") && !id.contains("beam-free")
                                        })
                                        .cloned()
                                })
                                .or_else(|| {
                                    ids.iter().find(|id| *id == "opencode/big-pickle").cloned()
                                })
                        })
                        .ok()
                        .flatten();
                    if let Some(model) = free {
                        if model != current
                            && !matches!(self.modal(), Some(Modal::ModelFallback { .. }))
                        {
                            self.push_modal(Modal::ModelFallback {
                                model,
                                reason: format!("{current} needs a valid credential"),
                            });
                        }
                    }
                    self.push_item(Item::Notice {
                        text: "opencode's key is expired, so any `opencode-go/*` model will fail. \
Run /login for the device login (browser, no key), or issue a key at \
opencode.ai/console (Personal API keys) and paste it in the key prompt, \
or switch to a free model with /model (the `opencode/*-free` ones and \
`opencode/big-pickle` need no key)."
                            .into(),
                        kind: NoticeKind::Info,
                    });
                    self.set_status("opencode key expired — try /model opencode/big-pickle");
                }
            }
        }
    }

    /// Handle a key press; returns an action for the event loop.
    pub fn handle_key(&mut self, key: KeyEvent) -> Action {
        // 0. Rebinding: the very next key becomes the new binding.
        if let Some(action) = self.capturing.clone() {
            if key.code == KeyCode::Esc {
                self.capturing = None;
                self.set_status("Rebinding cancelled");
                return Action::None;
            }
            let spec = format_key(&key);
            self.bind(&action, &spec);
            self.capturing = None;
            self.set_status(format!("Bound \"{action}\" to {spec}"));
            return Action::None;
        }
        // 1. Approval prompt has priority.
        if self.approval.is_some() {
            return self.handle_approval_key(key);
        }
        // 2. Modal dialogs (Esc returns to the previous level).
        if self.has_modal() {
            return self.handle_modal_key(key);
        }
        // 3. File picker.
        if self.file_picker_open {
            return self.handle_file_picker_key(key);
        }
        // 4. Slash menu (menu visible, navigation intercepts arrows).
        if self.slash_menu_open {
            if let Some(action) = self.handle_slash_menu_key(key) {
                return action;
            }
        }
        // 4.5 User keybindings (rebindable shortcuts).
        if let Some(binding) = self.keys.resolve(&key) {
            if let Some(action) = self.run_key_binding(&binding, key) {
                return action;
            }
        }
        // 5. Global shortcuts.
        let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
        match (key.code, ctrl, key.modifiers.contains(KeyModifiers::ALT)) {
            (KeyCode::Char('c'), true, _) => {
                if self.busy {
                    self.quit_armed = None;
                    return Action::Cancel;
                }
                if !self.editor.is_empty() {
                    self.editor.clear();
                    self.quit_armed = None;
                    return Action::None;
                }
                // Nothing to cancel or clear: ask once more before quitting.
                let armed = self
                    .quit_armed
                    .map(|at| at.elapsed() < std::time::Duration::from_secs(4))
                    .unwrap_or(false);
                if armed {
                    return Action::Quit;
                }
                self.quit_armed = Some(std::time::Instant::now());
                self.set_status("Press Ctrl+C again to quit");
                return Action::None;
            }
            (KeyCode::Char('d'), true, _) | (KeyCode::Char('q'), true, _) => {
                self.push_modal(Modal::Quit);
                return Action::None;
            }
            (KeyCode::Enter, true, _) => {
                // Ctrl+Enter inserts a newline, like the reference.
                self.editor.insert_char('\n');
                self.update_slash_menu();
                return Action::None;
            }
            (KeyCode::Esc, _, _) => {
                if self.busy {
                    self.quit_armed = None;
                    return Action::Cancel;
                }
                if self.quit_armed.is_some() {
                    self.quit_armed = None;
                    self.set_status("Quit cancelled");
                    return Action::None;
                }
                if self.selection.is_some() {
                    self.clear_selection();
                    return Action::None;
                }
                if !self.editor.is_empty() {
                    self.editor.clear();
                }
                return Action::None;
            }
            (KeyCode::Char('o'), true, _) => {
                self.always_approve = !self.always_approve;
                self.set_status(if self.always_approve {
                    "Always-approve mode is on"
                } else {
                    "Always-approve mode is off"
                });
                return Action::None;
            }
            (KeyCode::Char('t'), true, _) => {
                self.show_todos = !self.show_todos;
                return Action::None;
            }
            (KeyCode::Char('r'), true, _) => {
                return Action::RefreshResumeList;
            }
            (KeyCode::Char('p'), true, _) => {
                self.open_palette();
                return Action::None;
            }
            (KeyCode::Char('x'), true, _) | (KeyCode::Char('.'), true, _) => {
                self.push_modal(Modal::Shortcuts(ShortcutsState {
                    selected: 0,
                    expanded: vec![0],
                    search: String::new(),
                    searching: false,
                }));
                return Action::None;
            }
            (KeyCode::F(2), false, _) => {
                self.push_modal(Modal::Settings(self.default_settings()));
                return Action::None;
            }
            (KeyCode::Char(','), true, _) => {
                self.push_modal(Modal::Settings(self.default_settings()));
                return Action::None;
            }
            (KeyCode::Char('m'), true, _) => {
                self.multiline = !self.multiline;
                self.set_status(if self.multiline {
                    "Multiline input on (Enter inserts a newline)"
                } else {
                    "Multiline input off"
                });
                return Action::None;
            }
            (KeyCode::Char('u'), true, _) => {
                self.editor.undo();
                return Action::None;
            }
            (KeyCode::Char('y'), true, _) => {
                self.editor.redo();
                return Action::None;
            }
            (KeyCode::End, true, _) => {
                self.scroll_to_bottom();
                return Action::None;
            }
            (KeyCode::Char('w'), true, _) => {
                self.editor.delete_word();
                return Action::None;
            }
            (KeyCode::Char('a'), true, _) => {
                self.editor.move_home();
                return Action::None;
            }
            (KeyCode::Char('e'), true, _) => {
                self.editor.move_end();
                return Action::None;
            }
            (KeyCode::Char('v'), true, _) => {
                // Bracketed paste arrives as an event; clipboard read via OSC52
                // is not supported by every terminal, so show a hint.
                self.set_status("Paste with the terminal shortcut (Ctrl+Shift+V / Cmd+V)");
                return Action::None;
            }
            (KeyCode::BackTab, _, _) => {
                self.mode = self.mode.next();
                if self.mode == UiMode::AlwaysApprove {
                    self.always_approve = true;
                }
                return Action::ModeChange(self.mode);
            }
            (KeyCode::PageUp, _, _) => {
                self.scroll_up(10);
                return Action::None;
            }
            (KeyCode::PageDown, _, _) => {
                self.scroll_down(10);
                return Action::None;
            }
            _ => {}
        }

        // 6. Text editing.
        match key.code {
            KeyCode::Enter => {
                if key.modifiers.contains(KeyModifiers::SHIFT)
                    || key.modifiers.contains(KeyModifiers::ALT)
                {
                    self.editor.insert_char('\n');
                    self.update_slash_menu();
                    return Action::None;
                }
                if self.editor.multiline || self.multiline {
                    self.editor.insert_char('\n');
                    self.update_slash_menu();
                    return Action::None;
                }
                return self.submit_prompt();
            }
            KeyCode::Char(c) => {
                self.editor.insert_char(c);
                self.update_slash_menu();
                self.update_file_picker();
                return Action::None;
            }
            KeyCode::Backspace => {
                self.editor.backspace();
                self.update_slash_menu();
                self.update_file_picker();
                return Action::None;
            }
            KeyCode::Delete => {
                self.editor.delete();
                return Action::None;
            }
            KeyCode::Left => {
                self.editor.move_left();
                return Action::None;
            }
            KeyCode::Right => {
                self.editor.move_right();
                return Action::None;
            }
            KeyCode::Up => {
                self.editor.move_up();
                self.update_slash_menu();
                return Action::None;
            }
            KeyCode::Down => {
                self.editor.move_down();
                self.update_slash_menu();
                return Action::None;
            }
            KeyCode::Home => {
                self.editor.move_home();
                return Action::None;
            }
            KeyCode::End => {
                self.editor.move_end();
                return Action::None;
            }
            KeyCode::Tab => {
                // Complete the first file candidate or slash command.
                if self.file_picker_open {
                    return self.accept_file_candidate();
                }
                if self.slash_menu_open {
                    return self.accept_slash_candidate();
                }
                return Action::None;
            }
            _ => {}
        }
        Action::None
    }

    fn handle_approval_key(&mut self, key: KeyEvent) -> Action {
        let Some(approval) = self.approval.as_mut() else {
            return Action::None;
        };
        let option_count = approval.options.len().max(1);
        match key.code {
            KeyCode::Up => {
                approval.selected = approval.selected.saturating_sub(1);
                Action::None
            }
            KeyCode::Down | KeyCode::Tab => {
                approval.selected = (approval.selected + 1).min(option_count - 1);
                Action::None
            }
            KeyCode::Char(c @ '1'..='4') => {
                approval.selected = (c as usize - '1' as usize).min(option_count - 1);
                Action::None
            }
            KeyCode::Enter => {
                let choice = match approval.selected {
                    0 => ApprovalChoice::AllowAlways,
                    1 => ApprovalChoice::AllowSession,
                    2 => ApprovalChoice::Allow,
                    _ => ApprovalChoice::Reject,
                };
                let feedback = if !approval.feedback.trim().is_empty() {
                    Some(approval.feedback.clone())
                } else {
                    None
                };
                let id = approval.id.clone();
                self.approval = None;
                Action::Approve {
                    id,
                    choice,
                    feedback,
                }
            }
            KeyCode::Char('o') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                let id = approval.id.clone();
                self.approval = None;
                Action::Approve {
                    id,
                    choice: ApprovalChoice::AllowAlways,
                    feedback: None,
                }
            }
            KeyCode::Esc => {
                // Scrollback: dismiss the prompt view without answering.
                self.approval = None;
                Action::None
            }
            KeyCode::Char(c) => {
                if approval.selected == option_count - 1 {
                    approval.feedback.push(c);
                }
                Action::None
            }
            KeyCode::Backspace => {
                approval.feedback.pop();
                Action::None
            }
            _ => Action::None,
        }
    }

    fn handle_modal_key(&mut self, key: KeyEvent) -> Action {
        let modal = self.take_modal();
        match modal {
            Some(Modal::Quit) => match key.code {
                KeyCode::Enter | KeyCode::Char('y') | KeyCode::Char('Y') => Action::Quit,
                KeyCode::Esc | KeyCode::Char('n') | KeyCode::Char('N') => Action::None,
                // Ctrl+D / Ctrl+Q confirm too.
                KeyCode::Char('d') | KeyCode::Char('q')
                    if key.modifiers.contains(KeyModifiers::CONTROL) =>
                {
                    Action::Quit
                }
                _ => {
                    self.push_modal(Modal::Quit);
                    Action::None
                }
            },
            Some(Modal::Palette(mut state)) => match key.code {
                KeyCode::Esc => Action::None,
                KeyCode::Enter => {
                    if let Some(item) = state.items.get(state.selected) {
                        let command = item.command.clone();
                        let (cmd, args) = slash::split_command(&command);
                        return Action::Slash { command: cmd, args };
                    }
                    Action::None
                }
                KeyCode::Up => {
                    state.selected = state.selected.saturating_sub(1);
                    self.push_modal(Modal::Palette(state));
                    Action::None
                }
                KeyCode::Down => {
                    let max = state.items.len().saturating_sub(1);
                    state.selected = (state.selected + 1).min(max);
                    self.push_modal(Modal::Palette(state));
                    Action::None
                }
                KeyCode::Char(c) => {
                    state.query.push(c);
                    self.refresh_palette(&mut state);
                    self.push_modal(Modal::Palette(state));
                    Action::None
                }
                KeyCode::Backspace => {
                    state.query.pop();
                    self.refresh_palette(&mut state);
                    self.push_modal(Modal::Palette(state));
                    Action::None
                }
                _ => {
                    self.push_modal(Modal::Palette(state));
                    Action::None
                }
            },
            Some(Modal::Shortcuts(mut state)) => {
                if state.searching {
                    return match key.code {
                        KeyCode::Esc => {
                            state.searching = false;
                            state.search.clear();
                            state.selected = 0;
                            self.push_modal(Modal::Shortcuts(state));
                            Action::None
                        }
                        KeyCode::Enter => {
                            // Leave the field; the filter stays applied.
                            state.searching = false;
                            state.selected = 0;
                            self.push_modal(Modal::Shortcuts(state));
                            Action::None
                        }
                        KeyCode::Up => {
                            state.selected = state.selected.saturating_sub(1);
                            self.push_modal(Modal::Shortcuts(state));
                            Action::None
                        }
                        KeyCode::Down => {
                            let len =
                                shortcut_rows(&state.search, &state.expanded, &self.keys).len();
                            state.selected = (state.selected + 1).min(len.saturating_sub(1));
                            self.push_modal(Modal::Shortcuts(state));
                            Action::None
                        }
                        KeyCode::Backspace => {
                            state.search.pop();
                            state.selected = 0;
                            self.push_modal(Modal::Shortcuts(state));
                            Action::None
                        }
                        KeyCode::Char(c) => {
                            state.search.push(c);
                            state.selected = 0;
                            self.push_modal(Modal::Shortcuts(state));
                            Action::None
                        }
                        _ => {
                            self.push_modal(Modal::Shortcuts(state));
                            Action::None
                        }
                    };
                }
                match key.code {
                    KeyCode::Esc => Action::None,
                    KeyCode::Char('/') => {
                        state.searching = true;
                        state.search.clear();
                        state.selected = 0;
                        self.push_modal(Modal::Shortcuts(state));
                        Action::None
                    }
                    KeyCode::Up | KeyCode::Char('k') => {
                        state.selected = state.selected.saturating_sub(1);
                        self.push_modal(Modal::Shortcuts(state));
                        Action::None
                    }
                    KeyCode::Down | KeyCode::Char('j') => {
                        let len = shortcut_rows(&state.search, &state.expanded, &self.keys).len();
                        state.selected = (state.selected + 1).min(len.saturating_sub(1));
                        self.push_modal(Modal::Shortcuts(state));
                        Action::None
                    }
                    KeyCode::Enter | KeyCode::Char(' ') | KeyCode::Right => {
                        let rows = shortcut_rows(&state.search, &state.expanded, &self.keys);
                        let row = rows.get(state.selected).cloned();
                        match row {
                            Some(row) if row.action.is_some() => {
                                let action = row.action.clone().unwrap_or_default();
                                self.push_modal(Modal::Shortcuts(state));
                                self.capturing = Some(action.trim_start_matches('*').to_string());
                                self.set_status(format!(
                                    "Press the new key for \"{}\" · Esc cancels",
                                    row.label
                                ));
                                Action::None
                            }
                            Some(row) if row.label.is_empty() => {
                                if let Some(gi) =
                                    SHORTCUT_GROUPS.iter().position(|(g, _)| *g == row.group)
                                {
                                    if let Some(pos) = state.expanded.iter().position(|e| *e == gi)
                                    {
                                        state.expanded.remove(pos);
                                    } else {
                                        state.expanded.push(gi);
                                    }
                                }
                                self.push_modal(Modal::Shortcuts(state));
                                Action::None
                            }
                            _ => {
                                self.push_modal(Modal::Shortcuts(state));
                                Action::None
                            }
                        }
                    }
                    KeyCode::Left => {
                        let rows = shortcut_rows(&state.search, &state.expanded, &self.keys);
                        if let Some(row) = rows.get(state.selected) {
                            if let Some(gi) =
                                SHORTCUT_GROUPS.iter().position(|(g, _)| *g == row.group)
                            {
                                if let Some(pos) = state.expanded.iter().position(|e| *e == gi) {
                                    state.expanded.remove(pos);
                                }
                            }
                        }
                        self.push_modal(Modal::Shortcuts(state));
                        Action::None
                    }
                    KeyCode::Char('d') | KeyCode::Backspace => {
                        let rows = shortcut_rows(&state.search, &state.expanded, &self.keys);
                        if let Some(row) = rows.get(state.selected) {
                            if let Some(action) = row.action.as_deref() {
                                let action = action.trim_start_matches('*').to_string();
                                let default = Keymap::default_value(&action);
                                self.bind(&action, &default);
                                self.set_status(format!("\"{}\" reset to {}", row.label, default));
                            }
                        }
                        self.push_modal(Modal::Shortcuts(state));
                        Action::None
                    }
                    _ => {
                        self.push_modal(Modal::Shortcuts(state));
                        Action::None
                    }
                }
            }
            Some(Modal::ModelFallback { model, reason }) => match key.code {
                KeyCode::Esc | KeyCode::Char('n') | KeyCode::Char('N') => {
                    self.set_status("kept the current model");
                    Action::None
                }
                KeyCode::Up | KeyCode::Down => {
                    self.push_modal(Modal::ModelFallback { model, reason });
                    Action::None
                }
                KeyCode::Enter | KeyCode::Char('y') | KeyCode::Char('Y') => Action::SetModel(model),
                _ => {
                    self.push_modal(Modal::ModelFallback { model, reason });
                    Action::None
                }
            },
            Some(Modal::Settings(mut state)) => {
                if state.searching {
                    return match key.code {
                        KeyCode::Esc => {
                            state.searching = false;
                            state.search.clear();
                            state.selected = 0;
                            self.push_modal(Modal::Settings(state));
                            Action::None
                        }
                        KeyCode::Up => {
                            state.selected = state.selected.saturating_sub(1);
                            self.push_modal(Modal::Settings(state));
                            Action::None
                        }
                        KeyCode::Down => {
                            let len = settings_items_filtered(&state.search, self).len();
                            state.selected = (state.selected + 1).min(len.saturating_sub(1));
                            self.push_modal(Modal::Settings(state));
                            Action::None
                        }
                        KeyCode::Backspace => {
                            state.search.pop();
                            state.selected = 0;
                            self.push_modal(Modal::Settings(state));
                            Action::None
                        }
                        KeyCode::Char(c) => {
                            state.search.push(c);
                            state.selected = 0;
                            self.push_modal(Modal::Settings(state));
                            Action::None
                        }
                        _ => {
                            self.push_modal(Modal::Settings(state));
                            Action::None
                        }
                    };
                }
                let items = settings_items_filtered(&state.search, self);
                match key.code {
                    KeyCode::Esc => Action::None,
                    KeyCode::Char('/') => {
                        state.searching = true;
                        state.search.clear();
                        state.selected = 0;
                        self.push_modal(Modal::Settings(state));
                        Action::None
                    }
                    KeyCode::Up | KeyCode::Char('k') => {
                        state.selected = state.selected.saturating_sub(1);
                        self.push_modal(Modal::Settings(state));
                        Action::None
                    }
                    KeyCode::Down | KeyCode::Char('j') => {
                        state.selected = (state.selected + 1).min(items.len().saturating_sub(1));
                        self.push_modal(Modal::Settings(state));
                        Action::None
                    }
                    KeyCode::Enter | KeyCode::Char(' ') => {
                        // Keep the settings modal on the stack: some entries open
                        // a child modal and Esc must come back here.
                        let selected = state.selected;
                        self.push_modal(Modal::Settings(state));
                        if let Some(item) = items.get(selected) {
                            return self.apply_setting(item.key);
                        }
                        Action::None
                    }
                    KeyCode::Char('d') => Action::None,
                    _ => {
                        self.push_modal(Modal::Settings(state));
                        Action::None
                    }
                }
            }
            Some(Modal::Resume(mut state)) | Some(Modal::Sessions(mut state)) => match key.code {
                KeyCode::Esc if !state.query.is_empty() => {
                    state.query.clear();
                    self.filter_picker(&mut state);
                    self.push_modal(Modal::Resume(state));
                    Action::None
                }
                KeyCode::Esc => Action::None,
                KeyCode::Up => {
                    state.selected = state.selected.saturating_sub(1);
                    self.push_modal(Modal::Resume(state));
                    Action::None
                }
                KeyCode::Down => {
                    let max = state.items.len().saturating_sub(1);
                    state.selected = (state.selected + 1).min(max);
                    self.push_modal(Modal::Resume(state));
                    Action::None
                }
                KeyCode::Enter => {
                    if let Some(value) = state.values.get(state.selected) {
                        let value = value.clone();
                        return Action::Resume(value);
                    }
                    Action::None
                }
                KeyCode::Char('/') if state.query.is_empty() => {
                    self.push_modal(Modal::Resume(state));
                    Action::None
                }
                KeyCode::Char(c) => {
                    state.query.push(c);
                    self.filter_picker(&mut state);
                    self.push_modal(Modal::Resume(state));
                    Action::None
                }
                KeyCode::Backspace => {
                    state.query.pop();
                    self.filter_picker(&mut state);
                    self.push_modal(Modal::Resume(state));
                    Action::None
                }
                _ => {
                    self.push_modal(Modal::Resume(state));
                    Action::None
                }
            },
            // The same picker as /provider, except that Enter signs in.
            Some(Modal::SignIn(mut state)) => match key.code {
                KeyCode::Esc if !state.query.is_empty() => {
                    state.query.clear();
                    self.filter_picker(&mut state);
                    self.push_modal(Modal::SignIn(state));
                    Action::None
                }
                KeyCode::Esc => Action::None,
                KeyCode::Up => {
                    state.selected = state.selected.saturating_sub(1);
                    self.push_modal(Modal::SignIn(state));
                    Action::None
                }
                KeyCode::Down => {
                    let max = state.items.len().saturating_sub(1);
                    state.selected = (state.selected + 1).min(max);
                    self.push_modal(Modal::SignIn(state));
                    Action::None
                }
                KeyCode::Enter => {
                    if let Some(value) = state.values.get(state.selected) {
                        let value = value.clone();
                        return Action::SignInProvider(value);
                    }
                    Action::None
                }
                KeyCode::Char('/') if state.query.is_empty() => {
                    self.push_modal(Modal::SignIn(state));
                    Action::None
                }
                KeyCode::Char(c) => {
                    state.query.push(c);
                    self.filter_picker(&mut state);
                    self.push_modal(Modal::SignIn(state));
                    Action::None
                }
                KeyCode::Backspace => {
                    state.query.pop();
                    self.filter_picker(&mut state);
                    self.push_modal(Modal::SignIn(state));
                    Action::None
                }
                _ => {
                    self.push_modal(Modal::SignIn(state));
                    Action::None
                }
            },
            Some(Modal::Provider(mut state)) => match key.code {
                KeyCode::Esc if !state.query.is_empty() => {
                    state.query.clear();
                    self.filter_picker(&mut state);
                    self.push_modal(Modal::Provider(state));
                    Action::None
                }
                KeyCode::Esc => Action::None,
                KeyCode::Up => {
                    state.selected = state.selected.saturating_sub(1);
                    self.push_modal(Modal::Provider(state));
                    Action::None
                }
                KeyCode::Down => {
                    let max = state.items.len().saturating_sub(1);
                    state.selected = (state.selected + 1).min(max);
                    self.push_modal(Modal::Provider(state));
                    Action::None
                }
                KeyCode::Enter => {
                    if let Some(value) = state.values.get(state.selected) {
                        let value = value.clone();
                        return Action::SetProvider(value);
                    }
                    Action::None
                }
                KeyCode::Char('/') if state.query.is_empty() => {
                    self.push_modal(Modal::Provider(state));
                    Action::None
                }
                KeyCode::Char(c) => {
                    state.query.push(c);
                    self.filter_picker(&mut state);
                    self.push_modal(Modal::Provider(state));
                    Action::None
                }
                KeyCode::Backspace => {
                    state.query.pop();
                    self.filter_picker(&mut state);
                    self.push_modal(Modal::Provider(state));
                    Action::None
                }
                _ => {
                    self.push_modal(Modal::Provider(state));
                    Action::None
                }
            },
            Some(Modal::Model(mut state)) => match key.code {
                KeyCode::Esc if !state.query.is_empty() => {
                    state.query.clear();
                    self.filter_picker(&mut state);
                    self.push_modal(Modal::Model(state));
                    Action::None
                }
                KeyCode::Esc => Action::None,
                KeyCode::Up => {
                    state.selected = state.selected.saturating_sub(1);
                    self.push_modal(Modal::Model(state));
                    Action::None
                }
                KeyCode::Down => {
                    let max = state.items.len().saturating_sub(1);
                    state.selected = (state.selected + 1).min(max);
                    self.push_modal(Modal::Model(state));
                    Action::None
                }
                KeyCode::Enter => {
                    if let Some(value) = state.values.get(state.selected) {
                        let value = value.clone();
                        return Action::SetModel(value);
                    }
                    Action::None
                }
                KeyCode::Char('/') if state.query.is_empty() => {
                    // `/` starts the search; it never becomes part of the query.
                    self.push_modal(Modal::Model(state));
                    Action::None
                }
                KeyCode::Char(c) => {
                    state.query.push(c);
                    self.filter_picker(&mut state);
                    self.push_modal(Modal::Model(state));
                    Action::None
                }
                KeyCode::Backspace => {
                    state.query.pop();
                    self.filter_picker(&mut state);
                    self.push_modal(Modal::Model(state));
                    Action::None
                }
                _ => {
                    self.push_modal(Modal::Model(state));
                    Action::None
                }
            },
            Some(Modal::Plugin(mut state)) => match key.code {
                KeyCode::Esc if !state.query.is_empty() => {
                    state.query.clear();
                    self.push_modal(Modal::Plugin(state));
                    Action::None
                }
                KeyCode::Esc => Action::None,
                KeyCode::Up => {
                    state.selected = state.selected.saturating_sub(1);
                    self.push_modal(Modal::Plugin(state));
                    Action::None
                }
                KeyCode::Down => {
                    let max = state.items.len().saturating_sub(1);
                    state.selected = (state.selected + 1).min(max);
                    self.push_modal(Modal::Plugin(state));
                    Action::None
                }
                KeyCode::Enter => match state.values.get(state.selected).cloned() {
                    Some(name) if !name.is_empty() => Action::InstallPlugin(name),
                    _ => Action::None,
                },
                _ => {
                    self.push_modal(Modal::Plugin(state));
                    Action::None
                }
            },
            Some(Modal::Tasks { rows, selected }) => match key.code {
                KeyCode::Esc => Action::None,
                KeyCode::Up | KeyCode::Char('k') => {
                    self.push_modal(Modal::Tasks {
                        rows,
                        selected: selected.saturating_sub(1),
                    });
                    Action::None
                }
                KeyCode::Down | KeyCode::Char('j') => {
                    let max = rows.len().saturating_sub(1);
                    self.push_modal(Modal::Tasks {
                        rows,
                        selected: (selected + 1).min(max),
                    });
                    Action::None
                }
                KeyCode::Enter => match rows.get(selected) {
                    Some(row) => {
                        let mut lines: Vec<String> = vec![
                            format!("  {} · {} · {}", row.id, row.kind, row.description),
                            format!("  status: {}", row.status),
                            String::new(),
                        ];
                        lines.extend(row.output.lines().map(|line| format!("  {line}")));
                        if lines.len() <= 3 {
                            lines.push("  (no output yet)".to_string());
                        }
                        self.push_modal(Modal::Tasks { rows, selected });
                        self.open_info("Task output", lines);
                        Action::None
                    }
                    None => Action::None,
                },
                KeyCode::Char('x') => match rows.get(selected) {
                    Some(row) if !row.finished => Action::KillTask(row.id.clone()),
                    _ => Action::None,
                },
                KeyCode::Char('r') => Action::RefreshTasks,
                _ => {
                    self.push_modal(Modal::Tasks { rows, selected });
                    Action::None
                }
            },
            Some(Modal::Workflow(mut state)) => match key.code {
                KeyCode::Esc if !state.query.is_empty() => {
                    state.query.clear();
                    self.filter_picker(&mut state);
                    self.push_modal(Modal::Workflow(state));
                    Action::None
                }
                KeyCode::Esc => Action::None,
                KeyCode::Up => {
                    state.selected = state.selected.saturating_sub(1);
                    self.push_modal(Modal::Workflow(state));
                    Action::None
                }
                KeyCode::Down => {
                    let max = state.items.len().saturating_sub(1);
                    state.selected = (state.selected + 1).min(max);
                    self.push_modal(Modal::Workflow(state));
                    Action::None
                }
                KeyCode::Enter => match state.values.get(state.selected).cloned() {
                    Some(name) if !name.is_empty() => Action::RunWorkflow(name),
                    _ => Action::None,
                },
                KeyCode::Char('/') if state.query.is_empty() => {
                    self.push_modal(Modal::Workflow(state));
                    Action::None
                }
                KeyCode::Char(c) => {
                    state.query.push(c);
                    self.filter_picker(&mut state);
                    self.push_modal(Modal::Workflow(state));
                    Action::None
                }
                KeyCode::Backspace => {
                    state.query.pop();
                    self.filter_picker(&mut state);
                    self.push_modal(Modal::Workflow(state));
                    Action::None
                }
                _ => {
                    self.push_modal(Modal::Workflow(state));
                    Action::None
                }
            },
            Some(Modal::Search(mut state)) => match key.code {
                KeyCode::Esc if !state.query.is_empty() => {
                    state.query.clear();
                    self.push_modal(Modal::Search(state));
                    Action::None
                }
                KeyCode::Esc => Action::None,
                KeyCode::Up => {
                    state.selected = state.selected.saturating_sub(1);
                    self.push_modal(Modal::Search(state));
                    Action::None
                }
                KeyCode::Down => {
                    let max = state.items.len().saturating_sub(1);
                    state.selected = (state.selected + 1).min(max);
                    self.push_modal(Modal::Search(state));
                    Action::None
                }
                KeyCode::Enter => match state.values.get(state.selected).cloned() {
                    Some(id) if !id.is_empty() => Action::Resume(id),
                    _ => Action::None,
                },
                _ => {
                    self.push_modal(Modal::Search(state));
                    Action::None
                }
            },
            Some(Modal::Jump(mut state)) => match key.code {
                KeyCode::Esc if !state.query.is_empty() => {
                    state.query.clear();
                    self.filter_picker(&mut state);
                    self.push_modal(Modal::Jump(state));
                    Action::None
                }
                KeyCode::Esc => Action::None,
                KeyCode::Up => {
                    state.selected = state.selected.saturating_sub(1);
                    self.push_modal(Modal::Jump(state));
                    Action::None
                }
                KeyCode::Down => {
                    let max = state.items.len().saturating_sub(1);
                    state.selected = (state.selected + 1).min(max);
                    self.push_modal(Modal::Jump(state));
                    Action::None
                }
                KeyCode::Enter => match state
                    .values
                    .get(state.selected)
                    .and_then(|v| v.parse().ok())
                {
                    Some(index) => Action::JumpTo(index),
                    None => Action::None,
                },
                KeyCode::Char('/') if state.query.is_empty() => {
                    self.push_modal(Modal::Jump(state));
                    Action::None
                }
                KeyCode::Char(c) => {
                    state.query.push(c);
                    self.filter_picker(&mut state);
                    self.push_modal(Modal::Jump(state));
                    Action::None
                }
                KeyCode::Backspace => {
                    state.query.pop();
                    self.filter_picker(&mut state);
                    self.push_modal(Modal::Jump(state));
                    Action::None
                }
                _ => {
                    self.push_modal(Modal::Jump(state));
                    Action::None
                }
            },
            Some(Modal::Persona(mut state)) => match key.code {
                KeyCode::Esc if !state.query.is_empty() => {
                    state.query.clear();
                    self.filter_picker(&mut state);
                    self.push_modal(Modal::Persona(state));
                    Action::None
                }
                KeyCode::Esc => Action::None,
                KeyCode::Up => {
                    state.selected = state.selected.saturating_sub(1);
                    self.push_modal(Modal::Persona(state));
                    Action::None
                }
                KeyCode::Down => {
                    let max = state.items.len().saturating_sub(1);
                    state.selected = (state.selected + 1).min(max);
                    self.push_modal(Modal::Persona(state));
                    Action::None
                }
                KeyCode::Enter => {
                    if let Some(value) = state.values.get(state.selected).cloned() {
                        return Action::SetPersona(value);
                    }
                    Action::None
                }
                KeyCode::Char('/') if state.query.is_empty() => {
                    self.push_modal(Modal::Persona(state));
                    Action::None
                }
                KeyCode::Char(c) => {
                    state.query.push(c);
                    self.filter_picker(&mut state);
                    self.push_modal(Modal::Persona(state));
                    Action::None
                }
                KeyCode::Backspace => {
                    state.query.pop();
                    self.filter_picker(&mut state);
                    self.push_modal(Modal::Persona(state));
                    Action::None
                }
                _ => {
                    self.push_modal(Modal::Persona(state));
                    Action::None
                }
            },
            Some(Modal::Theme(mut state)) => match key.code {
                KeyCode::Esc if !state.query.is_empty() => {
                    state.query.clear();
                    self.filter_picker(&mut state);
                    self.push_modal(Modal::Theme(state));
                    Action::None
                }
                KeyCode::Esc => Action::None,
                KeyCode::Up => {
                    state.selected = state.selected.saturating_sub(1);
                    self.push_modal(Modal::Theme(state));
                    Action::None
                }
                KeyCode::Down => {
                    let max = state.items.len().saturating_sub(1);
                    state.selected = (state.selected + 1).min(max);
                    self.push_modal(Modal::Theme(state));
                    Action::None
                }
                KeyCode::Enter => {
                    if let Some(value) = state.values.get(state.selected) {
                        let value = value.clone();
                        return Action::SetTheme(value);
                    }
                    Action::None
                }
                KeyCode::Char('/') if state.query.is_empty() => {
                    self.push_modal(Modal::Theme(state));
                    Action::None
                }
                KeyCode::Char(c) => {
                    state.query.push(c);
                    self.filter_picker(&mut state);
                    self.push_modal(Modal::Theme(state));
                    Action::None
                }
                KeyCode::Backspace => {
                    state.query.pop();
                    self.filter_picker(&mut state);
                    self.push_modal(Modal::Theme(state));
                    Action::None
                }
                _ => {
                    self.push_modal(Modal::Theme(state));
                    Action::None
                }
            },
            Some(Modal::Info {
                title,
                lines,
                mut scroll,
            }) => match key.code {
                KeyCode::Esc | KeyCode::Enter | KeyCode::Char('q') => Action::None,
                KeyCode::Up | KeyCode::Char('k') => {
                    scroll = scroll.saturating_sub(1);
                    self.push_modal(Modal::Info {
                        title,
                        lines,
                        scroll,
                    });
                    Action::None
                }
                KeyCode::Down | KeyCode::Char('j') => {
                    scroll = (scroll + 1).min(lines.len().saturating_sub(1) as u16);
                    self.push_modal(Modal::Info {
                        title,
                        lines,
                        scroll,
                    });
                    Action::None
                }
                _ => {
                    self.push_modal(Modal::Info {
                        title,
                        lines,
                        scroll,
                    });
                    Action::None
                }
            },
            Some(Modal::Memory { lines, mut scroll }) => match key.code {
                KeyCode::Esc | KeyCode::Enter | KeyCode::Char('q') => Action::None,
                KeyCode::Up | KeyCode::Char('k') => {
                    scroll = scroll.saturating_sub(1);
                    self.push_modal(Modal::Memory { lines, scroll });
                    Action::None
                }
                KeyCode::Down | KeyCode::Char('j') => {
                    scroll = (scroll + 1).min(lines.len().saturating_sub(1) as u16);
                    self.push_modal(Modal::Memory { lines, scroll });
                    Action::None
                }
                _ => {
                    self.push_modal(Modal::Memory { lines, scroll });
                    Action::None
                }
            },
            None => Action::None,
        }
    }

    fn handle_file_picker_key(&mut self, key: KeyEvent) -> Action {
        match key.code {
            KeyCode::Esc => {
                self.file_picker_open = false;
                Action::None
            }
            KeyCode::Enter | KeyCode::Tab => self.accept_file_candidate(),
            KeyCode::Up => {
                self.file_selected = self.file_selected.saturating_sub(1);
                Action::None
            }
            KeyCode::Down => {
                let max = self.file_candidates.len().saturating_sub(1);
                self.file_selected = (self.file_selected + 1).min(max);
                Action::None
            }
            KeyCode::Char(c) => {
                self.editor.insert_char(c);
                self.update_file_picker();
                Action::None
            }
            KeyCode::Backspace => {
                self.editor.backspace();
                self.update_file_picker();
                Action::None
            }
            _ => self.handle_key_editing_fallback(key),
        }
    }

    fn handle_key_editing_fallback(&mut self, key: KeyEvent) -> Action {
        match key.code {
            KeyCode::Left => self.editor.move_left(),
            KeyCode::Right => self.editor.move_right(),
            KeyCode::Home => self.editor.move_home(),
            KeyCode::End => self.editor.move_end(),
            _ => {}
        }
        Action::None
    }

    fn handle_slash_menu_key(&mut self, key: KeyEvent) -> Option<Action> {
        let matches = self.slash_matches();
        match key.code {
            KeyCode::Up => {
                self.slash_selected = self.slash_selected.saturating_sub(1);
                Some(Action::None)
            }
            KeyCode::Down => {
                // The menu may be showing argument options rather than the
                // command list, so clamp against what is actually on screen.
                let max = self.menu_len().saturating_sub(1);
                self.slash_selected = (self.slash_selected + 1).min(max);
                Some(Action::None)
            }
            KeyCode::Esc => {
                self.slash_menu_open = false;
                Some(Action::None)
            }
            KeyCode::Tab => Some(self.accept_slash_candidate()),
            KeyCode::Enter => {
                if !self.arg_options.is_empty() {
                    // The argument menu completes the value; Enter on the value
                    // already in the input runs the command.
                    let complete = match self.arg_options.get(self.slash_selected) {
                        Some((value, _)) => {
                            self.current_argument().as_deref() != Some(value.as_str())
                        }
                        None => false,
                    };
                    if complete {
                        return Some(self.accept_slash_candidate());
                    }
                    return None;
                }
                if matches.is_empty() {
                    return None;
                }
                // Enter completes the command when the input is still the bare
                // command prefix; otherwise it runs it.
                let current = self.editor.text.trim().to_string();
                if let Some(cmd) = matches.get(self.slash_selected) {
                    if current != cmd.name && !current.contains(' ') {
                        self.editor.set_text(format!("{} ", cmd.name));
                        self.slash_menu_open = true;
                        return Some(Action::None);
                    }
                }
                None
            }
            _ => None,
        }
    }

    /// Rows the `/` menu shows right now (argument options or commands).
    pub fn menu_len(&self) -> usize {
        if !self.arg_options.is_empty() {
            self.arg_options.len()
        } else {
            self.slash_matches().len()
        }
    }

    /// Measured speed for a model, when `/bench` has seen it.
    fn bench_hint(&self, id: &str) -> Option<String> {
        let results = self.bench_results.lock().ok()?;
        let seconds = results.get(id)?;
        Some(if *seconds < 10.0 {
            format!("{seconds:.1}s")
        } else {
            format!("{seconds:.0}s")
        })
    }

    /// Note shown next to a model in the menus.
    pub fn model_hint(&self, id: &str) -> String {
        if !self.provider_is_opencode {
            return String::new();
        }
        let measured = self.bench_hint(id);
        if id.starts_with("opencode-go/") {
            if self.opencode_key_expired {
                return "· key expired".to_string();
            }
            return match measured {
                Some(time) => format!("· uses your opencode-go key · {time}"),
                None => "· uses your opencode-go key".to_string(),
            };
        }
        if id.ends_with("-free") || id == "opencode/big-pickle" {
            return match measured {
                Some(time) => format!("· free, no key · {time}"),
                None => "· free, no key".to_string(),
            };
        }
        match measured {
            Some(time) => format!("· {time}"),
            None => String::new(),
        }
    }

    /// Argument being typed after `/command `.
    fn current_argument(&self) -> Option<String> {
        let text = self.editor.text.trim().to_string();
        let (_, rest) = text.split_once(char::is_whitespace)?;
        Some(rest.trim().to_string())
    }

    /// The models of every provider you are signed in to, as
    /// `provider::model` — computed when asked, because the opencode list
    /// arrives in the background after startup.
    pub fn model_options(&self) -> Vec<(String, String)> {
        let mut out: Vec<(String, String)> = Vec::new();
        for kind in &self.connected_providers {
            match kind {
                BackendKind::Opencode => {
                    if let Ok(list) = self.opencode_model_options.lock() {
                        for (value, hint) in list.iter() {
                            out.push((format!("{}::{value}", kind.provider_name()), hint.clone()));
                        }
                    }
                }
                BackendKind::Native => {
                    out.extend(self.native_ready_options.iter().cloned());
                }
                BackendKind::Acp => {}
                other => {
                    for model in crate::providers::models_for(*other) {
                        out.push((
                            format!("{}::{model}", other.provider_name()),
                            format!("{} · {model}", other.provider_name()),
                        ));
                    }
                }
            }
        }
        out
    }

    pub fn slash_matches(&self) -> Vec<&'static slash::SlashCommand> {
        match self.editor.active_slash() {
            Some(prefix) => slash::filter(&prefix),
            None => Vec::new(),
        }
    }

    fn accept_slash_candidate(&mut self) -> Action {
        if !self.arg_options.is_empty() {
            if let Some((value, _)) = self.arg_options.get(self.slash_selected).cloned() {
                let text = self.editor.text.trim().to_string();
                let token = text
                    .split_once(char::is_whitespace)
                    .map(|(token, _)| token.to_string())
                    .unwrap_or(text);
                self.editor.set_text(format!("{token} {value}"));
                self.arg_options.clear();
                self.slash_menu_open = false;
                return Action::None;
            }
        }
        let matches = self.slash_matches();
        if let Some(cmd) = matches.get(self.slash_selected) {
            let current = self.editor.text.trim().to_string();
            if current == cmd.name || current == cmd.name.trim_end() {
                // Already complete: run it.
                self.slash_menu_open = false;
                let (command, args) = slash::split_command(&current);
                self.editor.clear();
                return Action::Slash { command, args };
            }
            self.editor.set_text(format!("{} ", cmd.name));
        }
        Action::None
    }

    pub fn update_slash_menu(&mut self) {
        self.slash_selected = 0;
        let Some(text) = self.editor.active_slash() else {
            self.slash_menu_open = false;
            self.arg_options.clear();
            self.update_file_picker();
            return;
        };
        // `/command arg` → show that command's options, like the reference.
        if let Some((token, rest)) = text.split_once(char::is_whitespace) {
            if let Some(cmd) = slash::resolve(token) {
                let partial = rest.trim().to_lowercase();
                let mut options: Vec<(String, String)> = Vec::new();
                if let Some(values) = slash::fixed_arg_options(cmd.name) {
                    options.extend(values.iter().map(|v| ((*v).to_string(), String::new())));
                } else if cmd.name == "/theme" {
                    options.extend(super::theme::Theme::all().iter().map(|theme| {
                        (
                            theme.name.to_lowercase().replace(' ', "-"),
                            theme.name.to_string(),
                        )
                    }));
                } else if cmd.name == "/provider" {
                    options.extend(self.provider_options.iter().cloned());
                } else if cmd.name == "/export" {
                    // Path completions for the file name.
                    let query = partial.trim_start_matches("~/").to_string();
                    options.extend(
                        self.file_index
                            .search(&query, false)
                            .into_iter()
                            .take(12)
                            .map(|path| (path.clone(), String::new())),
                    );
                } else if cmd.name == "/model" {
                    // The models of every provider you can use, as
                    // `provider::model` — choosing one switches provider too.
                    let merged = self.model_options();
                    if !merged.is_empty() {
                        options.extend(merged);
                    } else if self.provider_is_opencode {
                        // Nothing to offer yet: the list is still being read, or
                        // could not be read at all. Never another provider's.
                        options.push((String::new(), "reading opencode models…".into()));
                    }
                }
                if !options.is_empty() {
                    options.retain(|(value, hint)| {
                        partial.is_empty()
                            || value.to_lowercase().starts_with(&partial)
                            || value.to_lowercase().contains(&partial)
                            || hint.to_lowercase().contains(&partial)
                    });
                }
                self.arg_options = options;
                // Like the reference, preselect the value already in use.
                self.slash_selected = if partial.is_empty() && cmd.name == "/model" {
                    self.arg_options
                        .iter()
                        .position(|(value, _)| *value == self.model_label)
                        .unwrap_or(0)
                } else {
                    0
                };
                self.slash_menu_open = !self.arg_options.is_empty();
                self.update_file_picker();
                return;
            }
            self.arg_options.clear();
            self.slash_menu_open = false;
            self.update_file_picker();
            return;
        }
        self.arg_options.clear();
        self.slash_menu_open = true;
        self.update_file_picker();
    }

    pub fn update_file_picker(&mut self) {
        let active = self.editor.active_at_reference();
        match active {
            Some((_, query)) => {
                let (path_query, _range) =
                    super::picker::parse_line_range(query.trim_start_matches('!'));
                let include_hidden = query.starts_with('!');
                if path_query.is_empty() {
                    self.file_candidates = self
                        .file_index
                        .search("", include_hidden)
                        .into_iter()
                        .take(200)
                        .collect();
                } else {
                    self.file_candidates = self.file_index.search(&path_query, include_hidden);
                }
                self.file_selected = 0;
                self.file_picker_open = true;
                self.file_picker_query = query;
            }
            None => {
                self.file_picker_open = false;
                self.file_candidates.clear();
            }
        }
    }

    fn accept_file_candidate(&mut self) -> Action {
        if let Some(candidate) = self.file_candidates.get(self.file_selected).cloned() {
            if let Some((start, query)) = self.editor.active_at_reference() {
                let (_, range) = super::picker::parse_line_range(query.trim_start_matches('!'));
                let mut replacement = format!("@{candidate}");
                if let Some((a, b)) = range {
                    replacement = format!("@{candidate}:{a}-{b}");
                }
                let end = self.editor.cursor;
                let mut text = self.editor.text.clone();
                text.replace_range(start..end, &replacement);
                let new_cursor = start + replacement.len();
                self.editor.set_text(text);
                self.editor.cursor = new_cursor;
            }
        }
        self.file_picker_open = false;
        Action::None
    }

    /// Queue a message typed while a turn is running (Enter queues, like the
    /// reference: "· N queued, Enter to send now").
    pub fn queue_prompt(&mut self, text: String) {
        self.queue.push(text);
        self.editor.clear();
        self.slash_menu_open = false;
        self.file_picker_open = false;
        self.set_status(format!(
            "{} queued — sent when this turn ends",
            self.queue.len()
        ));
    }

    fn submit_prompt(&mut self) -> Action {
        let text = self.editor.value().trim().to_string();
        if text.is_empty() {
            return Action::None;
        }
        // Slash command?
        if text.starts_with('/') {
            if let Some(_cmd) = slash::resolve(&text) {
                let (command, args) = slash::split_command(&text);
                self.editor.remember(&text);
                self.editor.clear();
                self.slash_menu_open = false;
                self.file_picker_open = false;
                return Action::Slash { command, args };
            }
            // A bare `/word` that is not a command is a typo, not a prompt:
            // never send it to the model. Clear the input (the reference does the
            // same) so the next keystroke starts fresh instead of piling up.
            if !text.contains(char::is_whitespace) {
                self.set_status(format!("unknown command {text} — type / for the list"));
                self.editor.clear();
                self.slash_menu_open = false;
                self.arg_options.clear();
                return Action::None;
            }
        }
        self.editor.remember(&text);
        self.editor.clear();
        self.slash_menu_open = false;
        self.file_picker_open = false;
        Action::Send(text)
    }

    // ------------------------------------------------------------------
    // Modal constructors
    // ------------------------------------------------------------------

    pub fn open_palette(&mut self) {
        let mut state = PaletteState {
            query: String::new(),
            selected: 0,
            items: Vec::new(),
        };
        self.refresh_palette(&mut state);
        self.push_modal(Modal::Palette(state));
    }

    fn refresh_palette(&self, state: &mut PaletteState) {
        let query = state.query.to_lowercase();
        let mut items = Vec::new();
        for cmd in slash::COMMANDS {
            if !query.is_empty()
                && !cmd.name.contains(&query)
                && !cmd.description.to_lowercase().contains(&query)
            {
                continue;
            }
            let shortcut = match cmd.name {
                "/new" => "Ctrl+N".to_string(),
                "/resume" => "Ctrl+R".to_string(),
                "/settings" => "F2".to_string(),
                "/shortcuts" => "Ctrl+X".to_string(),
                "/exit" => "Ctrl+Q".to_string(),
                "/todos" => "Ctrl+T".to_string(),
                "/always-approve" => "Ctrl+O".to_string(),
                "/model" => "".to_string(),
                _ => String::new(),
            };
            items.push(PaletteItem {
                label: cmd.name.trim_start_matches('/').to_string(),
                shortcut,
                category: cmd.category.to_string(),
                command: cmd.name.to_string(),
            });
        }
        state.items = items;
        if state.selected >= state.items.len() {
            state.selected = state.items.len().saturating_sub(1);
        }
    }

    /// Update a binding and remember it so the event loop can persist it.
    pub fn bind(&mut self, action: &str, spec: &str) {
        self.keys.set(action, spec);
        self.keymap_edits
            .push((action.to_string(), spec.to_string()));
    }

    /// Runs a rebound action. `None` means "not a known action".
    fn run_key_binding(&mut self, action: &str, key: KeyEvent) -> Option<Action> {
        match action {
            "send" => {
                if self.editor.multiline || self.multiline {
                    self.editor.insert_char('\n');
                    self.update_slash_menu();
                    return Some(Action::None);
                }
                Some(self.submit_prompt())
            }
            "newline" => {
                self.editor.insert_char('\n');
                self.update_slash_menu();
                Some(Action::None)
            }
            "cancel" => {
                if self.busy {
                    self.quit_armed = None;
                    return Some(Action::Cancel);
                }
                if key.code == KeyCode::Esc && self.quit_armed.is_some() {
                    // Esc backs out of the quit confirmation.
                    self.quit_armed = None;
                    self.set_status("Quit cancelled");
                    return Some(Action::None);
                }
                if key.code == KeyCode::Esc && self.selection.is_some() {
                    self.clear_selection();
                    return Some(Action::None);
                }
                if !self.editor.is_empty() {
                    self.editor.clear();
                    self.quit_armed = None;
                    return Some(Action::None);
                }
                if key.code == KeyCode::Char('c') {
                    let armed = self
                        .quit_armed
                        .map(|at| at.elapsed() < std::time::Duration::from_secs(4))
                        .unwrap_or(false);
                    if armed {
                        return Some(Action::Quit);
                    }
                    self.quit_armed = Some(std::time::Instant::now());
                    self.set_status("Press Ctrl+C again to quit");
                }
                Some(Action::None)
            }
            "quit" => {
                self.push_modal(Modal::Quit);
                Some(Action::None)
            }
            "always_approve" => {
                self.always_approve = !self.always_approve;
                self.set_status(if self.always_approve {
                    "Always-approve mode is on"
                } else {
                    "Always-approve mode is off"
                });
                Some(Action::None)
            }
            "todos" => {
                self.show_todos = !self.show_todos;
                Some(Action::None)
            }
            "resume" => Some(Action::RefreshResumeList),
            "palette" => {
                self.open_palette();
                Some(Action::None)
            }
            "shortcuts" => {
                self.push_modal(Modal::Shortcuts(ShortcutsState::new()));
                Some(Action::None)
            }
            "settings" => {
                self.push_modal(Modal::Settings(self.default_settings()));
                Some(Action::None)
            }
            "multiline" => {
                self.multiline = !self.multiline;
                self.editor.multiline = self.multiline;
                self.set_status(if self.multiline {
                    "Multiline input on (Enter inserts a newline)"
                } else {
                    "Multiline input off"
                });
                Some(Action::None)
            }
            "undo" => {
                self.editor.undo();
                Some(Action::None)
            }
            "redo" => {
                self.editor.redo();
                Some(Action::None)
            }
            _ => None,
        }
    }

    /// Queue a notification for a turn that ran long enough to matter.
    fn maybe_notify(&mut self, seconds: f64, summary: &str) {
        if self.ui_notify == "off" || seconds < self.ui_notify_after as f64 {
            return;
        }
        let mut text = String::from("cachycli");
        if !self.session_title.trim().is_empty() {
            text.push_str(" · ");
            text.push_str(self.session_title.trim());
        }
        text.push_str(" · ");
        text.push_str(summary);
        self.pending_notification = Some(text);
    }

    fn default_settings(&self) -> SettingsState {
        SettingsState {
            selected: 0,
            search: String::new(),
            searching: false,
        }
    }

    fn apply_setting(&mut self, key: &str) -> Action {
        match key {
            "theme" => {
                let themes = Theme::all();
                let items: Vec<String> = themes.iter().map(|t| t.name.to_string()).collect();
                let values: Vec<String> = themes
                    .iter()
                    .map(|t| t.name.to_lowercase().replace(' ', "-"))
                    .collect();
                self.push_modal(Modal::Theme(PickerState {
                    query: String::new(),
                    selected: 0,
                    items,
                    values,
                }));
                Action::None
            }
            "todos" => {
                self.show_todos = !self.show_todos;
                if !super::persist_setting(
                    &self.cwd,
                    "ui",
                    "todo_panel",
                    if self.show_todos { "true" } else { "false" },
                ) {
                    self.set_status("could not write ~/.cachy/config.toml");
                }
                self.set_status(if self.show_todos {
                    "Todo panel on"
                } else {
                    "Todo panel off"
                });
                Action::None
            }
            "timestamps" => {
                self.ui_timestamps = !self.ui_timestamps;
                if !super::persist_setting(
                    &self.cwd,
                    "ui",
                    "timestamps",
                    if self.ui_timestamps { "true" } else { "false" },
                ) {
                    self.set_status("could not write ~/.cachy/config.toml");
                }
                self.set_status(if self.ui_timestamps {
                    "Timestamps on"
                } else {
                    "Timestamps off"
                });
                Action::None
            }
            "thinking" => {
                self.ui_thinking = !self.ui_thinking;
                if !super::persist_setting(
                    &self.cwd,
                    "ui",
                    "show_thinking",
                    if self.ui_thinking { "true" } else { "false" },
                ) {
                    self.set_status("could not write ~/.cachy/config.toml");
                }
                self.set_status(if self.ui_thinking {
                    "Thinking blocks on"
                } else {
                    "Thinking blocks off"
                });
                Action::None
            }
            "thoughts_width" => {
                const STEPS: [u16; 5] = [80, 100, 120, 160, 200];
                let current = STEPS
                    .iter()
                    .position(|width| *width == self.ui_thoughts_width)
                    .unwrap_or(2);
                self.ui_thoughts_width = STEPS[(current + 1) % STEPS.len()];
                if !super::persist_setting(
                    &self.cwd,
                    "ui",
                    "max_thoughts_width",
                    &self.ui_thoughts_width.to_string(),
                ) {
                    self.set_status("could not write ~/.cachy/config.toml");
                }
                self.set_status(format!("Thoughts width: {}", self.ui_thoughts_width));
                Action::None
            }
            "notify" => {
                self.ui_notify = match self.ui_notify.as_str() {
                    "off" => "bell".into(),
                    "bell" => "osc".into(),
                    _ => "off".into(),
                };
                if !super::persist_setting(
                    &self.cwd,
                    "ui",
                    "notify",
                    &format!("{:?}", self.ui_notify),
                ) {
                    self.set_status("could not write ~/.cachy/config.toml");
                }
                let how = match self.ui_notify.as_str() {
                    "bell" => "Bell".to_string(),
                    "osc" => "Desktop notification".to_string(),
                    _ => "Off".to_string(),
                };
                self.set_status(format!(
                    "Notify when a turn ends: {how} (after {}s)",
                    self.ui_notify_after
                ));
                Action::None
            }
            "tool_density" => {
                self.ui_tool_density = match self.ui_tool_density.as_str() {
                    "compact" => "normal".into(),
                    "normal" => "verbose".into(),
                    _ => "compact".into(),
                };
                if !super::persist_setting(
                    &self.cwd,
                    "ui",
                    "tool_density",
                    &format!("{:?}", self.ui_tool_density),
                ) {
                    self.set_status("could not write ~/.cachy/config.toml");
                }
                self.set_status(format!("Tool call density: {}", self.ui_tool_density));
                Action::None
            }
            "memory" => {
                self.memory_enabled = !self.memory_enabled;
                if !super::persist_setting(
                    &self.cwd,
                    "memory",
                    "enabled",
                    if self.memory_enabled { "true" } else { "false" },
                ) {
                    self.set_status("could not write ~/.cachy/config.toml");
                }
                self.set_status(if self.memory_enabled {
                    "Memory capture on (new turns)"
                } else {
                    "Memory capture off"
                });
                Action::None
            }
            "agentsmd" => {
                self.push_modal(Modal::Settings(self.default_settings()));
                Action::InitProject
            }
            "quit" => {
                self.push_modal(Modal::Quit);
                Action::None
            }
            _ => {
                self.push_modal(Modal::Settings(self.default_settings()));
                Action::None
            }
        }
    }

    fn filter_picker(&self, state: &mut PickerState) {
        // A stray leading slash (typed or pasted) is a search marker, not text.
        if state.query.starts_with('/') {
            state.query = state.query.trim_start_matches('/').to_string();
        }
        let query = state.query.to_lowercase();
        if query.is_empty() {
            return;
        }
        // Keep the original list to re-filter from: stored in values/items pair.
        let pairs: Vec<(String, String)> = state
            .items
            .iter()
            .cloned()
            .zip(state.values.iter().cloned())
            .filter(|(label, _)| label.to_lowercase().contains(&query))
            .collect();
        if !pairs.is_empty() {
            state.items = pairs.iter().map(|(l, _)| l.clone()).collect();
            state.values = pairs.into_iter().map(|(_, v)| v).collect();
            state.selected = 0;
        }
    }

    pub fn open_info(&mut self, title: impl Into<String>, lines: Vec<String>) {
        self.push_modal(Modal::Info {
            title: title.into(),
            lines,
            scroll: 0,
        });
    }
}

/// Settings entries shown in the settings modal.
pub struct SettingItem {
    pub key: &'static str,
    pub label: &'static str,
    pub value: String,
}

/// Where `/bench` keeps its measurements.
pub fn bench_file() -> std::path::PathBuf {
    cachy_core::paths::cachy_home().join("bench.json")
}

/// Measured time-to-first-token per model, loaded from disk.
pub fn load_bench_results() -> std::collections::BTreeMap<String, f64> {
    std::fs::read_to_string(bench_file())
        .ok()
        .and_then(|text| serde_json::from_str(&text).ok())
        .unwrap_or_default()
}

/// Persist the measurements (called after every model, so a partial run helps).
pub fn save_bench_results(results: &std::collections::BTreeMap<String, f64>) {
    if let Ok(text) = serde_json::to_string_pretty(results) {
        let _ = std::fs::write(bench_file(), text);
    }
}

pub fn settings_items(app: &App) -> Vec<SettingItem> {
    let on_off = |value: bool| if value { "on" } else { "off" }.to_string();
    let density = match app.ui_tool_density.as_str() {
        "normal" => "Normal",
        "verbose" => "Verbose",
        _ => "Compact",
    };
    vec![
        SettingItem {
            key: "tool_density",
            label: "Tool call density",
            value: format!("{density}  ›"),
        },
        SettingItem {
            key: "theme",
            label: "Theme",
            value: format!("{}  ›", app.theme.name),
        },
        SettingItem {
            key: "timestamps",
            label: "Timestamps",
            value: format!("{}  ›", if app.ui_timestamps { "On" } else { "Off" }),
        },
        SettingItem {
            key: "thinking",
            label: "Show thinking blocks",
            value: on_off(app.ui_thinking),
        },
        SettingItem {
            key: "thoughts_width",
            label: "Max thoughts width",
            value: app.ui_thoughts_width.to_string(),
        },
        SettingItem {
            key: "todos",
            label: "Todo panel",
            value: on_off(app.show_todos),
        },
        SettingItem {
            key: "notify",
            label: "Notify when a turn ends",
            value: match app.ui_notify.as_str() {
                "bell" => "Bell  ›".to_string(),
                "osc" => "Desktop  ›".to_string(),
                _ => "Off  ›".to_string(),
            },
        },
        SettingItem {
            key: "memory",
            label: "Memory",
            value: on_off(app.memory_enabled),
        },
        SettingItem {
            key: "agentsmd",
            label: "Save project settings",
            value: "AGENTS.md  ›".into(),
        },
        SettingItem {
            key: "quit",
            label: "Quit cachycli",
            value: "".into(),
        },
    ]
}

/// Shortcut groups for the keyboard shortcuts modal.
/// Label -> rebindable action id.
pub const SHORTCUT_ACTION_IDS: &[(&str, &str)] = &[
    ("Send", "send"),
    ("Newline", "newline"),
    ("Cancel turn", "cancel"),
    ("Quit", "quit"),
    ("Command palette", "palette"),
    ("Keyboard shortcuts", "shortcuts"),
    ("Open the settings modal", "settings"),
    ("Multiline input", "multiline"),
    ("Undo input change", "undo"),
    ("Redo input change", "redo"),
    ("Resume session picker", "resume"),
    ("Toggle todo panel", "todos"),
    ("Toggle always-approve", "always_approve"),
];

pub fn action_for_label(label: &str) -> Option<&'static str> {
    SHORTCUT_ACTION_IDS
        .iter()
        .find(|(l, _)| *l == label)
        .map(|(_, a)| *a)
}

/// One rendered row of the shortcuts modal.
#[derive(Debug, Clone)]
pub struct ShortcutRow {
    pub group: String,
    /// Empty for group headers.
    pub label: String,
    pub keys: String,
    /// Set when the row can be rebound.
    pub action: Option<String>,
}

/// Flat rows for the shortcuts modal, honouring the search filter.
pub fn shortcut_rows(search: &str, expanded: &[usize], keys: &Keymap) -> Vec<ShortcutRow> {
    let query = search.to_lowercase();
    let mut rows: Vec<ShortcutRow> = Vec::new();
    for (index, (group, entries)) in SHORTCUT_GROUPS.iter().enumerate() {
        let matches: Vec<(&str, &str)> = entries
            .iter()
            .filter(|(label, key)| {
                query.is_empty()
                    || label.to_lowercase().contains(&query)
                    || key.to_lowercase().contains(&query)
                    || group.to_lowercase().contains(&query)
            })
            .map(|(l, k)| (*l, *k))
            .collect();
        if matches.is_empty() {
            continue;
        }
        rows.push(ShortcutRow {
            group: (*group).to_string(),
            label: String::new(),
            keys: String::new(),
            action: None,
        });
        let open = !query.is_empty() || expanded.contains(&index);
        if !open {
            continue;
        }
        for (label, key) in matches {
            let action = action_for_label(label);
            let shown = match action {
                Some(a) => keys.keys_for(a),
                None => key.to_string(),
            };
            rows.push(ShortcutRow {
                group: (*group).to_string(),
                label: label.to_string(),
                keys: shown,
                action: action.map(|a| a.to_string()),
            });
        }
    }
    rows
}

/// Settings entries matching the search field.
pub fn settings_items_filtered(query: &str, app: &App) -> Vec<SettingItem> {
    let query = query.to_lowercase();
    let all = settings_items(app);
    if query.is_empty() {
        return all;
    }
    all.into_iter()
        .filter(|item| {
            item.label.to_lowercase().contains(&query) || item.value.to_lowercase().contains(&query)
        })
        .collect()
}

pub const SHORTCUT_GROUPS: &[(&str, &[(&str, &str)])] = &[
    (
        "Essentials",
        &[
            ("Send", "Enter"),
            ("Newline", "Shift+Enter / Alt+Enter"),
            ("Cancel turn", "Ctrl+c / Esc"),
            ("Cycle mode (Normal / Plan / Always-approve)", "Shift+Tab"),
            ("Quit", "Ctrl+q / Ctrl+d"),
            ("Command palette", "Ctrl+p"),
            ("Keyboard shortcuts", "Ctrl+x / Ctrl+."),
            ("Open the settings modal", "F2 / Ctrl+,"),
        ],
    ),
    (
        "Input",
        &[
            ("Multiline input", "Ctrl+m"),
            ("Undo input change", "Ctrl+u"),
            ("Redo input change", "Ctrl+y"),
            ("Delete word", "Ctrl+w"),
            ("History (previous / next)", "Up / Down"),
            ("Home / End of line", "Ctrl+a / Ctrl+e"),
        ],
    ),
    (
        "Conversation Actions",
        &[
            ("Resume session picker", "Ctrl+r"),
            ("Toggle todo panel", "Ctrl+t"),
            ("Toggle always-approve", "Ctrl+o"),
            ("Scroll scrollback", "PgUp / PgDn, wheel"),
            ("Jump to the newest message", "Ctrl+End"),
        ],
    ),
    (
        "Session",
        &[
            ("New session", "/new"),
            ("Compact history", "/compact"),
            ("Switch model", "/model"),
            ("Export transcript", "/export"),
        ],
    ),
];

/// Every session's turns, newest first: when, how long, how much work and
/// where it ran. The current session's own view is `timeline_lines`.
pub fn activity_lines(turns: &[cachy_core::activity::TurnRecord]) -> Vec<String> {
    if turns.is_empty() {
        return vec!["  no turns recorded yet".to_string()];
    }
    let slowest = turns.iter().map(|t| t.seconds).fold(0.0_f64, f64::max);
    let total: f64 = turns.iter().map(|t| t.seconds).sum();
    let sessions: std::collections::BTreeSet<&str> =
        turns.iter().map(|t| t.session_id.as_str()).collect();
    let mut lines = Vec::new();
    for turn in turns {
        let width = if slowest > 0.0 {
            ((turn.seconds / slowest) * 10.0).round() as usize
        } else {
            0
        };
        let bar = "█".repeat(if turn.seconds > 0.0 { width.max(1) } else { 0 });
        let mut label = turn.label();
        if turn.tools > 0 {
            label = format!("{label} · {} tools", turn.tools);
        }
        lines.push(format!(
            "  {}  {:>6}  {:<10}  {:<30}  {}",
            turn.when(),
            format!("{:.1}s", turn.seconds),
            bar,
            clip(&label, 30),
            clip(&turn.project(), 16),
        ));
    }
    lines.push(String::new());
    lines.push(format!(
        "  {} turns · {} of work · slowest {:.1}s · {} sessions",
        turns.len(),
        format_duration(total),
        slowest,
        sessions.len()
    ));
    lines
}

fn clip(text: &str, max: usize) -> String {
    if text.chars().count() <= max {
        return text.to_string();
    }
    let mut out: String = text.chars().take(max.saturating_sub(1)).collect();
    out.push('…');
    out
}

fn format_duration(secs: f64) -> String {
    if secs < 1.0 {
        format!("{:.1}s", secs)
    } else if secs < 60.0 {
        format!("{}s", secs.round() as u64)
    } else {
        let mins = (secs / 60.0).floor() as u64;
        let rem = (secs - mins as f64 * 60.0).round() as u64;
        format!("{mins}m{rem}s")
    }
}
