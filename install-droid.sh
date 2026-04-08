#!/bin/bash
set -e

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
VERSION=$(cat "$SCRIPT_DIR/VERSION" 2>/dev/null || echo "unknown")

echo "========================================"
echo "  Agentic Workflow — Droid Installer v${VERSION}"
echo "========================================"
echo ""

# Check prerequisites
echo "Checking prerequisites..."

if ! command -v python3 &> /dev/null; then
    echo "  ✗ Python 3 not found. Please install python3 and re-run."
    exit 1
fi
echo "  ✓ Python 3 found: $(python3 --version)"

if ! command -v droid &> /dev/null; then
    echo "  ⚠ Droid CLI not found in PATH"
    echo "    Install: curl -fsSL https://app.factory.ai/cli | sh"
    echo "    Continuing anyway — you can install Droid later."
else
    echo "  ✓ Droid CLI found: $(droid --version 2>/dev/null || echo 'version unknown')"
fi
echo ""

# Droid config directories
DROID_GLOBAL="$HOME/.factory"

# Build droids to home directory (creates ~/.factory/droids/)
echo "Installing droids (global)..."
python3 "$SCRIPT_DIR/scripts/build-agents.py" droid --output "$HOME" || {
    echo "  ✗ Failed to build droids"
    exit 1
}

# Also build droids to project-local .factory/
echo ""
echo "Installing droids (project)..."
python3 "$SCRIPT_DIR/scripts/build-agents.py" droid --output "$SCRIPT_DIR" || {
    echo "  ⚠ Failed to build project-local droids (non-fatal)"
}

# Copy workflow config to Droid global config dir
echo ""
echo "Installing config..."
mkdir -p "$DROID_GLOBAL"
if [ -f "$DROID_GLOBAL/workflow-config.yaml" ]; then
    echo "  ⚠ Existing config found, creating backup..."
    cp "$DROID_GLOBAL/workflow-config.yaml" "$DROID_GLOBAL/workflow-config.yaml.backup.$(date +%Y%m%d%H%M%S)"
fi
cp "$SCRIPT_DIR/config/workflow-config.yaml" "$DROID_GLOBAL/"
echo "  ✓ workflow-config.yaml → $DROID_GLOBAL/"

# Also copy to .factory/ (project-level fallback for config cascade)
mkdir -p "$SCRIPT_DIR/.factory"
cp "$SCRIPT_DIR/config/workflow-config.yaml" "$SCRIPT_DIR/.factory/"
echo "  ✓ workflow-config.yaml → .factory/"

# Install MCP server
echo ""
echo "Installing MCP server..."
MCP_SERVER_DIR="$SCRIPT_DIR/mcp/agentic-workflow-server"

if [ -d "$MCP_SERVER_DIR" ]; then
    if command -v pip3 &> /dev/null; then
        PIP_CMD="pip3"
    elif command -v pip &> /dev/null; then
        PIP_CMD="pip"
    else
        echo "  ⚠ pip not found, skipping MCP server installation"
        echo "    Install with: pip install -e $MCP_SERVER_DIR"
        PIP_CMD=""
    fi

    if [ -n "$PIP_CMD" ]; then
        echo "  Installing Python package..."
        $PIP_CMD install -q -e "$MCP_SERVER_DIR" 2>/dev/null || {
            echo "  ⚠ Failed to install MCP server package"
            echo "    Try manually: $PIP_CMD install -e $MCP_SERVER_DIR"
        }
    fi
else
    echo "  ⚠ MCP server directory not found: $MCP_SERVER_DIR"
fi

# Register MCP server in Droid config (~/.factory/mcp.json)
echo ""
echo "Registering MCP server..."

python3 << 'PYTHON_SCRIPT'
import json, os

config_path = os.path.expanduser("~/.factory/mcp.json")
os.makedirs(os.path.dirname(config_path), exist_ok=True)

config = {}
if os.path.isfile(config_path):
    try:
        with open(config_path) as f:
            config = json.load(f)
    except (json.JSONDecodeError, IOError):
        import shutil, datetime
        backup = config_path + ".bak." + datetime.datetime.now().strftime("%Y%m%d%H%M%S")
        try:
            shutil.copy2(config_path, backup)
            print(f"  ⚠ Corrupt mcp.json backed up to {backup}, resetting")
        except Exception:
            pass

mcp = config.setdefault("mcpServers", {})
mcp["agentic-workflow"] = {
    "command": "python3",
    "args": ["-m", "agentic_workflow_server.server"],
    "transport": "stdio"
}

with open(config_path, "w") as f:
    json.dump(config, f, indent=2)
    f.write("\n")

print(f"  ✓ MCP server registered in {config_path}")
PYTHON_SCRIPT

echo ""
echo "========================================"
echo "  Installation complete! (v${VERSION})"
echo "========================================"
echo ""
echo "Quick start:"
echo "  droid"
echo "  /crew \"Your task description\""
echo ""
echo "Or use the /crew droid for orchestrated workflows."
echo ""
echo "Verify MCP server:"
echo "  droid mcp list"
echo ""
echo "See README.md for full documentation."
echo ""
