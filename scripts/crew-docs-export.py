#!/usr/bin/env python3
"""
Export knowledge base structure as reusable templates.

Strips project-specific content from docs/ai-context/ files, keeping
section headers and structure. Templates can be imported into other
projects via crew-docs-import.py.

Usage:
    python3 scripts/crew-docs-export.py                    # Export to ~/.claude/doc-templates/
    python3 scripts/crew-docs-export.py --output /path/to  # Export to custom path
    python3 scripts/crew-docs-export.py --json             # Output template manifest as JSON
"""

import json
import re
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


def _strip_project_specific(content: str, filename: str) -> str:
    """Strip project-specific content, keep structure and section headers.

    Preserves:
    - Markdown headers (# ## ###)
    - List item prefixes (- [ ])
    - Code fence markers (```)
    - Generic patterns and descriptions

    Strips:
    - Specific file paths (src/foo/bar.ts)
    - Specific class/function names in prose
    - Project-specific examples
    """
    lines = content.split("\n")
    result = []
    in_code_block = False
    skip_code_block = False

    for line in lines:
        # Track code blocks
        if line.strip().startswith("```"):
            if in_code_block:
                in_code_block = False
                if skip_code_block:
                    skip_code_block = False
                    result.append(line)  # Close the fence
                    result.append("<!-- TODO: Add project-specific example -->")
                    continue
                result.append(line)
                continue
            else:
                in_code_block = True
                # Check if this is a project-specific code example
                result.append(line)
                continue

        if in_code_block:
            result.append(line)
            continue

        # Keep all headers
        if line.strip().startswith("#"):
            result.append(line)
            continue

        # Keep list items but genericize paths
        if re.match(r'\s*[-*]\s', line):
            # Replace specific file paths with placeholders
            genericized = re.sub(
                r'`[a-zA-Z][a-zA-Z0-9_/.-]+\.(ts|js|py|rs|go|java|rb|md)`',
                '`<path/to/file>`',
                line
            )
            result.append(genericized)
            continue

        # Keep non-empty prose lines
        if line.strip():
            result.append(line)
        else:
            result.append(line)

    return "\n".join(result)


def export_templates(kb_dir: Path, output_dir: Path) -> dict:
    """Export KB docs as templates."""
    output_dir.mkdir(parents=True, exist_ok=True)

    manifest = {
        "source_project": _find_repo_root().name,
        "files": [],
    }

    for f in sorted(kb_dir.rglob("*.md")):
        rel = f.relative_to(kb_dir)
        content = f.read_text()
        template = _strip_project_specific(content, f.name)

        out_file = output_dir / rel
        out_file.parent.mkdir(parents=True, exist_ok=True)
        out_file.write_text(template)

        manifest["files"].append({
            "path": str(rel),
            "original_size": len(content),
            "template_size": len(template),
        })

    # Write manifest
    manifest_file = output_dir / "manifest.json"
    manifest_file.write_text(json.dumps(manifest, indent=2))

    return manifest


def main():
    repo_root = _find_repo_root()
    kb_dir = repo_root / "docs" / "ai-context"

    if not kb_dir.is_dir():
        print("No docs/ai-context/ directory found.")
        sys.exit(1)

    # Parse output dir
    output_dir = _default_templates_dir()
    if "--output" in sys.argv:
        idx = sys.argv.index("--output")
        if idx + 1 < len(sys.argv):
            output_dir = Path(sys.argv[idx + 1])

    manifest = export_templates(kb_dir, output_dir)

    if "--json" in sys.argv:
        print(json.dumps(manifest, indent=2))
        return

    print(f"Exported {len(manifest['files'])} templates to {output_dir}")
    for f in manifest["files"]:
        print(f"  {f['path']}")


if __name__ == "__main__":
    main()
