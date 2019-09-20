#![windows_subsystem = "windows"]

#[macro_use]
extern crate failure;
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
use wchar::*;

fn main() {
    if let Err(e) = run_app() {
        unsafe {
            use winapi::um::winuser::*;
            MessageBoxW(
                std::ptr::null_mut(),
                e.to_wide().as_ptr(),
                wch_c!("Error").as_ptr(),
                MB_OK | MB_ICONERROR | MB_SERVICE_NOTIFICATION,
            );
        }
    }
}

fn run_app() -> Result<(), Error> {
    // if program was started with the first and only argument being a .sh file
    // this handles a script file dragged and dropped to wslscript.exe
    if env::args_os().len() == 2 {
        if let Some(arg) = env::args_os()
            .nth(1)
            .filter(|arg| PathBuf::from(arg).exists())
            .filter(|arg| {
                let p = PathBuf::from(arg);
                let ext = p.extension().unwrap_or_default().to_string_lossy();
                ext == "sh"
            })
        {
            return execute_wsl(vec![arg], wsl::WSLOptions::default());
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
    let wsl_paths = wsl::paths_to_wsl(&paths)?;
    wsl::run_wsl(&wsl_paths[0], &wsl_paths[1..], opts)
}
