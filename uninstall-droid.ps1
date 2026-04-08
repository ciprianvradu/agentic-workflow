# Droid (Factory.ai) Uninstaller for Agentic Workflow
# This script removes the agentic-workflow system from Droid CLI

$ErrorActionPreference = "Stop"

$DROID_DIR = "$env:USERPROFILE\.factory"

Write-Host "========================================"  -ForegroundColor Cyan
Write-Host "  Agentic Workflow Droid Uninstaller"     -ForegroundColor Cyan
Write-Host "========================================"  -ForegroundColor Cyan
Write-Host ""

Write-Host "Removing custom droids..." -ForegroundColor Yellow
$removed = 0
$droidFiles = Get-ChildItem "$DROID_DIR\droids\crew*.md" -ErrorAction SilentlyContinue

foreach ($droidFile in $droidFiles) {
    Remove-Item $droidFile.FullName -Force
    Write-Host "  + Removed $($droidFile.Name)" -ForegroundColor Green
    $removed++
}

if ($removed -eq 0) {
    Write-Host "  No droids found to remove" -ForegroundColor Gray
}

Write-Host ""
Write-Host "Removing config..." -ForegroundColor Yellow
if (Test-Path "$DROID_DIR\workflow-config.yaml") {
    Remove-Item "$DROID_DIR\workflow-config.yaml" -Force
    Write-Host "  + Removed workflow-config.yaml" -ForegroundColor Green
} else {
    Write-Host "  Config file not found" -ForegroundColor Gray
}

Write-Host ""
Write-Host "Removing MCP server registration..." -ForegroundColor Yellow
$mcpConfigPath = "$DROID_DIR\mcp.json"

if (Test-Path $mcpConfigPath) {
    $mcpConfig = Get-Content $mcpConfigPath | ConvertFrom-Json

    if ($mcpConfig.mcpServers -and $mcpConfig.mcpServers."agentic-workflow") {
        $mcpConfig.mcpServers.PSObject.Properties.Remove("agentic-workflow")
        $mcpConfig | ConvertTo-Json -Depth 10 | Out-File $mcpConfigPath -Encoding utf8
        Write-Host "  + Removed agentic-workflow from MCP config" -ForegroundColor Green
    } else {
        Write-Host "  No agentic-workflow MCP server found in config" -ForegroundColor Gray
    }
} else {
    Write-Host "  MCP config file not found" -ForegroundColor Gray
}

Write-Host ""
Write-Host "Uninstalling Python package..." -ForegroundColor Yellow
try {
    $output = pip uninstall -y agentic-workflow-server 2>&1
    Write-Host "  + Python package uninstalled" -ForegroundColor Green
} catch {
    Write-Host "  Package not found or already uninstalled" -ForegroundColor Gray
}

Write-Host ""
Write-Host "========================================"  -ForegroundColor Cyan
Write-Host "  Uninstallation Complete!"               -ForegroundColor Green
Write-Host "========================================"  -ForegroundColor Cyan
Write-Host ""

Write-Host "Note: Preserved:" -ForegroundColor Yellow
Write-Host "  * .tasks/ (workflow state)" -ForegroundColor Gray
Write-Host ""

Write-Host "To reinstall, run: .\install-droid.ps1" -ForegroundColor Gray
Write-Host ""
