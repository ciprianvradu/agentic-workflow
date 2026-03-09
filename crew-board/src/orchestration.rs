use serde::Deserialize;
use std::time::Instant;

/// Orchestration operating mode.
#[derive(Debug, Default, Clone, Copy, PartialEq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum OrchestrationMode {
    /// No automatic actions -- user controls everything.
    #[default]
    Manual,
    /// Suggest actions but require confirmation.
    SemiAuto,
    /// Execute actions automatically within guardrails.
    FullAuto,
}

/// Status of an orchestrated task.
#[derive(Debug, Clone, PartialEq)]
#[allow(dead_code)]
pub enum TaskStatus {
    Pending,
    Running,
    Completed,
    Failed { error: String },
}

/// A task being orchestrated.
#[derive(Debug, Clone)]
#[allow(dead_code)]
pub struct OrchestratedTask {
    pub task_id: String,
    pub status: TaskStatus,
    pub depends_on: Vec<String>,
    pub terminal_id: Option<String>,
    pub retries: u32,
    pub created_at: Instant,
    pub started_at: Option<Instant>,
    pub completed_at: Option<Instant>,
}

/// Circuit breaker: if N failures occur within a time window, downgrade mode.
#[derive(Debug)]
#[allow(dead_code)]
pub struct CircuitBreaker {
    failure_timestamps: Vec<Instant>,
    threshold: u32,
    window_secs: u64,
    pub tripped: bool,
}

#[allow(dead_code)]
impl CircuitBreaker {
    pub fn new(threshold: u32, window_secs: u64) -> Self {
        Self {
            failure_timestamps: Vec::new(),
            threshold,
            window_secs,
            tripped: false,
        }
    }

    /// Record a failure. Returns true if the circuit breaker tripped.
    pub fn record_failure(&mut self) -> bool {
        let now = Instant::now();
        self.failure_timestamps.push(now);
        // Remove old entries
        let cutoff = now.checked_sub(std::time::Duration::from_secs(self.window_secs));
        if let Some(cutoff) = cutoff {
            self.failure_timestamps.retain(|t| *t >= cutoff);
        }
        if self.failure_timestamps.len() >= self.threshold as usize {
            self.tripped = true;
        }
        self.tripped
    }

    /// Reset the circuit breaker.
    pub fn reset(&mut self) {
        self.failure_timestamps.clear();
        self.tripped = false;
    }
}

/// Guardrail limits for orchestration.
#[derive(Debug, Clone)]
pub struct Guardrails {
    pub cost_ceiling: f64,
    pub max_concurrent: u32,
    pub max_retries: u32,
}

impl Default for Guardrails {
    fn default() -> Self {
        Self {
            cost_ceiling: 50.0,
            max_concurrent: 5,
            max_retries: 5,
        }
    }
}

/// An action suggested or to be executed by the orchestration engine.
#[derive(Debug, Clone)]
#[allow(dead_code)]
pub enum OrchestrationAction {
    /// Launch a new terminal for a task.
    LaunchTask { task_id: String },
    /// Retry a failed task.
    RetryTask { task_id: String },
    /// Downgrade mode due to circuit breaker.
    DowngradeMode {
        from: OrchestrationMode,
        to: OrchestrationMode,
    },
    /// Cost limit reached -- pause all.
    CostLimitReached { current: f64, limit: f64 },
}

/// The main orchestration engine.
#[allow(dead_code)]
pub struct OrchestrationState {
    pub mode: OrchestrationMode,
    pub tasks: Vec<OrchestratedTask>,
    pub circuit_breaker: CircuitBreaker,
    pub guardrails: Guardrails,
    pub total_cost: f64,
    pub action_queue: Vec<OrchestrationAction>,
}

#[allow(dead_code)]
impl OrchestrationState {
    pub fn new(mode: OrchestrationMode) -> Self {
        Self {
            mode,
            tasks: Vec::new(),
            circuit_breaker: CircuitBreaker::new(3, 600),
            guardrails: Guardrails::default(),
            total_cost: 0.0,
            action_queue: Vec::new(),
        }
    }

    /// Add a task to be orchestrated.
    pub fn add_task(&mut self, task_id: String, depends_on: Vec<String>) {
        self.tasks.push(OrchestratedTask {
            task_id,
            status: TaskStatus::Pending,
            depends_on,
            terminal_id: None,
            retries: 0,
            created_at: Instant::now(),
            started_at: None,
            completed_at: None,
        });
    }

    /// Mark a task as running.
    pub fn start_task(&mut self, task_id: &str, terminal_id: &str) {
        if let Some(task) = self.tasks.iter_mut().find(|t| t.task_id == task_id) {
            task.status = TaskStatus::Running;
            task.terminal_id = Some(terminal_id.to_string());
            task.started_at = Some(Instant::now());
        }
    }

