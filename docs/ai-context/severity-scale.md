# Severity Scale

All agents use this unified severity scale. Do not define severity ad-hoc in agent prompts — reference this file instead.

## Levels

| Level | Label    | Definition                                                              | Examples                                                        |
|-------|----------|-------------------------------------------------------------------------|-----------------------------------------------------------------|
| 4     | CRITICAL | Blocks production release. Data loss, security vulnerability, outage.   | SQL injection, auth bypass, data corruption, hardcoded secrets  |
| 3     | HIGH     | Degrades user experience significantly. Broken feature or regression.   | API returning wrong data, 10x latency regression, broken UI     |
| 2     | MEDIUM   | Code smell or tech debt. Functional but suboptimal.                     | Missing error handling, duplicated code, inconsistent naming    |
| 1     | LOW      | Style or preference. No functional impact.                              | Formatting, comment wording, import ordering                    |

## Usage by Agent

- **Reviewer** — Reports concerns at any level; flags CRITICAL/HIGH as blocking.
- **Skeptic** — Challenges assumptions; raises CRITICAL/HIGH concerns that block approval.
- **Feedback** — Surfaces all levels to the human; groups by severity in output.
- **Quality Guard** — Enforces the `concern_severity_threshold` gate; fails the phase if unresolved concerns meet or exceed the threshold.
- **Security Auditor** — Focuses on CRITICAL and HIGH; any security finding is CRITICAL unless demonstrably unexploitable.

## Config Mapping

`concern_severity_threshold` in `config/workflow-config.yaml` uses the string label:

```yaml
concern_severity_threshold: high   # block on HIGH and above
```

| Threshold value | Blocks on         |
|-----------------|-------------------|
| `critical`      | CRITICAL only     |
| `high`          | HIGH and above    |
| `medium`        | MEDIUM and above  |
| `low`           | Everything        |

The Quality Guard phase fails if any unresolved concern has severity at or above the configured threshold. The checkpoint only fires when `concern_threshold` (minimum count) is exceeded.
