use std::path::{Path, PathBuf};
use std::sync::atomic::Ordering;
use std::sync::mpsc;
use std::thread;
use winapi::shared::windef;
use winapi::um::winuser;
use wslscript_common::error::*;
use wslscript_common::wsl;

use crate::progress::ProgressWindow;

mod interface;
mod progress;

/// Number of paths to convert without displaying a graphical progress indicator.
#[cfg(not(feature = "debug"))]
const CONVERT_WITH_PROGRESS_THRESHOLD: usize = 10;
#[cfg(feature = "debug")]
const CONVERT_WITH_PROGRESS_THRESHOLD: usize = 1;

/// Handle files dropped to registered filetype.
///
/// See: https://docs.microsoft.com/en-us/windows/win32/api/oleidl/nf-oleidl-idroptarget-drop
fn handle_dropped_files(
    target: PathBuf,
    mut paths: Vec<PathBuf>,
    key_state: interface::KeyState,
) -> Result<(), Error> {
    log::debug!(
        "Dropped {} items to {} with keys {:?}",
        paths.len(),
        target.to_string_lossy(),
        key_state
    );
    let opts = get_wsl_options(&target)?;
    paths.insert(0, target);
    // increment thread counter
    interface::THREAD_COUNTER.fetch_add(1, Ordering::SeqCst);
    // move further processing to thread
    thread::spawn(move || {
        log::debug!("Spawned thread to invoke WSL");
        if let Err(e) = run_wsl(paths, opts) {
            log::error!("Failed to invoke WSL: {}", e);
        }
        // Decrement counter when thread finishes. Here all moved variables
        // (paths and opts) have already been dropped, so DLL may be safely unloaded.
        interface::THREAD_COUNTER.fetch_sub(1, Ordering::SeqCst);
    });
    Ok(())
}

/// Invoke WSL with given path arguments.
///
/// Paths are in Win32 context.
fn run_wsl(win_paths: Vec<PathBuf>, opts: wsl::WSLOptions) -> Result<(), Error> {
    let wsl_paths = if win_paths.len() > CONVERT_WITH_PROGRESS_THRESHOLD {
        convert_paths_with_progress(win_paths, &opts)?
    } else {
        wsl::paths_to_wsl(&win_paths, &opts, None)?
    };
    wsl::run_wsl(&wsl_paths[0], &wsl_paths[1..], &opts)
}

/// Wrapped progress window handle.
struct ProgressWindowHandle(windef::HWND);
/// Window handles are safe to send across threads.
unsafe impl Send for ProgressWindowHandle {}

/// Convert paths to WSL context with a graphical progress indicator.
fn convert_paths_with_progress(
    win_paths: Vec<PathBuf>,
    opts: &wsl::WSLOptions,
) -> Result<Vec<PathBuf>, Error> {
    let path_count = win_paths.len();
    // channel to transfer current progress as in number of paths converted
    let (tx_progress, rx_progress) = mpsc::channel::<usize>();
    // channel to signal cancellation
    let (tx_cancel, rx_cancel) = mpsc::channel::<()>();
    // wait for progress updates in a seperate thread
    let progress_joiner = thread::spawn(move || {
        // channel to transfer progress window handle to this thread
        let (tx_hwnd, rx_hwnd) = mpsc::channel::<ProgressWindowHandle>();
        // run window in a seperate thread
        let window_joiner = thread::spawn(move || {
            let wnd = match ProgressWindow::new(path_count, tx_cancel) {
                Ok(wnd) => wnd,
                Err(e) => {
                    log::error!("Failed to create progress window: {}", e);
                    return;
                }
            };
            // send window handle to parent thread
            if tx_hwnd
                .send(ProgressWindowHandle { 0: wnd.handle() })
                .is_err()
            {
                log::error!("Failed to send progress window handle to parent thread");
                wnd.close();
            }
            drop(tx_hwnd);
            // run message loop
            if let Err(e) = wnd.run() {
                log::error!("Window thread returned error: {}", e);
            }
        });
        // wait for progress window handle
        let hwnd = match rx_hwnd.recv() {
            Ok(h) => h.0,
            Err(_) => {
                log::error!("Failed to receive progress window handle");
                return;
            }
        };
        drop(rx_hwnd);
        // post progress to window
        let update_progress = |n: usize| {
            // post WM_PROGRESS message to window's queue
            unsafe { winuser::PostMessageW(hwnd, progress::WM_PROGRESS, n, path_count as _) };
        };
        // blocking receive progress updates
        while let Ok(count) = rx_progress.recv() {
            update_progress(count);
        }
        // flush remaining messages
        while let Ok(count) = rx_progress.try_recv() {
            update_progress(count);
        }
        // close progress window
        unsafe { winuser::PostMessageW(hwnd, winuser::WM_CLOSE, 0, 0) };
        // wait for window to be destroyed
        window_joiner.join().unwrap_or_else(|_| {
            log::error!("Progress window thread panicked");
        });
    });
    // convert paths and send progress via channel
    let result = wsl::paths_to_wsl(
        &win_paths,
        &opts,
        Some(Box::new(move |count| {
            // if conversion was cancelled
            if rx_cancel.try_recv().is_ok() {
                return false;
            }
            tx_progress.send(count).unwrap_or_else(|_| {
                log::error!("Failed to communicate with channel");
            });
            // artificial delay while developing
            #[cfg(feature = "debug")]
            std::thread::sleep(std::time::Duration::from_secs(1));
            true
        })),
    );
    // wait for progress thread to finish
    progress_joiner.join().unwrap_or_else(|_| {
        log::error!("Path conversion progress thread panicked");
    });
    result
}

/// Get WSL options from registry based on given filename's extension.
fn get_wsl_options(path: &Path) -> Result<wsl::WSLOptions, Error> {
    path.extension()
        .ok_or_else(|| Error::DropHandlerError("No filename extension".to_owned()))
        .and_then(|s| {
            wsl::WSLOptions::from_ext(&s.to_string_lossy()).ok_or_else(|| {
                Error::DropHandlerError(format!(
                    "Extension {} not registered.",
                    s.to_string_lossy()
                ))
            })
        })
}
