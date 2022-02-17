use crate::error::*;
use crate::registry::{self, HoldMode};
use crate::wcstring;
use crate::win32::*;
use failure::ResultExt;
use std::env;
use std::ffi::{OsStr, OsString};
use std::os::windows::ffi::{OsStrExt, OsStringExt};
use std::os::windows::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{self, Stdio};
use wchar::*;
use widestring::*;
use winapi::shared::minwindef::MAX_PATH;
use winapi::um::winbase;

/// Maximum command line length on Windows.
const MAX_CMD_LEN: usize = 8191;

/// Run script with optional arguments in a WSL.
///
/// Paths must be in WSL context.
pub fn run_wsl(script_path: &Path, args: &[PathBuf], opts: &WSLOptions) -> Result<(), Error> {
    // maximum length of the bash command
    const MAX_BASH_LEN: usize = MAX_CMD_LEN - MAX_PATH - MAX_PATH - 20;
    let mut bash_cmd = compose_bash_command(script_path, args, opts, false)?;
    // if arguments won't fit into command line
    if bash_cmd.cmd.len() > MAX_BASH_LEN {
        // retry and force to write arguments into temporary file
        bash_cmd = compose_bash_command(script_path, args, opts, true)?;
        if bash_cmd.cmd.len() > MAX_BASH_LEN {
            return Err(Error::from(ErrorKind::CommandTooLong));
        }
    }
    log::debug!("Bash command: {}", bash_cmd.cmd.to_string_lossy());
    // build command to start WSL process in a terminal window
    let mut cmd = process::Command::new(cmd_bin_path().as_os_str());
    cmd.args(&[OsStr::new("/C"), wsl_bin_path()?.as_os_str()]);
    if let Some(distro) = &opts.distribution {
        cmd.args(&[OsStr::new("-d"), distro]);
    }
    cmd.args(&[OsStr::new("-e"), OsStr::new("bash")]);
    if opts.interactive {
        cmd.args(&[OsStr::new("-i")]);
    }
    cmd.args(&[OsStr::new("-c"), &bash_cmd.cmd.to_os_string()]);
    // start as a detached process in a new process group so we can safely
    // exit this program and have the script execute on it's own
    cmd.creation_flags(winbase::DETACHED_PROCESS | winbase::CREATE_NEW_PROCESS_GROUP);
    let mut proc: process::Child = cmd
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .context(ErrorKind::WSLProcessError)?;
    // if a temporary file was created for the arguments
    if let Some(tmpfile) = bash_cmd.tmpfile {
        // wait for the process to exit
        let _ = proc.wait();
        log::debug!("Removing temporary file {}", tmpfile.to_string_lossy());
        if std::fs::remove_file(tmpfile).is_err() {
            log::debug!("Failed to remove temporary file");
        }
    }
    Ok(())
}

struct BashCmdResult {
    /// Command line for bash
    cmd: WideString,
    /// Path to temporary file containing the script arguments
    tmpfile: Option<PathBuf>,
}

