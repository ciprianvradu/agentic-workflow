# Claude Code Installer for Agentic Workflow (Windows)
# PowerShell equivalent of install.sh for native Windows Claude Code

param(
    [switch]$Force,
    [switch]$SkipMcp,
    [switch]$SkipHooks
)

$ErrorActionPreference = "Stop"

$SCRIPT_DIR = Split-Path -Parent $MyInvocation.MyCommand.Path
$CLAUDE_DIR = "$env:USERPROFILE\.claude"
$VERSION = Get-Content "$SCRIPT_DIR\VERSION" -ErrorAction SilentlyContinue
if (-not $VERSION) { $VERSION = "unknown" }

Write-Host "========================================"  -ForegroundColor Cyan
Write-Host "  Agentic Workflow Installer v$VERSION"    -ForegroundColor Cyan
Write-Host "  (Windows / Claude Code)"                 -ForegroundColor Cyan
Write-Host "========================================"  -ForegroundColor Cyan
Write-Host ""

# --- Prerequisites -----------------------------------------------------------

Write-Host "Checking prerequisites..." -ForegroundColor Yellow

# Python 3 — try python3 first (some installs alias it), then python
$pythonCmd = $null
if (Get-Command python3 -ErrorAction SilentlyContinue) {
    $pythonCmd = "python3"
} elseif (Get-Command python -ErrorAction SilentlyContinue) {
    # Verify it's Python 3, not 2
    $ver = & python --version 2>&1
    if ($ver -match "Python 3") {
        $pythonCmd = "python"
    }
}

if (-not $pythonCmd) {
    Write-Host "  X Python 3 not found" -ForegroundColor Red
    Write-Host "    Install from https://python.org or: winget install Python.Python.3.12" -ForegroundColor Red
    exit 1
}
$pythonVersion = & $pythonCmd --version 2>&1
Write-Host "  + Python found: $pythonVersion" -ForegroundColor Green

# Claude CLI
if (-not (Get-Command claude -ErrorAction SilentlyContinue)) {
    Write-Host "  ! Claude CLI not found (needed for MCP registration)" -ForegroundColor Yellow
    Write-Host "    Install: npm install -g @anthropic-ai/claude-code" -ForegroundColor Yellow
} else {
    Write-Host "  + Claude CLI found" -ForegroundColor Green
}

Write-Host ""

# --- Create directories ------------------------------------------------------

Write-Host "Creating directories..." -ForegroundColor Yellow
foreach ($sub in @("commands", "agents", "scripts", "config")) {
    $dir = Join-Path $CLAUDE_DIR $sub
    if (-not (Test-Path $dir)) {
        New-Item -ItemType Directory -Path $dir -Force | Out-Null
    }
}
Write-Host "  + $CLAUDE_DIR\{commands,agents,scripts,config}" -ForegroundColor Green
Write-Host ""

# --- Remove legacy workflow commands (renamed to crew) ------------------------

$legacyFile = Join-Path $CLAUDE_DIR "commands\workflow.md"
if (Test-Path $legacyFile) {
    $content = Get-Content $legacyFile -Raw -ErrorAction SilentlyContinue
    if ($content -match "Agentic Development Workflow") {
        Write-Host "Removing old workflow commands (renamed to /crew)..." -ForegroundColor Yellow
        foreach ($f in @("workflow.md", "workflow-config.md", "workflow-status.md", "workflow-resume.md")) {
            $p = Join-Path $CLAUDE_DIR "commands\$f"
            if (Test-Path $p) { Remove-Item $p -Force }
        }
        Write-Host "  + Removed legacy /workflow commands" -ForegroundColor Green
        Write-Host ""
    } else {
        Write-Host "  ! Found workflow.md but it's not from agentic-workflow - keeping it" -ForegroundColor Yellow
        Write-Host ""
    }
}

# --- Build agents and commands ------------------------------------------------

Write-Host "Installing agents and commands..." -ForegroundColor Yellow
& $pythonCmd "$SCRIPT_DIR\scripts\build-agents.py" claude --output $CLAUDE_DIR
if ($LASTEXITCODE -ne 0) {
    Write-Host "  X Failed to build agents and commands" -ForegroundColor Red
    exit 1
}
Write-Host ""

# --- Copy enforcement scripts ------------------------------------------------

