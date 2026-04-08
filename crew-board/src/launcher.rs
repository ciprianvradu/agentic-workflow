use std::path::Path;
use std::process::Command;

/// Detected terminal environment.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum TerminalEnv {
    Embedded,
    WindowsTerminalNative,
    WindowsTerminalWsl,
    Tmux,
    MacOs,
    LinuxGeneric,
}

/// AI host to launch.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum AiHost {
    Claude,
    Copilot,
    Gemini,
    OpenCode,
    Devin,
    Droid,
    Shell,
}

/// Color scheme with hex strings for terminal commands.
pub struct ColorSchemeHex {
    pub name: &'static str,
    pub tab: &'static str,
    pub bg: &'static str,
    pub fg: &'static str,
}

pub const COLOR_SCHEME_HEX: &[ColorSchemeHex] = &[
    ColorSchemeHex { name: "Crew Ocean",    tab: "#1A6B8A", bg: "#0C1B2A", fg: "#C8D6E5" },
    ColorSchemeHex { name: "Crew Forest",   tab: "#2D7D46", bg: "#0E1F14", fg: "#C5D1C0" },
    ColorSchemeHex { name: "Crew Sunset",   tab: "#C75B39", bg: "#1F120E", fg: "#D8C8BA" },
    ColorSchemeHex { name: "Crew Amethyst", tab: "#7B5EA7", bg: "#16121F", fg: "#CCC4D8" },
    ColorSchemeHex { name: "Crew Steel",    tab: "#5C7A8A", bg: "#141C22", fg: "#C0CCD4" },
    ColorSchemeHex { name: "Crew Ember",    tab: "#B85C3A", bg: "#1A110D", fg: "#D4C4B4" },
    ColorSchemeHex { name: "Crew Frost",    tab: "#4BA3C7", bg: "#0D1820", fg: "#C4D4E0" },
    ColorSchemeHex { name: "Crew Earth",    tab: "#8D7B4A", bg: "#1A170E", fg: "#D0C8B8" },
];

/// Get hex color scheme by index (wraps around).
pub fn get_hex_scheme(index: usize) -> &'static ColorSchemeHex {
    &COLOR_SCHEME_HEX[index % COLOR_SCHEME_HEX.len()]
}

impl AiHost {
    pub fn label(&self) -> &'static str {
        match self {
            AiHost::Claude => "Claude Code",
            AiHost::Copilot => "GitHub Copilot",
            AiHost::Gemini => "Gemini CLI",
            AiHost::OpenCode => "OpenCode",
            AiHost::Devin => "Devin",
            AiHost::Droid => "Droid (Factory.ai)",
            AiHost::Shell => if cfg!(target_os = "windows") { "Shell (pwsh)" } else { "Shell (bash)" },
        }
    }

    pub fn command(&self) -> &'static str {
        match self {
            AiHost::Claude => "claude",
            AiHost::Copilot => "copilot",
            AiHost::Gemini => "gemini",
            AiHost::OpenCode => "opencode",
            AiHost::Devin => "devin",
            AiHost::Droid => "droid",
            AiHost::Shell => if cfg!(target_os = "windows") { "pwsh" } else { "bash" },
        }
    }
}

impl TerminalEnv {
    pub fn label(&self) -> &'static str {
        match self {
            TerminalEnv::Embedded => "Embedded (in crew-board)",
            TerminalEnv::WindowsTerminalNative => "Windows Terminal (native tab)",
            TerminalEnv::WindowsTerminalWsl => "Windows Terminal (WSL tab)",
            TerminalEnv::Tmux => "tmux (new window)",
            TerminalEnv::MacOs => "macOS Terminal",
            TerminalEnv::LinuxGeneric => "Terminal",
        }
    }
}

