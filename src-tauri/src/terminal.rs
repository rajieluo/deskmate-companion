//! Opening a command in a new terminal window on Linux (and other Unix desktops).
//!
//! Windows gives every console program its own window with CREATE_NEW_CONSOLE.
//! Linux has no such thing, so the command is handed to a terminal emulator:
//! `$TERMINAL` if set, else the first of the usual ones that is installed.

#[cfg(not(windows))]
use std::path::{Path, PathBuf};

/// Terminal emulators to try, with the argument that says "run the rest".
#[cfg(not(windows))]
const TERMINALS: &[(&str, &[&str])] = &[
    ("x-terminal-emulator", &["-e"]),
    ("gnome-terminal", &["--"]),
    ("kgx", &["--"]),
    ("ptyxis", &["--"]),
    ("konsole", &["-e"]),
    ("xfce4-terminal", &["-x"]),
    ("mate-terminal", &["-x"]),
    ("lxterminal", &["-e"]),
    ("tilix", &["-e"]),
    ("terminator", &["-x"]),
    ("kitty", &[]),
    ("alacritty", &["-e"]),
    ("wezterm", &["start", "--"]),
    ("foot", &[]),
    ("xterm", &["-e"]),
];

/// The "run this" argument for a terminal named `name` (`$TERMINAL` may be any of them).
#[cfg(not(windows))]
fn exec_flag(name: &str) -> Vec<String> {
    TERMINALS
        .iter()
        .find(|(t, _)| *t == name)
        .map(|(_, flag)| flag.iter().map(|s| s.to_string()).collect())
        .unwrap_or_else(|| vec!["-e".into()])
}

#[cfg(not(windows))]
fn on_path(name: &str) -> Option<PathBuf> {
    let candidate = Path::new(name);
    if candidate.is_absolute() {
        return candidate.is_file().then(|| candidate.to_path_buf());
    }
    std::env::split_paths(&std::env::var_os("PATH")?).map(|dir| dir.join(name)).find(|p| p.is_file())
}

/// The terminal program and the arguments that come before the command.
#[cfg(not(windows))]
pub fn find() -> Option<(PathBuf, Vec<String>)> {
    if let Some(custom) = std::env::var("TERMINAL").ok().filter(|t| !t.trim().is_empty()) {
        if let Some(path) = on_path(custom.trim()) {
            let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
            return Some((path, exec_flag(&name)));
        }
    }
    TERMINALS.iter().find_map(|(name, _)| on_path(name).map(|p| (p, exec_flag(name))))
}

/// Opens `program args…` in a new terminal window, in `cwd` when given. Returns at once.
#[cfg(not(windows))]
pub fn open(program: &Path, args: &[std::ffi::OsString], cwd: Option<&Path>) -> Result<(), String> {
    let (terminal, pre) = find().ok_or(
        "I couldn't find a terminal app to open. Install one (for example gnome-terminal, konsole or xterm), \
or set the TERMINAL environment variable to yours.",
    )?;
    let mut cmd = std::process::Command::new(&terminal);
    cmd.args(pre);
    if let Some(dir) = cwd {
        // Some terminals (gnome-terminal's server) ignore the cwd they were started in, so cd first.
        cmd.args(["/bin/sh", "-c", r#"cd "$0" && exec "$@""#]).arg(dir);
        cmd.current_dir(dir);
    }
    cmd.arg(program).args(args);
    cmd.spawn()
        .map(|_| ())
        .map_err(|e| format!("Couldn't start {}: {e}", terminal.display()))
}

#[cfg(all(test, not(windows)))]
mod tests {
    use super::*;

    #[test]
    fn knows_each_terminals_exec_flag() {
        assert_eq!(exec_flag("gnome-terminal"), vec!["--"]);
        assert_eq!(exec_flag("konsole"), vec!["-e"]);
        assert_eq!(exec_flag("wezterm"), vec!["start", "--"]);
        assert!(exec_flag("kitty").is_empty());
        // An unknown $TERMINAL gets the xterm convention.
        assert_eq!(exec_flag("my-term"), vec!["-e"]);
    }
}
