# Droid (Factory.ai) Installer for Agentic Workflow
# This script installs the agentic-workflow system for Droid CLI

param(
    [switch]$Force,
    [switch]$SkipMcp
)

$ErrorActionPreference = "Stop"

$SCRIPT_DIR = Split-Path -Parent $MyInvocation.MyCommand.Path
$DROID_DIR = "$env:USERPROFILE\.factory"
$VERSION = Get-Content "$SCRIPT_DIR\VERSION" -ErrorAction SilentlyContinue
if (-not $VERSION) { $VERSION = "unknown" }

Write-Host "========================================"  -ForegroundColor Cyan
Write-Host "  Agentic Workflow Droid Installer"       -ForegroundColor Cyan
Write-Host "  Version: $VERSION"                       -ForegroundColor Cyan
Write-Host "========================================"  -ForegroundColor Cyan
Write-Host ""

# Check prerequisites
Write-Host "Checking prerequisites..." -ForegroundColor Yellow

# Check Python 3 (try python3 first, then python)
$pythonCmd = $null
if (Get-Command python3 -ErrorAction SilentlyContinue) {
    $pythonCmd = "python3"
} elseif (Get-Command python -ErrorAction SilentlyContinue) {
    $pythonCmd = "python"
}

if (-not $pythonCmd) {
    Write-Host "  X Python 3 not found" -ForegroundColor Red
    Write-Host ""
    Write-Host "Please install Python 3 and re-run this script." -ForegroundColor Red
    exit 1
}
$pythonVersion = & $pythonCmd --version 2>&1
Write-Host "  + Python found: $pythonVersion" -ForegroundColor Green

# Check Droid CLI
if (-not (Get-Command droid -ErrorAction SilentlyContinue)) {
    Write-Host "  ! Droid CLI not found" -ForegroundColor Yellow
    Write-Host "    Install from: https://app.factory.ai/cli" -ForegroundColor Yellow
} else {
    Write-Host "  + Droid CLI found" -ForegroundColor Green
}

Write-Host ""

# Build platform-specific droids using build script
Write-Host ""
Write-Host "Building droids for Droid CLI..." -ForegroundColor Yellow
& $pythonCmd "$SCRIPT_DIR\scripts\build-agents.py" droid --output .
if ($LASTEXITCODE -ne 0) {
    Write-Host "  X Failed to build droids" -ForegroundColor Red
    exit 1
}

# Install droids to user config dir
Write-Host ""
Write-Host "Installing droids to user config..." -ForegroundColor Yellow
New-Item -ItemType Directory -Path "$DROID_DIR\droids" -Force | Out-Null

# Copy built droids to global location
Copy-Item ".factory\droids\*.md" -Destination "$DROID_DIR\droids\" -Force
Write-Host "  + Copied droids to $DROID_DIR\droids" -ForegroundColor Green

# Copy config (backup existing if present)
Write-Host ""
Write-Host "Installing config..." -ForegroundColor Yellow
if (Test-Path "$DROID_DIR\workflow-config.yaml") {
    $timestamp = Get-Date -Format "yyyyMMddHHmmss"
    Copy-Item "$DROID_DIR\workflow-config.yaml" "$DROID_DIR\workflow-config.yaml.backup.$timestamp"
    Write-Host "  ! Existing config backed up" -ForegroundColor Yellow
}
Copy-Item "$SCRIPT_DIR\config\workflow-config.yaml" "$DROID_DIR\"
Write-Host "  + workflow-config.yaml" -ForegroundColor Green

# Install MCP server
if (-not $SkipMcp) {
    Write-Host ""
    Write-Host "Installing MCP server..." -ForegroundColor Yellow
    $MCP_SERVER_DIR = "$SCRIPT_DIR\mcp\agentic-workflow-server"

    if (Test-Path $MCP_SERVER_DIR) {
        try {
            Write-Host "  Installing Python package..." -ForegroundColor Gray
            Push-Location $MCP_SERVER_DIR
            $output = & $pythonCmd -m pip install -q -e . 2>&1
            Pop-Location
            Write-Host "  + MCP server package installed" -ForegroundColor Green
        } catch {
            Write-Host "  ! Failed to install MCP server package" -ForegroundColor Yellow
            Write-Host "    Try manually: pip install -e $MCP_SERVER_DIR" -ForegroundColor Yellow
        }

        # Register MCP server with Droid
        Write-Host "  Registering MCP server with Droid..." -ForegroundColor Gray
        $mcpConfigPath = "$DROID_DIR\mcp.json"

        # Read existing config or create new
        if (Test-Path $mcpConfigPath) {
            $mcpConfig = Get-Content $mcpConfigPath | ConvertFrom-Json
        } else {
            $mcpConfig = @{ mcpServers = @{} }
        }

        # Add agentic-workflow server
        if (-not $mcpConfig.mcpServers) {
            $mcpConfig | Add-Member -NotePropertyName mcpServers -NotePropertyValue @{} -Force
        }

        $mcpConfig.mcpServers | Add-Member -NotePropertyName "agentic-workflow" -NotePropertyValue @{
            command = $pythonCmd
            args = @("-m", "agentic_workflow_server.server")
            transport = "stdio"
        } -Force

        # Save config
        [System.IO.File]::WriteAllText($mcpConfigPath, ($mcpConfig | ConvertTo-Json -Depth 10))
        Write-Host "  + MCP server registered in $mcpConfigPath" -ForegroundColor Green
    } else {
        Write-Host "  ! MCP server directory not found: $MCP_SERVER_DIR" -ForegroundColor Yellow
    }
} else {
    Write-Host ""
    Write-Host "Skipping MCP server installation (--SkipMcp flag)" -ForegroundColor Yellow
}

Write-Host ""
Write-Host "========================================"  -ForegroundColor Cyan
Write-Host "  Installation Complete!"                 -ForegroundColor Green
Write-Host "========================================"  -ForegroundColor Cyan
Write-Host ""

Write-Host "Installed components:" -ForegroundColor Yellow
Write-Host "  * Custom droids: $DROID_DIR\droids\" -ForegroundColor Gray
Write-Host "  * Config: $DROID_DIR\workflow-config.yaml" -ForegroundColor Gray
Write-Host "  * MCP server: agentic-workflow-server (Python package)" -ForegroundColor Gray
Write-Host "  * MCP config: $DROID_DIR\mcp.json" -ForegroundColor Gray
Write-Host ""

Write-Host "Next steps:" -ForegroundColor Yellow
Write-Host "  1. Start Droid CLI in this repository: droid" -ForegroundColor White
Write-Host "  2. Try: /crew 'Your task description'" -ForegroundColor White
Write-Host "  3. Check MCP tools: droid mcp list" -ForegroundColor White
Write-Host ""

Write-Host "To uninstall, run: .\uninstall-droid.ps1" -ForegroundColor Gray
Write-Host ""
