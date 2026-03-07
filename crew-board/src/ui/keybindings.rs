//! Keybinding registry: single source of truth for Fn key labels and actions.
//!
//! Both `status_bar.rs` (rendering) and the help popup read from this module.
//! Key routing in `main.rs` stays as explicit match arms but must stay in sync
//! with what is defined here.

use crate::app::{ActiveView, DetailMode, ModifierBarState, TerminalInputMode};

/// Which sub-context within a view is currently active.
/// Drives which bindings are visible in the bottom bar.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum SubContext {
    /// No special sub-context — show the view's base bindings.
    Base,
    /// Tasks view: the doc list is open.
    DocList,
    /// Tasks view: a document is open for reading.
    DocReader,
    /// Tasks view: history is open.
    History,
}

/// A single Fn key binding entry.
#[derive(Debug, Clone)]
pub struct FKeyBinding {
    /// F-key number (1-12).
    pub slot: u8,
    /// Short label (fits in CELL_WIDTH=9 chars, e.g. "Docs").
    pub label_short: &'static str,
    /// Long label for wide terminals (e.g. "Documents").
    pub label_long: &'static str,
    /// Whether this binding is currently active (false → render as empty/dimmed).
    pub active: bool,
}

impl FKeyBinding {
    pub fn new(slot: u8, label_short: &'static str, label_long: &'static str) -> Self {
        Self { slot, label_short, label_long, active: true }
    }

    pub fn inactive(slot: u8) -> Self {
        Self { slot, label_short: "", label_long: "", active: false }
    }
}

/// The full context needed to look up bindings.
pub struct BindingContext {
    pub view: ActiveView,
    pub sub_ctx: SubContext,
    pub modifier: ModifierBarState,
    pub terminal_mode: TerminalInputMode,
}

/// Return the ordered list of F-key bindings (F1-F10) for the given context.
/// The returned array always has exactly 10 entries (one per slot F1..=F10).
/// Entries with `active=false` render as dimmed empty slots.
pub fn bindings_for(ctx: &BindingContext) -> [FKeyBinding; 10] {
    use ModifierBarState::*;
    use ActiveView::*;
    use SubContext::*;

    // ── Shift layer: always view switching ───────────────────────────────
    if ctx.modifier == Shift {
        return shift_layer();
    }

    // ── Ctrl layer: view-specific or global ──────────────────────────────
    if ctx.modifier == Ctrl {
        return ctrl_layer(ctx.view);
    }

    // ── TerminalFocused mode overrides base layer ─────────────────────────
    if ctx.terminal_mode == TerminalInputMode::TerminalFocused {
        return terminal_focused_layer();
    }

    // ── Base layer per view + sub-context ────────────────────────────────
    match (ctx.view, ctx.sub_ctx) {
        (Tasks, Base) => tasks_base(),
        (Tasks, DocList) => tasks_doclist(),
        (Tasks, DocReader) | (Tasks, History) => tasks_reader(),
        (BeadsIssues, _) => issues_base(),
        (Config, _) => config_base(),
        (CostSummary, _) => cost_base(),
        (Terminals, _) => terminals_base(),
        (ActivityFeed, _) => activity_base(),
    }
}

/// Derive the current sub-context from app state.
pub fn sub_context_for(view: ActiveView, detail_mode: &DetailMode) -> SubContext {
    if view != ActiveView::Tasks {
        return SubContext::Base;
    }
    match detail_mode {
        DetailMode::Overview => SubContext::Base,
        DetailMode::DocList { .. } => SubContext::DocList,
        DetailMode::DocReader { .. } => SubContext::DocReader,
        DetailMode::History => SubContext::History,
    }
}

// ── Per-view binding tables ───────────────────────────────────────────────

