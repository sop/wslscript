#![windows_subsystem = "windows"]

#[macro_use]
extern crate failure;
extern crate guid_create;
extern crate shell32;
extern crate wchar;
extern crate winapi;
extern crate winreg;

mod error;
mod font;
mod gui;
mod icon;
mod registry;
mod win32;
mod wsl;

use error::*;
use std::env;
use std::ffi::OsString;
use std::path::PathBuf;
use widestring::*;

fn main() {
    if let Err(e) = run_app() {
        log::error!("{}", e);
        unsafe {
            use winapi::um::winuser::*;
            MessageBoxW(
                std::ptr::null_mut(),
                e.to_wide().as_ptr(),
                wcstr!("Error").as_ptr(),
                MB_OK | MB_ICONERROR | MB_SERVICE_NOTIFICATION,
            );
        }
    }
}

fn run_app() -> Result<(), Error> {
    // set up logging
    #[cfg(feature = "debug")]
    if let Ok(mut exe) = env::current_exe() {
        exe.pop();
        exe.push("wslscript.log");
        simple_logging::log_to_file(exe, log::LevelFilter::Debug)?;
    }
    // if program was started with the first and only argument being a .sh file
    // or one of the registered extensions.
    // this handles a script file being dragged and dropped to wslscript.exe.
    if env::args_os().len() == 2 {
        if let Some(arg) = env::args_os()
            .nth(1)
            .filter(|arg| PathBuf::from(arg).exists())
        {
            let path = PathBuf::from(&arg);
            let ext = path.extension().unwrap_or_default().to_string_lossy();
            // check whether extension is registered
            let opts = match wsl::WSLOptions::from_ext(&ext) {
                Some(opts) => Some(opts),
                // if extension is ".sh", use default options
                None if ext == "sh" => Some(wsl::WSLOptions::default()),
                _ => None,
            };
            if let Some(opts) = opts {
                return execute_wsl(vec![arg], opts);
            }
        }
    }
    // seek for -E flag and collect all arguments after that
    let wsl_args: Vec<OsString> = env::args_os()
        .skip_while(|arg| arg != "-E")
        .skip(1)
        .collect();
    if !wsl_args.is_empty() {
        // collect arguments preceding -E
        let opts: Vec<OsString> = env::args_os().take_while(|arg| arg != "-E").collect();
        return execute_wsl(wsl_args, wsl::WSLOptions::from_args(opts));
    }
    // start Windows GUI
    gui::start_gui()
}

fn execute_wsl(args: Vec<OsString>, opts: wsl::WSLOptions) -> Result<(), Error> {
    // convert args to paths, canonicalize when possible
    let paths: Vec<PathBuf> = args
        .iter()
        .map(PathBuf::from)
        .map(|p| p.canonicalize().unwrap_or(p))
        .collect();
    // ensure not trying to invoke self
    if let Some(exe_os) = env::current_exe().ok().and_then(|p| p.canonicalize().ok()) {
        if paths[0] == exe_os {
            return Err(Error::from(ErrorKind::InvalidPathError));
        }
    }
    // convert paths to WSL equivalents
    let wsl_paths = wsl::paths_to_wsl(&paths, &opts)?;
    wsl::run_wsl(&wsl_paths[0], &wsl_paths[1..], &opts)
}
