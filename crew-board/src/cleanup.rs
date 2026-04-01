use std::path::{Path, PathBuf};
use std::process::Command;

/// Information about a single worktree candidate for cleanup.
/// NOTE: Cleanup only removes the git worktree directory and optionally the branch.
/// It NEVER deletes anything in the .tasks/ directory — all task artifacts are preserved.
#[derive(Clone, Debug)]
#[allow(dead_code)]
pub struct WorktreeCandidate {
    pub task_id: String,
    pub description: String,
    pub branch: String,
    pub base_branch: String,
    pub worktree_path: String,
    pub worktree_abs: Option<String>,
    pub status: String,
    pub color_scheme_index: usize,
    pub is_complete: bool,
    pub has_unmerged: bool,
    pub disk_size: Option<u64>,
    pub phase: Option<String>,
}

/// What the cleanup will do for one worktree.
#[derive(Clone, Debug)]
pub struct CleanupAction {
    pub task_id: String,
    pub commands: Vec<String>,
    pub warnings: Vec<String>,
}

/// Result of a single cleanup execution.
#[derive(Clone, Debug)]
pub struct CleanupResult {
    pub task_id: String,
    pub success: bool,
    pub message: String,
}

/// List all worktrees that are candidates for cleanup.
/// Candidates have worktree.status == "active" (exclude "cleaned" and "recyclable").
pub fn list_cleanup_candidates(repo_path: &Path) -> Vec<WorktreeCandidate> {
    let tasks_dir = repo_path.join(".tasks");
    let resolved = if tasks_dir.is_symlink() {
        std::fs::read_link(&tasks_dir)
            .ok()
            .map(|t| if t.is_absolute() { t } else { repo_path.join(t) })
            .and_then(|p| p.canonicalize().ok())
            .unwrap_or_else(|| tasks_dir.clone())
    } else {
        tasks_dir.clone()
    };

    let tasks = crate::data::task::load_tasks(&resolved);
    let mut candidates = Vec::new();

    for loaded in &tasks {
        if loaded.archived {
            continue;
        }
        let task = &loaded.state;
        let wt = match &task.worktree {
            Some(wt) if wt.status == "active" || wt.status == "done" => wt,
            _ => continue,
        };

        let wt_abs = resolve_worktree_abs(repo_path, wt);
        let disk_size = wt_abs.as_ref().and_then(|p| dir_size(Path::new(p)));
        let has_unmerged =
            !wt.branch.is_empty() && check_unmerged(repo_path, &wt.branch, &wt.base_branch);

        candidates.push(WorktreeCandidate {
            task_id: task.task_id.clone(),
            description: task.description.clone(),
            branch: wt.branch.clone(),
            base_branch: wt.base_branch.clone(),
            worktree_path: wt.path.clone(),
            worktree_abs: wt_abs,
            status: wt.status.clone(),
            color_scheme_index: wt.color_scheme_index,
            is_complete: task.is_complete(),
            has_unmerged,
            disk_size,
            phase: task.phase.clone(),
        });
    }

    candidates.sort_by(|a, b| a.task_id.cmp(&b.task_id));
    candidates
}

fn resolve_worktree_abs(
    repo_path: &Path,
    wt: &crate::data::task::WorktreeInfo,
) -> Option<String> {
    if let Some(ref launch) = wt.launch {
        if !launch.worktree_abs_path.is_empty() {
            return Some(launch.worktree_abs_path.clone());
        }
    }
    if !wt.path.is_empty() {
        let p = PathBuf::from(&wt.path);
        let abs = if p.is_absolute() { p } else { repo_path.join(&p) };
        return abs
            .canonicalize()
            .ok()
            .map(|p| p.to_string_lossy().to_string());
    }
    None
}

/// Check if branch has commits not in base_branch.
fn check_unmerged(repo_path: &Path, branch: &str, base: &str) -> bool {
    if base.is_empty() {
        return false;
    }
    let output = Command::new("git")
        .args(["log", "--oneline", &format!("{}..{}", base, branch)])
        .current_dir(repo_path)
        .output();
    match output {
        Ok(o) if o.status.success() => {
            let text = String::from_utf8_lossy(&o.stdout);
            !text.trim().is_empty()
        }
        _ => false,
    }
}

