# Crew Permissions

Auto-detect project tools and generate a `.claude/settings.local.json` allowlist to eliminate repeated permission prompts.

## Command: /crew-permissions

### Step 1: Detect Project Tools

Use Glob to check for the following files in the current working directory (repo root):

- `package.json` → npm (also check `packageManager` field for yarn/pnpm)
- `Cargo.toml` → cargo
- `pyproject.toml`, `setup.py`, or `requirements.txt` → pip/pytest
- `Makefile` → make
- `tsconfig.json` → tsc
- `.eslintrc*` or `eslint.config.*` → eslint
- `Dockerfile` → docker
- `go.mod` → go
- `.prettierrc*` or `prettier.config.*` → prettier
- `jest.config.*` → jest
- `vitest.config.*` → vitest

If `package.json` exists, read it to check the `packageManager` field to detect yarn or pnpm.

### Step 2: Build Permission List

Start with the always-included baseline permissions:

```
Bash(git status:*)
Bash(git diff:*)
Bash(git log:*)
Bash(git show:*)
Bash(git branch:*)
Bash(git add:*)
Bash(git commit:*)
Bash(tree:*)
Bash(find:*)
Bash(ls:*)
Bash(wc:*)
Bash(python3:*)
```

Then append based on detected tools:

- **npm** (`package.json` with no `packageManager`, or `packageManager` starts with `npm`):
  `Bash(npm test:*)`, `Bash(npm run:*)`, `Bash(npx:*)`
- **yarn** (`packageManager` starts with `yarn`):
  `Bash(yarn:*)`, `Bash(yarn test:*)`, `Bash(yarn run:*)`
- **pnpm** (`packageManager` starts with `pnpm`):
  `Bash(pnpm:*)`, `Bash(pnpm test:*)`, `Bash(pnpm run:*)`
- **cargo** (`Cargo.toml`):
  `Bash(cargo test:*)`, `Bash(cargo build:*)`, `Bash(cargo check:*)`, `Bash(cargo clippy:*)`
- **pytest** (`pyproject.toml`, `setup.py`, or `requirements.txt`):
  `Bash(python3 -m pytest:*)`
- **make** (`Makefile`):
  `Bash(make:*)`
- **go** (`go.mod`):
  `Bash(go test:*)`, `Bash(go build:*)`, `Bash(go vet:*)`

Also always include MCP tool patterns:
```
mcp__agentic-workflow__*
```

### Step 3: Read Existing Settings

Check if `.claude/settings.local.json` exists. If it does, read it and extract the current `allow` array. You will merge — never remove existing entries.

If the file does not exist, start with an empty `allow` array.

### Step 4: Show Diff and Confirm

Display to the user:

- The project tools detected (e.g., "Detected: npm, pytest, make")
- The new permission patterns that will be added (entries not already present)
- The patterns already present (will be kept unchanged)

Ask the user to confirm before writing:

```
Ready to write .claude/settings.local.json with X permission(s) (Y new, Z existing).
Proceed? [Yes / No / Show full list]
```

If the user says "Show full list", display all entries that will be in the final file, then ask again.

If the user says No, abort without writing.

### Step 5: Write Settings File

Merge the new permissions with existing ones (deduplicate, preserve order — existing entries first, then new ones appended). Write the result to `.claude/settings.local.json`:

```json
{
  "allow": [
    "Bash(git status:*)",
    "..."
  ]
}
```

Preserve any other top-level keys already in the file (e.g., `deny`, `env`). Only merge the `allow` array.

### Step 6: Confirm Success

Display:

```
Written .claude/settings.local.json
  Tools detected: <list>
  Permissions:    <total count> entries (<new count> added)

Restart Claude Code (or open a new session) for the new permissions to take effect.
```

This command is idempotent — running it again is safe and will only add missing entries.
