//! Security rules engine for hook-based tool governance.
//!
//! Parses security rules from settings, evaluates tool requests against them,
//! and provides credential pattern scanning.

use regex::Regex;
use serde::Deserialize;
use std::collections::{HashMap, VecDeque};
use std::time::Instant;

/// Action to take when a security rule matches.
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum RuleAction {
    Deny,
    Ask,
    Allow,
}

/// A security rule configuration from settings.
#[derive(Debug, Clone, Deserialize)]
pub struct SecurityRuleConfig {
    pub name: String,
    #[serde(default)]
    pub tool_pattern: Option<String>,
    #[serde(default)]
    pub input_pattern: Option<String>,
    #[serde(default)]
    pub file_pattern: Option<String>,
    pub action: RuleAction,
    #[serde(default)]
    pub reason: Option<String>,
}

/// Compiled security rule ready for evaluation.
#[derive(Debug, Clone)]
pub struct SecurityRule {
    pub name: String,
    pub tool_regex: Option<Regex>,
    pub input_regex: Option<Regex>,
    pub file_regex: Option<Regex>,
    pub action: RuleAction,
    pub reason: String,
}

/// Result of evaluating security rules against a tool request.
#[derive(Debug, Clone, PartialEq)]
pub enum SecurityDecision {
    /// No rule matched — proceed with normal permission flow.
    NoMatch,
    /// A rule matched with Allow action.
    Allow { rule_name: String },
    /// A rule matched with Ask action — force queue for human review.
    Ask { rule_name: String, reason: String },
    /// A rule matched with Deny action — immediately deny.
    Deny { rule_name: String, reason: String },
}

/// Security statistics tracking.
#[derive(Debug, Clone, Default)]
pub struct SecurityStats {
    pub denied: u32,
    pub warned: u32,
    pub auto_approved: u32,
    pub human_approved: u32,
    pub credential_exposures: u32,
}

/// Sensitive file protection configuration.
#[derive(Debug, Clone, Deserialize, Default)]
pub struct SensitiveFiles {
    /// Files that should never be accessed (auto-deny).
    #[serde(default)]
    pub never_access: Vec<String>,
    /// Files that trigger a warning on access.
    #[serde(default)]
    pub warn_on_access: Vec<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum SensitiveFileDecision {
    Ok,
    Warn { reason: String },
    Deny { reason: String },
}

fn glob_matches(pattern: &str, path: &str) -> bool {
    // Simple glob: just check if pattern is a suffix or prefix match
    if let Some(suffix) = pattern.strip_prefix('*') {
        path.ends_with(suffix)
    } else if let Some(prefix) = pattern.strip_suffix('*') {
        path.starts_with(prefix)
    } else {
        path == pattern
    }
}

/// Simple sliding-window rate limiter per terminal.
#[derive(Debug)]
pub struct RateLimiter {
    /// Max requests per window.
    max_requests: u32,
    /// Window duration in seconds.
    window_secs: u64,
    /// terminal_id -> list of request timestamps.
    windows: HashMap<String, VecDeque<Instant>>,
}

impl RateLimiter {
    pub fn new(max_requests: u32, window_secs: u64) -> Self {
        Self {
            max_requests,
            window_secs,
            windows: HashMap::new(),
        }
    }

    /// Check if a request should be rate-limited.
    /// Returns true if the request is allowed, false if rate-limited.
    pub fn check(&mut self, terminal_id: &str) -> bool {
        let window = self.windows.entry(terminal_id.to_string()).or_default();
        let now = Instant::now();
        let cutoff = now - std::time::Duration::from_secs(self.window_secs);

        // Remove expired entries
        while window.front().is_some_and(|t| *t < cutoff) {
            window.pop_front();
        }

        if window.len() >= self.max_requests as usize {
            false
        } else {
            window.push_back(now);
            true
        }
    }