/// Detect available terminal environments for the current OS.
pub fn detect_terminals() -> Vec<TerminalEnv> {
    // Embedded is always first — the main differentiator of crew-board
    let mut terminals = vec![TerminalEnv::Embedded];

    // Check tmux (available on any platform)
    if std::env::var("TMUX").is_ok() {
        terminals.push(TerminalEnv::Tmux);
    }

    // Native Windows
    if cfg!(target_os = "windows") && command_exists("wt") {
        terminals.push(TerminalEnv::WindowsTerminalNative);
    }

    // WSL2 detection
    if is_wsl() {
        terminals.push(TerminalEnv::WindowsTerminalWsl);
    }

    // macOS
    if cfg!(target_os = "macos") {
        terminals.push(TerminalEnv::MacOs);
    }

    // Generic Linux fallback
    if cfg!(target_os = "linux") && terminals.len() <= 1 {
        terminals.push(TerminalEnv::LinuxGeneric);
    }

    terminals
}

/// Detect available AI hosts by checking if commands exist on PATH.
pub fn detect_ai_hosts() -> Vec<AiHost> {
    let mut hosts = Vec::new();

    if command_exists("claude") {
        hosts.push(AiHost::Claude);
    }
    if command_exists("copilot") {
        hosts.push(AiHost::Copilot);
    }
    if command_exists("gemini") {
        hosts.push(AiHost::Gemini);
    }
    if command_exists("opencode") {
        hosts.push(AiHost::OpenCode);
    }
    if command_exists("devin") {
        hosts.push(AiHost::Devin);
    }
    if command_exists("droid") {
        hosts.push(AiHost::Droid);
    }

    // Always show all options even if not detected,
    // since they might be available in the launched shell
    if hosts.is_empty() {
        hosts = vec![AiHost::Claude, AiHost::Copilot, AiHost::Gemini, AiHost::OpenCode, AiHost::Devin, AiHost::Droid];
    }

    // Shell is always available as fallback
    hosts.push(AiHost::Shell);

    hosts
}

