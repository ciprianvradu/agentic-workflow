#!/bin/bash
set -e

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
VERSION=$(cat "$SCRIPT_DIR/VERSION" 2>/dev/null || echo "unknown")

echo "========================================"
echo "  Agentic Workflow — Devin Installer v${VERSION}"
echo "========================================"
echo ""

# Check prerequisites
echo "Checking prerequisites..."

if ! command -v python3 &> /dev/null; then
    echo "  ✗ Python 3 not found. Please install python3 and re-run."
    exit 1
fi
echo "  ✓ Python 3 found: $(python3 --version)"

if ! command -v devin &> /dev/null; then
    echo "  ⚠ Devin CLI not found in PATH"
    echo "    Install: curl -fsSL https://cli.devin.ai/install.sh | sh"
    echo "    Continuing anyway — you can install Devin later."
else
    echo "  ✓ Devin CLI found: $(devin version 2>/dev/null || echo 'version unknown')"
fi
echo ""

# Devin config directories
DEVIN_GLOBAL="$HOME/.config/devin"

# Build skills to home directory (creates ~/.config/devin/skills/)
echo "Installing skills (global)..."
python3 "$SCRIPT_DIR/scripts/build-agents.py" devin --output "$HOME" || {
    echo "  ✗ Failed to build skills"
    exit 1
}

# Also build skills to project-local .devin/
echo ""
echo "Installing skills (project)..."
python3 "$SCRIPT_DIR/scripts/build-agents.py" devin --output "$SCRIPT_DIR" || {
    echo "  ⚠ Failed to build project-local skills (non-fatal)"
}

# Copy workflow config to Devin global config dir
echo ""
echo "Installing config..."
mkdir -p "$DEVIN_GLOBAL"
if [ -f "$DEVIN_GLOBAL/workflow-config.yaml" ]; then
    echo "  ⚠ Existing config found, creating backup..."
    cp "$DEVIN_GLOBAL/workflow-config.yaml" "$DEVIN_GLOBAL/workflow-config.yaml.backup.$(date +%Y%m%d%H%M%S)"
fi
cp "$SCRIPT_DIR/config/workflow-config.yaml" "$DEVIN_GLOBAL/"
echo "  ✓ workflow-config.yaml → $DEVIN_GLOBAL/"

# Also copy to .devin/ (project-level fallback for config cascade)
mkdir -p "$SCRIPT_DIR/.devin"
cp "$SCRIPT_DIR/config/workflow-config.yaml" "$SCRIPT_DIR/.devin/"
echo "  ✓ workflow-config.yaml → .devin/"

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

# Register MCP server in Devin config (~/.config/devin/config.json)
echo ""
echo "Registering MCP server..."

python3 << 'PYTHON_SCRIPT'
import json, os

config_path = os.path.expanduser("~/.config/devin/config.json")
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
            print(f"  ⚠ Corrupt config.json backed up to {backup}, resetting")
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
echo "  devin"
echo "  /crew \"Your task description\""
echo ""
echo "Or use the /crew skill for orchestrated workflows."
echo ""
echo "Verify MCP server:"
echo "  devin mcp list"
echo ""
echo "See README.md for full documentation."
echo ""