    /// Reset rate limiter for a terminal (e.g., on session start).
    #[allow(dead_code)]
    pub fn reset(&mut self, terminal_id: &str) {
        self.windows.remove(terminal_id);
    }
}

/// The security rules engine.
pub struct RulesEngine {
    rules: Vec<SecurityRule>,
    credential_patterns: Vec<Regex>,
    pub stats: SecurityStats,
    sensitive_files: SensitiveFiles,
}

impl RulesEngine {
    /// Create from settings configuration.
    pub fn from_config(rules: &[SecurityRuleConfig], credential_patterns: &[String]) -> Self {
        let compiled_rules: Vec<SecurityRule> = rules
            .iter()
            .filter_map(|cfg| {
                let tool_regex = cfg.tool_pattern.as_ref().and_then(|p| Regex::new(p).ok());
                let input_regex = cfg.input_pattern.as_ref().and_then(|p| Regex::new(p).ok());
                let file_regex = cfg.file_pattern.as_ref().and_then(|p| Regex::new(p).ok());

                // Skip rules where all patterns failed to compile
                if cfg.tool_pattern.is_some() && tool_regex.is_none()
                    && cfg.input_pattern.is_none()
                    && cfg.file_pattern.is_none()
                {
                    return None;
                }

                Some(SecurityRule {
                    name: cfg.name.clone(),
                    tool_regex,
                    input_regex,
                    file_regex,
                    action: cfg.action.clone(),
                    reason: cfg.reason.clone().unwrap_or_else(|| cfg.name.clone()),
                })
            })
            .collect();

        let cred_patterns: Vec<Regex> = credential_patterns
            .iter()
            .filter_map(|p| Regex::new(p).ok())
            .collect();

        Self {
            rules: compiled_rules,
            credential_patterns: cred_patterns,
            stats: SecurityStats::default(),
            sensitive_files: SensitiveFiles::default(),
        }
    }

    /// Create an empty rules engine (no rules configured).
    pub fn empty() -> Self {
        Self {
            rules: Vec::new(),
            credential_patterns: Vec::new(),
            stats: SecurityStats::default(),
            sensitive_files: SensitiveFiles::default(),
        }
    }

    /// Evaluate a tool request against all rules.
    /// Priority: Deny > Ask > Allow. First matching rule of highest priority wins.
    pub fn evaluate(&self, tool_name: &str, tool_input_summary: &str) -> SecurityDecision {
        let mut best_deny: Option<&SecurityRule> = None;
        let mut best_ask: Option<&SecurityRule> = None;
        let mut best_allow: Option<&SecurityRule> = None;

        for rule in &self.rules {
            if self.rule_matches(rule, tool_name, tool_input_summary) {
                match rule.action {
                    RuleAction::Deny => {
                        if best_deny.is_none() {
                            best_deny = Some(rule);
                        }
                    }
                    RuleAction::Ask => {
                        if best_ask.is_none() {
                            best_ask = Some(rule);
                        }
                    }
                    RuleAction::Allow => {
                        if best_allow.is_none() {
                            best_allow = Some(rule);
                        }
                    }
                }
            }
        }

        // Priority: Deny > Ask > Allow
        if let Some(rule) = best_deny {
            return SecurityDecision::Deny {
                rule_name: rule.name.clone(),
                reason: rule.reason.clone(),
            };
        }
        if let Some(rule) = best_ask {
            return SecurityDecision::Ask {
                rule_name: rule.name.clone(),
                reason: rule.reason.clone(),
            };
        }
        if let Some(rule) = best_allow {
            return SecurityDecision::Allow {
                rule_name: rule.name.clone(),
            };
        }

        SecurityDecision::NoMatch
    }

    /// Check if a rule matches the given tool request.
    fn rule_matches(&self, rule: &SecurityRule, tool_name: &str, tool_input_summary: &str) -> bool {
        // All specified patterns must match (AND logic)
        if let Some(ref re) = rule.tool_regex {
            if !re.is_match(tool_name) {
                return false;
            }
        }
        if let Some(ref re) = rule.input_regex {
            if !re.is_match(tool_input_summary) {
                return false;
            }
        }
        if let Some(ref re) = rule.file_regex {
            if !re.is_match(tool_input_summary) {
                return false;
            }
        }
        // At least one pattern must be specified
        rule.tool_regex.is_some() || rule.input_regex.is_some() || rule.file_regex.is_some()
    }

    /// Scan text for credential patterns. Returns matching pattern names.
    pub fn scan_credentials(&self, text: &str) -> Vec<String> {
        self.credential_patterns
            .iter()
            .filter(|re| re.is_match(text))
            .map(|re| re.as_str().to_string())
            .collect()
    }

    /// Check if a file path matches sensitive file patterns.
    pub fn check_sensitive_file(&self, file_path: &str) -> SensitiveFileDecision {
        for pattern in &self.sensitive_files.never_access {
            if file_path.contains(pattern) || glob_matches(pattern, file_path) {
                return SensitiveFileDecision::Deny {
                    reason: format!("File '{}' is in the never-access list", file_path),
                };
            }
        }
        for pattern in &self.sensitive_files.warn_on_access {
            if file_path.contains(pattern) || glob_matches(pattern, file_path) {
                return SensitiveFileDecision::Warn {
                    reason: format!("File '{}' is in the warn-on-access list", file_path),
                };
            }
        }
        SensitiveFileDecision::Ok
    }

