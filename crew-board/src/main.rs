mod app;
mod cleanup;
mod data;
mod discovery;
mod hook_bridge;
mod hook_server;
mod orchestration;
mod launcher;
mod security;
mod settings;
mod terminal;
mod ui;
mod worktree;

use anyhow::Result;
use app::{ActiveView, App, DetailMode, FocusPane, ModifierBarState, TerminalInputMode};
use clap::Parser;
use crossterm::{
    event::{
        self, DisableBracketedPaste, DisableMouseCapture, EnableBracketedPaste, EnableMouseCapture,
        Event, KeyCode, KeyEventKind, KeyModifiers, KeyboardEnhancementFlags, ModifierKeyCode,
        MouseButton, MouseEventKind, PopKeyboardEnhancementFlags, PushKeyboardEnhancementFlags,
    },
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::{backend::CrosstermBackend, Terminal};
use std::io;
use std::time::Duration;

#[derive(Parser, Debug)]
#[command(
    name = "crew-board",
    version,
    about = "Cross-project task dashboard for agentic-workflow"
)]
struct Cli {
    /// Repository paths to monitor (repeatable)
    #[arg(short, long = "repo")]
    repos: Vec<String>,

    /// Parent directory to scan for repos containing .tasks/ (repeatable)
    #[arg(short, long = "scan")]
    scans: Vec<String>,

    /// Poll interval in seconds
    #[arg(short, long)]
    poll_interval: Option<u64>,

    /// Quick-start: create worktree with this description and launch AI host immediately.
    #[arg(long = "quick", value_name = "DESCRIPTION")]
    quick: Option<String>,

    /// Resume the most recently updated active task immediately.
    #[arg(long = "resume-latest")]
    resume_latest: bool,

    /// AI host for --quick/--resume-latest (claude/copilot/gemini/opencode/shell).
    #[arg(long = "host", value_name = "HOST")]
    host: Option<String>,
}

