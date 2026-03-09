//! HTTP hook server for receiving Claude Code hook events.
//!
//! Runs a lightweight HTTP server on `127.0.0.1:0` (OS-assigned port) that
//! accepts POST /hook/<terminal-id> requests from embedded Claude Code terminals.
//! Events are validated, parsed and forwarded through an mpsc channel to the
//! main event loop.
//!
//! SessionStart hooks respond with pre-computed task context (markdown) so that
//! Claude Code injects it into the session at startup.
//!
//! PreToolUse and PermissionRequest hooks support blocking approval flow (Phase 2):
//! the server holds the HTTP connection open while waiting for user input via a
//! oneshot channel, then returns the appropriate permission decision JSON.

use anyhow::Result;
use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, RwLock};

/// Permission decision sent from main thread back to the hook server thread.
#[derive(Debug, Clone)]
pub enum PermissionDecision {
    Allow,
    Deny { message: String },
}

/// A pending permission request that must be shown in the F8 popup.
///
/// The HTTP request is held open on the server thread while waiting for the user
/// to approve or deny via the oneshot `response_tx` channel.
pub struct PendingPermission {
    pub terminal_id: String,
    pub tool_name: String,
    /// Short summary of the tool input for display.
    pub tool_input_summary: String,
    /// Full tool input JSON (for detailed display in popup).
    pub tool_input: Option<serde_json::Value>,
    /// "PreToolUse" or "PermissionRequest"
    pub event_type: String,
    /// Send the decision back through this channel to unblock the HTTP response.
    pub response_tx: std::sync::mpsc::Sender<PermissionDecision>,
}

impl std::fmt::Debug for PendingPermission {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PendingPermission")
            .field("terminal_id", &self.terminal_id)
            .field("tool_name", &self.tool_name)
            .field("tool_input_summary", &self.tool_input_summary)
            .field("event_type", &self.event_type)
            .finish()
    }
}

/// Per-terminal registration data stored in the token registry.
#[derive(Debug, Clone)]
pub struct TerminalRegistration {
    /// Auth token (Bearer) expected in every hook request.
    pub token: String,
    /// Optional markdown context injected on SessionStart.
    /// Pre-computed before the terminal is spawned so the server thread
    /// can include it in the response without accessing `App`.
    pub session_context: Option<String>,
    /// Permission profile: "interactive", "trusted", or "autonomous".
    pub permission_profile: String,
    /// Raw regex pattern strings for auto-approval (trusted profile).
    pub auto_approve_patterns: Vec<String>,
    /// AI host type: "claude", "gemini", "copilot", "opencode", "shell".
    #[allow(dead_code)]
    pub ai_host: String,
}

/// A hook event received from Claude Code.
#[derive(Debug, Clone)]
#[allow(dead_code)]
pub enum HookEvent {
    SessionStart {
        terminal_id: String,
        session_id: String,
    },
    PreToolUse {
        terminal_id: String,
        tool_name: String,
        tool_input_summary: String,
    },
    PostToolUse {
        terminal_id: String,
        tool_name: String,
        tool_input_summary: String,
        success: bool,
    },
    Notification {
        terminal_id: String,
        message: String,
    },
    Stop {
        terminal_id: String,
        preview: String,
    },
    SessionEnd {
        terminal_id: String,
    },
    /// PermissionRequest: Claude Code needs explicit user permission for a tool.
    PermissionRequest {
        terminal_id: String,
        tool_name: String,
        tool_input: serde_json::Value,
    },
    /// UserPromptSubmit: user submitted a new prompt to Claude Code.
    UserPromptSubmit {
        terminal_id: String,
        prompt_preview: String,
    },
}

/// The HTTP hook server handle.
pub struct HookServer {
    /// Port the server is listening on.
    pub port: u16,
    /// Shutdown signal — set to true to stop the server thread.
    shutdown: Arc<AtomicBool>,
    /// Token registry mapping terminal_id → registration data.
    tokens: Arc<RwLock<HashMap<String, TerminalRegistration>>>,
    /// Channel for sending PendingPermission requests to the main thread.
    /// Stored here to keep the channel alive (prevents sender from being dropped).
    #[allow(dead_code)]
    pending_tx: std::sync::mpsc::Sender<PendingPermission>,
}

