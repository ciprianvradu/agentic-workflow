#!/bin/bash
set -e

DEVIN_GLOBAL="$HOME/.config/devin"

echo "========================================"
echo "  Agentic Workflow Devin Uninstaller"
echo "========================================"
echo ""

# Remove skills from global location
echo "Removing global skills..."
removed=0
SKILLS_DIR="$DEVIN_GLOBAL/skills"
if [ -d "$SKILLS_DIR" ]; then
    for skill_dir in "$SKILLS_DIR"/crew-* "$SKILLS_DIR/crew"; do
        [ -d "$skill_dir" ] || continue
        rm -rf "$skill_dir"
        echo "  + Removed $skill_dir"
        removed=$((removed + 1))
    done
fi
if [ "$removed" -eq 0 ]; then
    echo "  No global skills found to remove"
fi

# Remove project-level skills (.devin/skills/)
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
echo ""
echo "Removing project skills..."
removed=0
PROJECT_SKILLS="$SCRIPT_DIR/.devin/skills"
if [ -d "$PROJECT_SKILLS" ]; then
    for skill_dir in "$PROJECT_SKILLS"/crew-* "$PROJECT_SKILLS/crew"; do
        [ -d "$skill_dir" ] || continue
        rm -rf "$skill_dir"
        echo "  + Removed $skill_dir"
        removed=$((removed + 1))
    done
fi
if [ "$removed" -eq 0 ]; then
    echo "  No project skills found to remove"
fi

# Remove config (global and project-level)
echo ""
echo "Removing config..."
removed=0
if [ -f "$DEVIN_GLOBAL/workflow-config.yaml" ]; then
    rm -f "$DEVIN_GLOBAL/workflow-config.yaml"
    echo "  + Removed $DEVIN_GLOBAL/workflow-config.yaml"
    removed=$((removed + 1))
fi
if [ -f "$SCRIPT_DIR/.devin/workflow-config.yaml" ]; then
    rm -f "$SCRIPT_DIR/.devin/workflow-config.yaml"
    echo "  + Removed $SCRIPT_DIR/.devin/workflow-config.yaml"
    removed=$((removed + 1))
fi
if [ "$removed" -eq 0 ]; then
    echo "  Config files not found"
fi

# Remove MCP server from global config (~/.config/devin/config.json)
echo ""
echo "Removing MCP server registration..."
if [ -f "$DEVIN_GLOBAL/config.json" ]; then
    python3 << 'PYTHON_SCRIPT'
import json, os

config_path = os.path.expanduser("~/.config/devin/config.json")

with open(config_path, 'r') as f:
    config = json.load(f)

changed = False
if "mcpServers" in config and "agentic-workflow" in config["mcpServers"]:
    del config["mcpServers"]["agentic-workflow"]
    changed = True
    print("  + Removed agentic-workflow from MCP config")
else:
    print("  No agentic-workflow MCP server found")

if changed:
    with open(config_path, 'w') as f:
        json.dump(config, f, indent=2)
        f.write('\n')
PYTHON_SCRIPT
else
    echo "  No global config.json found"
fi

# Uninstall Python package
echo ""
echo "Uninstalling Python package..."
pip3 uninstall -y agentic-workflow-server 2>/dev/null && {
    echo "  + Python package uninstalled"
} || {
    echo "  Package not found or already uninstalled"
}

echo ""
echo "========================================"
echo "  Uninstallation Complete!"
echo "========================================"
echo ""
echo "Note: Preserved:"
echo "  * .tasks/ (workflow state)"
echo ""
echo "To reinstall, run: ./install-devin.sh"
echo ""