    /// Whether any rules are configured.
    pub fn has_rules(&self) -> bool {
        !self.rules.is_empty()
    }

    /// Number of configured rules.
    #[allow(dead_code)]
    pub fn rule_count(&self) -> usize {
        self.rules.len()
    }
}

/// Structured history event types (replacing inline format! strings).
#[derive(Debug, Clone, serde::Serialize)]
#[serde(tag = "event")]
pub enum HistoryEvent {
    SessionStart {
        ts: String,
        terminal_id: String,
        session_id: String,
    },
    SessionEnd {
        ts: String,
        terminal_id: String,
    },
    PreToolUse {
        ts: String,
        terminal_id: String,
        tool: String,
        detail: String,
    },
    PostToolUse {
        ts: String,
        terminal_id: String,
        tool: String,
        detail: String,
        success: bool,
    },
    Notification {
        ts: String,
        terminal_id: String,
        message: String,
    },
    Stop {
        ts: String,
        terminal_id: String,
        preview: String,
    },
    PermissionRequest {
        ts: String,
        terminal_id: String,
        tool: String,
    },
    PermissionDecision {
        ts: String,
        terminal_id: String,
        tool: String,
        decision: String,
        decided_by: String,
        decided_via: String,
    },
    UserPrompt {
        ts: String,
        terminal_id: String,
        prompt_preview: String,
    },
    SecurityDeny {
        ts: String,
        terminal_id: String,
        tool: String,
        rule_name: String,
        reason: String,
    },
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_empty_engine_no_match() {
        let engine = RulesEngine::empty();
        let decision = engine.evaluate("Bash", "rm -rf /");
        assert_eq!(decision, SecurityDecision::NoMatch);
    }

    #[test]
    fn test_deny_rule() {
        let rules = vec![SecurityRuleConfig {
            name: "no-force-push".to_string(),
            tool_pattern: Some("Bash".to_string()),
            input_pattern: Some("(?i)force.*push|push.*force".to_string()),
            file_pattern: None,
            action: RuleAction::Deny,
            reason: Some("Force push is not allowed".to_string()),
        }];
        let engine = RulesEngine::from_config(&rules, &[]);

        let decision = engine.evaluate("Bash", "git push --force origin main");
        assert!(matches!(decision, SecurityDecision::Deny { .. }));
    }

    #[test]
    fn test_ask_rule() {
        let rules = vec![SecurityRuleConfig {
            name: "review-delete".to_string(),
            tool_pattern: Some("Bash".to_string()),
            input_pattern: Some("(?i)rm\\s+-r".to_string()),
            file_pattern: None,
            action: RuleAction::Ask,
            reason: Some("Recursive delete needs review".to_string()),
        }];
        let engine = RulesEngine::from_config(&rules, &[]);

        let decision = engine.evaluate("Bash", "rm -rf /tmp/test");
        assert!(matches!(decision, SecurityDecision::Ask { .. }));
    }

    #[test]
    fn test_allow_rule() {
        let rules = vec![SecurityRuleConfig {
            name: "allow-read".to_string(),
            tool_pattern: Some("Read".to_string()),
            input_pattern: None,
            file_pattern: None,
            action: RuleAction::Allow,
            reason: None,
        }];
        let engine = RulesEngine::from_config(&rules, &[]);

        let decision = engine.evaluate("Read", "/src/main.rs");
        assert!(matches!(decision, SecurityDecision::Allow { .. }));
    }

    #[test]
    fn test_deny_takes_priority() {
        let rules = vec![
            SecurityRuleConfig {
                name: "allow-bash".to_string(),
                tool_pattern: Some("Bash".to_string()),
                input_pattern: None,
                file_pattern: None,
                action: RuleAction::Allow,
                reason: None,
            },
            SecurityRuleConfig {
                name: "deny-rm".to_string(),
                tool_pattern: Some("Bash".to_string()),
                input_pattern: Some("rm".to_string()),
                file_pattern: None,
                action: RuleAction::Deny,
                reason: Some("No rm commands".to_string()),
            },
        ];
        let engine = RulesEngine::from_config(&rules, &[]);

        let decision = engine.evaluate("Bash", "rm -rf /tmp");
        assert!(matches!(decision, SecurityDecision::Deny { .. }));
    }

    #[test]
    fn test_credential_scan() {
        let cred_patterns = vec![
            r"(?i)api[_-]?key\s*[:=]".to_string(),
            r"(?i)password\s*[:=]".to_string(),
        ];
        let engine = RulesEngine::from_config(&[], &cred_patterns);

        let matches = engine.scan_credentials("API_KEY=sk-12345");
        assert_eq!(matches.len(), 1);

        let no_match = engine.scan_credentials("just some normal text");
        assert!(no_match.is_empty());
    }

