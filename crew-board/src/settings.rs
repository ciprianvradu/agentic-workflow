use serde::Deserialize;
use std::path::PathBuf;

/// Persistent user settings loaded from ~/.config/crew-board.toml
#[derive(Debug, Deserialize)]
#[allow(dead_code)]
pub struct Settings {
    /// Explicit repo paths
    #[serde(default)]
    pub repos: Vec<String>,

    /// Directories to scan one level deep for repos
    #[serde(default)]
    pub scan: Vec<String>,

    /// Poll interval in seconds
    pub poll_interval: Option<u64>,

    // ── Embedded Terminals ─────────────────────────────────────────

    /// Master toggle for embedded terminal feature.
    #[serde(default = "default_true")]
    pub embed_terminals: bool,

    /// Prefix key for navigation (tmux syntax: "C-b", "C-a", "C-Space").
    #[serde(default = "default_prefix_key")]
    pub prefix_key: String,

    /// Default layout mode: "focused", "tiled-2", "tiled-4", "stacked".
    #[serde(default = "default_terminal_layout")]
    pub terminal_layout: String,

    /// Scrollback buffer size (lines per terminal).
    #[serde(default = "default_scrollback_lines")]
    pub scrollback_lines: u32,

    /// Left pane width percentage for Tasks view (default: 40).
    #[serde(default = "default_pane_width_tasks")]
    pub pane_width_tasks: u8,

    /// Left pane width percentage for Issues view (default: 40).
    #[serde(default = "default_pane_width_issues")]
    pub pane_width_issues: u8,

    /// Crew list width in characters for Terminals view (default: 20).
    #[serde(default = "default_pane_width_terminals")]
    pub pane_width_terminals: u8,

    /// Auto-launch embedded terminal on F4 worktree creation.
    #[serde(default = "default_true")]
    pub auto_embed_on_create: bool,

    // ── Attention & Notifications ──────────────────────────────────

    /// Idle detection timeout in seconds.
    #[serde(default = "default_idle_timeout")]
    pub idle_timeout_secs: u64,

    /// Flash the crew's status indicator when it needs attention.
    #[serde(default = "default_true")]
    pub visual_bell: bool,

    /// Trigger a terminal bell (\x07) when a crew needs attention.
    #[serde(default)]
    pub system_bell: bool,

    /// Directory for terminal output log files. Each terminal logs to
    /// `<log_directory>/<terminal_id>.log`. None = logging disabled.
    #[serde(default)]
    pub log_directory: Option<String>,

    // ── Permission Profiles ─────────────────────────────────────────

    /// Permission profile: "interactive" (default), "trusted", or "autonomous".
    /// - interactive: all prompts require manual approval
    /// - trusted: auto-approve prompts matching `auto_approve_patterns`
    /// - autonomous: auto-approve all permission prompts (y\n)
    #[serde(default = "default_permission_profile")]
    pub permission_profile: String,

    /// Regex patterns for auto-approval in "trusted" profile.
    /// Each pattern is matched against the permission context line.
    /// Example: ["(?i)read file", "(?i)list directory"]
    #[serde(default)]
    pub auto_approve_patterns: Vec<String>,

    /// Send desktop notification on terminal attention events.
    #[serde(default)]
    pub desktop_notifications: bool,

    /// Default value for per-terminal auto-accept toggle.
    /// When true, new terminals start with auto-accept enabled.
    /// Default: false (safe — all prompts require approval).
    #[serde(default)]
    pub auto_accept_default: bool,

    // ── Hook Communication ──────────────────────────────────────────

    /// Enable HTTP hook communication with embedded Claude Code terminals.
    /// When enabled, crew-board starts an HTTP server that receives structured
    /// hook events from Claude Code, replacing fragile screen parsing.
    #[serde(default = "default_true")]
    pub hook_communication: bool,

    // ── Security Rules ──────────────────────────────────────────────────

    /// Enable security rules enforcement on tool requests.
    #[serde(default)]
    pub security_enabled: bool,

    /// Security rules evaluated before permission profile checks.
    #[serde(default)]
    pub security_rules: Vec<crate::security::SecurityRuleConfig>,

    /// Regex patterns for credential detection in tool output.
    #[serde(default)]
    pub credential_patterns: Vec<String>,

    /// Maximum tool requests per minute per terminal (0 = unlimited).
    #[serde(default)]
    pub rate_limit_per_minute: u32,