/// Build bash command to execute script with given arguments.
///
/// If arguments are too long to fit on a command line, write them to temporary
/// file and fetch on WSL side using bash's `mapfile` builtin.
fn compose_bash_command(
    script_path: &Path,
    args: &[PathBuf],
    opts: &WSLOptions,
    force_args_in_file: bool,
) -> Result<BashCmdResult, Error> {
    let script_dir = script_path
        .parent()
        .ok_or(ErrorKind::InvalidPathError)?
        .as_os_str();
    let script_file = script_path.file_name().ok_or(ErrorKind::InvalidPathError)?;
    // command line to invoke in WSL
    let mut cmd = WideString::new();
    let tmpfile = if force_args_in_file ||
        // heuristic test whether argument list is too long to be passed on command line
        args.iter().fold(0, |acc, s| acc + s.as_os_str().len()) > (MAX_CMD_LEN / 2)
    {
        let argfile = write_args_to_temp_file(args)?;
        let path = path_to_wsl(&argfile, opts)?;
        // read arguments from temporary file into $args variable
        cmd.push_slice(wch!("mapfile -d '' -t args < '"));
        cmd.push_os_str(single_quote_escape(path.as_os_str()));
        cmd.push_slice(wch!("' && "));
        Some(argfile)
    } else {
        None
    };
    // cd 'dir' && './progname'
    cmd.push_slice(wch!("cd '"));
    cmd.push_os_str(single_quote_escape(script_dir));
    cmd.push_slice(wch!("' && './"));
    cmd.push_os_str(single_quote_escape(script_file));
    cmd.push_slice(wch!("'"));
    // if arguments are being passed via temporary file
    if tmpfile.is_some() {
        cmd.push_slice(wch!(" \"${args[@]}\""));
    }
    // insert arguments to command line
    else {
        for arg in args {
            cmd.push_slice(wch!(" '"));
            cmd.push_os_str(single_quote_escape(arg.as_os_str()));
            cmd.push_slice(wch!("'"));
        }
    }
    // commands after script exits
    match opts.hold_mode {
        HoldMode::Never => {}
        HoldMode::Always | HoldMode::Error => {
            if opts.hold_mode == HoldMode::Always {
                cmd.push_slice(wch!(";"));
            } else {
                cmd.push_slice(wch!(" ||"))
            }
            cmd.push_os_str(OsString::from_wide(wch!(
                r#" { printf >&2 '\n[Process exited - exit code %d] ' "$?"; read -n 1 -s; }"#
            )));
        }
    }
    Ok(BashCmdResult { cmd, tmpfile })
}

/// Write arguments to temporary file as a nul separated list.
fn write_args_to_temp_file(args: &[PathBuf]) -> Result<PathBuf, Error> {
    use std::io::prelude::*;
    let temp = create_temp_file()?;
    let paths: Result<Vec<_>, _> = args
        .iter()
        .map(|p| {
            p.to_str()
                .ok_or_else(|| Error::from(ErrorKind::StringToPathUTF8Error))
        })
        .collect();
    let s = match paths {
        Err(e) => return Err(e),
        Ok(p) => p.join("\0"),
    };
    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .truncate(true)
        .open(&temp)?;
    file.write_all(s.as_bytes())?;
    log::debug!("Args written to: {}", temp.to_string_lossy());
    Ok(temp)
}