impl HookServer {
    /// Start the HTTP hook server on `127.0.0.1:0`.
    ///
    /// Returns `(server, port, hook_receiver, pending_receiver)`.
    /// - `hook_receiver` delivers `HookEvent`s as they arrive (non-blocking).
    /// - `pending_receiver` delivers `PendingPermission`s that need user approval.
    ///   The HTTP connection is held open until the main thread sends a decision
    ///   through `PendingPermission.response_tx`.
    pub fn start() -> Result<(
        HookServer,
        u16,
        std::sync::mpsc::Receiver<HookEvent>,
        std::sync::mpsc::Receiver<PendingPermission>,
    )> {
        let server = tiny_http::Server::http("127.0.0.1:0")
            .map_err(|e| anyhow::anyhow!("Failed to bind hook server: {}", e))?;

        let port = server.server_addr().to_ip().map(|a| a.port()).unwrap_or(0);

        let shutdown = Arc::new(AtomicBool::new(false));
        let tokens: Arc<RwLock<HashMap<String, TerminalRegistration>>> =
            Arc::new(RwLock::new(HashMap::new()));

        let (tx, rx) = std::sync::mpsc::channel::<HookEvent>();
        let (pending_tx, pending_rx) = std::sync::mpsc::channel::<PendingPermission>();

        // Clone for background thread
        let shutdown_clone = Arc::clone(&shutdown);
        let tokens_clone = Arc::clone(&tokens);
        let pending_tx_clone = pending_tx.clone();

        std::thread::spawn(move || {
            run_server(server, shutdown_clone, tokens_clone, tx, pending_tx_clone);
        });

        let hook_server = HookServer {
            port,
            shutdown,
            tokens,
            pending_tx,
        };

        Ok((hook_server, port, rx, pending_rx))
    }

    /// Register a terminal_id with its auth token and optional session context.
    ///
    /// Uses the "interactive" permission profile (all requests queued to user).
    /// Use `register_token_with_profile` for full profile control.
    #[allow(dead_code)]
    pub fn register_token(
        &self,
        terminal_id: String,
        token: String,
        session_context: Option<String>,
    ) {
        self.register_token_with_profile(terminal_id, token, session_context, "interactive", vec![]);
    }

    /// Register with full permission profile configuration.
    pub fn register_token_with_profile(
        &self,
        terminal_id: String,
        token: String,
        session_context: Option<String>,
        permission_profile: &str,
        auto_approve_patterns: Vec<String>,
    ) {
        if let Ok(mut map) = self.tokens.write() {
            map.insert(
                terminal_id,
                TerminalRegistration {
                    token,
                    session_context,
                    permission_profile: permission_profile.to_string(),
                    auto_approve_patterns,
                    ai_host: "claude".to_string(),
                },
            );
        }
    }

    /// Deregister a terminal's token (on dismiss or exit).
    pub fn deregister_token(&self, terminal_id: &str) {
        if let Ok(mut map) = self.tokens.write() {
            map.remove(terminal_id);
        }
    }

    /// Shut down the server thread.
    pub fn shutdown(&self) {
        self.shutdown.store(true, Ordering::Relaxed);
    }
}

impl Drop for HookServer {
    fn drop(&mut self) {
        self.shutdown.store(true, Ordering::Relaxed);
    }
}

/// Background thread: accept and handle requests until shutdown.
fn run_server(
    server: tiny_http::Server,
    shutdown: Arc<AtomicBool>,
    tokens: Arc<RwLock<HashMap<String, TerminalRegistration>>>,
    tx: std::sync::mpsc::Sender<HookEvent>,
    pending_tx: std::sync::mpsc::Sender<PendingPermission>,
) {
    loop {
        if shutdown.load(Ordering::Relaxed) {
            break;
        }

        // Poll with a short timeout so we can check shutdown flag
        match server.recv_timeout(std::time::Duration::from_millis(200)) {
            Ok(Some(request)) => {
                handle_request(request, &tokens, &tx, &pending_tx);
            }
            Ok(None) => {
                // Timeout — check shutdown and loop
            }
            Err(_) => {
                // Server error — exit thread
                break;
            }
        }

        if shutdown.load(Ordering::Relaxed) {
            break;
        }
    }
}