/// Get total size of directory in bytes (best-effort).
fn dir_size(path: &Path) -> Option<u64> {
    let output = Command::new("du")
        .args(["-sb", &path.to_string_lossy()])
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let text = String::from_utf8_lossy(&output.stdout);
    text.split_whitespace().next()?.parse().ok()
}

/// Generate a dry-run preview of what cleanup will do for selected worktrees.
pub fn preview_cleanup(
    _repo_path: &Path,
    candidates: &[&WorktreeCandidate],
    remove_branch: bool,
    keep_on_disk: bool,
) -> Vec<CleanupAction> {
    candidates
        .iter()
        .map(|c| {
            let mut commands = Vec::new();
            let mut warnings = Vec::new();

            if !keep_on_disk {
                let abs = c.worktree_abs.as_deref().unwrap_or(&c.worktree_path);
                commands.push(format!("git worktree remove {}", abs));
            }
            if remove_branch && !c.branch.is_empty() {
                commands.push(format!("git branch -d {}", c.branch));
            }
            if keep_on_disk {
                commands.push("state.json: worktree.status = \"recyclable\"".to_string());
            } else {
                commands.push("state.json: worktree.status = \"cleaned\"".to_string());
            }

            if c.has_unmerged {
                warnings.push(format!(
                    "Branch '{}' has unmerged commits into '{}'",
                    c.branch, c.base_branch
                ));
            }
            if !c.is_complete {
                let phase = c.phase.as_deref().unwrap_or("unknown");
                warnings.push(format!("Workflow not complete (current phase: {})", phase));
            }

            CleanupAction {
                task_id: c.task_id.clone(),
                commands,
                warnings,
            }
        })
        .collect()
}

/// Execute cleanup for multiple worktrees. Runs synchronously in native Rust.
/// IMPORTANT: This only removes the git worktree and branch. It NEVER deletes .tasks/ data.
pub fn execute_cleanup(
    repo_path: &Path,
    task_ids: &[String],
    remove_branch: bool,
    keep_on_disk: bool,
) -> Vec<CleanupResult> {
    task_ids
        .iter()
        .map(|task_id| execute_single_cleanup(repo_path, task_id, remove_branch, keep_on_disk))
        .collect()
}