fn parse_host_arg(s: &Option<String>) -> launcher::AiHost {
    match s.as_deref() {
        Some("claude") => launcher::AiHost::Claude,
        Some("copilot") => launcher::AiHost::Copilot,
        Some("gemini") => launcher::AiHost::Gemini,
        Some("opencode") => launcher::AiHost::OpenCode,
        Some("shell") => launcher::AiHost::Shell,
        _ => launcher::detect_ai_hosts()
            .into_iter()
            .next()
            .unwrap_or(launcher::AiHost::Claude),
    }
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    let cfg = settings::Settings::load();
    let initial_layout = cfg.parsed_terminal_layout();

    // Merge: CLI args override config. If CLI has values, use them; otherwise fall back to config.
    let repos = if cli.repos.is_empty() {
        cfg.repos
    } else {
        cli.repos
    };

    let scans = if cli.scans.is_empty() {
        cfg.scan
    } else {
        cli.scans
    };

    let poll_interval = cli.poll_interval.or(cfg.poll_interval).unwrap_or(3);

    let repo_paths = discovery::discover_repos(&repos, &scans);
    if repo_paths.is_empty() {
        eprintln!("No repos found.");
        if let Some(path) = settings::config_path() {
            eprintln!(
                "Create {} with:\n\n  scan = [\"/path/to/your/projects\"]\n",
                path.display()
            );
        }
        eprintln!("Or use: crew-board --repo <path> or --scan <dir>");
        std::process::exit(1);
    }

    // Setup terminal
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen, EnableMouseCapture, EnableBracketedPaste)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    // Try to enable kitty keyboard protocol for modifier-only key detection.
    // Always attempt to push the flags — supports_keyboard_enhancement() is
    // unreliable (returns false in WezTerm even when kitty protocol works).
    // If the terminal truly doesn't support it, the escape sequence is
    // silently ignored and we fall back to flash-on-modified-F-key.
    let kitty_enabled = execute!(
        io::stdout(),
        PushKeyboardEnhancementFlags(
            KeyboardEnhancementFlags::DISAMBIGUATE_ESCAPE_CODES
                | KeyboardEnhancementFlags::REPORT_EVENT_TYPES
                | KeyboardEnhancementFlags::REPORT_ALL_KEYS_AS_ESCAPE_CODES
        )
    )
    .is_ok();

    // Create app
    let mut app = App::new(repo_paths, poll_interval);

    // Apply terminal settings from config
    app.terminal_layout = initial_layout;
    app.pane_width_tasks = cfg.pane_width_tasks.clamp(10, 90);
    app.pane_width_issues = cfg.pane_width_issues.clamp(10, 90);
    app.pane_width_terminals = cfg.pane_width_terminals.clamp(10, 50);
    app.kitty_protocol_enabled = kitty_enabled;
    app.system_bell = cfg.system_bell;
    app.visual_bell = cfg.visual_bell;
    app.log_directory = cfg.log_directory.map(std::path::PathBuf::from);
    app.permission_profile = app::PermissionProfile::from_str(&cfg.permission_profile);
    app.auto_approve_patterns = cfg
        .auto_approve_patterns
        .iter()
        .filter_map(|p| regex::Regex::new(p).ok())
        .collect();
    app.desktop_notifications = cfg.desktop_notifications;
    app.auto_accept_default = cfg.auto_accept_default;
    app.hook_communication = cfg.hook_communication;

    // Apply splash setting
    app.show_splash = cfg.show_splash_on_start;

    // Apply task filtering settings
    app.recent_done_days = cfg.recent_done_days;
    app.task_filter = match cfg.default_task_filter.as_str() {
        "active" => app::TaskFilter::Active,
        "active-recent" => app::TaskFilter::ActiveAndRecentDone,
        _ => app::TaskFilter::All,
    };
    if app.task_filter != app::TaskFilter::All {
        app.rebuild_tree();
    }

    // Initialize security rules engine
    if cfg.security_enabled {
        app.rules_engine = crate::security::RulesEngine::from_config(
            &cfg.security_rules,
            &cfg.credential_patterns,
        );
    }

    // Initialize orchestration engine
    {
        let mode = match cfg.orchestration_mode.as_str() {
            "semi-auto" => orchestration::OrchestrationMode::SemiAuto,
            "full-auto" => orchestration::OrchestrationMode::FullAuto,
            _ => orchestration::OrchestrationMode::Manual,
        };
        let mut orch = orchestration::OrchestrationState::new(mode);
        orch.guardrails.max_concurrent = cfg.max_concurrent;
        orch.guardrails.cost_ceiling = cfg.cost_limit;
        orch.guardrails.max_retries = cfg.max_retries;
        app.orchestration = Some(orch);
    }

    // Start hook server if enabled
    app.init_hook_server();

    // Handle --resume-latest: find and launch most recent active task
    if cli.resume_latest {
        // Find the most recently updated active task
        let mut best: Option<(usize, usize, String)> = None;
        for (ri, repo) in app.repos.iter().enumerate() {
            for (ti, task) in repo.tasks.iter().enumerate() {
                if task.archived || task.state.is_complete() {
                    continue;
                }
                let dominated = best.as_ref().is_some_and(|(_, _, best_ts)| {
                    task.state.updated_at.as_str() <= best_ts.as_str()
                });
                if !dominated {
                    best = Some((ri, ti, task.state.updated_at.clone()));
                }
            }
        }
        if let Some((ri, ti, _)) = best {
            app.show_splash = false;
            // Set splash cursor and launch
            app.splash_task_cursor = 0;
            app.splash_active_tasks = vec![(ri, ti)];
            app.splash_launch_task();
        }
    }

    // Handle --quick: create worktree and launch
    if let Some(ref description) = cli.quick {
        let host = parse_host_arg(&cli.host);
        if !app.repos.is_empty() {
            let repo_path = app.repos[0].path.clone();
            match worktree::create_worktree(&repo_path, description, host, true) {
                Ok(result) => {
                    app.show_splash = false;
                    let task_id = result.task_id.clone();
                    let work_dir = result.worktree_abs.clone();
                    let color_idx = Some(result.color_scheme_index);
                    let (command, args) = launcher::embed_cmd_args(host, &task_id);
                    app.spawn_terminal(
                        &task_id, host.label(), &command, &args, &work_dir, color_idx,
                    );
                    app.active_view = app::ActiveView::Terminals;
                    app.refresh();
                }
                Err(e) => {
                    // Print error but continue to TUI
                    eprintln!("Quick-start failed: {}", e);
                }
            }
        }
    }

    // Main loop
    let result = run_app(&mut terminal, &mut app);

    // Shutdown hook server
    if let Some(ref server) = app.hook_server {
        server.shutdown();
    }

    // Cleanup embedded terminals before restoring (also removes settings.local.json)
    if let Some(mgr) = &mut app.terminal_manager {
        // Clean up hook settings files for all terminals
        for term in &mgr.terminals {
            if let Some(ref cwd) = term.hook_settings_cwd {
                let settings_path = cwd.join(".claude").join("settings.local.json");
                let _ = std::fs::remove_file(&settings_path);
            }
        }
        mgr.cleanup_all();
    }

    // Pop kitty keyboard protocol if it was enabled
    if kitty_enabled {
        let _ = execute!(terminal.backend_mut(), PopKeyboardEnhancementFlags);
    }

    // Restore terminal
    disable_raw_mode()?;
    execute!(
        terminal.backend_mut(),
        LeaveAlternateScreen,
        DisableMouseCapture,
        DisableBracketedPaste
    )?;
    terminal.show_cursor()?;

    // On Windows, ClosePseudoConsole can deadlock if the ConPTY output pipe
    // isn't fully drained (the reader thread is blocked on read()). Since we've
    // already restored the terminal, force-exit to avoid hanging.
    // The OS will clean up all child processes and handles.
    if cfg!(target_os = "windows") {
        if let Some(ref mgr) = app.terminal_manager {
            if !mgr.terminals.is_empty() {
                std::process::exit(result.as_ref().map_or(1, |_| 0));
            }
        }
    }

    result
}

/// Map current modifier flags to a bar state (supports combined modifiers).
fn modifier_bar_from_event(modifiers: KeyModifiers) -> ModifierBarState {
    let shift = modifiers.contains(KeyModifiers::SHIFT);
    let ctrl = modifiers.contains(KeyModifiers::CONTROL);
    let alt = modifiers.contains(KeyModifiers::ALT);
    match (shift, ctrl, alt) {
        (true, true, _) => ModifierBarState::ShiftCtrl,
        (true, false, true) => ModifierBarState::AltShift,
        (false, true, true) => ModifierBarState::CtrlAlt,
        (true, false, false) => ModifierBarState::Shift,
        (false, true, false) => ModifierBarState::Ctrl,
        (false, false, true) => ModifierBarState::Alt,
        (false, false, false) => ModifierBarState::Normal,
    }
}

