#!/usr/bin/env python3
"""Ultra-light UserPromptSubmit hook: append user input to interactions.jsonl.

No heavy imports, no workflow_state, no subprocess spawning.
~5ms vs ~200ms for the full log-crew-interaction.py.

Only fires during active crew workflows (checks .tasks/.active_task).
"""
import json
import os
import sys

# Fast-path: no .tasks/ directory = no crew workflow
_tasks = os.path.join(os.getcwd(), ".tasks")
if not os.path.isdir(_tasks):
    sys.exit(0)

try:
    input_data = json.load(sys.stdin)
except Exception:
    sys.exit(0)

if input_data.get("hook_event_name") != "UserPromptSubmit":
    sys.exit(0)

prompt = input_data.get("prompt", "").strip()
if not prompt or prompt.startswith("/crew"):
    sys.exit(0)

# Find active task
_active = os.path.join(_tasks, ".active_task")
task_id = ""
if os.path.isfile(_active):
    try:
        task_id = open(_active).read().strip()
    except OSError:
        pass

if not task_id:
    sys.exit(0)

_task_dir = os.path.join(_tasks, task_id)
_state_file = os.path.join(_task_dir, "state.json")
if not os.path.isfile(_state_file):
    sys.exit(0)

# Read phase from state.json (minimal parse)
phase = ""
try:
    with open(_state_file) as f:
        state = json.load(f)
    if state.get("status") in ("completed", "complete"):
        sys.exit(0)
    phase = state.get("phase", "")
except Exception:
    pass

# Classify input (inline, no imports)
lower = prompt.lower()
if lower.endswith("?") or any(lower.startswith(w) for w in (
    "how ", "what ", "why ", "where ", "when ", "which ",
    "can you ", "could you ", "is there ", "does ", "should ",
)):
    itype = "question"
elif any(lower.startswith(w) for w in (
    "no,", "no ", "don't ", "stop ", "actually", "instead",
    "not that", "wrong ", "fix ", "revert ", "undo ",
)):
    itype = "correction"
elif any(s in lower for s in (
    "also ", "add ", "additionally", "one more thing",
    "please also", "include ",
)):
    itype = "new_requirement"
else:
    itype = "guidance"

# Append to interactions.jsonl
import time
entry = json.dumps({
    "timestamp": time.strftime("%Y-%m-%dT%H:%M:%SZ", time.gmtime()),
    "role": "human",
    "type": itype,
    "content": prompt,
    "phase": phase,
    "agent": "user",
    "source": "hook",
})

try:
    with open(os.path.join(_task_dir, "interactions.jsonl"), "a") as f:
        f.write(entry + "\n")
except OSError:
    pass

sys.exit(0)
