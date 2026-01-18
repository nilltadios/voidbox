//! GUI helpers using zenity/kdialog for desktop integration
//!
//! This module provides cross-desktop GUI dialogs for:
//! - Installation confirmation
//! - Progress bars
//! - Error/info messages
//! - Yes/No questions

use std::io::Write;
use std::process::{Child, Command, Stdio};

/// Check if we're running in a GUI environment (not a TTY)
pub fn is_gui_mode() -> bool {
    // Check if stdin is NOT a TTY (double-clicked from file manager)
    !atty::is(atty::Stream::Stdin)
}

/// Check if any GUI dialog tool is available
pub fn has_gui_support() -> bool {
    which_dialog().is_some()
}

/// Detect which dialog tool is available
fn which_dialog() -> Option<DialogTool> {
    // Prefer zenity (GTK/GNOME), fall back to kdialog (KDE)
    if Command::new("zenity").arg("--version").output().is_ok() {
        Some(DialogTool::Zenity)
    } else if Command::new("kdialog").arg("--version").output().is_ok() {
        Some(DialogTool::Kdialog)
    } else {
        None
    }
}

#[derive(Debug, Clone, Copy)]
enum DialogTool {
    Zenity,
    Kdialog,
}

/// Show an info message dialog
pub fn show_info(title: &str, message: &str) {
    match which_dialog() {
        Some(DialogTool::Zenity) => {
            Command::new("zenity")
                .args([
                    "--info", "--title", title, "--text", message, "--width", "400",
                ])
                .status()
                .ok();
        }
        Some(DialogTool::Kdialog) => {
            Command::new("kdialog")
                .args(["--title", title, "--msgbox", message])
                .status()
                .ok();
        }
        None => {
            println!("{}: {}", title, message);
        }
    }
}

/// Show an error message dialog
pub fn show_error(title: &str, message: &str) {
    match which_dialog() {
        Some(DialogTool::Zenity) => {
            Command::new("zenity")
                .args([
                    "--error", "--title", title, "--text", message, "--width", "400",
                ])
                .status()
                .ok();
        }
        Some(DialogTool::Kdialog) => {
            Command::new("kdialog")
                .args(["--title", title, "--error", message])
                .status()
                .ok();
        }
        None => {
            eprintln!("Error - {}: {}", title, message);
        }
    }
}

/// Show a yes/no question dialog, returns true if user clicked Yes
pub fn ask_yes_no(title: &str, message: &str) -> bool {
    match which_dialog() {
        Some(DialogTool::Zenity) => Command::new("zenity")
            .args([
                "--question",
                "--title",
                title,
                "--text",
                message,
                "--width",
                "400",
            ])
            .status()
            .map(|s| s.success())
            .unwrap_or(false),
        Some(DialogTool::Kdialog) => Command::new("kdialog")
            .args(["--title", title, "--yesno", message])
            .status()
            .map(|s| s.success())
            .unwrap_or(false),
        None => {
            print!("{} [y/N] ", message);
            std::io::stdout().flush().ok();
            let mut input = String::new();
            std::io::stdin().read_line(&mut input).ok();
            input.trim().to_lowercase() == "y"
        }
    }
}

/// Progress bar handle for long operations
pub struct ProgressDialog {
    child: Option<Child>,
    tool: Option<DialogTool>,
}

impl ProgressDialog {
    /// Create and show a new progress dialog
    pub fn new(title: &str, text: &str) -> Self {
        match which_dialog() {
            Some(DialogTool::Zenity) => {
                let child = Command::new("zenity")
                    .args([
                        "--progress",
                        "--title",
                        title,
                        "--text",
                        text,
                        "--pulsate",
                        "--auto-close",
                        "--no-cancel",
                        "--width",
                        "400",
                    ])
                    .stdin(Stdio::piped())
                    .spawn()
                    .ok();
                Self {
                    child,
                    tool: Some(DialogTool::Zenity),
                }
            }
            Some(DialogTool::Kdialog) => {
                // kdialog uses dbus for progress, more complex
                // For now, just show a passive popup
                Command::new("kdialog")
                    .args(["--title", title, "--passivepopup", text, "30"])
                    .spawn()
                    .ok();
                Self {
                    child: None,
                    tool: Some(DialogTool::Kdialog),
                }
            }
            None => {
                println!("{}: {}", title, text);
                Self {
                    child: None,
                    tool: None,
                }
            }
        }
    }

    /// Create a determinate progress dialog (0-100%)
    pub fn new_determinate(title: &str, text: &str) -> Self {
        match which_dialog() {
            Some(DialogTool::Zenity) => {
                let child = Command::new("zenity")
                    .args([
                        "--progress",
                        "--title",
                        title,
                        "--text",
                        text,
                        "--auto-close",
                        "--no-cancel",
                        "--width",
                        "400",
                    ])
                    .stdin(Stdio::piped())
                    .spawn()
                    .ok();
                Self {
                    child,
                    tool: Some(DialogTool::Zenity),
                }
            }
            _ => Self::new(title, text),
        }
    }

    /// Update progress (0-100)
    pub fn set_progress(&mut self, percent: u32) {
        if let Some(ref mut child) = self.child {
            if let Some(ref mut stdin) = child.stdin {
                writeln!(stdin, "{}", percent.min(100)).ok();
            }
        }
    }

    /// Update the text message
    pub fn set_text(&mut self, text: &str) {
        if let Some(ref mut child) = self.child {
            if let Some(ref mut stdin) = child.stdin {
                // Zenity uses # prefix for text updates
                if matches!(self.tool, Some(DialogTool::Zenity)) {
                    writeln!(stdin, "# {}", text).ok();
                }
            }
        }
    }

    /// Close the progress dialog
    pub fn close(mut self) {
        if let Some(ref mut child) = self.child {
            if let Some(ref mut stdin) = child.stdin {
                writeln!(stdin, "100").ok();
            }
            child.wait().ok();
        }
    }
}

impl Drop for ProgressDialog {
    fn drop(&mut self) {
        if let Some(ref mut child) = self.child {
            child.kill().ok();
            child.wait().ok();
        }
    }
}

/// Show a notification (non-blocking)
pub fn notify(title: &str, message: &str) {
    // Try notify-send first (works on most desktops)
    if Command::new("notify-send")
        .args([title, message])
        .status()
        .is_ok()
    {
        return;
    }

    // Fallback to zenity/kdialog notification
    match which_dialog() {
        Some(DialogTool::Zenity) => {
            Command::new("zenity")
                .args([
                    "--notification",
                    "--text",
                    &format!("{}: {}", title, message),
                ])
                .spawn()
                .ok();
        }
        Some(DialogTool::Kdialog) => {
            Command::new("kdialog")
                .args(["--title", title, "--passivepopup", message, "5"])
                .spawn()
                .ok();
        }
        None => {
            println!("{}: {}", title, message);
        }
    }
}