/// Handle a single HTTP request.
fn handle_request(
    mut request: tiny_http::Request,
    tokens: &Arc<RwLock<HashMap<String, TerminalRegistration>>>,
    tx: &std::sync::mpsc::Sender<HookEvent>,
    pending_tx: &std::sync::mpsc::Sender<PendingPermission>,
) {
    // Only accept POST
    if *request.method() != tiny_http::Method::Post {
        let _ = request.respond(
            tiny_http::Response::from_string("Method Not Allowed")
                .with_status_code(tiny_http::StatusCode(405)),
        );
        return;
    }

    // Parse path: /hook/<terminal-id>
    let url = request.url().to_string();
    let terminal_id = match parse_terminal_id(&url) {
        Some(id) => id,
        None => {
            let _ = request.respond(
                tiny_http::Response::from_string("{}")
                    .with_status_code(tiny_http::StatusCode(404)),
            );
            return;
        }
    };

    // Validate Authorization: Bearer <token>
    let registration = {
        let map = match tokens.read() {
            Ok(m) => m,
            Err(_) => {
                let _ = request.respond(
                    tiny_http::Response::from_string("{}")
                        .with_status_code(tiny_http::StatusCode(500)),
                );
                return;
            }
        };
        map.get(terminal_id).cloned()
    };

    // If we don't know this terminal_id, still respond 200 but drop the event (error path E5)
    let reg = match registration {
        Some(r) => r,
        None => {
            let _ = request.respond(
                tiny_http::Response::from_string("{}")
                    .with_status_code(tiny_http::StatusCode(200)),
            );
            return;
        }
    };

    // Find the Authorization header
    let auth_header = request
        .headers()
        .iter()
        .find(|h| h.field.as_str().to_string().eq_ignore_ascii_case("Authorization"))
        .map(|h| h.value.as_str().to_string());

    let authorized = auth_header
        .as_deref()
        .and_then(|v| v.strip_prefix("Bearer "))
        .map(|provided| provided == reg.token)
        .unwrap_or(false);

    if !authorized {
        let _ = request.respond(
            tiny_http::Response::from_string("{}")
                .with_status_code(tiny_http::StatusCode(401)),
        );
        return;
    }

    // Read and parse the JSON body
    let mut body = String::new();
    if std::io::Read::read_to_string(request.as_reader(), &mut body).is_err() {
        let _ = request.respond(
            tiny_http::Response::from_string("{}")
                .with_status_code(tiny_http::StatusCode(400)),
        );
        return;
    }

    let event = match parse_hook_event(terminal_id.to_string(), &body) {
        Ok(ev) => ev,
        Err(_) => {
            let _ = request.respond(
                tiny_http::Response::from_string("{}")
                    .with_status_code(tiny_http::StatusCode(400)),
            );
            return;
        }
    };

    // Determine event type for special handling
    let is_session_start = matches!(&event, HookEvent::SessionStart { .. });
    let is_pre_tool_use = matches!(&event, HookEvent::PreToolUse { .. });
    let is_permission_request = matches!(&event, HookEvent::PermissionRequest { .. });

    // For PreToolUse and PermissionRequest: check permission profile and possibly block
    if is_pre_tool_use || is_permission_request {
        let (tool_name, tool_input_summary, tool_input_full) = match &event {
            HookEvent::PreToolUse { tool_name, tool_input_summary, .. } => {
                (tool_name.clone(), tool_input_summary.clone(), None)
            }
            HookEvent::PermissionRequest { tool_name, tool_input, .. } => {
                let summary = extract_tool_input_summary_from_value(tool_input);
                (tool_name.clone(), summary, Some(tool_input.clone()))
            }
            _ => unreachable!(),
        };

        let event_type_str = if is_pre_tool_use { "PreToolUse" } else { "PermissionRequest" };

        // Check permission profile
        let decision = check_permission_profile(&reg, &tool_name, &tool_input_summary);

        match decision {
            ProfileDecision::Allow => {
                // Immediate allow — send event for observability, then respond
                let _ = tx.send(event);
                let response_body = build_allow_response(event_type_str);
                let response = tiny_http::Response::from_string(response_body)
                    .with_header(json_content_type())
                    .with_status_code(tiny_http::StatusCode(200));
                let _ = request.respond(response);
                return;
            }
            ProfileDecision::Queue => {
                // Send event for observability
                let _ = tx.send(event);

                // Create a oneshot-style channel for the decision
                let (resp_tx, resp_rx) = std::sync::mpsc::channel::<PermissionDecision>();

                let pending = PendingPermission {
                    terminal_id: terminal_id.to_string(),
                    tool_name,
                    tool_input_summary,
                    tool_input: tool_input_full,
                    event_type: event_type_str.to_string(),
                    response_tx: resp_tx,
                };

                // If sending to main thread fails (app exited), default to allow
                if pending_tx.send(pending).is_err() {
                    let response_body = build_allow_response(event_type_str);
                    let response = tiny_http::Response::from_string(response_body)
                        .with_header(json_content_type())
                        .with_status_code(tiny_http::StatusCode(200));
                    let _ = request.respond(response);
                    return;
                }

                // Block waiting for user decision (25s timeout — 5s buffer before 30s Claude timeout).
                // On timeout, default to Allow for both PreToolUse and PermissionRequest
                // (unblocks Claude rather than leaving it stuck). For PreToolUse this corresponds
                // to the "ask" behavior (let Claude show its own native prompt).
                let decision = resp_rx
                    .recv_timeout(std::time::Duration::from_secs(25))
                    .unwrap_or(PermissionDecision::Allow);

                let response_body = build_decision_response(event_type_str, &decision);
                let response = tiny_http::Response::from_string(response_body)
                    .with_header(json_content_type())
                    .with_status_code(tiny_http::StatusCode(200));
                let _ = request.respond(response);
                return;
            }
        }
    }

    // Send the event (ignore send error if main loop exited)
    let _ = tx.send(event);

    // Build the response body.
    // For SessionStart, include pre-computed task context so Claude Code
    // injects it as additionalContext into the session.
    let response_body = if is_session_start {
        if let Some(ctx) = reg.session_context.as_deref() {
            // Claude Code reads `additionalContext` from the SessionStart hook response
            // and prepends it to the session context window.
            match serde_json::to_string(&serde_json::json!({
                "additionalContext": ctx
            })) {
                Ok(json) => json,
                Err(_) => "{}".to_string(),
            }
        } else {
            "{}".to_string()
        }
    } else {
        "{}".to_string()
    };

    let response = tiny_http::Response::from_string(response_body)
        .with_header(json_content_type())
        .with_status_code(tiny_http::StatusCode(200));
    let _ = request.respond(response);
}

