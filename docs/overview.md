# Agentic Development Workflow

### Your AI Development Team, Working Together

---

> **What if you could give a task to a team of AI specialists -- a planner, a code reviewer, a builder, a quality guard, and a technical writer -- and they would plan, challenge, build, verify, and document the work, checking in with you at every critical decision?**

That is what the Agentic Development Workflow does. It orchestrates multiple AI agents, each with a distinct role and personality, to take a software task from idea to implementation with built-in quality gates and human oversight.

This document explains the workflow for **managers, team leads, stakeholders, and anyone curious** about how structured AI-assisted development works -- no programming knowledge required.

---

## Table of Contents

- [The Big Picture](#the-big-picture)
- [How It Works: The Assembly Line](#how-it-works-the-assembly-line)
- [Meet the Team](#meet-the-team)
- [The Workflow Step by Step](#the-workflow-step-by-step)
- [Human Checkpoints: You Stay in Control](#human-checkpoints-you-stay-in-control)
- [Workflow Modes: Right-Sizing the Process](#workflow-modes-right-sizing-the-process)
- [Specialist Agents: Called When Needed](#specialist-agents-called-when-needed)
- [Cross-Platform Support](#cross-platform-support)
- [Configuration: Tuning the Process](#configuration-tuning-the-process)
- [Advanced Capabilities](#advanced-capabilities)
- [Getting Started](#getting-started)
- [Frequently Asked Questions](#frequently-asked-questions)

---

## The Big Picture

Traditional software development follows a pattern: someone designs a solution, someone builds it, someone reviews it, and someone documents it. Each step catches different kinds of problems.

The Agentic Development Workflow **replicates this proven process using AI agents**, where each agent has a focused specialty. Instead of one AI trying to do everything at once, the work flows through a structured pipeline -- the same way a well-run engineering team operates.

```mermaid
graph LR
    A["You describe\na task"] --> B["AI team plans,\nreviews, and\nchallenges the plan"]
    B --> C["You approve\nthe approach"]
    C --> D["AI builds it\nstep by step"]
    D --> E["AI documents\nthe changes"]
    E --> F["You review and\ncommit the work"]

    style A fill:#4A90D9,stroke:#2C5F8A,color:#fff
    style B fill:#7B68EE,stroke:#5A4FBF,color:#fff
    style C fill:#F5A623,stroke:#D4891E,color:#fff
    style D fill:#7B68EE,stroke:#5A4FBF,color:#fff
    style E fill:#7B68EE,stroke:#5A4FBF,color:#fff
    style F fill:#F5A623,stroke:#D4891E,color:#fff
```

**Key insight:** The human stays in the driver's seat. AI agents do the heavy lifting, but you make the decisions at every important junction.

---

## How It Works: The Assembly Line

Think of it like an automotive assembly line, but for software. Each station has a specialist who focuses on one thing and does it exceptionally well. The work moves forward only when the current station is satisfied.

```mermaid
graph TB
    subgraph planning ["Phase 1: Planning"]
        direction LR
        PL[Planner] --> REV[Reviewer]
    end

    subgraph building ["Phase 2: Building"]
        direction LR
        IMP[Implementer]
    end

    subgraph quality ["Phase 3: Quality (thorough only)"]
        direction LR
        QG[Quality Guard] ~~~ SA[Security Auditor]
    end

    subgraph documenting ["Phase 4: Documentation"]
        direction LR
        TW[Technical Writer]
    end

    planning --> building
    building --> quality
    quality --> documenting

    style planning fill:#E8F0FE,stroke:#4A90D9,color:#333
    style building fill:#E8F5E9,stroke:#4CAF50,color:#333
    style quality fill:#FFECB3,stroke:#FFA726,color:#333
    style documenting fill:#FFF3E0,stroke:#F5A623,color:#333
    style PL fill:#4A90D9,stroke:#2C5F8A,color:#fff
    style REV fill:#26A69A,stroke:#1B7A72,color:#fff
    style IMP fill:#66BB6A,stroke:#388E3C,color:#fff
    style QG fill:#FFA726,stroke:#E65100,color:#fff
    style SA fill:#EF5350,stroke:#C62828,color:#fff
    style TW fill:#AB47BC,stroke:#7B1FA2,color:#fff
```

The Skeptic runs in both **standard** and **thorough** mode, stress-testing plans before implementation begins. In thorough mode, the Reviewer and Skeptic run in parallel, and Quality Guard + Security Auditor run in parallel post-implementation. The phases ensure that **no code is written until the plan has been challenged from multiple perspectives**, and no code is committed until it matches the approved plan.

---

## Meet the Team

Each AI agent has a distinct personality, focus area, and set of permissions. Here is who does what, explained through real-world analogies.

### The Core Team

| Agent | Role Analogy | What They Do | Permissions |
|-------|-------------|--------------|-------------|
| **Planner** | Chief Engineer + Senior Architect | Analyzes how the task fits into the overall system, identifies risks and dependencies, then creates a detailed step-by-step implementation plan. Combines the big-picture thinking with the detailed spec writing in one pass. | Read-only |
| **Reviewer** | Experienced PR Reviewer | Checks the plan for completeness, correctness, and gaps. Verifies code examples, import paths, and pattern compliance against the knowledge base. (Thorough only) | Read-only |
| **Skeptic** | Devil's Advocate + Chaos Engineer | Stress-tests the plan for real-world failure modes -- 3 AM outages, edge cases, race conditions, hostile external dependencies. Provides a fundamentally different perspective from the Planner. (Standard + Thorough) | Read-only |
| **Implementer** | Developer writing the code | Executes the approved plan step by step, running tests after each step. The only agent that actually writes code. Convention files from the project's `ai-context/` folder are injected directly into its prompt. | Read & Write |
| **Quality Guard** | QA Lead at a checkpoint | Reviews the built code against the approved plan. Checks code quality, plan adherence, and test coverage. Recommends whether to continue, adjust, or restart. Runs in parallel with Security Auditor. Convention files are also injected into its prompt. (Thorough only) | Read-only |
| **Security Auditor** | Penetration Tester | Reviews code for security vulnerabilities -- OWASP Top 10, secrets exposure, authentication flaws, authorization bypasses. Runs in parallel with Quality Guard. (Thorough only) | Read-only |
| **Technical Writer** | Documentation specialist | Updates project documentation to capture new knowledge, patterns, and decisions. Keeps the knowledge base current. | Read & Write |

> **Note:** The legacy Architect and Developer agents remain available for ad-hoc consultation via `/crew ask`, but they are no longer part of the default pipeline.

```mermaid
graph TD
    subgraph readonly ["Read-Only Agents (Cannot Change Code)"]
        PL["Planner\n\nSees the forest\nAND writes the recipe"]
        REV["Reviewer\n\nThe thorough proofreader\n+ devil's advocate"]
        QG["Quality Guard\n\nThe quality\ngatekeeper"]
        SA["Security Auditor\n\nThe penetration\ntester"]
    end

    subgraph readwrite ["Read-Write Agents (Can Change Code)"]
        IMP["Implementer\n\nFollows the recipe\nprecisely"]
        TW["Technical Writer\n\nCaptures new\nknowledge"]
    end

    style readonly fill:#F3E5F5,stroke:#AB47BC,color:#333
    style readwrite fill:#E8F5E9,stroke:#66BB6A,color:#333
    style PL fill:#4A90D9,stroke:#2C5F8A,color:#fff
    style REV fill:#26A69A,stroke:#1B7A72,color:#fff
    style QG fill:#FFA726,stroke:#E65100,color:#fff
    style SA fill:#EF5350,stroke:#C62828,color:#fff
    style IMP fill:#66BB6A,stroke:#388E3C,color:#fff
    style TW fill:#AB47BC,stroke:#7B1FA2,color:#fff
```

**Notice:** Four of the six core agents are **read-only** -- they can look at the code and analyze it, but they cannot change anything. This separation of concerns is a deliberate safety measure. Only the Implementer and Technical Writer can modify files, and both follow plans that have been reviewed and approved.

---

## The Workflow Step by Step

Here is the full journey of a task from start to finish.

### Phase 1: The Planning Loop

Planning is streamlined but thorough. The Planner analyzes the system and creates the implementation plan in a single pass. In thorough mode, the Reviewer then stress-tests the plan for gaps and failure modes before building begins.

```mermaid
flowchart TD
    START([Task Described]) --> PLAN
    PLAN["1. Planner\nAnalyzes system impact,\ncreates step-by-step plan"]
    PLAN --> HC1{Human\nCheckpoint}
    HC1 -->|Approve| THOROUGH{Thorough\nmode?}
    HC1 -->|Revise| PLAN

    THOROUGH -->|Yes| REV
    THOROUGH -->|No| IMPL_PHASE

    REV["2. Reviewer\nChecks plan for gaps,\nfailure modes, edge cases"]
    REV --> HC2{Human\nCheckpoint}
    HC2 -->|Approve| IMPL_PHASE
    HC2 -->|Revise| PLAN

    IMPL_PHASE([Proceed to Building])

    style START fill:#4A90D9,stroke:#2C5F8A,color:#fff
    style PLAN fill:#4A90D9,stroke:#2C5F8A,color:#fff
    style REV fill:#26A69A,stroke:#1B7A72,color:#fff
    style THOROUGH fill:#E0E0E0,stroke:#9E9E9E,color:#333
    style HC1 fill:#F5A623,stroke:#D4891E,color:#fff
    style HC2 fill:#F5A623,stroke:#D4891E,color:#fff
    style IMPL_PHASE fill:#66BB6A,stroke:#388E3C,color:#fff
```

**Why iterate?** The first plan is rarely perfect. By having the Planner combine architectural analysis with detailed planning, and having the Reviewer challenge the plan *before* any code is written, most problems are caught on paper -- where they are cheap to fix -- rather than in code, where they are expensive.

### Phase 2: The Implementation Loop

Once the plan is approved, the Implementer builds it step by step. After each step, tests run automatically.

```mermaid
flowchart TD
    START([Approved Plan]) --> STEP

    STEP["Implementer\nexecutes next step"] --> TEST
    TEST["Run automated\ntests"] --> CHECK{Tests\npass?}

    CHECK -->|Yes| PROGRESS{Milestone\nreached?}
    CHECK -->|No| RETRY["Retry with\ndifferent approach"]
    RETRY --> STEP

    PROGRESS -->|"25% / 75%"| STEP
    PROGRESS -->|"50%"| HC_MID{Human\nCheckpoint}
    PROGRESS -->|"100%"| THOROUGH2{Thorough\nmode?}

    HC_MID -->|Continue| STEP
    HC_MID -->|Adjust| ADJ["Update\nremaining steps"]
    ADJ --> STEP

    THOROUGH2 -->|Yes| PARALLEL
    THOROUGH2 -->|No| DOC

    PARALLEL["Quality Guard + Security Auditor\n(run in parallel)"] --> VERDICT{Verdict}

    VERDICT -->|Continue| DOC([Proceed to\nDocumentation])
    VERDICT -->|Adjust| ADJ2["Revise remaining\nsteps"]
    ADJ2 --> STEP
    VERDICT -->|Restart| START

    style START fill:#66BB6A,stroke:#388E3C,color:#fff
    style STEP fill:#66BB6A,stroke:#388E3C,color:#fff
    style TEST fill:#29B6F6,stroke:#0277BD,color:#fff
    style CHECK fill:#FFA726,stroke:#E65100,color:#fff
    style PROGRESS fill:#FFA726,stroke:#E65100,color:#fff
    style RETRY fill:#EF5350,stroke:#C62828,color:#fff
    style HC_MID fill:#F5A623,stroke:#D4891E,color:#fff
    style THOROUGH2 fill:#E0E0E0,stroke:#9E9E9E,color:#333
    style PARALLEL fill:#FFA726,stroke:#E65100,color:#fff
    style VERDICT fill:#FFA726,stroke:#E65100,color:#fff
    style DOC fill:#AB47BC,stroke:#7B1FA2,color:#fff
    style ADJ fill:#5C6BC0,stroke:#3F4FA0,color:#fff
    style ADJ2 fill:#5C6BC0,stroke:#3F4FA0,color:#fff
```

**Key safeguard:** If an implementation step fails its tests repeatedly, the system escalates to the human rather than continuing in a broken state. In thorough mode, the Quality Guard and Security Auditor run in parallel after implementation to check code quality and security before documentation.

### Phase 3: Documentation

The Technical Writer agent reviews all the changes that were made and updates the project's documentation accordingly -- capturing new patterns, decisions, and knowledge so the next task benefits from what was learned.

---

## Human Checkpoints: You Stay in Control

One of the most important design principles is that **humans remain in the decision loop**. The system pauses at configurable checkpoints and asks for your input.

```mermaid
flowchart LR
    AGENT["Agent completes\nits work"] --> PRESENT["Summary\npresented to you"]
    PRESENT --> DECIDE{Your Decision}

    DECIDE -->|"Approve"| NEXT["Continue to\nnext agent"]
    DECIDE -->|"Revise"| BACK["Send back\nwith feedback"]
    DECIDE -->|"Restart"| OVER["Start phase\nover"]
    DECIDE -->|"Skip"| SKIP["Skip this\ncheckpoint"]

    style AGENT fill:#7B68EE,stroke:#5A4FBF,color:#fff
    style PRESENT fill:#29B6F6,stroke:#0277BD,color:#fff
    style DECIDE fill:#F5A623,stroke:#D4891E,color:#fff
    style NEXT fill:#66BB6A,stroke:#388E3C,color:#fff
    style BACK fill:#FFA726,stroke:#E65100,color:#fff
    style OVER fill:#EF5350,stroke:#C62828,color:#fff
    style SKIP fill:#BDBDBD,stroke:#757575,color:#333
```

### Default Checkpoint Configuration

| Checkpoint | Default | Why It Matters |
|-----------|---------|----------------|
| After Planner | Enabled | Review the plan (system analysis + implementation steps) before building begins |
| After Reviewer | Enabled | Review gaps and failure modes found before committing to building (thorough mode only) |
| At 50% implementation | Enabled | Mid-build sanity check -- is it on track? |
| Before commit | Enabled | Always review before changes become permanent |
| After Technical Writer | Enabled | Review documentation updates before they are applied |

All checkpoints are configurable. You can enable or disable any of them, tailoring the level of human involvement to your team's needs and comfort level.

### Additional Safety Triggers

Beyond scheduled checkpoints, the system will **automatically escalate** to you if it detects:

- Implementation deviating significantly from the approved plan
- Tests failing unexpectedly
- Scope creep -- the work expanding beyond the original task
- Security concerns
- Repeated failures on the same step

---

## Workflow Modes: Right-Sizing the Process

Not every task needs the full six-agent treatment. A simple typo fix does not require a Planner's review. The workflow supports three modes that match the process to the task's complexity.

```mermaid
graph TB
    TASK["Incoming Task"] --> AUTO{Auto-Detect\nMode}

    AUTO -->|"Security, migrations,\nbreaking changes"| THOROUGH
    AUTO -->|"Standard features,\nrefactoring"| STANDARD
    AUTO -->|"Typos, renames,\ntrivial fixes"| QUICK

    subgraph THOROUGH ["Thorough Mode"]
        direction LR
        TH1["Planner"] --> TH2["Reviewer"] --> TH3["Implementer"] --> TH4["Quality Guard\n+ Security Auditor\n(parallel)"] --> TH5["Tech Writer"]
    end

    subgraph STANDARD ["Standard Mode"]
        direction LR
        S1["Planner"] --> S2["Implementer"] --> S3["Tech Writer"]
    end

    subgraph QUICK ["Quick Mode"]
        direction LR
        Q1["Implementer"]
    end

    style TASK fill:#4A90D9,stroke:#2C5F8A,color:#fff
    style AUTO fill:#F5A623,stroke:#D4891E,color:#fff
    style THOROUGH fill:#FFCDD2,stroke:#EF5350,color:#333
    style STANDARD fill:#C8E6C9,stroke:#66BB6A,color:#333
    style QUICK fill:#F5F5F5,stroke:#BDBDBD,color:#333
```

### Mode Comparison

| Mode | Agents Used | Best For | Estimated Cost |
|------|------------|----------|---------------|
| **Thorough** | All 6 agents (Planner, Reviewer, Implementer, Quality Guard + Security Auditor in parallel, Writer) | Security changes, database migrations, breaking API changes, critical systems | $0.50+ |
| **Standard** | 3 agents (Planner, Implementer, Technical Writer) | Standard features, refactoring, routine to non-trivial changes | ~$0.15 |
| **Quick** | 1 agent (Implementer only) | Trivial fixes -- typos, renames, comment updates | ~$0.05 |

### Auto Mode

By default, the system runs in **Auto** mode, which analyzes your task description and picks the right workflow mode automatically. For example:

- *"Fix the typo in the README"* -- selects **Quick**
- *"Add a caching layer to the API"* -- selects **Standard**
- *"Implement OAuth2 authentication"* -- selects **Thorough** (because it involves security)
- *"Refactor the payment module"* -- selects **Standard** or **Thorough** depending on scope

You can always override this by specifying the mode explicitly.

---

## Specialist Agents: Called When Needed

In addition to the six core agents, there are **specialist agents** that activate automatically when the task involves their area of expertise.

```mermaid
flowchart TD
    TASK["Task Keywords\nand File Patterns"] --> DETECT{Auto-Detection}

    DETECT -->|"performance, cache,\noptimize, scale"| PERF["Performance Analyst\n\nChecks for\nbottlenecks"]
    DETECT -->|"API, endpoint,\nbreaking, schema"| API["API Guardian\n\nProtects API\ncontracts"]
    DETECT -->|"UI, component,\nform, accessibility"| A11Y["Accessibility Reviewer\n\nEnsures inclusive\ndesign"]

    PERF --> PIPELINE["Inserted into the\nmain workflow pipeline"]
    API --> PIPELINE
    A11Y --> PIPELINE

    style TASK fill:#4A90D9,stroke:#2C5F8A,color:#fff
    style DETECT fill:#F5A623,stroke:#D4891E,color:#fff
    style PERF fill:#FF7043,stroke:#D84315,color:#fff
    style API fill:#5C6BC0,stroke:#3F4FA0,color:#fff
    style A11Y fill:#26A69A,stroke:#1B7A72,color:#fff
    style PIPELINE fill:#7B68EE,stroke:#5A4FBF,color:#fff
```

| Specialist | Triggered By | What They Check |
|-----------|-------------|-----------------|
| **Performance Analyst** | Keywords like *performance*, *cache*, *optimize*, *scale*; or database/query files | Slow queries, missing caching, N+1 problems, scalability bottlenecks |
| **API Guardian** | Keywords like *API*, *endpoint*, *schema*, *breaking change*; or API/routes files | Backward compatibility, contract changes, versioning, deprecation |
| **Accessibility Reviewer** | Keywords like *UI*, *component*, *form*, *accessibility*; or frontend component files | Screen reader support, keyboard navigation, color contrast, WCAG compliance |

> **Note:** The **Security Auditor** is no longer a specialist agent. It is part of the thorough pipeline, running in parallel with Quality Guard after implementation.

These specialists are **automatically detected** based on the task description and the files being changed. They can also be enabled or disabled manually.

---

## Cross-Platform Support

The Agentic Development Workflow works across three major AI coding platforms:

```mermaid
graph LR
    AW["Agentic\nDevelopment\nWorkflow"] --> CLAUDE["Claude Code\n(Anthropic)"]
    AW --> COPILOT["GitHub Copilot\nCLI"]
    AW --> GEMINI["Gemini CLI\n(Google)"]

    style AW fill:#7B68EE,stroke:#5A4FBF,color:#fff
    style CLAUDE fill:#D4A574,stroke:#A67B4B,color:#fff
    style COPILOT fill:#333,stroke:#555,color:#fff
    style GEMINI fill:#4285F4,stroke:#2C5F8A,color:#fff
```

The same agent definitions and workflow configuration work regardless of which platform you use. This means:

- **No vendor lock-in** -- switch between platforms freely
- **Consistent process** -- the workflow behaves the same everywhere
- **Team flexibility** -- different team members can use different platforms

---

## Configuration: Tuning the Process

The workflow is highly configurable through a simple settings file (YAML format). Configuration follows a **cascade** -- each level can override the one before it.

```mermaid
graph TD
    G["Global Defaults\n(applies everywhere)"] --> P["Project Config\n(per-repository)"]
    P --> T["Task Config\n(per-task overrides)"]
    T --> C["Command Arguments\n(one-time overrides)"]

    style G fill:#E8F0FE,stroke:#4A90D9,color:#333
    style P fill:#C8E6C9,stroke:#66BB6A,color:#333
    style T fill:#FFF3E0,stroke:#F5A623,color:#333
    style C fill:#FFCDD2,stroke:#EF5350,color:#333
```

**What this means in practice:**

1. **Global defaults** set the baseline for all projects (e.g., "always pause before committing")
2. **Project config** customizes for a specific codebase (e.g., "this project needs the Security Auditor always on")
3. **Task config** adjusts for a specific piece of work (e.g., "use Thorough mode for this task")
4. **Command arguments** let you override anything on the fly (e.g., "run with loop mode this time")

### What Can Be Configured?

| Category | Examples |
|----------|---------|
| **Checkpoints** | Which human approval points are active |
| **Workflow mode** | Which agents are included in the pipeline |
| **AI models** | Which AI model each agent uses |
| **Auto-actions** | What agents can do without asking (run tests, create files, etc.) |
| **Iteration limits** | Maximum retries before escalating to a human |
| **Cost tracking** | Token usage and cost reporting |
| **Integrations** | Jira issue tracking, Beads issue linking |

---

## Advanced Capabilities

Beyond the core workflow, the system includes several advanced features for teams that need them.

### Parallel Workflows with Git Worktrees

Teams can run **multiple workflows simultaneously** on different tasks, each in its own isolated branch. This is like having multiple workbenches -- each task gets its own clean workspace without interfering with others.

### Gemini Research Integration

For large codebases, the system can use Google's Gemini (which has a massive context window) to pre-analyze the codebase and provide focused context to each agent. This makes the agents smarter about the specific project they are working on.

### Custom Phases (Lifecycle Hooks)

Teams can inject their own steps into the workflow pipeline without modifying the core system. Custom phases support three types:

- **Skills** -- invoke a Claude Code slash command (e.g., a Jira triage step before planning)
- **Scripts** -- run any shell command (e.g., license checks, static analysis, encoding validation)
- **Agents** -- spawn a subagent with a custom prompt file (e.g., domain-specific review)

Each custom phase specifies where it runs (`after: init`, `before: complete`, `after: reviewer`, etc.) and optional conditions that control when it activates (by keyword, workflow mode, or file patterns). See [Custom Phases](ai-context/custom-phases.md) for details.

### Cross-Task Memory

The system **learns across tasks**. Decisions, patterns, gotchas, and blockers discovered during one task are saved and can inform future tasks. This means the AI team gets better at working with your codebase over time.

### Error Pattern Learning

When a specific error is encountered and solved, the solution is recorded. If the same error appears in a future task, the system can suggest the known fix automatically.

### Cost Tracking

Every agent's token usage and estimated cost is tracked and reported at the end of each workflow, giving full transparency into resource consumption.

### Model Resilience

If an AI model is temporarily unavailable (rate limits, outages), the system automatically falls back to alternative models with exponential backoff and retry logic. Work is not lost due to transient failures.

---

## Getting Started

Getting started with the Agentic Development Workflow is straightforward.

### Step 1: Install

The workflow is installed as a plugin for your AI coding assistant. Installation scripts are provided for each supported platform.

### Step 2: Start a Workflow

Once installed, you interact with the system through simple commands in your terminal:

| Command | What It Does |
|---------|-------------|
| `/crew start "Add user login"` | Start a new workflow with a task description |
| `/crew status` | Check the status of all active workflows |
| `/crew resume` | Resume a paused workflow |
| `/crew ask architect "Is this approach safe?"` | Consult a single agent without starting a full workflow |
| `/crew config` | View your current configuration |

### Step 3: Follow the Checkpoints

The system guides you through the workflow. At each checkpoint, you will see a summary of what the agent found and be asked how to proceed. Your options are always clear:

- **Approve** -- move forward
- **Revise** -- send it back with your feedback
- **Restart** -- start the phase over

### Step 4: Review and Commit

When the workflow completes, you review the changes and approve the final commit. The system generates a commit message summarizing what was done.

---

## Frequently Asked Questions

### General

**Q: Do I need to be a developer to use this?**
A: You need to be working in a software development context, but you do not need to understand the code the AI is writing. The checkpoints present information in plain language, and you can always ask a specific agent for clarification using `/crew ask`.

**Q: How much does it cost per task?**
A: It depends on the workflow mode and the complexity of the task. Trivial fixes (Quick mode) cost around $0.05 in AI tokens. Complex features (Thorough mode) typically cost $0.50 or more. The system tracks and reports exact costs for every workflow.

**Q: Can the AI make changes I did not approve?**
A: No. The system is designed with configurable checkpoints, and by default, it pauses before committing any changes. Git operations (staging, committing, pushing) require explicit human approval by default.

**Q: What happens if the AI gets stuck?**
A: The system has built-in escalation. If an agent fails repeatedly, exceeds its iteration limit, or encounters something unexpected, it pauses and asks for human input rather than continuing blindly.

### About the Agents

**Q: Why separate agents instead of one AI doing everything?**
A: For the same reason engineering teams have specialists. A planner thinks differently than a quality reviewer. By giving each agent a focused role and specific instructions, the output is more thorough and catches more issues than a single-pass approach. The Reviewer, for example, is specifically prompted to think about failure modes and edge cases -- something a "do everything" agent tends to gloss over.

**Q: Can the Reviewer or Quality Guard block the workflow?**
A: They can raise concerns that trigger a human checkpoint, but they cannot block the workflow on their own. The human always makes the final decision about whether to proceed, revise, or restart.

**Q: What if I disagree with an agent's assessment?**
A: At any checkpoint, you can override the agent's recommendation. You can approve despite concerns, send the plan back for revision with your own guidance, or skip the checkpoint entirely.

### Technical

**Q: Does this work with any programming language?**
A: Yes. The workflow is language-agnostic. The AI agents can work with any programming language that the underlying AI model supports, which includes all major languages.

**Q: Can I customize which agents are used?**
A: Absolutely. The workflow modes control which agents run, and you can define custom configurations per project or per task. You can also enable or disable specialist agents as needed.

**Q: Is my code sent to external services?**
A: The workflow uses the same AI services (Claude, Gemini, Copilot) that your team already uses for AI-assisted coding. No additional services are involved. Your code goes only to the AI provider you have chosen.

**Q: Can multiple people use this on the same project simultaneously?**
A: Yes. The git worktree support allows multiple workflows to run in parallel on isolated branches, avoiding conflicts.

### Process

**Q: How does this compare to a human code review?**
A: It complements rather than replaces human review. The AI agents catch many mechanical issues (missed edge cases, inconsistencies with project patterns, common security mistakes) quickly and consistently. Human reviewers can then focus on higher-level concerns like business logic correctness and design decisions.

**Q: Can I use only parts of the workflow?**
A: Yes. You can consult any individual agent using `/crew ask <agent> "question"` without starting a full workflow. You can also use Quick mode for trivial fixes that only need the Implementer.

**Q: What if I want to add my own custom agents?**
A: Agent definitions are markdown files with instructions. Adding a new specialist agent involves creating a new markdown file and configuring its trigger conditions in the workflow configuration. You can also use **Custom Phases** to inject skills, scripts, or agents at any point in the pipeline -- see `custom_phases` in `workflow-config.yaml` for examples.

---

## Summary

The Agentic Development Workflow brings **structured, multi-specialist AI collaboration** to software development. Instead of treating AI as a single tool that does everything, it creates a team of focused specialists that plan, challenge, build, verify, and document -- with humans making the key decisions throughout.

```mermaid
mindmap
  root((Agentic\nWorkflow))
    Planning
      Planner analyzes and plans
      Reviewer checks gaps + failure modes
    Building
      Implementer executes steps
      Tests run automatically
    Quality (thorough)
      Quality Guard checks alignment
      Security Auditor checks vulnerabilities
      Run in parallel
    Quality
      Human checkpoints
      Configurable safety gates
      Automatic escalation
    Flexibility
      Three workflow modes
      Auto-detection
      Specialist agents
      Cross-platform support
```

**The result:** Higher quality software changes, delivered faster, with full transparency and human oversight at every critical decision point.

---

<p align="center"><em>Agentic Development Workflow is open source and available on <a href="https://github.com/Spiris-Innovation-Tech-Dev/agentic-workflow">GitHub</a>.</em></p>
