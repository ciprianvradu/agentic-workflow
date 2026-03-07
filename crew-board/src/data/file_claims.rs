//! File claims registry for tracking terminal file access.
//!
//! Tracks which terminals have claimed (Edit/Write/NotebookEdit) which files,
//! with 10-minute expiry. Used for conflict detection when multiple terminals
//! work on the same codebase.

use std::collections::HashMap;
use std::time::Instant;

/// How long a file claim remains valid.
const CLAIM_EXPIRY_SECS: u64 = 600; // 10 minutes

/// A single file claim by a terminal.
#[derive(Debug, Clone)]
pub struct FileClaim {
    pub terminal_id: String,
    pub tool_name: String,
    pub claimed_at: Instant,
}

impl FileClaim {
    /// Whether this claim has expired.
    pub fn is_expired(&self) -> bool {
        self.claimed_at.elapsed().as_secs() > CLAIM_EXPIRY_SECS
    }
}

/// Registry tracking file path → claims mapping.
pub struct FileClaimsRegistry {
    claims: HashMap<String, Vec<FileClaim>>,
}

impl FileClaimsRegistry {
    pub fn new() -> Self {
        Self {
            claims: HashMap::new(),
        }
    }

    /// Record a file claim. Deduplicates by terminal_id (updates timestamp).
    pub fn claim(&mut self, path: &str, terminal_id: &str, tool_name: &str) {
        let claims = self.claims.entry(path.to_string()).or_default();

        // Update existing claim from same terminal
        if let Some(existing) = claims.iter_mut().find(|c| c.terminal_id == terminal_id) {
            existing.claimed_at = Instant::now();
            existing.tool_name = tool_name.to_string();
            return;
        }

        claims.push(FileClaim {
            terminal_id: terminal_id.to_string(),
            tool_name: tool_name.to_string(),
            claimed_at: Instant::now(),
        });
    }

    /// Check for conflicts: returns other terminals that have claimed the same file.
    /// Excludes expired claims and the requesting terminal itself.
    pub fn check_conflicts(&self, path: &str, requesting_terminal: &str) -> Vec<&FileClaim> {
        match self.claims.get(path) {
            Some(claims) => claims
                .iter()
                .filter(|c| c.terminal_id != requesting_terminal && !c.is_expired())
                .collect(),
            None => vec![],
        }
    }

    /// Get all files claimed by a specific terminal (non-expired).
    #[allow(dead_code)]
    pub fn files_for_terminal(&self, terminal_id: &str) -> Vec<&str> {
        self.claims
            .iter()
            .filter_map(|(path, claims)| {
                if claims.iter().any(|c| c.terminal_id == terminal_id && !c.is_expired()) {
                    Some(path.as_str())
                } else {
                    None
                }
            })
            .collect()
    }

    /// Remove all expired claims across all files.
    /// Call periodically to prevent unbounded growth.
    #[allow(dead_code)]
    pub fn gc(&mut self) {
        for claims in self.claims.values_mut() {
            claims.retain(|c| !c.is_expired());
        }
        self.claims.retain(|_, v| !v.is_empty());
    }

    /// Total number of active (non-expired) claims.
    #[allow(dead_code)]
    pub fn active_claim_count(&self) -> usize {
        self.claims
            .values()
            .flat_map(|v| v.iter())
            .filter(|c| !c.is_expired())
            .count()
    }
}

impl Default for FileClaimsRegistry {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_claim_and_check_no_conflict() {
        let mut reg = FileClaimsRegistry::new();
        reg.claim("src/main.rs", "T1", "Edit");

        let conflicts = reg.check_conflicts("src/main.rs", "T1");
        assert!(conflicts.is_empty(), "No conflict with self");
    }

    #[test]
    fn test_claim_and_check_conflict() {
        let mut reg = FileClaimsRegistry::new();
        reg.claim("src/main.rs", "T1", "Edit");
        reg.claim("src/main.rs", "T2", "Write");

        let conflicts = reg.check_conflicts("src/main.rs", "T1");
        assert_eq!(conflicts.len(), 1);
        assert_eq!(conflicts[0].terminal_id, "T2");
    }

    #[test]
    fn test_files_for_terminal() {
        let mut reg = FileClaimsRegistry::new();
        reg.claim("src/main.rs", "T1", "Edit");
        reg.claim("src/lib.rs", "T1", "Write");
        reg.claim("src/app.rs", "T2", "Edit");

        let files = reg.files_for_terminal("T1");
        assert_eq!(files.len(), 2);
    }

    #[test]
    fn test_dedup_same_terminal() {
        let mut reg = FileClaimsRegistry::new();
        reg.claim("src/main.rs", "T1", "Edit");
        reg.claim("src/main.rs", "T1", "Write"); // Should update, not duplicate

        assert_eq!(reg.claims.get("src/main.rs").unwrap().len(), 1);
    }

    #[test]
    fn test_no_conflict_unknown_file() {
        let reg = FileClaimsRegistry::new();
        let conflicts = reg.check_conflicts("unknown.rs", "T1");
        assert!(conflicts.is_empty());
    }

    #[test]
    fn test_gc_empty() {
        let mut reg = FileClaimsRegistry::new();
        reg.gc(); // Should not panic
    }

    #[test]
    fn test_conflict_detection_chain() {
        let mut registry = FileClaimsRegistry::new();
        registry.claim("src/main.rs", "T1", "Edit");
        registry.claim("src/app.rs", "T1", "Edit");
        registry.claim("src/main.rs", "T2", "Write"); // conflict!

        // T2 sees T1's claim on src/main.rs as a conflict
        let conflicts = registry.check_conflicts("src/main.rs", "T2");
        assert_eq!(conflicts.len(), 1);
        assert_eq!(conflicts[0].terminal_id, "T1");

        // T1 sees T2's claim on src/main.rs as a conflict (bidirectional)
        let t1_conflicts = registry.check_conflicts("src/main.rs", "T1");
        assert_eq!(t1_conflicts.len(), 1);
        assert_eq!(t1_conflicts[0].terminal_id, "T2");

        // No conflict for a file only T1 has claimed
        let no_conflict = registry.check_conflicts("src/app.rs", "T1");
        assert!(no_conflict.is_empty());
    }
}