/// The profile-based decision for a permission request.
enum ProfileDecision {
    Allow,
    Queue,
}

/// Check the permission profile and decide whether to allow immediately or queue.
fn check_permission_profile(
    reg: &TerminalRegistration,
    tool_name: &str,
    tool_input_summary: &str,
) -> ProfileDecision {
    match reg.permission_profile.as_str() {
        "autonomous" => ProfileDecision::Allow,
        "trusted" => {
            // Auto-approve if tool_name or summary matches any pattern
            let context = format!("{} {}", tool_name, tool_input_summary);
            let matches = reg.auto_approve_patterns.iter().any(|pattern| {
                regex::Regex::new(pattern)
                    .map(|re| re.is_match(&context))
                    .unwrap_or(false)
            });
            if matches {
                ProfileDecision::Allow
            } else {
                ProfileDecision::Queue
            }
        }
        _ => {
            // "interactive" (default) — always queue
            ProfileDecision::Queue
        }
    }
}

/// Build a Content-Type: application/json header.
fn json_content_type() -> tiny_http::Header {
    "Content-Type: application/json"
        .parse::<tiny_http::Header>()
        .unwrap()
}

/// Build the JSON response body for an Allow decision.
fn build_allow_response(event_type: &str) -> String {
    match event_type {
        "PreToolUse" => {
            serde_json::json!({
                "hookSpecificOutput": {
                    "hookEventName": "PreToolUse",
                    "permissionDecision": "allow",
                    "permissionDecisionReason": "Approved by crew-board"
                }
            })
            .to_string()
        }
        "PermissionRequest" => {
            serde_json::json!({
                "hookSpecificOutput": {
                    "hookEventName": "PermissionRequest",
                    "decision": {
                        "behavior": "allow"
                    }
                }
            })
            .to_string()
        }
        _ => "{}".to_string(),
    }
}

/// Build the JSON response body for a Deny decision.
fn build_deny_response(event_type: &str, message: &str) -> String {
    match event_type {
        "PreToolUse" => {
            serde_json::json!({
                "hookSpecificOutput": {
                    "hookEventName": "PreToolUse",
                    "permissionDecision": "deny",
                    "permissionDecisionReason": message
                }
            })
            .to_string()
        }
        "PermissionRequest" => {
            serde_json::json!({
                "hookSpecificOutput": {
                    "hookEventName": "PermissionRequest",
                    "decision": {
                        "behavior": "deny",
                        "message": message
                    }
                }
            })
            .to_string()
        }
        _ => "{}".to_string(),
    }
}

/// Build the JSON response based on the permission decision.
fn build_decision_response(event_type: &str, decision: &PermissionDecision) -> String {
    match decision {
        PermissionDecision::Allow => build_allow_response(event_type),
        PermissionDecision::Deny { message } => build_deny_response(event_type, message),
    }
}

/// Parse terminal_id from URL path `/hook/<terminal-id>`.
fn parse_terminal_id(url: &str) -> Option<&str> {
    let path = url.split('?').next().unwrap_or(url);
    path.strip_prefix("/hook/").and_then(|id| {
        if id.is_empty() {
            None
        } else {
            Some(id)
        }
    })
}