fn shift_layer() -> [FKeyBinding; 10] {
    [
        FKeyBinding::new(1,  "Tasks",  "Tasks"),
        FKeyBinding::new(2,  "Issues", "Issues"),
        FKeyBinding::new(3,  "Config", "Config"),
        FKeyBinding::new(4,  "Cost",   "Cost"),
        FKeyBinding::new(5,  "Terms",  "Terminals"),
        FKeyBinding::new(6,  "Actvty", "Activity"),
        FKeyBinding::inactive(7),
        FKeyBinding::inactive(8),
        FKeyBinding::inactive(9),
        FKeyBinding::inactive(10),
    ]
}

fn ctrl_layer(view: ActiveView) -> [FKeyBinding; 10] {
    match view {
        ActiveView::Terminals => [
            FKeyBinding::inactive(1),
            FKeyBinding::inactive(2),
            FKeyBinding::inactive(3),
            FKeyBinding::new(4,  "DsmAll", "Dismiss All"),
            FKeyBinding::new(5,  "Live",   "Live View"),
            FKeyBinding::new(6,  "Stats",  "Statistics"),
            FKeyBinding::inactive(7),
            FKeyBinding::new(8,  "ScrlBk", "Scroll Back"),
            FKeyBinding::inactive(9),
            FKeyBinding::inactive(10),
        ],
        ActiveView::ActivityFeed => [
            FKeyBinding::inactive(1),
            FKeyBinding::inactive(2),
            FKeyBinding::inactive(3),
            FKeyBinding::new(4,  "Crew",   "Crew Filter"),
            FKeyBinding::new(5,  "Event",  "Event Filter"),
            FKeyBinding::new(6,  "Tool",   "Tool Filter"),
            FKeyBinding::new(7,  "Auto",   "Auto Scroll"),
            FKeyBinding::new(8,  "Gantt",  "Gantt View"),
            FKeyBinding::inactive(9),
            FKeyBinding::inactive(10),
        ],
        _ => [
            FKeyBinding::inactive(1),
            FKeyBinding::inactive(2),
            FKeyBinding::inactive(3),
            FKeyBinding::inactive(4),
            FKeyBinding::inactive(5),
            FKeyBinding::new(6,  "Stats",  "Statistics"),
            FKeyBinding::inactive(7),
            FKeyBinding::inactive(8),
            FKeyBinding::inactive(9),
            FKeyBinding::inactive(10),
        ],
    }
}

fn terminal_focused_layer() -> [FKeyBinding; 10] {
    [
        FKeyBinding::inactive(1),
        FKeyBinding::inactive(2),
        FKeyBinding::inactive(3),
        FKeyBinding::inactive(4),
        FKeyBinding::new(5,  "Prev",   "Prev Terminal"),
        FKeyBinding::new(6,  "Next",   "Next Terminal"),
        FKeyBinding::new(7,  "Attn",   "Attention"),
        FKeyBinding::inactive(8),
        FKeyBinding::inactive(9),
        FKeyBinding::inactive(10),
    ]
}

fn tasks_base() -> [FKeyBinding; 10] {
    [
        FKeyBinding::new(1,  "Help",   "Help"),
        FKeyBinding::new(2,  "Launch", "Launch"),
        FKeyBinding::new(3,  "Search", "Search"),
        FKeyBinding::new(4,  "New",    "New Worktree"),
        FKeyBinding::new(5,  "Rfrsh",  "Refresh"),
        FKeyBinding::new(6,  "Docs",   "Documents"),
        FKeyBinding::new(7,  "Hist",   "History"),
        FKeyBinding::new(8,  "Perms",  "Permissions"),
        FKeyBinding::inactive(9),
        FKeyBinding::new(10, "Quit",   "Quit"),
    ]
}

