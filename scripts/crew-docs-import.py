#!/usr/bin/env python3
"""
Import doc templates into a project's knowledge base.

Creates placeholder docs in docs/ai-context/ from templates stored in
~/.claude/doc-templates/. The Technical Writer fills these during the
first workflow run.

Usage:
    python3 scripts/crew-docs-import.py                     # Import from default templates
    python3 scripts/crew-docs-import.py --from /path/to     # Import from custom path
    python3 scripts/crew-docs-import.py --dry-run           # Show what would be created
"""

import json
import sys
from pathlib import Path


def _find_repo_root() -> Path:
    current = Path.cwd().resolve()
    while True:
        if (current / ".git").is_dir() or (current / ".git").is_file():
            return current
        parent = current.parent
        if parent == current:
            return Path.cwd()
        current = parent


def _default_templates_dir() -> Path:
    return Path.home() / ".claude" / "doc-templates"


def import_templates(templates_dir: Path, kb_dir: Path, dry_run: bool = False) -> dict:
    """Import templates into the knowledge base."""
    kb_dir.mkdir(parents=True, exist_ok=True)

    created = []
    skipped = []

    for f in sorted(templates_dir.rglob("*.md")):
        rel = f.relative_to(templates_dir)
        target = kb_dir / rel

        if target.exists():
            skipped.append(str(rel))
            continue

        if not dry_run:
            target.parent.mkdir(parents=True, exist_ok=True)
            target.write_text(f.read_text())

        created.append(str(rel))

    return {
        "created": created,
        "skipped": skipped,
        "templates_dir": str(templates_dir),
        "kb_dir": str(kb_dir),
        "dry_run": dry_run,
    }


def main():
    repo_root = _find_repo_root()
    kb_dir = repo_root / "docs" / "ai-context"

    # Parse templates dir
    templates_dir = _default_templates_dir()
    if "--from" in sys.argv:
        idx = sys.argv.index("--from")
        if idx + 1 < len(sys.argv):
            templates_dir = Path(sys.argv[idx + 1])

    if not templates_dir.is_dir():
        print(f"No templates found at {templates_dir}")
        print("Run crew-docs-export.py first to create templates.")
        sys.exit(1)

    dry_run = "--dry-run" in sys.argv

    result = import_templates(templates_dir, kb_dir, dry_run)

    if "--json" in sys.argv:
        print(json.dumps(result, indent=2))
        return

    prefix = "[dry-run] " if dry_run else ""

    if result["created"]:
        print(f"{prefix}Created {len(result['created'])} docs in {kb_dir}:")
        for f in result["created"]:
            print(f"  + {f}")
    else:
        print(f"{prefix}No new docs to create (all templates already exist).")

    if result["skipped"]:
        print(f"  Skipped {len(result['skipped'])} (already exist)")


if __name__ == "__main__":
    main()