/// Parse a Claude Code hook event from JSON body.
///
/// Claude Code sends hook events with a `hook_event_name` field that identifies
/// the event type. The remaining fields depend on the event type.
fn parse_hook_event(terminal_id: String, body: &str) -> Result<HookEvent> {
    let v: serde_json::Value = serde_json::from_str(body)
        .map_err(|e| anyhow::anyhow!("JSON parse error: {}", e))?;

    let event_name = v
        .get("hook_event_name")
        .or_else(|| v.get("hookEventName"))
        .and_then(|n| n.as_str())
        .ok_or_else(|| anyhow::anyhow!("Missing hook_event_name"))?;

    let event = match event_name {
        "SessionStart" => HookEvent::SessionStart {
            terminal_id,
            session_id: v
                .get("session_id")
                .and_then(|s| s.as_str())
                .unwrap_or("")
                .to_string(),
        },
        "PreToolUse" => {
            let tool_name = v
                .get("tool_name")
                .and_then(|s| s.as_str())
                .unwrap_or("")
                .to_string();
            let tool_input_summary = extract_tool_input_summary(&v);
            HookEvent::PreToolUse {
                terminal_id,
                tool_name,
                tool_input_summary,
            }
        }
        "PostToolUse" => {
            let tool_name = v
                .get("tool_name")
                .and_then(|s| s.as_str())
                .unwrap_or("")
                .to_string();
            let tool_input_summary = extract_tool_input_summary(&v);
            // success is true unless the event contains an explicit failure indicator
            let success = !v
                .get("tool_response")
                .and_then(|r| r.get("is_error"))
                .and_then(|e| e.as_bool())
                .unwrap_or(false);
            HookEvent::PostToolUse {
                terminal_id,
                tool_name,
                tool_input_summary,
                success,
            }
        }
        "Notification" => {
            let message = v
                .get("message")
                .and_then(|s| s.as_str())
                .unwrap_or("")
                .to_string();
            HookEvent::Notification {
                terminal_id,
                message,
            }
        }
        "Stop" => {
            let preview = v
                .get("stop_hook_active")
                .and_then(|s| s.as_str())
                .or_else(|| v.get("transcript_path").and_then(|s| s.as_str()))
                .unwrap_or("")
                .to_string();
            HookEvent::Stop {
                terminal_id,
                preview,
            }
        }
        "SessionEnd" => HookEvent::SessionEnd { terminal_id },
        "PermissionRequest" => {
            let tool_name = v
                .get("tool_name")
                .and_then(|s| s.as_str())
                .unwrap_or("")
                .to_string();
            let tool_input = v
                .get("tool_input")
                .cloned()
                .unwrap_or(serde_json::Value::Object(serde_json::Map::new()));
            HookEvent::PermissionRequest {
                terminal_id,
                tool_name,
                tool_input,
            }
        }
        "UserPromptSubmit" => {
            let prompt = v
                .get("prompt")
                .or_else(|| v.get("user_prompt"))
                .and_then(|s| s.as_str())
                .unwrap_or("")
                .to_string();
            // Keep only first 200 chars as preview
            let preview = if prompt.len() > 200 {
                format!("{}...", &prompt[..197])
            } else {
                prompt
            };
            HookEvent::UserPromptSubmit {
                terminal_id,
                prompt_preview: preview,
            }
        }
        _ => {
            return Err(anyhow::anyhow!("Unknown event: {}", event_name));
        }
    };

    Ok(event)
}

/// Extract a short human-readable summary from tool_input.
///
/// For file tools (Edit, Read, Write, Bash, etc.) extracts the most relevant
/// argument (file path, command, etc.) and truncates to ~50 chars.
fn extract_tool_input_summary(v: &serde_json::Value) -> String {
    let input = match v.get("tool_input") {
        Some(i) => i,
        None => return String::new(),
    };
    extract_tool_input_summary_from_value(input)
}

/// Extract a short human-readable summary from a tool_input Value object.
fn extract_tool_input_summary_from_value(input: &serde_json::Value) -> String {
    // Try common field names in priority order
    if let Some(s) = input.get("file_path").and_then(|v| v.as_str()) {
        return shorten_path(s, 40);
    }
    if let Some(s) = input.get("path").and_then(|v| v.as_str()) {
        return shorten_path(s, 40);
    }
    if let Some(s) = input.get("command").and_then(|v| v.as_str()) {
        return truncate(s, 40);
    }
    if let Some(s) = input.get("query").and_then(|v| v.as_str()) {
        return truncate(s, 40);
    }
    if let Some(s) = input.get("pattern").and_then(|v| v.as_str()) {
        return truncate(s, 40);
    }

    String::new()
}

/// Shorten a file path to fit in `max_len` chars by keeping the filename
/// and as much prefix as possible, eliding the middle with "...".
fn shorten_path(path: &str, max_len: usize) -> String {
    if path.len() <= max_len {
        return path.to_string();
    }
    // Keep the filename
    let filename = std::path::Path::new(path)
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or(path);
    if filename.len() >= max_len {
        return truncate(filename, max_len);
    }
    let prefix_len = max_len.saturating_sub(filename.len() + 4); // 4 = ".../".len()
    if prefix_len == 0 {
        return filename.to_string();
    }
    let boundary = path.char_indices()
        .map(|(i, _)| i)
        .take_while(|&i| i <= prefix_len.min(path.len()))
        .last()
        .unwrap_or(0);
    format!("{}.../{}", &path[..boundary], filename)
}

