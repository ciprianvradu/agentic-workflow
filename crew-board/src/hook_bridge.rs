//! Cross-platform hook bridge for non-Claude AI hosts.
//!
//! Generates shell scripts and configuration files that allow Gemini CLI,
//! GitHub Copilot, and OpenCode to communicate with the crew-board hook server.

use std::path::{Path, PathBuf};

/// AI host type for hook configuration.
#[derive(Debug, Clone, Copy, PartialEq)]
#[allow(dead_code)]
pub enum AiHostType {
    Claude,
    Gemini,
    Copilot,
    OpenCode,
    Shell,
}

#[allow(dead_code)]
impl AiHostType {
    /// Normalize external event names to internal event names.
    pub fn normalize_event_name(&self, event: &str) -> Option<&'static str> {
        match self {
            AiHostType::Claude => match event {
                "PreToolUse" => Some("PreToolUse"),
                "PostToolUse" => Some("PostToolUse"),
                "SessionStart" => Some("SessionStart"),
                "SessionEnd" => Some("SessionEnd"),
                "Notification" => Some("Notification"),
                "Stop" => Some("Stop"),
                "PermissionRequest" => Some("PermissionRequest"),
                "UserPromptSubmit" => Some("UserPromptSubmit"),
                _ => None,
            },
            AiHostType::Gemini => match event {
                "BeforeTool" | "before_tool" => Some("PreToolUse"),
                "AfterTool" | "after_tool" => Some("PostToolUse"),
                "SessionStart" | "session_start" => Some("SessionStart"),
                "SessionEnd" | "session_end" => Some("SessionEnd"),
                _ => None,
            },
            AiHostType::Copilot => match event {
                "tool.execute.before" => Some("PreToolUse"),
                "tool.execute.after" => Some("PostToolUse"),
                "session.start" => Some("SessionStart"),
                "session.end" => Some("SessionEnd"),
                _ => None,
            },
            AiHostType::OpenCode => match event {
                "pre_tool" | "PreTool" => Some("PreToolUse"),
                "post_tool" | "PostTool" => Some("PostToolUse"),
                "session_start" => Some("SessionStart"),
                "session_end" => Some("SessionEnd"),
                _ => None,
            },
            AiHostType::Shell => None,
        }
    }
}

/// Generate the crew-hook.sh bridge script content.
///
/// This script reads JSON from stdin, POSTs it to crew-board, and echoes the response.
/// It's used by non-Claude hosts that support shell-based hooks.
#[allow(dead_code)]
pub fn generate_bridge_script(port: u16, terminal_id: &str, token: &str) -> String {
    format!(
        r#"#!/bin/bash
# crew-board hook bridge — auto-generated, do not edit
# Reads JSON from stdin, POSTs to crew-board hook server, echoes response.

CREW_BOARD_PORT={}
CREW_BOARD_TASK_ID="{}"
CREW_BOARD_TOKEN="{}"

INPUT=$(cat)
RESPONSE=$(curl -s -X POST \
  -H "Content-Type: application/json" \
  -H "Authorization: Bearer $CREW_BOARD_TOKEN" \
  -d "$INPUT" \
  "http://127.0.0.1:$CREW_BOARD_PORT/hook/$CREW_BOARD_TASK_ID" \
  --max-time 30 2>/dev/null)

if [ $? -eq 0 ] && [ -n "$RESPONSE" ]; then
  echo "$RESPONSE"
else
  echo '{{}}'
fi
"#,
        port, terminal_id, token
    )
}

/// Generated hook configuration files for a specific host.
#[derive(Debug)]
#[allow(dead_code)]
pub struct HookConfig {
    /// Files to write: (path, content).
    pub files: Vec<(PathBuf, String)>,
    /// Paths to clean up on terminal dismiss.
    pub cleanup_paths: Vec<PathBuf>,
}