    /// Sensitive file protection lists.
    #[serde(default)]
    pub sensitive_files: crate::security::SensitiveFiles,

    // ── History Capture ─────────────────────────────────────────────────

    /// Capture tool use events in history.jsonl.
    #[serde(default = "default_true")]
    pub capture_tool_events: bool,

    /// Capture user prompts in history.jsonl.
    #[serde(default)]
    pub capture_prompts: bool,

    /// Capture permission decisions in history.jsonl.
    #[serde(default = "default_true")]
    pub capture_permissions: bool,

    // ── Orchestration ─────────────────────────────────────────────────

    /// Orchestration mode: "manual" (default), "semi-auto", or "full-auto".
    #[serde(default)]
    pub orchestration_mode: String,

    /// Maximum concurrent orchestrated terminals.
    #[serde(default = "default_max_concurrent")]
    pub max_concurrent: u32,

    /// Cost limit in dollars for orchestration. 0 = unlimited.
    #[serde(default = "default_cost_limit")]
    pub cost_limit: f64,

    /// Maximum retries per orchestrated task.
    #[serde(default = "default_max_retries")]
    pub max_retries: u32,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            repos: Vec::new(),
            scan: Vec::new(),
            poll_interval: None,
            embed_terminals: true,
            prefix_key: "C-b".to_string(),
            terminal_layout: "focused".to_string(),
            scrollback_lines: 10000,
            pane_width_tasks: 40,
            pane_width_issues: 40,
            pane_width_terminals: 20,
            auto_embed_on_create: true,
            idle_timeout_secs: 120,
            visual_bell: true,
            system_bell: false,
            log_directory: None,
            permission_profile: "interactive".to_string(),
            auto_approve_patterns: Vec::new(),
            desktop_notifications: false,
            auto_accept_default: false,
            hook_communication: true,
            security_enabled: false,
            security_rules: Vec::new(),
            credential_patterns: Vec::new(),
            rate_limit_per_minute: 0,
            sensitive_files: crate::security::SensitiveFiles::default(),
            capture_tool_events: true,
            capture_prompts: false,
            capture_permissions: true,
            orchestration_mode: String::new(),
            max_concurrent: 5,
            cost_limit: 50.0,
            max_retries: 5,
        }
    }
}

fn default_true() -> bool {
    true
}

fn default_prefix_key() -> String {
    "C-b".to_string()
}

fn default_terminal_layout() -> String {
    "focused".to_string()
}

fn default_scrollback_lines() -> u32 {
    10000
}

fn default_pane_width_tasks() -> u8 {
    40
}

fn default_pane_width_issues() -> u8 {
    40
}

fn default_pane_width_terminals() -> u8 {
    20
}

fn default_idle_timeout() -> u64 {
    120
}

fn default_permission_profile() -> String {
    "interactive".to_string()
}

fn default_max_concurrent() -> u32 {
    5
}

fn default_cost_limit() -> f64 {
    50.0
}

fn default_max_retries() -> u32 {
    5
}

impl Settings {
    /// Load from the default config path. Returns Default if missing or malformed.
    pub fn load() -> Self {
        if let Some(path) = config_path() {
            Self::load_from(&path)
        } else {
            Self::default()
        }
    }

    fn load_from(path: &PathBuf) -> Self {
        let content = match std::fs::read_to_string(path) {
            Ok(c) => c,
            Err(_) => return Self::default(),
        };
        match toml::from_str::<Settings>(&content) {
            Ok(s) => s,
            Err(e) => {
                eprintln!(
                    "Warning: failed to parse {}: {}",
                    path.display(),
                    e
                );
                Self::default()
            }
        }
    }

    /// Parse the terminal_layout setting into a TerminalLayout enum.
    pub fn parsed_terminal_layout(&self) -> crate::app::TerminalLayout {
        match self.terminal_layout.as_str() {
            "tiled-2" => crate::app::TerminalLayout::Tiled2,
            "tiled-4" => crate::app::TerminalLayout::Tiled4,
            "stacked" => crate::app::TerminalLayout::Stacked,
            _ => crate::app::TerminalLayout::Focused,
        }
    }
}

