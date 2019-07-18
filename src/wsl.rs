use crate::error::*;
use failure::ResultExt;
use std::env;
use std::ffi::{OsStr, OsString};
use std::os::windows::ffi::{OsStrExt, OsStringExt};
use std::os::windows::process::CommandExt;
use std::path::PathBuf;
use std::process::{self, Stdio};
use winapi::um::winbase;

/// Run script with optional arguments in a WSL.
///
/// Paths must be in WSL context.
pub fn run_wsl(script_path: &PathBuf, args: &[PathBuf]) -> Result<(), Error> {
    // TODO: ensure not trying to invoke self
    let script_dir = script_path
        .parent()
        .ok_or_else(|| ErrorKind::InvalidPathError)?
        .as_os_str();
    let script_file = script_path
        .file_name()
        .ok_or_else(|| ErrorKind::InvalidPathError)?;
    // command line to invoke in WSL
    let mut bash_cmd = OsString::new();
    // cd 'dir' && './progname'
    bash_cmd.push("cd '");
    bash_cmd.push(single_quote_escape(script_dir));
    bash_cmd.push("' && './");
    bash_cmd.push(single_quote_escape(script_file));
    bash_cmd.push("'");
    // arguments from drag & drop
    for arg in args {
        bash_cmd.push(" '");
        bash_cmd.push(single_quote_escape(arg.as_os_str()));
        bash_cmd.push("'");
    }
    // pause if the script exits with an error
    bash_cmd.push(" || { echo -e \"\\n[Process exited - exit code $?]\"; read -n 1 -s; }");
    // build command to start WSL process
    let mut cmd = process::Command::new(cmd_bin_path().as_os_str());
    cmd.arg("/C");
    cmd.arg(wsl_bin_path()?.as_os_str());
    cmd.arg("-e");
    cmd.arg("bash");
    cmd.arg("-c");
    cmd.arg(bash_cmd);
    // start as a detached process in a new process group so we can safely
    // exit this program and have the script execute on it's own
    cmd.creation_flags(winbase::DETACHED_PROCESS | winbase::CREATE_NEW_PROCESS_GROUP);
    cmd.stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .context(ErrorKind::WSLProcessError)?;
    Ok(())
}

/// Escape single quotes in an OsString.
fn single_quote_escape(s: &OsStr) -> OsString {
    let mut w: Vec<u16> = vec![];
    for c in s.encode_wide() {
        // escape ' to '\''
        if c == '\'' as u16 {
            w.extend_from_slice(&['\'' as u16, '\\' as u16, '\'' as u16, '\'' as u16]);
        } else {
            w.push(c);
        }
    }
    OsString::from_wide(&w)
}

/// Convert Windows paths to WSL equivalents.
pub fn paths_to_wsl(paths: &[PathBuf]) -> Result<Vec<PathBuf>, Error> {
    // compose arguments for bash printf
    let printf_args: Vec<String> = paths
        .iter()
        .map(|path| {
            // escape single quotes
            let escaped = path.to_str().unwrap().replace("'", "'\\''");
            // each printf argument is a subshell with wslpath invocation
            format!("\"$(wslpath -u '{}')\"", escaped)
        })
        .collect();
    // format with null separators
    let printf_cmd = format!("printf '%s\\0' {}", printf_args.join(" "));
    let mut cmd = process::Command::new(wsl_bin_path()?);
    cmd.creation_flags(winbase::CREATE_NO_WINDOW);
    cmd.args(&["-e", "bash", "-c", &printf_cmd]);
    let output = cmd.output().context(ErrorKind::WinToUnixPathError)?;
    if !output.status.success() {
        Err(ErrorKind::WinToUnixPathError)?
    }
    Ok(std::str::from_utf8(&output.stdout)
        .context(ErrorKind::StringToPathUTF8Error)?
        .trim()
        .trim_matches('\0')
        .split('\0')
        .map(PathBuf::from)
        .collect())
}

/// Returns the path to Windows command prompt executable.
fn cmd_bin_path() -> PathBuf {
    // if %COMSPEC% points to existing file
    if let Ok(path) = env::var("COMSPEC") {
        let p = PathBuf::from(path);
        if p.is_file() {
            return p;
        }
    }
    // try %SYSTEMROOT\System32\cmd.exe
    if let Ok(root) = env::var("SYSTEMROOT") {
        let mut p = PathBuf::from(root);
        p.push("System32\\cmd.exe");
        if p.is_file() {
            return p;
        }
    }
    // hardcoded fallback
    PathBuf::from(r#"C:\Windows\System32\cmd.exe"#)
}

/// Returns the path to WSL executable.
fn wsl_bin_path() -> Result<PathBuf, Error> {
    // try %SYSTEMROOT\System32\wsl.exe
    if let Ok(root) = env::var("SYSTEMROOT") {
        let mut p = PathBuf::from(root);
        p.push("System32\\wsl.exe");
        if p.is_file() {
            return Ok(p);
        }
    }
    // no dice
    Err(ErrorKind::WSLNotFound)?
}
