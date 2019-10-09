use crate::error::*;
use crate::registry::HoldMode;
use failure::ResultExt;
use std::env;
use std::ffi::{OsStr, OsString};
use std::os::windows::ffi::{OsStrExt, OsStringExt};
use std::os::windows::process::CommandExt;
use std::path::PathBuf;
use std::process::{self, Stdio};
use wchar::*;
use widestring::*;
use winapi::um::winbase;

/// Run script with optional arguments in a WSL.
///
/// Paths must be in WSL context.
pub fn run_wsl(script_path: &PathBuf, args: &[PathBuf], opts: WSLOptions) -> Result<(), Error> {
    let script_dir = script_path
        .parent()
        .ok_or_else(|| ErrorKind::InvalidPathError)?
        .as_os_str();
    let script_file = script_path
        .file_name()
        .ok_or_else(|| ErrorKind::InvalidPathError)?;
    // command line to invoke in WSL
    let mut bash_cmd = WideString::new();
    // cd 'dir' && './progname'
    bash_cmd.push_slice(wch!("cd '"));
    bash_cmd.push_os_str(single_quote_escape(script_dir));
    bash_cmd.push_slice(wch!("' && './"));
    bash_cmd.push_os_str(single_quote_escape(script_file));
    bash_cmd.push_slice(wch!("'"));
    // arguments from drag & drop
    for arg in args {
        bash_cmd.push_slice(wch!(" '"));
        bash_cmd.push_os_str(single_quote_escape(arg.as_os_str()));
        bash_cmd.push_slice(wch!("'"));
    }
    // commands after script exits
    match opts.hold_mode {
        HoldMode::Never => {}
        HoldMode::Always | HoldMode::Error => {
            if opts.hold_mode == HoldMode::Always {
                bash_cmd.push_slice(wch!(";"));
            } else {
                bash_cmd.push_slice(wch!(" ||"))
            }
            bash_cmd.push_os_str(OsString::from_wide(wch!(
                r#" { printf >&2 '\n[Process exited - exit code %d] ' "$?"; read -n 1 -s; }"#
            )));
        }
    }
    // build command to start WSL process
    let mut cmd = process::Command::new(cmd_bin_path().as_os_str());
    cmd.args(&[OsStr::new("/C"), wsl_bin_path()?.as_os_str()]);
    if let Some(distro) = opts.distribution {
        cmd.args(&[OsStr::new("-d"), &distro]);
    }
    cmd.args(&[
        OsStr::new("-e"),
        OsStr::new("bash"),
        OsStr::new("-c"),
        &bash_cmd.to_os_string(),
    ]);
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
            w.extend_from_slice(wch!(r"'\''"));
        } else {
            w.push(c);
        }
    }
    OsString::from_wide(&w)
}

/// Convert Windows paths to WSL equivalents.
///
/// Multiple paths can be converted on a single WSL invocation.
/// Converted paths are returned in the same order as given.
pub fn paths_to_wsl(paths: &[PathBuf]) -> Result<Vec<PathBuf>, Error> {
    // build a printf command that prints null separated results
    let mut printf_cmd = WideString::new();
    printf_cmd.push_slice(wch!(r"printf '%s\0'"));
    paths
        .iter()
        .map(|path| {
            // execute wslpath for each argument in subprocess
            let mut s = WideString::new();
            s.push_slice(wch!(r#" "$(wslpath -u '"#));
            s.push_os_str(single_quote_escape(path.as_os_str()));
            s.push_slice(wch!(r#"')""#));
            s
        })
        .for_each(|s| printf_cmd.push(s));
    let mut cmd = process::Command::new(wsl_bin_path()?);
    cmd.creation_flags(winbase::CREATE_NO_WINDOW);
    cmd.args(&[
        OsStr::new("-e"),
        OsStr::new("bash"),
        OsStr::new("-c"),
        &printf_cmd.to_os_string(),
    ]);
    let output = cmd.output().context(ErrorKind::WinToUnixPathError)?;
    if !output.status.success() {
        return Err(Error::from(ErrorKind::WinToUnixPathError));
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
    if let Some(p) = env::var_os("COMSPEC")
        .map(PathBuf::from)
        .filter(|p| p.is_file())
    {
        return p;
    }
    // try %SYSTEMROOT\System32\cmd.exe
    if let Some(mut p) = env::var_os("SYSTEMROOT").map(PathBuf::from) {
        p.push(r#"System32\cmd.exe"#);
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
    if let Some(mut p) = env::var_os("SYSTEMROOT").map(PathBuf::from) {
        p.push(r#"System32\wsl.exe"#);
        if p.is_file() {
            return Ok(p);
        }
    }
    // no dice
    Err(Error::from(ErrorKind::WSLNotFound))
}

/// Options for WSL invocation
pub struct WSLOptions {
    /// mode after command exits
    hold_mode: HoldMode,
    /// WSL distribution to invoke
    distribution: Option<OsString>,
}

impl WSLOptions {
    pub fn from_args(args: Vec<OsString>) -> Self {
        let mut hold_mode = HoldMode::default();
        let mut distribution = None;
        let mut iter = args.iter();
        while let Some(arg) = iter.next() {
            if arg == "-h" {
                if let Some(mode) = iter
                    .next()
                    .and_then(|s| WideCString::from_os_str(s).ok())
                    .and_then(|s| HoldMode::from_wcstr(&s))
                {
                    hold_mode = mode;
                }
            } else if arg == "-d" {
                distribution = iter.next().map(|s| s.to_os_string());
            }
        }
        Self {
            hold_mode,
            distribution,
        }
    }
}

impl Default for WSLOptions {
    fn default() -> Self {
        Self {
            hold_mode: HoldMode::default(),
            distribution: None,
        }
    }
}
