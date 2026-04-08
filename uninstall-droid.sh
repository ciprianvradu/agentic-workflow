#!/bin/bash
set -e

DROID_GLOBAL="$HOME/.factory"

echo "========================================"
echo "  Agentic Workflow Droid Uninstaller"
echo "========================================"
echo ""

# Remove droids from global location
echo "Removing global droids..."
removed=0
DROIDS_DIR="$DROID_GLOBAL/droids"
if [ -d "$DROIDS_DIR" ]; then
    for droid_file in "$DROIDS_DIR"/crew-* "$DROIDS_DIR/crew.md"; do
        [ -f "$droid_file" ] || continue
        rm -f "$droid_file"
        echo "  + Removed $droid_file"
        removed=$((removed + 1))
    done
fi
if [ "$removed" -eq 0 ]; then
    echo "  No global droids found to remove"
fi

# Remove project-level droids (.factory/droids/)
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
echo ""
echo "Removing project droids..."
removed=0
PROJECT_DROIDS="$SCRIPT_DIR/.factory/droids"
if [ -d "$PROJECT_DROIDS" ]; then
    for droid_file in "$PROJECT_DROIDS"/crew-* "$PROJECT_DROIDS/crew.md"; do
        [ -f "$droid_file" ] || continue
        rm -f "$droid_file"
        echo "  + Removed $droid_file"
        removed=$((removed + 1))
    done
fi
if [ "$removed" -eq 0 ]; then
    echo "  No project droids found to remove"
fi

# Remove config (global and project-level)
echo ""
echo "Removing config..."
removed=0
if [ -f "$DROID_GLOBAL/workflow-config.yaml" ]; then
    rm -f "$DROID_GLOBAL/workflow-config.yaml"
    echo "  + Removed $DROID_GLOBAL/workflow-config.yaml"
    removed=$((removed + 1))
fi
if [ -f "$SCRIPT_DIR/.factory/workflow-config.yaml" ]; then
    rm -f "$SCRIPT_DIR/.factory/workflow-config.yaml"
    echo "  + Removed $SCRIPT_DIR/.factory/workflow-config.yaml"
    removed=$((removed + 1))
fi
if [ "$removed" -eq 0 ]; then
    echo "  Config files not found"
fi

# Remove MCP server from global config (~/.factory/mcp.json)
echo ""
echo "Removing MCP server registration..."
if [ -f "$DROID_GLOBAL/mcp.json" ]; then
    python3 << 'PYTHON_SCRIPT'
import json, os

config_path = os.path.expanduser("~/.factory/mcp.json")

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
    echo "  No global mcp.json found"
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
echo "To reinstall, run: ./install-droid.sh"
echo ""