/// Launch a terminal with the given AI host in the specified directory.
pub fn launch(
    terminal: TerminalEnv,
    host: AiHost,
    work_dir: &Path,
    task_id: &str,
    _task_description: &str,
    color_scheme: Option<&ColorSchemeHex>,
) -> Result<(), String> {
    let dir = work_dir.to_string_lossy();
    let resume_prompt = format!("/crew resume {}", task_id);

    // Copilot and OpenCode don't accept a prompt argument.
    // The .crew-resume file in the worktree provides context instead.
    // Claude, Gemini, Devin, and Droid accept prompt as CLI argument.
    let shell_cmd_for_host = |dir: &str| -> String {
        match host {
            AiHost::Copilot | AiHost::OpenCode => format!(
                "cd '{}' && {}",
                shell_escape(dir),
                host.command(),
            ),
            _ => format!(
                "cd '{}' && {} \"{}\"",
                shell_escape(dir),
                host.command(),
                resume_prompt,
            ),
        }
    };

    // PowerShell command for native Windows terminals
    let pwsh_cmd_for_host = |dir: &str| -> String {
        match host {
            AiHost::Copilot | AiHost::OpenCode => format!(
                "Set-Location '{}'; {}",
                dir.replace('\'', "''"),
                host.command(),
            ),
            AiHost::Shell => format!(
                "Set-Location '{}'",
                dir.replace('\'', "''"),
            ),
            _ => format!(
                "Set-Location '{}'; {} '{}'",
                dir.replace('\'', "''"),
                host.command(),
                resume_prompt.replace('\'', "''"),
            ),
        }
    };

    match terminal {
        TerminalEnv::Embedded => {
            // Handled by app.rs spawn_terminal — should not reach here
            return Err("Embedded terminals are spawned via TerminalManager, not launcher".to_string());
        }
        TerminalEnv::WindowsTerminalNative => {
            // wt.exe new-tab: open a native PowerShell tab in Windows Terminal
            let pwsh_cmd = pwsh_cmd_for_host(&dir);
            let mut args: Vec<String> = vec![
                "new-tab".to_string(),
                "--title".to_string(),
                task_id.to_string(),
            ];
            if let Some(cs) = color_scheme {
                args.extend([
                    "--tabColor".to_string(), cs.tab.to_string(),
                    "--colorScheme".to_string(), cs.name.to_string(),
                ]);
            }
            args.extend([
                "pwsh".to_string(),
                "-NoExit".to_string(),
                "-Command".to_string(),
                pwsh_cmd,
            ]);
            Command::new("wt")
                .args(&args)
                .spawn()
                .map_err(|e| format!("Failed to launch Windows Terminal: {}", e))?;
        }
        TerminalEnv::WindowsTerminalWsl => {
            // wt.exe new-tab: open a new WSL tab in Windows Terminal
            // Explicit cd in the bash command since bash -l may reset cwd
            let shell_cmd = shell_cmd_for_host(&dir);
            let mut args: Vec<&str> = vec!["new-tab", "--title", task_id];
            // Storage for owned strings that args references
            let tab_color;
            let scheme_name;
            if let Some(cs) = color_scheme {
                tab_color = cs.tab.to_string();
                scheme_name = cs.name.to_string();
                args.extend(["--tabColor", &tab_color, "--colorScheme", &scheme_name]);
            }
            args.extend(["wsl.exe", "--cd", &dir, "--", "bash", "-lic", &shell_cmd]);
            Command::new("wt.exe")
                .args(&args)
                .spawn()
                .map_err(|e| format!("Failed to launch Windows Terminal: {}", e))?;
        }
        TerminalEnv::Tmux => {
            let shell_cmd = shell_cmd_for_host(&dir);
            Command::new("tmux")
                .args([
                    "new-window",
                    "-n",
                    task_id,
                    "-c",
                    &dir,
                    &shell_cmd,
                ])
                .spawn()
                .map_err(|e| format!("Failed to launch tmux window: {}", e))?;
            // Apply color scheme to tmux window (best-effort)
            if let Some(cs) = color_scheme {
                let style = format!("bg={},fg={}", cs.bg, cs.fg);
                Command::new("tmux")
                    .args(["set-option", "-t", task_id, "-w", "window-style", &style])
                    .spawn()
                    .ok();
            }
        }
        TerminalEnv::MacOs => {
            // Use osascript to open Terminal.app
            let shell_cmd = shell_cmd_for_host(&dir);
            let script = format!(
                "tell application \"Terminal\" to do script \"{}\"",
                shell_cmd,
            );
            Command::new("osascript")
                .args(["-e", &script])
                .spawn()
                .map_err(|e| format!("Failed to launch macOS Terminal: {}", e))?;
        }
        TerminalEnv::LinuxGeneric => {
            // Try common terminal emulators
            let shell_cmd = shell_cmd_for_host(&dir);
            let terminals_to_try = [
                ("gnome-terminal", vec!["--", "bash", "-c", &shell_cmd]),
                ("xterm", vec!["-e", "bash", "-c", &shell_cmd]),
                ("konsole", vec!["-e", "bash", "-c", &shell_cmd]),
            ];
            let mut launched = false;
            for (cmd, args) in &terminals_to_try {
                if command_exists(cmd) {
                    Command::new(cmd)
                        .args(args)
                        .spawn()
                        .map_err(|e| format!("Failed to launch {}: {}", cmd, e))?;
                    launched = true;
                    break;
                }
            }
            if !launched {
                return Err("No supported terminal emulator found".to_string());
            }
        }
    }

    Ok(())
}

fn is_wsl() -> bool {
    // /proc/version doesn't exist on native Windows
    std::fs::read_to_string("/proc/version")
        .map(|v| v.to_lowercase().contains("microsoft"))
        .unwrap_or(false)
}

fn command_exists(cmd: &str) -> bool {
    if cfg!(target_os = "windows") {
        Command::new("where.exe")
            .arg(cmd)
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status()
            .map(|s| s.success())
            .unwrap_or(false)
    } else {
        Command::new("which")
            .arg(cmd)
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status()
            .map(|s| s.success())
            .unwrap_or(false)
    }
}