fn tasks_doclist() -> [FKeyBinding; 10] {
    [
        FKeyBinding::new(1,  "Help",   "Help"),
        FKeyBinding::new(2,  "Launch", "Launch"),
        FKeyBinding::new(3,  "Search", "Search"),
        FKeyBinding::inactive(4),
        FKeyBinding::new(5,  "Rfrsh",  "Refresh"),
        FKeyBinding::new(6,  "Open",   "Open Doc"),
        FKeyBinding::new(7,  "Back",   "Back"),
        FKeyBinding::new(8,  "Perms",  "Permissions"),
        FKeyBinding::inactive(9),
        FKeyBinding::new(10, "Quit",   "Quit"),
    ]
}

fn tasks_reader() -> [FKeyBinding; 10] {
    [
        FKeyBinding::new(1,  "Help",   "Help"),
        FKeyBinding::new(2,  "Launch", "Launch"),
        FKeyBinding::new(3,  "Search", "Search"),
        FKeyBinding::inactive(4),
        FKeyBinding::new(5,  "Rfrsh",  "Refresh"),
        FKeyBinding::inactive(6),
        FKeyBinding::new(7,  "Back",   "Back"),
        FKeyBinding::new(8,  "Perms",  "Permissions"),
        FKeyBinding::inactive(9),
        FKeyBinding::new(10, "Quit",   "Quit"),
    ]
}

fn issues_base() -> [FKeyBinding; 10] {
    [
        FKeyBinding::new(1,  "Help",   "Help"),
        FKeyBinding::new(2,  "Launch", "Launch"),
        FKeyBinding::new(3,  "Search", "Search"),
        FKeyBinding::inactive(4),
        FKeyBinding::new(5,  "Rfrsh",  "Refresh"),
        FKeyBinding::inactive(6),
        FKeyBinding::inactive(7),
        FKeyBinding::new(8,  "Perms",  "Permissions"),
        FKeyBinding::inactive(9),
        FKeyBinding::new(10, "Quit",   "Quit"),
    ]
}

fn config_base() -> [FKeyBinding; 10] {
    [
        FKeyBinding::new(1,  "Help",   "Help"),
        FKeyBinding::new(2,  "Launch", "Launch"),
        FKeyBinding::new(3,  "Search", "Search"),
        FKeyBinding::inactive(4),
        FKeyBinding::new(5,  "Rfrsh",  "Refresh"),
        FKeyBinding::inactive(6),
        FKeyBinding::inactive(7),
        FKeyBinding::new(8,  "Perms",  "Permissions"),
        FKeyBinding::inactive(9),
        FKeyBinding::new(10, "Quit",   "Quit"),
    ]
}

fn cost_base() -> [FKeyBinding; 10] {
    config_base()  // same as config for now
}

fn terminals_base() -> [FKeyBinding; 10] {
    [
        FKeyBinding::new(1,  "Help",   "Help"),
        FKeyBinding::new(2,  "Launch", "Launch"),
        FKeyBinding::new(3,  "Search", "Search"),
        FKeyBinding::new(4,  "Layot",  "Layout"),
        FKeyBinding::new(5,  "Rfrsh",  "Refresh"),
        FKeyBinding::new(6,  "Dsmis",  "Dismiss"),
        FKeyBinding::new(7,  "Attn",   "Attention"),
        FKeyBinding::new(8,  "Perms",  "Permissions"),
        FKeyBinding::new(9,  "Focus",  "Focus Term"),
        FKeyBinding::new(10, "Quit",   "Quit"),
    ]
}

fn activity_base() -> [FKeyBinding; 10] {
    [
        FKeyBinding::new(1,  "Help",   "Help"),
        FKeyBinding::new(2,  "Launch", "Launch"),
        FKeyBinding::new(3,  "Search", "Search"),
        FKeyBinding::inactive(4),
        FKeyBinding::new(5,  "Rfrsh",  "Refresh"),
        FKeyBinding::inactive(6),
        FKeyBinding::inactive(7),
        FKeyBinding::new(8,  "Perms",  "Permissions"),
        FKeyBinding::inactive(9),
        FKeyBinding::new(10, "Quit",   "Quit"),
    ]
}