/// Generate hook configuration for a specific AI host.
#[allow(dead_code)]
pub fn generate_hook_config(
    host: AiHostType,
    port: u16,
    terminal_id: &str,
    token: &str,
    cwd: &Path,
) -> Option<HookConfig> {
    match host {
        AiHostType::Claude => None, // Claude uses settings.local.json (handled elsewhere)
        AiHostType::Shell => None,  // Shell has no hook system
        AiHostType::Gemini => Some(generate_gemini_config(port, terminal_id, token, cwd)),
        AiHostType::Copilot => Some(generate_copilot_config(port, terminal_id, token, cwd)),
        AiHostType::OpenCode => Some(generate_opencode_config(port, terminal_id, token, cwd)),
    }
}

#[allow(dead_code)]
fn generate_gemini_config(port: u16, terminal_id: &str, token: &str, cwd: &Path) -> HookConfig {
    let script = generate_bridge_script(port, terminal_id, token);
    let script_path = cwd.join(".gemini").join("crew-hook.sh");

    // Gemini CLI settings.json hook configuration
    let settings = serde_json::json!({
        "hooks": {
            "before_tool": {
                "command": script_path.to_string_lossy(),
                "timeout": 30
            },
            "after_tool": {
                "command": script_path.to_string_lossy(),
                "timeout": 5
            },
            "session_start": {
                "command": script_path.to_string_lossy(),
                "timeout": 5
            }
        }
    });
    let settings_path = cwd.join(".gemini").join("settings.json");

    HookConfig {
        files: vec![
            (script_path.clone(), script),
            (settings_path.clone(), serde_json::to_string_pretty(&settings).unwrap_or_default()),
        ],
        cleanup_paths: vec![script_path, settings_path],
    }
}

#[allow(dead_code)]
fn generate_copilot_config(port: u16, terminal_id: &str, token: &str, cwd: &Path) -> HookConfig {
    let script = generate_bridge_script(port, terminal_id, token);
    let script_path = cwd.join(".github").join("hooks").join("crew-hook.sh");

    let hooks = serde_json::json!({
        "hooks": [
            {
                "event": "tool.execute.before",
                "command": script_path.to_string_lossy(),
                "timeout": 30
            },
            {
                "event": "tool.execute.after",
                "command": script_path.to_string_lossy(),
                "timeout": 5
            },
            {
                "event": "session.start",
                "command": script_path.to_string_lossy(),
                "timeout": 5
            }
        ]
    });
    let hooks_path = cwd.join(".github").join("hooks").join("hooks.json");

    HookConfig {
        files: vec![
            (script_path.clone(), script),
            (hooks_path.clone(), serde_json::to_string_pretty(&hooks).unwrap_or_default()),
        ],
        cleanup_paths: vec![script_path, hooks_path],
    }
}

#[allow(dead_code)]
fn generate_opencode_config(port: u16, terminal_id: &str, token: &str, cwd: &Path) -> HookConfig {
    let script = generate_bridge_script(port, terminal_id, token);
    let script_path = cwd.join(".opencode").join("plugins").join("crew-hook.sh");

    // OpenCode uses a TypeScript plugin, but we provide a shell script alternative
    let plugin_ts = format!(
        r#"// crew-board hook plugin — auto-generated
// Forwards hook events to the crew-board HTTP server
const PORT = {};
const TASK_ID = "{}";
const TOKEN = "{}";

export async function onPreTool(event: any) {{
  return fetch(`http://127.0.0.1:${{PORT}}/hook/${{TASK_ID}}`, {{
    method: "POST",
    headers: {{ "Authorization": `Bearer ${{TOKEN}}`, "Content-Type": "application/json" }},
    body: JSON.stringify({{ hook_event_name: "PreToolUse", ...event }}),
  }}).then(r => r.json()).catch(() => ({{}}));
}}

export async function onPostTool(event: any) {{
  return fetch(`http://127.0.0.1:${{PORT}}/hook/${{TASK_ID}}`, {{
    method: "POST",
    headers: {{ "Authorization": `Bearer ${{TOKEN}}`, "Content-Type": "application/json" }},
    body: JSON.stringify({{ hook_event_name: "PostToolUse", ...event }}),
  }}).then(r => r.json()).catch(() => ({{}}));
}}
"#,
        port, terminal_id, token
    );
    let ts_path = cwd.join(".opencode").join("plugins").join("crew-board.ts");

    HookConfig {
        files: vec![
            (script_path.clone(), script),
            (ts_path.clone(), plugin_ts),
        ],
        cleanup_paths: vec![script_path, ts_path],
    }
}