Write-Host "Installing enforcement scripts..." -ForegroundColor Yellow
$scriptsDest = Join-Path $CLAUDE_DIR "scripts"
Get-ChildItem "$SCRIPT_DIR\scripts\*.py" | ForEach-Object {
    Copy-Item $_.FullName -Destination $scriptsDest -Force
}
Write-Host "  + workflow_state.py (state management)" -ForegroundColor Green
Write-Host "  + validate-transition.py (PreToolUse hook)" -ForegroundColor Green
Write-Host "  + check-bash-safety.py (Bash safety hook)" -ForegroundColor Green
Write-Host "  + check-workflow-complete.py (Stop hook)" -ForegroundColor Green
Write-Host ""

# --- Copy config --------------------------------------------------------------

Write-Host "Installing config..." -ForegroundColor Yellow
$configDest = Join-Path $CLAUDE_DIR "workflow-config.yaml"
if (Test-Path $configDest) {
    $timestamp = Get-Date -Format "yyyyMMddHHmmss"
    Copy-Item $configDest "$configDest.backup.$timestamp"
    Write-Host "  ! Existing config backed up" -ForegroundColor Yellow
}
Copy-Item "$SCRIPT_DIR\config\workflow-config.yaml" $CLAUDE_DIR
Write-Host "  + workflow-config.yaml" -ForegroundColor Green

# Worktree permissions template
Copy-Item "$SCRIPT_DIR\config\worktree-permissions.json" (Join-Path $CLAUDE_DIR "config")
Write-Host "  + worktree-permissions.json" -ForegroundColor Green
Write-Host ""

# --- Worktree base in additionalDirectories -----------------------------------

Write-Host "Configuring worktree permissions..." -ForegroundColor Yellow
$settingsLocalFile = Join-Path $CLAUDE_DIR "settings.local.json"
$repoDir = (Resolve-Path $SCRIPT_DIR).Path
$wtBase = Join-Path (Split-Path $repoDir -Parent) ((Split-Path $repoDir -Leaf) + "-worktrees")
# Normalise to forward slashes for Claude's JSON
$wtBase = $wtBase -replace '\\', '/'

try {
    if (Test-Path $settingsLocalFile) {
        $localSettings = Get-Content $settingsLocalFile -Raw | ConvertFrom-Json
    } else {
        $localSettings = [PSCustomObject]@{}
    }

    # Ensure additionalDirectories exists
    if (-not $localSettings.PSObject.Properties["additionalDirectories"]) {
        $localSettings | Add-Member -NotePropertyName "additionalDirectories" -NotePropertyValue @()
    }

    if ($localSettings.additionalDirectories -notcontains $wtBase) {
        $localSettings.additionalDirectories += $wtBase
        [System.IO.File]::WriteAllText($settingsLocalFile, ($localSettings | ConvertTo-Json -Depth 10))
        Write-Host "  + Added worktree base to additionalDirectories: $wtBase" -ForegroundColor Green
    } else {
        Write-Host "  + Worktree base already in additionalDirectories" -ForegroundColor Green
    }
} catch {
    Write-Host "  ! Could not update Claude settings (non-fatal): $_" -ForegroundColor Yellow
}
Write-Host ""

# --- Install MCP server ------------------------------------------------------

if (-not $SkipMcp) {
    Write-Host "Installing MCP server..." -ForegroundColor Yellow
    $MCP_SERVER_DIR = Join-Path $SCRIPT_DIR "mcp\agentic-workflow-server"

    if (Test-Path $MCP_SERVER_DIR) {
        $mcpInstallOk = $false
        try {
            Write-Host "  Installing Python package..." -ForegroundColor Gray
            & $pythonCmd -m pip install -q -e $MCP_SERVER_DIR 2>&1 | Out-Null
            $mcpInstallOk = $true
            Write-Host "  + MCP server package installed" -ForegroundColor Green
        } catch {
            Write-Host "  ! Failed to install MCP server package" -ForegroundColor Yellow
            Write-Host "    Try manually: $pythonCmd -m pip install -e $MCP_SERVER_DIR" -ForegroundColor Yellow
        }

        # Register with Claude CLI
        if (-not $mcpInstallOk) {
            Write-Host "  ! Skipping MCP registration (package not installed)" -ForegroundColor Yellow
        } elseif (Get-Command claude -ErrorAction SilentlyContinue) {
            Write-Host "  Registering MCP server with Claude..." -ForegroundColor Gray
            claude mcp remove agentic-workflow 2>$null
            claude mcp add agentic-workflow -s user -- $pythonCmd -m agentic_workflow_server.server 2>$null
            if ($LASTEXITCODE -eq 0) {
                Write-Host "  + MCP server registered: agentic-workflow" -ForegroundColor Green
            } else {
                Write-Host "  ! Failed to register MCP server" -ForegroundColor Yellow
                Write-Host "    Try manually: claude mcp add agentic-workflow -s user -- $pythonCmd -m agentic_workflow_server.server" -ForegroundColor Yellow
            }
        } else {
            Write-Host "  ! Claude CLI not found, skipping MCP registration" -ForegroundColor Yellow
            Write-Host "    Register manually with: claude mcp add agentic-workflow -s user -- $pythonCmd -m agentic_workflow_server.server" -ForegroundColor Yellow
        }
    } else {
        Write-Host "  ! MCP server directory not found: $MCP_SERVER_DIR" -ForegroundColor Yellow
    }
} else {
    Write-Host "Skipping MCP server installation (-SkipMcp)" -ForegroundColor Yellow
}
Write-Host ""