/// Return the command and args for embedding an AI host in an embedded PTY terminal.
///
/// Unlike shell-based launching, this returns the command and args directly — no shell
/// wrapper is needed because `spawn_pty` sets `cwd` via `CommandBuilder` and `portable-pty`
/// handles process spawning natively on all platforms.
///
/// For `AiHost::Shell`, returns the platform-appropriate interactive shell.
pub fn embed_cmd_args(host: AiHost, _task_id: &str) -> (String, Vec<String>) {
    match host {
        AiHost::Shell => {
            let shell = platform_shell();
            (shell, vec![])
        }
        _ => {
            if cfg!(target_os = "windows") {
                // On Windows, npm-installed tools are .cmd/.ps1 shims that
                // CreateProcessW can't launch directly. For Claude, we bypass
                // cmd.exe and the shim by running node.exe directly with the
                // CLI entry point. This avoids extra process wrapping inside
                // ConPTY which causes massive startup delays.
                let (cmd, args) = resolve_windows_command(host);
                (cmd, args)
            } else {
                let cmd = match host {
                    AiHost::Copilot => "copilot".to_string(),
                    AiHost::OpenCode => "opencode".to_string(),
                    AiHost::Devin => "devin".to_string(),
                    AiHost::Droid => "droid".to_string(),
                    _ => host.command().to_string(),
                };
                (cmd, vec![])
            }
        }
    }
}

/// Return the platform-appropriate interactive shell command.
pub fn platform_shell() -> String {
    if cfg!(target_os = "windows") {
        if command_exists("pwsh") { "pwsh".to_string() } else { "cmd".to_string() }
    } else {
        "bash".to_string()
    }
}


/// On Windows, resolve an AI host command to its actual executable + args,
/// bypassing .cmd/.ps1 shims. This avoids spawning cmd.exe or pwsh.exe
/// inside ConPTY which adds massive startup overhead.
///
/// For npm-installed tools (claude, copilot), finds the node.js entry point
/// and returns ("node", ["path/to/cli.js"]).
/// Falls back to ("cmd", ["/c", "command"]) if resolution fails.
fn resolve_windows_command(host: AiHost) -> (String, Vec<String>) {
    let cmd_name = match host {
        AiHost::Copilot => "copilot",
        AiHost::OpenCode => "opencode",
        AiHost::Claude => "claude",
        AiHost::Gemini => "gemini",
        AiHost::Devin => "devin",
        AiHost::Droid => "droid",
        AiHost::Shell => return (platform_shell(), vec![]),
    };

    // Try to find the .cmd shim and extract the node.js entry point
    if let Ok(output) = Command::new("where.exe")
        .arg(cmd_name)
        .output()
    {
        let paths = String::from_utf8_lossy(&output.stdout);
        for line in paths.lines() {
            let path = line.trim();
            // Look for the .cmd file
            if path.ends_with(".cmd") || path.ends_with(".CMD") {
                if let Ok(content) = std::fs::read_to_string(path) {
                    // npm .cmd shims contain: "%~dp0\node.exe" "%~dp0\node_modules\...\cli.js"
                    // Extract the cli.js path
                    if let Some(js_path) = extract_node_entry_point(&content, path) {
                        return ("node".to_string(), vec![js_path]);
                    }
                }
            }
            // If it's a .exe, use it directly
            if path.ends_with(".exe") || path.ends_with(".EXE") {
                return (path.to_string(), vec![]);
            }
        }
    }

    // Fallback: use cmd /c
    ("cmd".to_string(), vec!["/c".to_string(), cmd_name.to_string()])
}

/// Extract the Node.js entry point from an npm .cmd shim.
/// Returns the absolute path to the .js file.
fn extract_node_entry_point(shim_content: &str, shim_path: &str) -> Option<String> {
    let basedir = Path::new(shim_path).parent()?;

    // npm .cmd shims have lines like:
    //   "%~dp0\node_modules\@anthropic-ai\claude-code\cli.js" %*
    // or in the .ps1 version:
    //   "$basedir/node_modules/@anthropic-ai/claude-code/cli.js"
    for line in shim_content.lines() {
        let line = line.trim();
        // Look for node_modules path with .js extension
        if let Some(start) = line.find("node_modules") {
            // Extract the path, handling both quotes and %~dp0
            let rest = &line[start..];
            let end = rest.find('"')
                .or_else(|| rest.find(' '))
                .or_else(|| rest.find('%'))
                .unwrap_or(rest.len());
            let rel_path = &rest[..end];
            let full_path = basedir.join(rel_path);
            if full_path.exists() {
                return Some(full_path.to_string_lossy().to_string());
            }
        }
    }
    None
}

fn shell_escape(s: &str) -> String {
    s.replace('\'', "'\\''")
}
