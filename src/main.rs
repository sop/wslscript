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
use std::ffi::{OsStr, OsString};
use std::path::PathBuf;
use win32::*;

fn main() {
    if let Err(e) = run_app() {
        error_message(&e.to_wide());
    }
}

fn run_app() -> Result<(), Error> {
    // if program was started with the first and only argument being a .sh file
    // this handles a script file dragged and dropped to wslscript.exe
    if env::args_os().len() == 2 {
        if let Some(arg) = env::args_os().nth(1) {
            let path = PathBuf::from(&arg);
            if path.exists() && path.extension().and_then(OsStr::to_str) == Some("sh") {
                return execute_wsl(vec![arg]);
            }
        }
    }
    // seek for -E flag and collect all arguments after that
    let wsl_args: Vec<OsString> = env::args_os()
        .skip_while(|arg| arg != "-E")
        .skip(1)
        .collect();
    if !wsl_args.is_empty() {
        return execute_wsl(wsl_args);
    }
    // start Windows GUI
    gui::start_gui()
}

fn execute_wsl(args: Vec<OsString>) -> Result<(), Error> {
    // convert args to paths, canonicalize when possible
    let paths: Vec<PathBuf> = args
        .iter()
        .map(PathBuf::from)
        .map(|p| p.canonicalize().unwrap_or(p))
        .collect();
    // convert paths to WSL equivalents
    let wsl_paths = wsl::paths_to_wsl(&paths)?;
    wsl::run_wsl(&wsl_paths[0], &wsl_paths[1..])
}