# --- Configure enforcement hooks ---------------------------------------------

if (-not $SkipHooks) {
    Write-Host "Configuring enforcement hooks..." -ForegroundColor Yellow
    $settingsFile = Join-Path $CLAUDE_DIR "settings.json"

    # Build hooks structure with Windows-compatible paths
    # Claude Code on Windows uses python (not python3) and %USERPROFILE% paths
    $scriptsPath = (Join-Path $CLAUDE_DIR "scripts") -replace '\\', '/'
    $hooksTemplate = @{
        hooks = @{
            UserPromptSubmit = @(
                @{
                    hooks = @(
                        @{
                            type = "command"
                            command = "$pythonCmd $scriptsPath/log-crew-interaction-lite.py"
                        }
                    )
                }
            )
            PreToolUse = @(
                @{
                    matcher = "Task"
                    hooks = @(
                        @{
                            type = "command"
                            command = "$pythonCmd $scriptsPath/validate-transition.py"
                        }
                    )
                },
                @{
                    matcher = "Bash"
                    hooks = @(
                        @{
                            type = "command"
                            command = "$pythonCmd $scriptsPath/check-bash-safety.py"
                        }
                    )
                }
            )
            Stop = @(
                @{
                    hooks = @(
                        @{
                            type = "command"
                            command = "$pythonCmd $scriptsPath/check-workflow-complete.py"
                        }
                    )
                }
            )
        }
    }

    if (Test-Path $settingsFile) {
        Write-Host "  Existing settings.json found, merging hooks..." -ForegroundColor Gray
        try {
            $settings = Get-Content $settingsFile -Raw | ConvertFrom-Json

            if (-not $settings.PSObject.Properties["hooks"]) {
                $settings | Add-Member -NotePropertyName "hooks" -NotePropertyValue ([PSCustomObject]@{})
            }

            foreach ($hookType in $hooksTemplate.hooks.Keys) {
                # Collect existing commands for this hook type
                $existingCommands = @()
                if ($settings.hooks.PSObject.Properties[$hookType]) {
                    foreach ($entry in $settings.hooks.$hookType) {
                        foreach ($h in $entry.hooks) {
                            if ($h.type -eq "command") {
                                $existingCommands += $h.command
                            }
                        }
                    }
                } else {
                    $settings.hooks | Add-Member -NotePropertyName $hookType -NotePropertyValue @()
                }

                # Add hooks not already present
                foreach ($newHook in $hooksTemplate.hooks[$hookType]) {
                    foreach ($h in $newHook.hooks) {
                        if ($h.type -eq "command" -and $h.command -notin $existingCommands) {
                            $settings.hooks.$hookType += $newHook
                            break
                        }
                    }
                }
            }

            [System.IO.File]::WriteAllText($settingsFile, ($settings | ConvertTo-Json -Depth 10))
            Write-Host "  + Merged hooks into settings.json" -ForegroundColor Green
        } catch {
            Write-Host "  ! Failed to merge hooks: $_" -ForegroundColor Yellow
            Write-Host "    You may need to configure hooks manually" -ForegroundColor Yellow
        }
    } else {
        Write-Host "  Creating new settings.json with hooks..." -ForegroundColor Gray
        [System.IO.File]::WriteAllText($settingsFile, ($hooksTemplate | ConvertTo-Json -Depth 10))
        Write-Host "  + Created settings.json with hooks" -ForegroundColor Green
    }
} else {
    Write-Host "Skipping hooks configuration (-SkipHooks)" -ForegroundColor Yellow
}
Write-Host ""

# --- Install Windows Terminal color schemes (native Windows) ------------------

