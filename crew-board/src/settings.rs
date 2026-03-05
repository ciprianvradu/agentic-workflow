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
            auto_embed_on_create: true,
            idle_timeout_secs: 120,
            visual_bell: true,
            system_bell: false,
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

fn default_idle_timeout() -> u64 {
    120
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
}