fn run_app(
    terminal: &mut Terminal<CrosstermBackend<io::Stdout>>,
    app: &mut App,
) -> Result<()> {
    loop {
        terminal.draw(|frame| ui::draw(frame, app))?;

        // Page size for PgUp/PgDn: terminal height minus status bar and borders
        let page_size = terminal.size().map(|s| s.height.saturating_sub(4).max(1)).unwrap_or(20);

        // Use shorter poll timeout when search debounce is pending or
        // terminals are actively running (for responsive PTY rendering).
        let search_pending = app.search_popup.as_ref().is_some_and(|p| p.dirty);
        let terminals_active = app.active_view == ActiveView::Terminals
            && app
                .terminal_manager
                .as_ref()
                .is_some_and(|m| m.has_running());
        let modifier_flash = app.modifier_bar_flash_until.is_some();
        let timeout = if search_pending || terminals_active || modifier_flash {
            Duration::from_millis(50)
        } else {
            Duration::from_millis(250)
        };
        if event::poll(timeout)? {
            let ev = event::read()?;
            // Handle terminal resize events
            if let Event::Resize(cols, rows) = ev {
                app.terminal_resize_all(rows, cols);
            }
            // Handle mouse events (selection + scroll in terminal panels)
            if let Event::Mouse(mouse) = ev {
                match mouse.kind {
                    MouseEventKind::Down(MouseButton::Left) => {
                        app.mouse_start_selection(mouse.column, mouse.row);
                    }
                    MouseEventKind::Drag(MouseButton::Left) => {
                        app.mouse_extend_selection(mouse.column, mouse.row);
                    }
                    MouseEventKind::Up(MouseButton::Left) => {
                        app.mouse_end_selection();
                    }
                    MouseEventKind::ScrollUp => {
                        app.mouse_scroll(mouse.column, mouse.row, true);
                    }
                    MouseEventKind::ScrollDown => {
                        app.mouse_scroll(mouse.column, mouse.row, false);
                    }
                    _ => {}
                }
            }
            // Handle paste events (bracketed paste mode)
            if let Event::Paste(ref text) = ev {
                if app.active_view == ActiveView::Terminals
                    && app.terminal_input_mode == TerminalInputMode::TerminalFocused
                {
                    // Wrap in bracketed paste sequences so the receiving application
                    // (bash, Claude Code, etc.) knows this is pasted text and won't
                    // treat newlines as command execution.
                    let mut bytes = Vec::with_capacity(text.len() + 12);
                    bytes.extend_from_slice(b"\x1b[200~");
                    bytes.extend_from_slice(text.as_bytes());
                    bytes.extend_from_slice(b"\x1b[201~");
                    app.terminal_send_input(&bytes);
                } else if app.permission_popup.is_some() {
                    // Forward paste to permission popup custom input if active
                    app.permission_popup_paste(text);
                } else if app.search_popup.is_some() {
                    // Forward paste to search input
                    app.search_paste(text);
                } else if app.create_popup.is_some() {
                    // Forward paste to create popup text input
                    app.create_popup_paste(text);
                }
            }
            if let Event::Key(key) = ev {
                // Handle modifier key release events — update bar based on
                // which modifiers are STILL held (e.g., releasing Shift while
                // Ctrl is held should show Ctrl layer, not Normal).
                if key.kind == KeyEventKind::Release {
                    if let KeyCode::Modifier(
                        ModifierKeyCode::LeftShift
                        | ModifierKeyCode::RightShift
                        | ModifierKeyCode::LeftControl
                        | ModifierKeyCode::RightControl
                        | ModifierKeyCode::LeftAlt
                        | ModifierKeyCode::RightAlt,
                    ) = key.code
                    {
                        let remaining = modifier_bar_from_event(key.modifiers);
                        app.modifier_bar_state = remaining;
                        if remaining == ModifierBarState::Normal {
                            app.modifier_bar_flash_until = None;
                        }
                    }
                    continue;
                }
                // Only handle Press events for all other keys
                if key.kind != KeyEventKind::Press {
                    continue;
                }

                // Handle modifier-only press events (bar updates when the
                // terminal sends these via kitty protocol). Always set a
                // flash timer as fallback — some terminals send Press but
                // not Release events for modifier keys.
                if let KeyCode::Modifier(mod_key) = key.code {
                    let new_state = modifier_bar_from_event(key.modifiers);
                    // Ignore modifier release-as-press quirks that would reset to Normal
                    if new_state != ModifierBarState::Normal {
                        app.modifier_bar_state = new_state;
                        app.modifier_bar_flash_until = Some(
                            std::time::Instant::now() + std::time::Duration::from_secs(2),
                        );
                    }
                    let _ = mod_key; // suppress unused warning
                    continue;
                }

                // Priority -1: Quit confirmation dialog
                if app.quit_confirm {
                    match key.code {
                        KeyCode::Char('y') | KeyCode::Char('Y') | KeyCode::Enter => {
                            app.should_quit = true;
                        }
                        _ => {
                            app.quit_confirm = false;
                        }
                    }
                }
                // Priority 0.1: Splash screen — actionable quick-start
                else if app.show_splash {
                    match key.code {
                        KeyCode::Up | KeyCode::Char('k') => {
                            if !app.splash_active_tasks.is_empty() && app.splash_task_cursor > 0 {
                                app.splash_task_cursor -= 1;
                            }
                        }
                        KeyCode::Down | KeyCode::Char('j') => {
                            if !app.splash_active_tasks.is_empty() {
                                let max = app.splash_active_tasks.len().saturating_sub(1);
                                if app.splash_task_cursor < max {
                                    app.splash_task_cursor += 1;
                                }
                            }
                        }
                        KeyCode::Enter => {
                            // Resume the highlighted task
                            app.show_splash = false;
                            app.splash_scroll = 0;
                            app.splash_launch_task();
                        }
                        KeyCode::Char('n') | KeyCode::Char('N') => {
                            // New task — open create popup for first repo
                            app.show_splash = false;
                            app.splash_scroll = 0;
                            if !app.repos.is_empty() {
                                app.open_create_popup_for_repo(0);
                            }
                        }
                        KeyCode::F(2) => {
                            app.show_splash = false;
                            app.splash_scroll = 0;
                            app.open_launch_popup();
                        }
                        KeyCode::F(4) => {
                            app.show_splash = false;
                            app.splash_scroll = 0;
                            // Open create popup for first repo
                            if !app.repos.is_empty() {
                                app.open_create_popup_for_repo(0);
                            }
                        }
                        KeyCode::F(3) => {
                            app.show_splash = false;
                            app.splash_scroll = 0;
                            app.open_search();
                        }
                        KeyCode::F(10) => {
                            app.should_quit = true;
                        }
                        KeyCode::Esc => {
                            // Dismiss to full dashboard
                            app.show_splash = false;
                            app.splash_scroll = 0;
                        }
                        KeyCode::PageUp => {
                            app.splash_scroll = app.splash_scroll.saturating_sub(10);
                        }
                        KeyCode::PageDown => {
                            app.splash_scroll = app.splash_scroll.saturating_add(10);
                        }
                        _ => {
                            // Any other key dismisses the splash
                            app.show_splash = false;
                            app.splash_scroll = 0;
                        }
                    }
                }
                // Priority 0: Help overlay (scrollable)
                else if app.show_help {
                    match key.code {
                        KeyCode::Esc | KeyCode::F(1) => {
                            app.show_help = false;
                            app.help_scroll = 0;
                        }
                        KeyCode::Up | KeyCode::Char('k') => {
                            app.help_scroll = app.help_scroll.saturating_sub(1);
                        }
                        KeyCode::Down | KeyCode::Char('j') => {
                            app.help_scroll = app.help_scroll.saturating_add(1);
                        }
                        KeyCode::PageUp => {
                            app.help_scroll = app.help_scroll.saturating_sub(10);
                        }
                        KeyCode::PageDown => {
                            app.help_scroll = app.help_scroll.saturating_add(10);
                        }
                        KeyCode::Home => {
                            app.help_scroll = 0;
                        }
                        KeyCode::End => {
                            app.help_scroll = u16::MAX; // clamped during render
                        }
                        _ => {} // ignore other keys (no longer closes)
                    }
                }
                // Priority 0.5: Stats popup
                else if app.stats_popup.is_some() {
                    if key.modifiers.contains(KeyModifiers::SHIFT) {
                        match key.code {
                            KeyCode::F(1) => {
                                app.stats_popup = None;
                                app.set_view(ActiveView::Tasks);
                            }
                            KeyCode::F(2) => {
                                app.stats_popup = None;
                                app.set_view(ActiveView::BeadsIssues);
                            }
                            KeyCode::F(3) => {
                                app.stats_popup = None;
                                app.set_view(ActiveView::Config);
                            }
                            KeyCode::F(4) => {
                                app.stats_popup = None;
                                app.set_view(ActiveView::CostSummary);
                            }
                            KeyCode::F(5) => {
                                app.stats_popup = None;
                                app.set_view(ActiveView::Terminals);
                            }
                            KeyCode::F(6) => {
                                app.stats_popup = None;
                                app.set_view(ActiveView::ActivityFeed);
                            }
                            _ => {}
                        }
                    } else {
                        match key.code {
                            KeyCode::Esc => { app.stats_popup = None; }
                            KeyCode::PageUp => {
                                if let Some(ref mut popup) = app.stats_popup {
                                    popup.scroll = popup.scroll.saturating_sub(10);
                                }
                            }
                            KeyCode::PageDown => {
                                if let Some(ref mut popup) = app.stats_popup {
                                    popup.scroll = popup.scroll.saturating_add(10);
                                }
                            }
                            KeyCode::Up | KeyCode::Char('k') => {
                                if let Some(ref mut popup) = app.stats_popup {
                                    popup.scroll = popup.scroll.saturating_sub(1);
                                }
                            }
                            KeyCode::Down | KeyCode::Char('j') => {
                                if let Some(ref mut popup) = app.stats_popup {
                                    popup.scroll = popup.scroll.saturating_add(1);
                                }
                            }
                            _ => {}
                        }
                    }
                }
                // Priority 0.6: Scroll-back mode
                else if app.active_view == ActiveView::Terminals
                    && app.terminal_input_mode == TerminalInputMode::ScrollBack
                {
                    // Search input mode active
                    if app.terminal_search_input.is_some() {
                        match key.code {
                            KeyCode::Enter => {
                                app.terminal_search_execute();
                            }
                            KeyCode::Esc => {
                                app.terminal_search_input = None;
                            }
                            _ => {
                                if let Some(ref mut input) = app.terminal_search_input {
                                    use tui_input::backend::crossterm::EventHandler;
                                    input.handle_event(
                                        &crossterm::event::Event::Key(key),
                                    );
                                }
                            }
                        }
                    // Shift+F-keys: global view switching (even from scroll-back)
                    } else if key.modifiers.contains(KeyModifiers::SHIFT) {
                        match key.code {
                            KeyCode::F(1) => {
                                app.terminal_input_mode = TerminalInputMode::Normal;
                                app.set_view(ActiveView::Tasks);
                            }
                            KeyCode::F(2) => {
                                app.terminal_input_mode = TerminalInputMode::Normal;
                                app.set_view(ActiveView::BeadsIssues);
                            }
                            KeyCode::F(3) => {
                                app.terminal_input_mode = TerminalInputMode::Normal;
                                app.set_view(ActiveView::Config);
                            }
                            KeyCode::F(4) => {
                                app.terminal_input_mode = TerminalInputMode::Normal;
                                app.set_view(ActiveView::CostSummary);
                            }
                            KeyCode::F(5) => {
                                // Already in Terminals — just exit scroll-back
                                app.terminal_input_mode = TerminalInputMode::Normal;
                            }
                            KeyCode::F(6) => {
                                app.terminal_input_mode = TerminalInputMode::Normal;
                                app.set_view(ActiveView::ActivityFeed);
                            }
                            _ => {}
                        }
                    } else {
                        match key.code {
                            KeyCode::Esc | KeyCode::Char('q') => {
                                app.terminal_scroll_reset();
                                app.terminal_search_query.clear();
                                app.terminal_search_matches.clear();
                                app.terminal_input_mode = TerminalInputMode::TerminalFocused;
                            }
                            KeyCode::Up | KeyCode::Char('k') => {
                                app.terminal_scroll_up(1);
                            }
                            KeyCode::Down | KeyCode::Char('j') => {
                                app.terminal_scroll_down(1);
                            }
                            KeyCode::PageUp => {
                                app.terminal_scroll_up(page_size as usize);
                            }
                            KeyCode::PageDown => {
                                app.terminal_scroll_down(page_size as usize);
                            }
                            KeyCode::Home => {
                                app.terminal_scroll_to_top();
                            }
                            KeyCode::End => {
                                app.terminal_scroll_reset();
                                app.terminal_search_query.clear();
                                app.terminal_search_matches.clear();
                                app.terminal_input_mode = TerminalInputMode::TerminalFocused;
                            }
                            KeyCode::Char('/') => {
                                app.terminal_search_start();
                            }
                            KeyCode::Char('n') => {
                                app.terminal_search_next();
                            }
                            KeyCode::Char('N') => {
                                app.terminal_search_prev();
                            }
                            _ => {}
                        }
                    }
                }
                // Priority 0.7: Terminal focus mode -- all keys go to PTY
                else if app.active_view == ActiveView::Terminals
                    && app.terminal_input_mode == TerminalInputMode::TerminalFocused
                {
                    // F12: toggle back to Normal mode (single-key exit)
                    if key.code == KeyCode::F(12) {
                        app.terminal_input_mode = TerminalInputMode::Normal;
                    }
                    // F5: previous terminal (bypass focused mode)
                    else if key.code == KeyCode::F(5) {
                        app.terminal_focus_prev_running();
                    }
                    // F6: next terminal (bypass focused mode)
                    else if key.code == KeyCode::F(6) {
                        app.terminal_focus_next_running();
                    }
                    // F7: jump to next attention terminal (bypass focused mode)
                    else if key.code == KeyCode::F(7) {
                        app.terminal_focus_next_attention();
                    }
                    // Shift+F-keys: global view switching (even while focused)
                    else if key.modifiers.contains(KeyModifiers::SHIFT) {
                        match key.code {
                            KeyCode::F(1) => {
                                app.terminal_input_mode = TerminalInputMode::Normal;
                                app.set_view(ActiveView::Tasks);
                            }
                            KeyCode::F(2) => {
                                app.terminal_input_mode = TerminalInputMode::Normal;
                                app.set_view(ActiveView::BeadsIssues);
                            }
                            KeyCode::F(3) => {
                                app.terminal_input_mode = TerminalInputMode::Normal;
                                app.set_view(ActiveView::Config);
                            }
                            KeyCode::F(4) => {
                                app.terminal_input_mode = TerminalInputMode::Normal;
                                app.set_view(ActiveView::CostSummary);
                            }
                            KeyCode::F(5) => {
                                // Already in Terminals — just exit focus
                                app.terminal_input_mode = TerminalInputMode::Normal;
                            }
                            KeyCode::F(6) => {
                                app.terminal_input_mode = TerminalInputMode::Normal;
                                app.set_view(ActiveView::ActivityFeed);
                            }
                            _ => {
                                // Other Shift keys: forward to PTY
                                let bytes =
                                    terminal::widget::key_to_bytes(key.code, key.modifiers);
                                if !bytes.is_empty() {
                                    app.terminal_send_input(&bytes);
                                }
                            }
                        }
                    } else if key.modifiers.contains(KeyModifiers::ALT) {
                        // Alt+Arrow keys: tile navigation (intercept before PTY forwarding)
                        match key.code {
                            KeyCode::Left  => app.terminal_tile_focus_left(),
                            KeyCode::Right => app.terminal_tile_focus_right(),
                            KeyCode::Up    => app.terminal_tile_focus_up(),
                            KeyCode::Down  => app.terminal_tile_focus_down(),
                            _ => {
                                // Forward other Alt+key combinations to PTY
                                let bytes = terminal::widget::key_to_bytes(key.code, key.modifiers);
                                if !bytes.is_empty() {
                                    app.terminal_send_input(&bytes);
                                }
                            }
                        }
                    } else {
                        let bytes =
                            terminal::widget::key_to_bytes(key.code, key.modifiers);
                        if !bytes.is_empty() {
                            app.terminal_send_input(&bytes);
                        }
                    }
                }
                // Priority 1: Search popup
                else if app.search_popup.is_some() {
                    app.search_handle_key(key);
                }
                // Priority 2: Create worktree popup
                else if app.create_popup.is_some() {
                    app.create_popup_handle_key(key);
                }
                // Priority 2.5: Cleanup worktree popup
                else if app.cleanup_popup.is_some() {
                    app.cleanup_popup_handle_key(key);
                }
                // Priority 2.7: Permission queue popup
                else if app.permission_popup.is_some() {
                    app.permission_popup_handle_key(key);
                }
                // Priority 3: Launch popup
                else if app.launch_popup.is_some() {
                    match key.code {
                        KeyCode::Esc => app.close_launch_popup(),
                        KeyCode::Up | KeyCode::Char('k') => app.popup_up(),
                        KeyCode::Down | KeyCode::Char('j') => app.popup_down(),
                        KeyCode::Enter => app.popup_confirm(),
                        _ => {}
                    }
                }
                // Priority 4: Right pane doc/history navigation
                else if app.focus_pane == FocusPane::Right
                    && app.detail_mode != DetailMode::Overview
                {
                    // Shift+F-keys: global view switching (even from detail pane)
                    if key.modifiers.contains(KeyModifiers::SHIFT) {
                        match key.code {
                            KeyCode::F(1) => {
                                app.detail_back();
                                app.set_view(ActiveView::Tasks);
                                app.flash_modifier_bar(ModifierBarState::Shift);
                            }
                            KeyCode::F(2) => {
                                app.detail_back();
                                app.set_view(ActiveView::BeadsIssues);
                                app.flash_modifier_bar(ModifierBarState::Shift);
                            }
                            KeyCode::F(3) => {
                                app.detail_back();
                                app.set_view(ActiveView::Config);
                                app.flash_modifier_bar(ModifierBarState::Shift);
                            }
                            KeyCode::F(4) => {
                                app.detail_back();
                                app.set_view(ActiveView::CostSummary);
                                app.flash_modifier_bar(ModifierBarState::Shift);
                            }
                            KeyCode::F(5) => {
                                app.detail_back();
                                app.set_view(ActiveView::Terminals);
                                app.flash_modifier_bar(ModifierBarState::Shift);
                            }
                            KeyCode::F(6) => {
                                app.detail_back();
                                app.set_view(ActiveView::ActivityFeed);
                                app.flash_modifier_bar(ModifierBarState::Shift);
                            }
                            _ => {}
                        }
                    } else {
                        match key.code {
                            KeyCode::Esc | KeyCode::Backspace => app.detail_back(),
                            KeyCode::Up | KeyCode::Char('k') => {
                                if matches!(app.detail_mode, DetailMode::DocList { .. }) {
                                    app.detail_nav_up();
                                } else {
                                    app.scroll_detail_up();
                                }
                            }
                            KeyCode::Down | KeyCode::Char('j') => {
                                if matches!(app.detail_mode, DetailMode::DocList { .. }) {
                                    app.detail_nav_down();
                                } else {
                                    app.scroll_detail_down();
                                }
                            }
                            KeyCode::Enter => app.detail_open_doc(),
                            KeyCode::PageDown => app.scroll_detail_page_down(page_size),
                            KeyCode::PageUp => app.scroll_detail_page_up(page_size),
                            KeyCode::Home => { app.detail_scroll = 0; }
                            KeyCode::End => {
                                app.detail_scroll = app.detail_scroll_max.get();
                            }
                            KeyCode::Tab => app.toggle_focus(),
                            KeyCode::Char('q') | KeyCode::F(10) => app.quit_confirm = true,
                            // Delete/d: clean up worktree for current task (Tasks view only)
                            KeyCode::Delete | KeyCode::Char('d') => {
                                if app.active_view == ActiveView::Tasks {
                                    app.open_single_task_cleanup();
                                }
                            }
                            _ => {}
                        }
                    }
                }
                // Priority 5: Default key handling
                else {
                    // ── Shift+F-key layer: view switching ─────────────
                    if key.modifiers.contains(KeyModifiers::SHIFT) {
                        match key.code {
                            KeyCode::F(1) => {
                                app.set_view(ActiveView::Tasks);
                                app.flash_modifier_bar(ModifierBarState::Shift);
                            }
                            KeyCode::F(2) => {
                                app.set_view(ActiveView::BeadsIssues);
                                app.flash_modifier_bar(ModifierBarState::Shift);
                            }
                            KeyCode::F(3) => {
                                app.set_view(ActiveView::Config);
                                app.flash_modifier_bar(ModifierBarState::Shift);
                            }
                            KeyCode::F(4) => {
                                app.set_view(ActiveView::CostSummary);
                                app.flash_modifier_bar(ModifierBarState::Shift);
                            }
                            KeyCode::F(5) => {
                                app.set_view(ActiveView::Terminals);
                                app.flash_modifier_bar(ModifierBarState::Shift);
                            }
                            KeyCode::F(6) => {
                                app.set_view(ActiveView::ActivityFeed);
                                app.flash_modifier_bar(ModifierBarState::Shift);
                            }
                            _ => {} // Shift+F7-F10 reserved
                        }
                    }
                    // ── Ctrl+F-key layer ─────────────────────────────
                    else if key.modifiers.contains(KeyModifiers::CONTROL) {
                        match key.code {
                            KeyCode::F(4) => {
                                // Ctrl+F4: Terminals=Dismiss All, Activity=Crew filter
                                match app.active_view {
                                    ActiveView::Terminals    => app.terminal_dismiss_all_exited(),
                                    ActiveView::ActivityFeed => app.activity_cycle_terminal_filter(),
                                    _ => {}
                                }
                                app.flash_modifier_bar(ModifierBarState::Ctrl);
                            }
                            KeyCode::F(5) => {
                                // Ctrl+F5: Terminals=Live view, Activity=Event filter
                                match app.active_view {
                                    ActiveView::ActivityFeed => app.activity_cycle_event_filter(),
                                    _ => app.terminal_scroll_reset(),
                                }
                                app.flash_modifier_bar(ModifierBarState::Ctrl);
                            }
                            KeyCode::F(6) => {
                                // Ctrl+F6: Activity=Tool filter, others=Stats popup
                                if app.active_view == ActiveView::ActivityFeed {
                                    app.activity_cycle_tool_filter();
                                } else if app.stats_popup.is_some() {
                                    app.stats_popup = None;
                                } else {
                                    app.stats_popup = Some(crate::ui::stats_popup::StatsPopup::new());
                                }
                                app.flash_modifier_bar(ModifierBarState::Ctrl);
                            }
                            KeyCode::F(7) => {
                                // Ctrl+F7: Terminals=Auto-accept toggle, Activity=Auto-scroll toggle
                                match app.active_view {
                                    ActiveView::Terminals => app.toggle_auto_accept(),
                                    ActiveView::ActivityFeed => {
                                        app.activity_filter.auto_scroll = !app.activity_filter.auto_scroll;
                                    }
                                    _ => {}
                                }
                                app.flash_modifier_bar(ModifierBarState::Ctrl);
                            }
                            KeyCode::F(8) => {
                                // Ctrl+F8: Activity=Gantt toggle, Terminals=Scroll back
                                match app.active_view {
                                    ActiveView::ActivityFeed => {
                                        app.activity_filter.timeline_mode = !app.activity_filter.timeline_mode;
                                    }
                                    ActiveView::Terminals => {
                                        if app.terminal_manager.as_ref().is_some_and(|m| !m.terminals.is_empty()) {
                                            app.terminal_input_mode = TerminalInputMode::ScrollBack;
                                        }
                                    }
                                    _ => {}
                                }
                                app.flash_modifier_bar(ModifierBarState::Ctrl);
                            }
                            KeyCode::F(_) => {
                                // Other Ctrl+F-keys: flash the layer for discovery
                                app.flash_modifier_bar(ModifierBarState::Ctrl);
                            }
                            // Fall through to existing Ctrl+key handlers below
                            _ => {
                                match key.code {
                                    KeyCode::Char('c') => {
                                        app.should_quit = true;
                                    }
                                    KeyCode::Left => {
                                        match app.active_view {
                                            ActiveView::Tasks => {
                                                app.pane_width_tasks = app.pane_width_tasks.saturating_sub(5).max(10);
                                            }
                                            ActiveView::BeadsIssues => {
                                                app.pane_width_issues = app.pane_width_issues.saturating_sub(5).max(10);
                                            }
                                            ActiveView::Terminals => {
                                                app.pane_width_terminals = app.pane_width_terminals.saturating_sub(2).max(10);
                                            }
                                            _ => {}
                                        }
                                    }
                                    KeyCode::Right => {
                                        match app.active_view {
                                            ActiveView::Tasks => {
                                                app.pane_width_tasks = (app.pane_width_tasks + 5).min(90);
                                            }
                                            ActiveView::BeadsIssues => {
                                                app.pane_width_issues = (app.pane_width_issues + 5).min(90);
                                            }
                                            ActiveView::Terminals => {
                                                app.pane_width_terminals = (app.pane_width_terminals + 2).min(50);
                                            }
                                            _ => {}
                                        }
                                    }
                                    _ => {}
                                }
                            }
                        }
                    }
                    // ── Base F-key layer (no modifier) ───────────────
                    else {
                    match (key.modifiers, key.code) {
                        // Quit (q and F10 show confirmation — Esc never quits)
                        (_, KeyCode::Char('q')) | (_, KeyCode::F(10)) => {
                            app.quit_confirm = true;
                        }
                        // Ctrl+C: force quit (no confirmation)
                        (KeyModifiers::CONTROL, KeyCode::Char('c')) => app.should_quit = true,

                        // Esc: back out progressively, never quit
                        (_, KeyCode::Esc) => {
                            if app.focus_pane == FocusPane::Right {
                                app.focus_pane = FocusPane::Left;
                            }
                        }

                        // Help
                        (_, KeyCode::F(1)) => {
                            app.show_help = true;
                            app.help_scroll = 0;
                        }

                        // Launch terminal
                        (_, KeyCode::F(2)) => app.open_launch_popup(),

                        // Search
                        (_, KeyCode::F(3)) => app.open_search(),

                        // F4: context-sensitive — Terminals=Layout cycle, others=New worktree
                        (_, KeyCode::F(4)) => {
                            if app.active_view == ActiveView::Terminals {
                                app.terminal_layout = app.terminal_layout.next();
                            } else {
                                app.open_create_popup();
                            }
                        }

                        // F5: context-sensitive — Terminals=Prev terminal, others=Refresh
                        (_, KeyCode::F(5)) => {
                            if app.active_view == ActiveView::Terminals {
                                app.terminal_focus_prev_running();
                            } else {
                                app.refresh();
                            }
                        }

                        // F6: context-sensitive — Tasks=Docs, Terminals=Next terminal, others=Cleanup
                        (_, KeyCode::F(6)) => {
                            match app.active_view {
                                ActiveView::Tasks => {
                                    match &app.detail_mode {
                                        DetailMode::DocList { .. } => app.detail_open_doc(),
                                        _ => app.enter_doc_list(),
                                    }
                                }
                                ActiveView::Terminals => {
                                    app.terminal_focus_next_running();
                                }
                                _ => app.open_cleanup_popup(),
                            }
                        }

                        // F7: context-sensitive — Tasks=History/Back, others=Attention
                        (_, KeyCode::F(7)) => {
                            match app.active_view {
                                ActiveView::Tasks => {
                                    match &app.detail_mode {
                                        DetailMode::Overview => app.enter_history(),
                                        _ => app.detail_back(),
                                    }
                                }
                                _ => app.terminal_focus_next_attention(),
                            }
                        }


                        // Tree: expand/collapse repo (Enter relaunch exited terminal in Terminals view)
                        (_, KeyCode::Enter) => {
                            if app.active_view == ActiveView::Terminals {
                                // Only relaunch exited terminals; F12 is the focus toggle
                                let is_exited = app
                                    .terminal_manager
                                    .as_ref()
                                    .and_then(|m| m.focused_terminal())
                                    .is_some_and(|t| {
                                        matches!(t.status, terminal::TerminalStatus::Exited(_))
                                    });
                                if is_exited {
                                    app.terminal_relaunch_focused();
                                }
                            } else {
                                app.tree_toggle();
                            }
                        }
                        (_, KeyCode::Char(' ')) => {
                            if app.active_view != ActiveView::Terminals {
                                app.tree_toggle();
                            }
                        }
                        // Alt+Arrow: tile navigation in Terminals view (must come before plain arrows)
                        (KeyModifiers::ALT, KeyCode::Left) => {
                            if app.active_view == ActiveView::Terminals {
                                app.terminal_tile_focus_left();
                            }
                        }
                        (KeyModifiers::ALT, KeyCode::Right) => {
                            if app.active_view == ActiveView::Terminals {
                                app.terminal_tile_focus_right();
                            }
                        }
                        (KeyModifiers::ALT, KeyCode::Up) => {
                            if app.active_view == ActiveView::Terminals {
                                app.terminal_tile_focus_up();
                            }
                        }
                        (KeyModifiers::ALT, KeyCode::Down) => {
                            if app.active_view == ActiveView::Terminals {
                                app.terminal_tile_focus_down();
                            }
                        }

                        (_, KeyCode::Right) => {
                            if app.active_view == ActiveView::Terminals {
                                app.terminal_layout = app.terminal_layout.next();
                            } else {
                                app.tree_expand();
                            }
                        }
                        (_, KeyCode::Left) => {
                            if app.active_view != ActiveView::Terminals {
                                app.tree_collapse();
                            }
                        }

                        // Item navigation
                        (_, KeyCode::Up) | (_, KeyCode::Char('k')) => {
                            if app.active_view == ActiveView::Terminals {
                                app.terminal_focus_prev();
                            } else {
                                app.prev_item();
                            }
                        }
                        (_, KeyCode::Down) | (_, KeyCode::Char('j')) => {
                            if app.active_view == ActiveView::Terminals {
                                app.terminal_focus_next();
                            } else {
                                app.next_item();
                            }
                        }
                        (_, KeyCode::PageDown) => app.tree_page_down(page_size),
                        (_, KeyCode::PageUp) => app.tree_page_up(page_size),

                        // Pane focus / Terminal focus toggle
                        (_, KeyCode::Tab) => {
                            app.toggle_focus();
                        }

                        // Permission queue
                        (_, KeyCode::F(8)) => app.open_permission_popup(),

                        // F9: toggle pane focus (all views), or enter terminal focus
                        (_, KeyCode::F(9)) => {
                            if app.active_view == ActiveView::Terminals
                                && app
                                    .terminal_manager
                                    .as_ref()
                                    .and_then(|m| m.focused_terminal())
                                    .is_some_and(|t| t.is_embedded())
                            {
                                app.terminal_input_mode = TerminalInputMode::TerminalFocused;
                            } else {
                                app.toggle_focus();
                            }
                        }
                        // F12: enter terminal focus (Terminals view only, embedded terminals only)
                        (_, KeyCode::F(12)) => {
                            if app.active_view == ActiveView::Terminals
                                && app
                                    .terminal_manager
                                    .as_ref()
                                    .and_then(|m| m.focused_terminal())
                                    .is_some_and(|t| t.is_embedded())
                            {
                                app.terminal_input_mode = TerminalInputMode::TerminalFocused;
                            }
                        }

                        // Scroll-back mode ([ in Terminals view Normal mode, embedded only)
                        (_, KeyCode::Char('[')) => {
                            if app.active_view == ActiveView::Terminals
                                && app
                                    .terminal_manager
                                    .as_ref()
                                    .and_then(|m| m.focused_terminal())
                                    .is_some_and(|t| t.is_embedded())
                            {
                                app.terminal_input_mode = TerminalInputMode::ScrollBack;
                            }
                        }

                        // Cycle views
                        (_, KeyCode::Char('`')) => app.next_view(),

                        // Dismiss ALL exited terminals at once (Shift+D)
                        (_, KeyCode::Char('D')) => {
                            if app.active_view == ActiveView::Terminals {
                                app.terminal_dismiss_all_exited();
                            }
                        }

                        // d: clean up worktree for current task (Tasks view only)
                        (_, KeyCode::Char('d')) => {
                            if app.active_view == ActiveView::Tasks {
                                app.open_single_task_cleanup();
                            }
                        }

                        // f: cycle task filter (Tasks view, left pane focused)
                        (_, KeyCode::Char('f')) => {
                            if app.active_view == ActiveView::Tasks
                                && app.focus_pane == FocusPane::Left
                            {
                                app.cycle_task_filter();
                            }
                        }

                        // Delete: Tasks view=cleanup worktree, Terminals view=dismiss exited
                        (_, KeyCode::Delete) => {
                            if app.active_view == ActiveView::Tasks {
                                app.open_single_task_cleanup();
                            } else if app.active_view == ActiveView::Terminals {
                                let is_exited = app
                                    .terminal_manager
                                    .as_ref()
                                    .and_then(|m| m.focused_terminal())
                                    .is_some_and(|t| {
                                        matches!(t.status, terminal::TerminalStatus::Exited(_))
                                    });
                                if is_exited {
                                    app.terminal_dismiss_focused();
                                }
                            }
                        }

                        _ => {}
                    }
                    } // close base else block
                }
            }
        }

        // Fire debounced search after keystrokes are consumed
        app.tick_search_debounce();

        // Tick modifier bar flash timeout (for non-kitty terminals)
        app.tick_modifier_bar();

        if app.should_quit {
            return Ok(());
        }

        // Check for create worktree completion each tick
        app.create_popup_check_completion();

        // Check for cleanup worktree completion each tick
        app.cleanup_popup_check_completion();

        // Check for background refresh completion each tick
        app.check_bg_refresh();

        // Drain hook events from the HTTP server
        app.drain_hook_events();

        // Poll embedded terminals for exit/attention status changes
        app.poll_terminals();

        // Auto-refresh on poll interval
        if app.last_refresh.elapsed() >= Duration::from_secs(app.poll_interval_secs) {
            app.refresh();
        }
    }
}