/// Create a temporary file.
///
/// Returned path is an empty file in Windows's temp file directory.
fn create_temp_file() -> Result<PathBuf, Error> {
    use winapi::um::fileapi as fa;
    let mut buf = [0u16; MAX_PATH + 1];
    let len = unsafe { fa::GetTempPathW(buf.len() as u32, buf.as_mut_ptr()) };
    if len == 0 {
        return Err(last_error());
    }
    let temp_dir = unsafe { WideCString::from_ptr_truncate(buf.as_ptr(), len as usize + 1) };
    let uniq = unsafe {
        fa::GetTempFileNameW(
            temp_dir.as_ptr(),
            wcstring("wsl").as_ptr(),
            0,
            buf.as_mut_ptr(),
        )
    };
    if uniq == 0 {
        return Err(last_error());
    }
    let temp_path = unsafe { WideCString::from_ptr_truncate(buf.as_ptr(), buf.len()) };
    log::debug!("Temp path {}", temp_path.to_string_lossy());
    Ok(PathBuf::from(temp_path.to_string_lossy()))
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

/// Convert single Windows path to WSL equivalent.
fn path_to_wsl(path: &Path, opts: &WSLOptions) -> Result<PathBuf, Error> {
    let mut paths = paths_to_wsl(&[path.to_owned()], opts)?;
    let p = paths
        .pop()
        .ok_or_else(|| Error::from(ErrorKind::WinToUnixPathError))?;
    Ok(p)
}

/// Convert Windows paths to WSL equivalents.
///
/// Multiple paths can be converted on a single WSL invocation.
/// Converted paths are returned in the same order as given.
pub fn paths_to_wsl(paths: &[PathBuf], opts: &WSLOptions) -> Result<Vec<PathBuf>, Error> {
    let mut wsl_paths: Vec<PathBuf> = Vec::with_capacity(paths.len());
    let mut path_idx = 0;
    while path_idx < paths.len() {
        // build a printf command that prints null separated results
        let mut printf = WideString::new();
        printf.push_slice(wch!(r"printf '%s\0'"));
        // convert multiple paths on single WSL invocation up to maximum command line length
        while path_idx < paths.len() && printf.len() < MAX_CMD_LEN - MAX_PATH - 100 {
            printf.push_slice(wch!(r#" "$(wslpath -u '"#));
            printf.push_os_str(single_quote_escape(paths[path_idx].as_os_str()));
            printf.push_slice(wch!(r#"')""#));
            path_idx += 1;
        }
        log::debug!("printf command length {}", printf.len());
        let mut cmd = process::Command::new(wsl_bin_path()?);
        cmd.creation_flags(winbase::CREATE_NO_WINDOW);
        if let Some(distro) = &opts.distribution {
            cmd.args(&[OsStr::new("-d"), distro]);
        }
        cmd.args(&[
            OsStr::new("-e"),
            OsStr::new("bash"),
            OsStr::new("-c"),
            &printf.to_os_string(),
        ]);
        let output = cmd.output().context(ErrorKind::WinToUnixPathError)?;
        if !output.status.success() {
            return Err(Error::from(ErrorKind::WinToUnixPathError));
        }
        wsl_paths.extend(
            std::str::from_utf8(&output.stdout)
                .context(ErrorKind::StringToPathUTF8Error)?
                .trim()
                .trim_matches('\0')
                .split('\0')
                .map(PathBuf::from),
        )
    }
    log::debug!("Converted {} Windows paths to WSL", wsl_paths.len());
    Ok(wsl_paths)
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
        p.push(r"System32\cmd.exe");
        if p.is_file() {
            return p;
        }
    }
    // hardcoded fallback
    PathBuf::from(r"C:\Windows\System32\cmd.exe")
}

/// Returns the path to WSL executable.
fn wsl_bin_path() -> Result<PathBuf, Error> {
    // try %SYSTEMROOT\System32\wsl.exe
    if let Some(mut p) = env::var_os("SYSTEMROOT").map(PathBuf::from) {
        p.push(r"System32\wsl.exe");
        if p.is_file() {
            return Ok(p);
        }
    }
    // no dice
    Err(Error::from(ErrorKind::WSLNotFound))
}

/// Options for WSL invocation.
pub struct WSLOptions {
    /// Mode after the command exits.
    hold_mode: HoldMode,
    /// Whether to run bash as an interactive shell.
    interactive: bool,
    /// Name of the WSL distribution to invoke.
    distribution: Option<OsString>,
}

impl WSLOptions {
    pub fn from_args(args: Vec<OsString>) -> Self {
        let mut hold_mode = HoldMode::default();
        let mut interactive = false;
        let mut distribution = None;
        let mut iter = args.iter();
        while let Some(arg) = iter.next() {
            // If extension parameter is present, load from registry.
            // This is the default after 0.5.0 version. Other arguments are
            // kept just for backwards compatibility for now.
            if arg == "--ext" {
                if let Some(ext) = iter.next().map(|s| s.to_string_lossy().into_owned()) {
                    if let Some(opts) = Self::from_ext(&ext) {
                        return opts;
                    }
                }
            } else if arg == "-h" {
                if let Some(mode) = iter
                    .next()
                    .and_then(|s| WideCString::from_os_str(s).ok())
                    .and_then(|s| HoldMode::from_wcstr(&s))
                {
                    hold_mode = mode;
                }
            } else if arg == "-i" {
                interactive = true;
            } else if arg == "-d" {
                distribution = iter.next().map(|s| s.to_owned());
            }
        }
        Self {
            hold_mode,
            interactive,
            distribution,
        }
    }

    /// Load options for registered extension.
    ///
    /// `ext` is the filename extension without a leading dot.
    pub fn from_ext(ext: &str) -> Option<Self> {
        if let Ok(config) = registry::get_extension_config(ext) {
            let distro = config
                .distro
                .and_then(registry::distro_guid_to_name)
                .map(OsString::from);
            Some(Self {
                hold_mode: config.hold_mode,
                interactive: config.interactive,
                distribution: distro,
            })
        } else {
            None
        }
    }
}

impl Default for WSLOptions {
    fn default() -> Self {
        Self {
            hold_mode: HoldMode::default(),
            interactive: false,
            distribution: None,
        }
    }
}