    #[test]
    fn test_no_pattern_rule_skipped() {
        let rules = vec![SecurityRuleConfig {
            name: "empty".to_string(),
            tool_pattern: None,
            input_pattern: None,
            file_pattern: None,
            action: RuleAction::Deny,
            reason: None,
        }];
        let engine = RulesEngine::from_config(&rules, &[]);

        // Rule with no patterns should never match
        let decision = engine.evaluate("Bash", "anything");
        assert_eq!(decision, SecurityDecision::NoMatch);
    }

    #[test]
    fn test_history_event_serialization() {
        let event = HistoryEvent::PostToolUse {
            ts: "2025-01-01T00:00:00Z".to_string(),
            terminal_id: "TASK_001".to_string(),
            tool: "Edit".to_string(),
            detail: "src/main.rs".to_string(),
            success: true,
        };
        let json = serde_json::to_string(&event).unwrap();
        assert!(json.contains("PostToolUse"));
        assert!(json.contains("TASK_001"));
    }

    #[test]
    fn test_rate_limiter_allows_within_limit() {
        let mut limiter = RateLimiter::new(5, 60);
        for _ in 0..5 {
            assert!(limiter.check("T1"));
        }
    }

    #[test]
    fn test_rate_limiter_blocks_over_limit() {
        let mut limiter = RateLimiter::new(3, 60);
        assert!(limiter.check("T1"));
        assert!(limiter.check("T1"));
        assert!(limiter.check("T1"));
        assert!(!limiter.check("T1")); // 4th should be blocked
    }

    #[test]
    fn test_rate_limiter_separate_terminals() {
        let mut limiter = RateLimiter::new(2, 60);
        assert!(limiter.check("T1"));
        assert!(limiter.check("T1"));
        assert!(!limiter.check("T1")); // T1 blocked
        assert!(limiter.check("T2")); // T2 still ok
    }

    #[test]
    fn test_sensitive_file_never_access() {
        let mut engine = RulesEngine::empty();
        engine.sensitive_files = SensitiveFiles {
            never_access: vec![".env".to_string(), "*.pem".to_string()],
            warn_on_access: vec![],
        };
        assert!(matches!(
            engine.check_sensitive_file(".env"),
            SensitiveFileDecision::Deny { .. }
        ));
        assert!(matches!(
            engine.check_sensitive_file("server.pem"),
            SensitiveFileDecision::Deny { .. }
        ));
        assert_eq!(engine.check_sensitive_file("src/main.rs"), SensitiveFileDecision::Ok);
    }

    #[test]
    fn test_sensitive_file_warn() {
        let mut engine = RulesEngine::empty();
        engine.sensitive_files = SensitiveFiles {
            never_access: vec![],
            warn_on_access: vec!["Cargo.lock".to_string()],
        };
        assert!(matches!(
            engine.check_sensitive_file("Cargo.lock"),
            SensitiveFileDecision::Warn { .. }
        ));
    }

    #[test]
    fn test_combined_security_workflow() {
        let rules = vec![
            SecurityRuleConfig {
                name: "deny-force-push".to_string(),
                tool_pattern: Some("Bash".to_string()),
                input_pattern: Some("(?i)push.*--force".to_string()),
                file_pattern: None,
                action: RuleAction::Deny,
                reason: Some("No force push".to_string()),
            },
            SecurityRuleConfig {
                name: "allow-read".to_string(),
                tool_pattern: Some("Read".to_string()),
                input_pattern: None,
                file_pattern: None,
                action: RuleAction::Allow,
                reason: None,
            },
        ];
        let cred_patterns = vec![r"(?i)api.?key\s*[:=]".to_string()];
        let engine = RulesEngine::from_config(&rules, &cred_patterns);

        // Test rule evaluation
        assert!(matches!(
            engine.evaluate("Bash", "git push --force origin main"),
            SecurityDecision::Deny { .. }
        ));
        assert!(matches!(
            engine.evaluate("Read", "src/main.rs"),
            SecurityDecision::Allow { .. }
        ));
        assert_eq!(
            engine.evaluate("Edit", "src/main.rs"),
            SecurityDecision::NoMatch
        );

        // Test credential scanning
        let creds = engine.scan_credentials("export API_KEY=sk-12345");
        assert_eq!(creds.len(), 1);

        // Test sensitive files (engine starts with empty sensitive_files)
        assert_eq!(
            engine.check_sensitive_file("src/main.rs"),
            SensitiveFileDecision::Ok
        );
    }
}