Write-Host "Installing terminal color schemes..." -ForegroundColor Yellow
$schemesSource = Join-Path $SCRIPT_DIR "config\terminal-colorschemes.json"
if (Test-Path $schemesSource) {
    # Find Windows Terminal settings.json
    $wtSettings = $null
    $localAppData = $env:LOCALAPPDATA
    foreach ($pkg in @(
        "Microsoft.WindowsTerminal_8wekyb3d8bbwe",
        "Microsoft.WindowsTerminalPreview_8wekyb3d8bbwe"
    )) {
        $candidate = Join-Path $localAppData "Packages\$pkg\LocalState\settings.json"
        if (Test-Path $candidate) {
            $wtSettings = $candidate
            break
        }
    }

    if ($wtSettings) {
        & $pythonCmd "$SCRIPT_DIR\scripts\install-wt-colorschemes.py" $wtSettings $schemesSource 2>$null
        if ($LASTEXITCODE -eq 0) {
            Write-Host "  + Color schemes installed to Windows Terminal" -ForegroundColor Green
        } else {
            # Fallback: try inline
            Write-Host "  ! Script failed, skipping color schemes" -ForegroundColor Yellow
        }
    } else {
        Write-Host "  ! Windows Terminal settings.json not found, skipping" -ForegroundColor Yellow
    }
} else {
    Write-Host "  ! Color schemes file not found, skipping" -ForegroundColor Yellow
}
Write-Host ""

# --- Install Copilot agents ---------------------------------------------------

Write-Host "Installing Copilot agents..." -ForegroundColor Yellow
$COPILOT_DIR = "$env:USERPROFILE\.copilot"

& $pythonCmd "$SCRIPT_DIR\scripts\build-agents.py" copilot --output $SCRIPT_DIR 2>$null
if ($LASTEXITCODE -eq 0) {
    New-Item -ItemType Directory -Path "$COPILOT_DIR\agents" -Force | Out-Null
    # Remove old-format files
    Get-ChildItem "$COPILOT_DIR\agents\crew-*.md" -ErrorAction SilentlyContinue |
        Where-Object { $_.Name -notlike "*.agent.md" } |
        ForEach-Object { Remove-Item $_.FullName -Force }
    if (Test-Path "$SCRIPT_DIR\.github\agents\*.agent.md") {
        Copy-Item "$SCRIPT_DIR\.github\agents\*.agent.md" -Destination "$COPILOT_DIR\agents\" -Force
    }
    # Config
    if (-not (Test-Path "$COPILOT_DIR\workflow-config.yaml")) {
        Copy-Item "$SCRIPT_DIR\config\workflow-config.yaml" $COPILOT_DIR
    }
    # MCP config
    $mcpConfigPath = "$COPILOT_DIR\mcp-config.json"
    if (Test-Path $mcpConfigPath) {
        $mcpConfig = Get-Content $mcpConfigPath -Raw | ConvertFrom-Json
    } else {
        $mcpConfig = [PSCustomObject]@{ mcpServers = [PSCustomObject]@{} }
    }
    $mcpConfig.mcpServers | Add-Member -NotePropertyName "agentic-workflow" -NotePropertyValue @{
        command = $pythonCmd
        args = @("-m", "agentic_workflow_server.server")
        env = @{}
    } -Force
    [System.IO.File]::WriteAllText($mcpConfigPath, ($mcpConfig | ConvertTo-Json -Depth 10))
    Write-Host "  + Agents, config, MCP registered" -ForegroundColor Green
} else {
    Write-Host "  ! Failed to build Copilot agents (non-fatal)" -ForegroundColor Yellow
}
Write-Host ""

# --- Install Gemini agents ---------------------------------------------------

Write-Host "Installing Gemini agents..." -ForegroundColor Yellow
$GEMINI_DIR = "$env:USERPROFILE\.gemini"