    /// Mark a task as completed.
    pub fn complete_task(&mut self, task_id: &str) {
        if let Some(task) = self.tasks.iter_mut().find(|t| t.task_id == task_id) {
            task.status = TaskStatus::Completed;
            task.completed_at = Some(Instant::now());
        }
    }

    /// Mark a task as failed.
    pub fn fail_task(&mut self, task_id: &str, error: String) {
        if let Some(task) = self.tasks.iter_mut().find(|t| t.task_id == task_id) {
            task.status = TaskStatus::Failed { error };
            task.completed_at = Some(Instant::now());

            // Record failure in circuit breaker
            let tripped = self.circuit_breaker.record_failure();
            if tripped && self.mode == OrchestrationMode::FullAuto {
                self.action_queue
                    .push(OrchestrationAction::DowngradeMode {
                        from: OrchestrationMode::FullAuto,
                        to: OrchestrationMode::SemiAuto,
                    });
                self.mode = OrchestrationMode::SemiAuto;
            }
        }
    }

    /// Record cost and check against ceiling.
    pub fn record_cost(&mut self, amount: f64) {
        self.total_cost += amount;
        if self.total_cost >= self.guardrails.cost_ceiling {
            self.action_queue
                .push(OrchestrationAction::CostLimitReached {
                    current: self.total_cost,
                    limit: self.guardrails.cost_ceiling,
                });
        }
    }

    /// Main tick: scan tasks, resolve dependencies, produce actions.
    pub fn tick(&mut self) {
        self.action_queue.clear();

        // Check cost ceiling
        if self.total_cost >= self.guardrails.cost_ceiling {
            self.action_queue
                .push(OrchestrationAction::CostLimitReached {
                    current: self.total_cost,
                    limit: self.guardrails.cost_ceiling,
                });
            return;
        }

        // Count running tasks
        let running = self
            .tasks
            .iter()
            .filter(|t| t.status == TaskStatus::Running)
            .count() as u32;

        // Find tasks whose dependencies are all completed
        let completed_ids: Vec<String> = self
            .tasks
            .iter()
            .filter(|t| t.status == TaskStatus::Completed)
            .map(|t| t.task_id.clone())
            .collect();

        let mut to_launch = Vec::new();
        for task in &self.tasks {
            if task.status != TaskStatus::Pending {
                continue;
            }
            let deps_met = task.depends_on.iter().all(|d| completed_ids.contains(d));
            if !deps_met {
                continue;
            }
            if running + to_launch.len() as u32 >= self.guardrails.max_concurrent {
                break;
            }
            to_launch.push(task.task_id.clone());
        }

        // Find failed tasks eligible for retry
        let mut to_retry = Vec::new();
        for task in &self.tasks {
            if let TaskStatus::Failed { .. } = &task.status {
                if task.retries < self.guardrails.max_retries {
                    if running + to_launch.len() as u32 + to_retry.len() as u32
                        >= self.guardrails.max_concurrent
                    {
                        break;
                    }
                    to_retry.push(task.task_id.clone());
                }
            }
        }

        // Generate actions based on mode
        match self.mode {
            OrchestrationMode::Manual => {
                // In manual mode, just populate queue as suggestions (no auto-execute)
                for id in to_launch {
                    self.action_queue
                        .push(OrchestrationAction::LaunchTask { task_id: id });
                }
                for id in to_retry {
                    self.action_queue
                        .push(OrchestrationAction::RetryTask { task_id: id });
                }
            }
            OrchestrationMode::SemiAuto | OrchestrationMode::FullAuto => {
                for id in to_launch {
                    self.action_queue
                        .push(OrchestrationAction::LaunchTask { task_id: id });
                }
                for id in to_retry {
                    self.action_queue
                        .push(OrchestrationAction::RetryTask { task_id: id });
                }
            }
        }

        // Check circuit breaker
        if self.circuit_breaker.tripped && self.mode == OrchestrationMode::FullAuto {
            self.action_queue
                .push(OrchestrationAction::DowngradeMode {
                    from: OrchestrationMode::FullAuto,
                    to: OrchestrationMode::SemiAuto,
                });
            self.mode = OrchestrationMode::SemiAuto;
        }
    }

    /// Count pending tasks.
    pub fn pending_count(&self) -> usize {
        self.tasks
            .iter()
            .filter(|t| t.status == TaskStatus::Pending)
            .count()
    }

    /// Count running tasks.
    pub fn running_count(&self) -> usize {
        self.tasks
            .iter()
            .filter(|t| t.status == TaskStatus::Running)
            .count()
    }

    /// Count completed tasks.
    pub fn completed_count(&self) -> usize {
        self.tasks
            .iter()
            .filter(|t| t.status == TaskStatus::Completed)
            .count()
    }

    /// Count failed tasks.
    pub fn failed_count(&self) -> usize {
        self.tasks
            .iter()
            .filter(|t| matches!(t.status, TaskStatus::Failed { .. }))
            .count()
    }