/// Returns ~/.config/crew-board.toml (XDG-style).
pub fn config_path() -> Option<PathBuf> {
    dirs::config_dir().map(|d| d.join("crew-board.toml"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn test_load_missing_file() {
        let settings = Settings::load_from(&PathBuf::from("/tmp/nonexistent-crew-board.toml"));
        assert!(settings.repos.is_empty());
        assert!(settings.scan.is_empty());
        assert!(settings.poll_interval.is_none());
        // New defaults
        assert!(settings.embed_terminals);
        assert_eq!(settings.prefix_key, "C-b");
        assert_eq!(settings.terminal_layout, "focused");
        assert_eq!(settings.scrollback_lines, 10000);
        assert_eq!(settings.idle_timeout_secs, 120);
        assert!(settings.visual_bell);
        assert!(!settings.system_bell);
    }

    #[test]
    fn test_load_valid_config() {
        let tmp = std::env::temp_dir().join("crew-board-test-config.toml");
        fs::write(
            &tmp,
            r#"
repos = ["/mnt/c/git/project-a"]
scan = ["/mnt/c/git"]
poll_interval = 5
"#,
        )
        .unwrap();
        let settings = Settings::load_from(&tmp);
        assert_eq!(settings.repos, vec!["/mnt/c/git/project-a"]);
        assert_eq!(settings.scan, vec!["/mnt/c/git"]);
        assert_eq!(settings.poll_interval, Some(5));
        let _ = fs::remove_file(&tmp);
    }

    #[test]
    fn test_load_partial_config() {
        let tmp = std::env::temp_dir().join("crew-board-test-partial.toml");
        fs::write(&tmp, "scan = [\"/mnt/c/git\"]\n").unwrap();
        let settings = Settings::load_from(&tmp);
        assert!(settings.repos.is_empty());
        assert_eq!(settings.scan, vec!["/mnt/c/git"]);
        assert!(settings.poll_interval.is_none());
        let _ = fs::remove_file(&tmp);
    }

    #[test]
    fn test_load_terminal_config() {
        let tmp = std::env::temp_dir().join("crew-board-test-terminal.toml");
        fs::write(
            &tmp,
            r#"
embed_terminals = false
prefix_key = "C-a"
terminal_layout = "tiled-2"
scrollback_lines = 5000
idle_timeout_secs = 60
visual_bell = false
system_bell = true
"#,
        )
        .unwrap();
        let settings = Settings::load_from(&tmp);
        assert!(!settings.embed_terminals);
        assert_eq!(settings.prefix_key, "C-a");
        assert_eq!(settings.terminal_layout, "tiled-2");
        assert_eq!(settings.scrollback_lines, 5000);
        assert_eq!(settings.idle_timeout_secs, 60);
        assert!(!settings.visual_bell);
        assert!(settings.system_bell);
        let _ = fs::remove_file(&tmp);
    }

    #[test]
    fn test_load_permission_config() {
        let tmp = std::env::temp_dir().join("crew-board-test-permission.toml");
        fs::write(
            &tmp,
            r#"
permission_profile = "trusted"
auto_approve_patterns = ["(?i)read file", "(?i)list directory"]
desktop_notifications = true
log_directory = "/tmp/crew-logs"
"#,
        )
        .unwrap();
        let settings = Settings::load_from(&tmp);
        assert_eq!(settings.permission_profile, "trusted");
        assert_eq!(settings.auto_approve_patterns.len(), 2);
        assert_eq!(settings.auto_approve_patterns[0], "(?i)read file");
        assert!(settings.desktop_notifications);
        assert_eq!(settings.log_directory, Some("/tmp/crew-logs".to_string()));
        let _ = fs::remove_file(&tmp);
    }

    #[test]
    fn test_default_permission_settings() {
        let settings = Settings::default();
        assert_eq!(settings.permission_profile, "interactive");
        assert!(settings.auto_approve_patterns.is_empty());
        assert!(!settings.desktop_notifications);
        assert!(settings.log_directory.is_none());
    }

    #[test]
    fn test_default_auto_accept_settings() {
        let settings = Settings::default();
        assert!(!settings.auto_accept_default);
    }

    #[test]
    fn test_load_auto_accept_config() {
        let tmp = std::env::temp_dir().join("crew-board-test-auto-accept.toml");
        fs::write(&tmp, "auto_accept_default = true\n").unwrap();
        let settings = Settings::load_from(&tmp);
        assert!(settings.auto_accept_default);
        let _ = fs::remove_file(&tmp);
    }

    #[test]
    fn test_default_security_settings() {
        let settings = Settings::default();
        assert!(!settings.security_enabled);
        assert!(settings.security_rules.is_empty());
        assert!(settings.credential_patterns.is_empty());
    }
}