fn execute_single_cleanup(
    repo_path: &Path,
    task_id: &str,
    remove_branch: bool,
    keep_on_disk: bool,
) -> CleanupResult {
    let state_file = repo_path.join(".tasks").join(task_id).join("state.json");
    let state_data = match std::fs::read_to_string(&state_file) {
        Ok(s) => s,
        Err(e) => return CleanupResult {
            task_id: task_id.to_string(), success: false,
            message: format!("Cannot read state.json: {}", e),
        },
    };
    let mut state: serde_json::Value = match serde_json::from_str(&state_data) {
        Ok(v) => v,
        Err(e) => return CleanupResult {
            task_id: task_id.to_string(), success: false,
            message: format!("Cannot parse state.json: {}", e),
        },
    };

    let worktree = match state.get("worktree") {
        Some(wt) if wt.is_object() => wt.clone(),
        _ => return CleanupResult {
            task_id: task_id.to_string(), success: false,
            message: "No worktree configured".to_string(),
        },
    };

    let wt_path = worktree.get("path").and_then(|v| v.as_str()).unwrap_or("");
    let branch = worktree.get("branch").and_then(|v| v.as_str()).unwrap_or("");
    let new_status = if keep_on_disk { "recyclable" } else { "cleaned" };
    let mut messages = Vec::new();

    if !keep_on_disk && !wt_path.is_empty() {
        let wt_abs = {
            let p = PathBuf::from(wt_path);
            if p.is_absolute() { p } else { repo_path.join(wt_path) }
        };
        let wt_abs_str = wt_abs.to_string_lossy().to_string();

        // Try git worktree remove first
        let result = Command::new("git")
            .args(["worktree", "remove", &wt_abs_str])
            .current_dir(repo_path)
            .output();

        let removed = match result {
            Ok(o) if o.status.success() => {
                messages.push("Worktree removed via git".to_string());
                true
            }
            Ok(o) => {
                let stderr = String::from_utf8_lossy(&o.stderr);
                // Fallback: if git says "not a working tree" or "prunable",
                // remove the directory manually and prune.
                if stderr.contains("not a working tree") || stderr.contains("not a valid") {
                    messages.push(format!("git worktree remove failed ({}), manual cleanup only", stderr.trim()));
                    // Only remove the specific worktree directory — never run
                    // `git worktree prune` as it is a global operation that can
                    // unregister other worktrees that appear stale (common on WSL).
                    if wt_abs.exists() {
                        if let Err(e) = std::fs::remove_dir_all(&wt_abs) {
                            return CleanupResult {
                                task_id: task_id.to_string(), success: false,
                                message: format!("Failed to remove directory: {}", e),
                            };
                        }
                        messages.push("Directory removed".to_string());
                    }
                    true
                } else {
                    return CleanupResult {
                        task_id: task_id.to_string(), success: false,
                        message: format!("git worktree remove failed: {}", stderr.trim()),
                    };
                }
            }
            Err(e) => return CleanupResult {
                task_id: task_id.to_string(), success: false,
                message: format!("Failed to run git: {}", e),
            },
        };

        if !removed {
            return CleanupResult {
                task_id: task_id.to_string(), success: false,
                message: "Worktree removal failed".to_string(),
            };
        }
    }

    // Delete branch if requested
    if remove_branch && !branch.is_empty() {
        let result = Command::new("git")
            .args(["branch", "-d", branch])
            .current_dir(repo_path)
            .output();
        match result {
            Ok(o) if o.status.success() => messages.push(format!("Branch '{}' deleted", branch)),
            Ok(o) => messages.push(format!("Branch delete warning: {}", String::from_utf8_lossy(&o.stderr).trim())),
            Err(_) => messages.push("Could not delete branch".to_string()),
        }
    }

    // Update state.json
    if let Some(wt) = state.get_mut("worktree").and_then(|v| v.as_object_mut()) {
        wt.insert("status".to_string(), serde_json::Value::String(new_status.to_string()));
        wt.insert("cleaned_at".to_string(), serde_json::Value::String(chrono_now()));
    }
    if let Some(o) = state.as_object_mut() {
        o.insert("updated_at".to_string(), serde_json::Value::String(chrono_now()));
    }

    if let Err(e) = std::fs::write(&state_file, serde_json::to_string_pretty(&state).unwrap_or_default()) {
        return CleanupResult {
            task_id: task_id.to_string(), success: false,
            message: format!("State update failed: {}", e),
        };
    }
    messages.push(format!("Status → {}", new_status));

    CleanupResult {
        task_id: task_id.to_string(),
        success: true,
        message: messages.join(". "),
    }
}

fn chrono_now() -> String {
    // Simple ISO 8601 timestamp without external crate
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    // Convert unix timestamp to ISO string (approximate, no TZ)
    let secs_per_day = 86400u64;
    let days = now / secs_per_day;
    let rem = now % secs_per_day;
    let hours = rem / 3600;
    let minutes = (rem % 3600) / 60;
    let seconds = rem % 60;

    // Days since epoch to Y-M-D (simplified)
    let mut y = 1970i64;
    let mut d = days as i64;
    loop {
        let days_in_year = if y % 4 == 0 && (y % 100 != 0 || y % 400 == 0) { 366 } else { 365 };
        if d < days_in_year { break; }
        d -= days_in_year;
        y += 1;
    }
    let leap = y % 4 == 0 && (y % 100 != 0 || y % 400 == 0);
    let month_days: [i64; 12] = [31, if leap {29} else {28}, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31];
    let mut m = 0usize;
    for (i, &md) in month_days.iter().enumerate() {
        if d < md { m = i; break; }
        d -= md;
    }
    format!("{:04}-{:02}-{:02}T{:02}:{:02}:{:02}", y, m + 1, d + 1, hours, minutes, seconds)
}