    /// Drain the action queue.
    pub fn drain_actions(&mut self) -> Vec<OrchestrationAction> {
        std::mem::take(&mut self.action_queue)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_new_state() {
        let state = OrchestrationState::new(OrchestrationMode::Manual);
        assert_eq!(state.mode, OrchestrationMode::Manual);
        assert!(state.tasks.is_empty());
        assert_eq!(state.total_cost, 0.0);
    }

    #[test]
    fn test_add_and_start_task() {
        let mut state = OrchestrationState::new(OrchestrationMode::Manual);
        state.add_task("T1".to_string(), vec![]);
        assert_eq!(state.pending_count(), 1);

        state.start_task("T1", "term1");
        assert_eq!(state.running_count(), 1);
        assert_eq!(state.pending_count(), 0);
    }

    #[test]
    fn test_complete_task() {
        let mut state = OrchestrationState::new(OrchestrationMode::Manual);
        state.add_task("T1".to_string(), vec![]);
        state.start_task("T1", "term1");
        state.complete_task("T1");
        assert_eq!(state.completed_count(), 1);
        assert_eq!(state.running_count(), 0);
    }

    #[test]
    fn test_dependency_resolution() {
        let mut state = OrchestrationState::new(OrchestrationMode::SemiAuto);
        state.add_task("T1".to_string(), vec![]);
        state.add_task("T2".to_string(), vec!["T1".to_string()]);

        state.tick();
        let actions = state.drain_actions();
        // T1 should be launchable, T2 should not (depends on T1)
        assert_eq!(actions.len(), 1);
        assert!(matches!(
            &actions[0],
            OrchestrationAction::LaunchTask { task_id } if task_id == "T1"
        ));
    }

    #[test]
    fn test_dependency_unblocks_after_completion() {
        let mut state = OrchestrationState::new(OrchestrationMode::SemiAuto);
        state.add_task("T1".to_string(), vec![]);
        state.add_task("T2".to_string(), vec!["T1".to_string()]);

        state.start_task("T1", "term1");
        state.complete_task("T1");
        state.tick();
        let actions = state.drain_actions();
        assert!(actions.iter().any(
            |a| matches!(a, OrchestrationAction::LaunchTask { task_id } if task_id == "T2")
        ));
    }

    #[test]
    fn test_max_concurrent_limit() {
        let mut state = OrchestrationState::new(OrchestrationMode::SemiAuto);
        state.guardrails.max_concurrent = 2;
        state.add_task("T1".to_string(), vec![]);
        state.add_task("T2".to_string(), vec![]);
        state.add_task("T3".to_string(), vec![]);

        // Start T1 as running
        state.start_task("T1", "term1");

        state.tick();
        let actions = state.drain_actions();
        // Only 1 more should be suggested (max 2 concurrent, 1 already running)
        let launch_count = actions
            .iter()
            .filter(|a| matches!(a, OrchestrationAction::LaunchTask { .. }))
            .count();
        assert_eq!(launch_count, 1);
    }

    #[test]
    fn test_cost_ceiling() {
        let mut state = OrchestrationState::new(OrchestrationMode::FullAuto);
        state.guardrails.cost_ceiling = 10.0;
        state.add_task("T1".to_string(), vec![]);

        state.record_cost(11.0);
        state.tick();
        let actions = state.drain_actions();
        assert!(actions
            .iter()
            .any(|a| matches!(a, OrchestrationAction::CostLimitReached { .. })));
    }

    #[test]
    fn test_circuit_breaker_trips() {
        let mut state = OrchestrationState::new(OrchestrationMode::FullAuto);
        state.circuit_breaker = CircuitBreaker::new(3, 600);

        state.add_task("T1".to_string(), vec![]);
        state.start_task("T1", "term1");
        state.fail_task("T1", "error1".to_string());

        state.add_task("T2".to_string(), vec![]);
        state.start_task("T2", "term2");
        state.fail_task("T2", "error2".to_string());

        state.add_task("T3".to_string(), vec![]);
        state.start_task("T3", "term3");
        state.fail_task("T3", "error3".to_string());

        // After 3 failures, mode should be downgraded
        assert_eq!(state.mode, OrchestrationMode::SemiAuto);
        assert!(state.circuit_breaker.tripped);
    }

    #[test]
    fn test_circuit_breaker_reset() {
        let mut breaker = CircuitBreaker::new(3, 600);
        breaker.record_failure();
        breaker.record_failure();
        breaker.record_failure();
        assert!(breaker.tripped);

        breaker.reset();
        assert!(!breaker.tripped);
        assert!(breaker.failure_timestamps.is_empty());
    }

    #[test]
    fn test_retry_action() {
        let mut state = OrchestrationState::new(OrchestrationMode::SemiAuto);
        state.add_task("T1".to_string(), vec![]);
        state.start_task("T1", "term1");
        state.fail_task("T1", "error".to_string());

        state.tick();
        let actions = state.drain_actions();
        assert!(actions.iter().any(
            |a| matches!(a, OrchestrationAction::RetryTask { task_id } if task_id == "T1")
        ));
    }
}