fn truncate(s: &str, max_len: usize) -> String {
    if s.len() <= max_len {
        s.to_string()
    } else {
        let limit = max_len.saturating_sub(3);
        let boundary = s.char_indices()
            .map(|(i, _)| i)
            .take_while(|&i| i <= limit)
            .last()
            .unwrap_or(0);
        format!("{}...", &s[..boundary])
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_terminal_id() {
        assert_eq!(parse_terminal_id("/hook/TASK_001"), Some("TASK_001"));
        assert_eq!(parse_terminal_id("/hook/"), None);
        assert_eq!(parse_terminal_id("/other/path"), None);
        assert_eq!(
            parse_terminal_id("/hook/TASK_001?foo=bar"),
            Some("TASK_001")
        );
    }

    #[test]
    fn test_parse_notification_event() {
        let body = r#"{"hook_event_name":"Notification","message":"Hello from Claude"}"#;
        let ev = parse_hook_event("TASK_001".to_string(), body).unwrap();
        match ev {
            HookEvent::Notification {
                terminal_id,
                message,
            } => {
                assert_eq!(terminal_id, "TASK_001");
                assert_eq!(message, "Hello from Claude");
            }
            _ => panic!("Wrong event type"),
        }
    }

    #[test]
    fn test_parse_pre_tool_use() {
        let body = r#"{"hook_event_name":"PreToolUse","tool_name":"Edit","tool_input":{"file_path":"/src/main.rs","new_string":"foo","old_string":"bar"}}"#;
        let ev = parse_hook_event("TASK_001".to_string(), body).unwrap();
        match ev {
            HookEvent::PreToolUse {
                terminal_id,
                tool_name,
                tool_input_summary,
            } => {
                assert_eq!(terminal_id, "TASK_001");
                assert_eq!(tool_name, "Edit");
                assert!(tool_input_summary.contains("main.rs"));
            }
            _ => panic!("Wrong event type"),
        }
    }

    #[test]
    fn test_parse_permission_request_event() {
        let body = r#"{
            "hook_event_name": "PermissionRequest",
            "tool_name": "Bash",
            "tool_input": {
                "command": "rm -rf /tmp/test",
                "description": "Remove temp files"
            }
        }"#;
        let ev = parse_hook_event("TASK_001".to_string(), body).unwrap();
        match ev {
            HookEvent::PermissionRequest {
                terminal_id,
                tool_name,
                tool_input,
            } => {
                assert_eq!(terminal_id, "TASK_001");
                assert_eq!(tool_name, "Bash");
                assert_eq!(
                    tool_input.get("command").and_then(|v| v.as_str()),
                    Some("rm -rf /tmp/test")
                );
            }
            _ => panic!("Wrong event type"),
        }
    }

    #[test]
    fn test_parse_permission_request_missing_tool_input() {
        let body = r#"{"hook_event_name":"PermissionRequest","tool_name":"Edit"}"#;
        let ev = parse_hook_event("TASK_001".to_string(), body).unwrap();
        match ev {
            HookEvent::PermissionRequest {
                terminal_id,
                tool_name,
                tool_input,
            } => {
                assert_eq!(terminal_id, "TASK_001");
                assert_eq!(tool_name, "Edit");
                assert!(tool_input.is_object());
            }
            _ => panic!("Wrong event type"),
        }
    }

    #[test]
    fn test_build_allow_response_pre_tool_use() {
        let json = build_allow_response("PreToolUse");
        let v: serde_json::Value = serde_json::from_str(&json).unwrap();
        let output = &v["hookSpecificOutput"];
        assert_eq!(output["hookEventName"], "PreToolUse");
        assert_eq!(output["permissionDecision"], "allow");
    }

    #[test]
    fn test_build_allow_response_permission_request() {
        let json = build_allow_response("PermissionRequest");
        let v: serde_json::Value = serde_json::from_str(&json).unwrap();
        let output = &v["hookSpecificOutput"];
        assert_eq!(output["hookEventName"], "PermissionRequest");
        assert_eq!(output["decision"]["behavior"], "allow");
    }

    #[test]
    fn test_build_deny_response_pre_tool_use() {
        let json = build_deny_response("PreToolUse", "Too dangerous");
        let v: serde_json::Value = serde_json::from_str(&json).unwrap();
        let output = &v["hookSpecificOutput"];
        assert_eq!(output["hookEventName"], "PreToolUse");
        assert_eq!(output["permissionDecision"], "deny");
        assert_eq!(output["permissionDecisionReason"], "Too dangerous");
    }

    #[test]
    fn test_build_deny_response_permission_request() {
        let json = build_deny_response("PermissionRequest", "Not allowed");
        let v: serde_json::Value = serde_json::from_str(&json).unwrap();
        let output = &v["hookSpecificOutput"];
        assert_eq!(output["hookEventName"], "PermissionRequest");
        assert_eq!(output["decision"]["behavior"], "deny");
        assert_eq!(output["decision"]["message"], "Not allowed");
    }

    #[test]
    fn test_build_decision_response_allow() {
        let decision = PermissionDecision::Allow;
        let json = build_decision_response("PreToolUse", &decision);
        let v: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(v["hookSpecificOutput"]["permissionDecision"], "allow");
    }

    #[test]
    fn test_build_decision_response_deny() {
        let decision = PermissionDecision::Deny { message: "denied".to_string() };
        let json = build_decision_response("PermissionRequest", &decision);
        let v: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(v["hookSpecificOutput"]["decision"]["behavior"], "deny");
        assert_eq!(v["hookSpecificOutput"]["decision"]["message"], "denied");
    }

    #[test]
    fn test_check_permission_profile_autonomous() {
        let reg = TerminalRegistration {
            token: "tok".to_string(),
            session_context: None,
            permission_profile: "autonomous".to_string(),
            auto_approve_patterns: vec![],
            ai_host: "claude".to_string(),
        };
        assert!(matches!(
            check_permission_profile(&reg, "Bash", "rm -rf /"),
            ProfileDecision::Allow
        ));
    }

    #[test]
    fn test_check_permission_profile_interactive() {
        let reg = TerminalRegistration {
            token: "tok".to_string(),
            session_context: None,
            permission_profile: "interactive".to_string(),
            auto_approve_patterns: vec![],
            ai_host: "claude".to_string(),
        };
        assert!(matches!(
            check_permission_profile(&reg, "Bash", "ls"),
            ProfileDecision::Queue
        ));
    }

    #[test]
    fn test_check_permission_profile_trusted_match() {
        let reg = TerminalRegistration {
            token: "tok".to_string(),
            session_context: None,
            permission_profile: "trusted".to_string(),
            auto_approve_patterns: vec!["(?i)read".to_string(), "(?i)list".to_string()],
            ai_host: "claude".to_string(),
        };
        // "Read" tool should match the "(?i)read" pattern
        assert!(matches!(
            check_permission_profile(&reg, "Read", "/tmp/file.txt"),
            ProfileDecision::Allow
        ));
    }

    #[test]
    fn test_check_permission_profile_trusted_no_match() {
        let reg = TerminalRegistration {
            token: "tok".to_string(),
            session_context: None,
            permission_profile: "trusted".to_string(),
            auto_approve_patterns: vec!["(?i)read".to_string()],
            ai_host: "claude".to_string(),
        };
        // "Bash" tool should NOT match the "(?i)read" pattern
        assert!(matches!(
            check_permission_profile(&reg, "Bash", "rm -rf /tmp"),
            ProfileDecision::Queue
        ));
    }

    #[test]
    fn test_shorten_path() {
        assert_eq!(shorten_path("/short.rs", 40), "/short.rs");
        let long = "/a/b/c/d/e/f/g/h/i/j/k/l/m/n/o/p/q/r/s/file.rs";
        let short = shorten_path(long, 20);
        assert!(short.len() <= 20, "shortened={}", short);
        assert!(short.contains("file.rs"));
    }

    #[test]
    fn test_server_starts_and_responds() {
        let (server, port, rx, _pending_rx) = HookServer::start().expect("server should start");
        assert!(port > 0);

        // Register a test token (no session context for this test)
        server.register_token("TASK_TEST".to_string(), "secret123".to_string(), None);

        // Send a valid hook request
        let client = std::net::TcpStream::connect(format!("127.0.0.1:{}", port)).unwrap();
        let body = r#"{"hook_event_name":"Notification","message":"test"}"#;
        let request = format!(
            "POST /hook/TASK_TEST HTTP/1.1\r\nHost: 127.0.0.1\r\nAuthorization: Bearer secret123\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
            body.len(),
            body
        );
        use std::io::Write;
        let mut client = client;
        client.write_all(request.as_bytes()).unwrap();
        drop(client);

        // Give the server thread time to process
        std::thread::sleep(std::time::Duration::from_millis(100));

        // Should have received the event
        match rx.try_recv() {
            Ok(HookEvent::Notification { terminal_id, message }) => {
                assert_eq!(terminal_id, "TASK_TEST");
                assert_eq!(message, "test");
            }
            other => panic!("Expected Notification, got {:?}", other),
        }

        server.shutdown();
    }

    #[test]
    fn test_session_start_context_injection() {
        let (server, port, rx, _pending_rx) = HookServer::start().expect("server should start");
        assert!(port > 0);

        // Register with session context
        let ctx = "# Crew Board Context: TASK_042\n\n## Task Assignment\n- **Phase**: implementer".to_string();
        server.register_token(
            "TASK_042".to_string(),
            "tokenabc".to_string(),
            Some(ctx.clone()),
        );

        // Send a SessionStart hook
        use std::io::{Read, Write};
        let mut client =
            std::net::TcpStream::connect(format!("127.0.0.1:{}", port)).unwrap();
        let body = r#"{"hook_event_name":"SessionStart","session_id":"sess-1"}"#;
        let raw = format!(
            "POST /hook/TASK_042 HTTP/1.1\r\nHost: 127.0.0.1\r\nAuthorization: Bearer tokenabc\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
            body.len(),
            body
        );
        client.write_all(raw.as_bytes()).unwrap();
        // Shut down write side so server sees EOF
        client
            .shutdown(std::net::Shutdown::Write)
            .unwrap_or_default();

        // Read the response
        let mut response = String::new();
        let _ = client.read_to_string(&mut response);

        // Give server time to process
        std::thread::sleep(std::time::Duration::from_millis(150));

        // Event should have arrived
        match rx.try_recv() {
            Ok(HookEvent::SessionStart { terminal_id, .. }) => {
                assert_eq!(terminal_id, "TASK_042");
            }
            other => panic!("Expected SessionStart, got {:?}", other),
        }

        // Response body should contain additionalContext with our markdown
        assert!(
            response.contains("additionalContext"),
            "Response missing additionalContext: {}",
            response
        );
        assert!(
            response.contains("TASK_042"),
            "Response missing task id: {}",
            response
        );
        assert!(
            response.contains("implementer"),
            "Response missing phase: {}",
            response
        );

        server.shutdown();
    }

    #[test]
    fn test_session_start_no_context_returns_empty_json() {
        let (server, port, rx, _pending_rx) = HookServer::start().expect("server should start");
        assert!(port > 0);

        // Register without session context
        server.register_token("TASK_NO_CTX".to_string(), "tok999".to_string(), None);

        use std::io::{Read, Write};
        let mut client =
            std::net::TcpStream::connect(format!("127.0.0.1:{}", port)).unwrap();
        let body = r#"{"hook_event_name":"SessionStart","session_id":"sess-2"}"#;
        let raw = format!(
            "POST /hook/TASK_NO_CTX HTTP/1.1\r\nHost: 127.0.0.1\r\nAuthorization: Bearer tok999\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
            body.len(),
            body
        );
        client.write_all(raw.as_bytes()).unwrap();
        client
            .shutdown(std::net::Shutdown::Write)
            .unwrap_or_default();

        let mut response = String::new();
        let _ = client.read_to_string(&mut response);

        std::thread::sleep(std::time::Duration::from_millis(150));

        // Event still arrives
        match rx.try_recv() {
            Ok(HookEvent::SessionStart { terminal_id, .. }) => {
                assert_eq!(terminal_id, "TASK_NO_CTX");
            }
            other => panic!("Expected SessionStart, got {:?}", other),
        }

        // Response body should just be {}
        assert!(
            response.contains("{}"),
            "Response should be empty JSON: {}",
            response
        );
        assert!(
            !response.contains("additionalContext"),
            "Response should not have context: {}",
            response
        );

        server.shutdown();
    }

    #[test]
    fn test_parse_user_prompt_submit() {
        let body = r#"{"hook_event_name":"UserPromptSubmit","prompt":"Fix the bug in authentication"}"#;
        let ev = parse_hook_event("TASK_001".to_string(), body).unwrap();
        match ev {
            HookEvent::UserPromptSubmit { terminal_id, prompt_preview } => {
                assert_eq!(terminal_id, "TASK_001");
                assert_eq!(prompt_preview, "Fix the bug in authentication");
            }
            _ => panic!("Wrong event type"),
        }
    }

    #[test]
    fn test_parse_user_prompt_submit_truncation() {
        let long_prompt = "x".repeat(300);
        let body = format!(r#"{{"hook_event_name":"UserPromptSubmit","prompt":"{}"}}"#, long_prompt);
        let ev = parse_hook_event("TASK_002".to_string(), &body).unwrap();
        match ev {
            HookEvent::UserPromptSubmit { prompt_preview, .. } => {
                assert!(prompt_preview.len() <= 200, "Preview should be truncated, got len={}", prompt_preview.len());
                assert!(prompt_preview.ends_with("..."));
            }
            _ => panic!("Wrong event type"),
        }
    }

    #[test]
    fn test_autonomous_pre_tool_use_immediate_allow() {
        let (server, port, rx, pending_rx) = HookServer::start().expect("server should start");
        assert!(port > 0);

        // Register with autonomous profile
        server.register_token_with_profile(
            "TASK_AUTO".to_string(),
            "tok-auto".to_string(),
            None,
            "autonomous",
            vec![],
        );

        // Send a PreToolUse hook with autonomous profile — should get immediate allow response
        use std::io::{Read, Write};
        let mut client =
            std::net::TcpStream::connect(format!("127.0.0.1:{}", port)).unwrap();
        let body = r#"{"hook_event_name":"PreToolUse","tool_name":"Bash","tool_input":{"command":"ls -la"}}"#;
        let raw = format!(
            "POST /hook/TASK_AUTO HTTP/1.1\r\nHost: 127.0.0.1\r\nAuthorization: Bearer tok-auto\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
            body.len(),
            body
        );
        client.write_all(raw.as_bytes()).unwrap();
        client
            .shutdown(std::net::Shutdown::Write)
            .unwrap_or_default();

        let mut response = String::new();
        let _ = client.read_to_string(&mut response);

        std::thread::sleep(std::time::Duration::from_millis(150));

        // Event should have been forwarded
        assert!(rx.try_recv().is_ok(), "Expected PreToolUse event");

        // Pending channel should be empty (no queuing for autonomous)
        assert!(
            pending_rx.try_recv().is_err(),
            "No pending permission expected for autonomous profile"
        );

        // Response should contain allow decision
        assert!(
            response.contains("allow"),
            "Response should contain allow: {}",
            response
        );

        server.shutdown();
    }
}