& $pythonCmd "$SCRIPT_DIR\scripts\build-agents.py" gemini --output $env:USERPROFILE 2>$null
if ($LASTEXITCODE -eq 0) {
    New-Item -ItemType Directory -Path $GEMINI_DIR -Force | Out-Null
    if (-not (Test-Path "$GEMINI_DIR\workflow-config.yaml")) {
        Copy-Item "$SCRIPT_DIR\config\workflow-config.yaml" $GEMINI_DIR
    }
    # Configure settings.json with MCP + experimental agents
    $geminiSettings = "$GEMINI_DIR\settings.json"
    try {
        if (Test-Path $geminiSettings) {
            $gs = Get-Content $geminiSettings -Raw | ConvertFrom-Json
        } else {
            $gs = [PSCustomObject]@{}
        }
        if (-not $gs.PSObject.Properties["experimental"]) {
            $gs | Add-Member -NotePropertyName "experimental" -NotePropertyValue ([PSCustomObject]@{})
        }
        $gs.experimental | Add-Member -NotePropertyName "enableAgents" -NotePropertyValue $true -Force
        if (-not $gs.PSObject.Properties["mcpServers"]) {
            $gs | Add-Member -NotePropertyName "mcpServers" -NotePropertyValue ([PSCustomObject]@{})
        }
        $gs.mcpServers | Add-Member -NotePropertyName "agentic-workflow" -NotePropertyValue @{
            command = $pythonCmd
            args = @("-m", "agentic_workflow_server.server")
        } -Force
        [System.IO.File]::WriteAllText($geminiSettings, ($gs | ConvertTo-Json -Depth 10))
        Write-Host "  + Agents, config, MCP, experimental agents" -ForegroundColor Green
    } catch {
        Write-Host "  ! Failed to configure Gemini settings: $_" -ForegroundColor Yellow
    }
} else {
    Write-Host "  ! Failed to build Gemini agents (non-fatal)" -ForegroundColor Yellow
}
Write-Host ""

# --- Install OpenCode agents -------------------------------------------------

Write-Host "Installing OpenCode agents..." -ForegroundColor Yellow
$OPENCODE_DIR = "$env:USERPROFILE\.config\opencode"

& $pythonCmd "$SCRIPT_DIR\scripts\build-agents.py" opencode --output $env:USERPROFILE 2>$null
if ($LASTEXITCODE -eq 0) {
    # Also build project-local agents
    & $pythonCmd "$SCRIPT_DIR\scripts\build-agents.py" opencode --output $SCRIPT_DIR 2>$null
    New-Item -ItemType Directory -Path $OPENCODE_DIR -Force | Out-Null
    if (-not (Test-Path "$OPENCODE_DIR\workflow-config.yaml")) {
        Copy-Item "$SCRIPT_DIR\config\workflow-config.yaml" $OPENCODE_DIR
    }
    # Register MCP in opencode.json
    $ocConfig = "$OPENCODE_DIR\opencode.json"
    try {
        if (Test-Path $ocConfig) {
            $oc = Get-Content $ocConfig -Raw | ConvertFrom-Json
        } else {
            $oc = [PSCustomObject]@{}
        }
        if (-not $oc.PSObject.Properties["mcp"]) {
            $oc | Add-Member -NotePropertyName "mcp" -NotePropertyValue ([PSCustomObject]@{})
        }
        $oc.mcp | Add-Member -NotePropertyName "agentic-workflow" -NotePropertyValue @{
            type = "local"
            command = @($pythonCmd, "-m", "agentic_workflow_server.server")
            enabled = $true
        } -Force
        [System.IO.File]::WriteAllText($ocConfig, ($oc | ConvertTo-Json -Depth 10))
        Write-Host "  + Agents, config, MCP registered" -ForegroundColor Green
    } catch {
        Write-Host "  ! Failed to configure OpenCode: $_" -ForegroundColor Yellow
    }
} else {
    Write-Host "  ! Failed to build OpenCode agents (non-fatal)" -ForegroundColor Yellow
}
Write-Host ""

# --- Done ---------------------------------------------------------------------

Write-Host "========================================"  -ForegroundColor Cyan
Write-Host "  Installation complete! (v$VERSION)"      -ForegroundColor Green
Write-Host "========================================"  -ForegroundColor Cyan
Write-Host ""

Write-Host "Installed for:" -ForegroundColor Yellow
Write-Host "  * Claude Code:     $CLAUDE_DIR" -ForegroundColor White
Write-Host "  * GitHub Copilot:  $COPILOT_DIR" -ForegroundColor White
Write-Host "  * Gemini CLI:      $GEMINI_DIR" -ForegroundColor White
Write-Host "  * OpenCode:        $OPENCODE_DIR" -ForegroundColor White
Write-Host ""

Write-Host "Quick start:" -ForegroundColor Yellow
Write-Host "  Claude:   claude  -> /crew `"Your task`"" -ForegroundColor White
Write-Host "  Copilot:  gh cs   -> Use crew-orchestrator to ..." -ForegroundColor White
Write-Host "  Gemini:   gemini  -> Use crew-orchestrator to ..." -ForegroundColor White
Write-Host "  OpenCode: opencode -> /crew `"Your task`"" -ForegroundColor White
Write-Host ""
Write-Host "Verify MCP server:" -ForegroundColor Yellow
Write-Host "  claude mcp list" -ForegroundColor White
Write-Host ""
Write-Host "See README.md for full documentation."
Write-Host ""