/// Format a response for a specific AI host.
/// Different hosts expect different JSON schemas for allow/deny decisions.
#[allow(dead_code)]
pub fn format_response(host: AiHostType, allow: bool, reason: &str) -> String {
    match host {
        AiHostType::Claude => {
            if allow {
                serde_json::json!({
                    "hookSpecificOutput": {
                        "permissionDecision": "allow",
                        "permissionDecisionReason": "Approved by crew-board"
                    }
                }).to_string()
            } else {
                serde_json::json!({
                    "hookSpecificOutput": {
                        "permissionDecision": "deny",
                        "permissionDecisionReason": reason
                    }
                }).to_string()
            }
        }
        AiHostType::Gemini | AiHostType::Copilot | AiHostType::OpenCode => {
            if allow {
                serde_json::json!({"action": "allow"}).to_string()
            } else {
                serde_json::json!({"action": "deny", "reason": reason}).to_string()
            }
        }
        AiHostType::Shell => "{}".to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_claude_event_normalization() {
        let host = AiHostType::Claude;
        assert_eq!(host.normalize_event_name("PreToolUse"), Some("PreToolUse"));
        assert_eq!(host.normalize_event_name("Unknown"), None);
    }

    #[test]
    fn test_gemini_event_normalization() {
        let host = AiHostType::Gemini;
        assert_eq!(host.normalize_event_name("BeforeTool"), Some("PreToolUse"));
        assert_eq!(host.normalize_event_name("AfterTool"), Some("PostToolUse"));
        assert_eq!(host.normalize_event_name("session_start"), Some("SessionStart"));
    }

    #[test]
    fn test_copilot_event_normalization() {
        let host = AiHostType::Copilot;
        assert_eq!(host.normalize_event_name("tool.execute.before"), Some("PreToolUse"));
        assert_eq!(host.normalize_event_name("tool.execute.after"), Some("PostToolUse"));
    }

    #[test]
    fn test_bridge_script_generation() {
        let script = generate_bridge_script(12345, "TASK_001", "secret-token");
        assert!(script.contains("12345"));
        assert!(script.contains("TASK_001"));
        assert!(script.contains("secret-token"));
        assert!(script.contains("curl"));
    }

    #[test]
    fn test_gemini_config_generation() {
        let config = generate_hook_config(
            AiHostType::Gemini, 12345, "TASK_001", "tok",
            std::path::Path::new("/tmp/test-repo"),
        );
        assert!(config.is_some());
        let config = config.unwrap();
        assert_eq!(config.files.len(), 2); // script + settings
        assert_eq!(config.cleanup_paths.len(), 2);
    }

    #[test]
    fn test_claude_no_config() {
        let config = generate_hook_config(
            AiHostType::Claude, 12345, "TASK_001", "tok",
            std::path::Path::new("/tmp"),
        );
        assert!(config.is_none()); // Claude handled elsewhere
    }

    #[test]
    fn test_response_formatting() {
        let allow = format_response(AiHostType::Claude, true, "");
        assert!(allow.contains("allow"));

        let deny = format_response(AiHostType::Gemini, false, "Not allowed");
        assert!(deny.contains("deny"));
        assert!(deny.contains("Not allowed"));
    }
}
