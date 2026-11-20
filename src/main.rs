//! End grain chopping board calculator: the desktop program.
//!
//! By default it opens the designer in its own window. With `--browser` it
//! serves the same designer on 127.0.0.1 for use in a web browser.

// On Windows the app with a window is not a console program, so that opening
// it does not bring up a black console window as well.
#![cfg_attr(all(windows, feature = "window"), windows_subsystem = "windows")]

mod server;
#[cfg(feature = "window")]
mod window;

use endgrain::api::App;
use endgrain::store::Store;
use std::path::PathBuf;

const HELP: &str = "\
endgrain - End Grain Calc, an end grain chopping board calculator

Usage: endgrain [options]

  --browser      Run in your web browser instead of the app's own window
  --port <n>     Browser mode: port to serve on (default 7878, or the next free one)
  --no-open      Browser mode: don't open the browser
  --data <dir>   Where boards and the timber list are kept (default ~/.endgrain)
  --dev          Load the ui/ folder from disk instead of the built-in copy
  --trace        Print each request the designer makes
  -h, --help     Show this help
";

struct Config {
    browser: bool,
    port: u16,
    open: bool,
    data: PathBuf,
    dev: bool,
    trace: bool,
}

fn parse_args() -> Config {
    let home = std::env::var_os("HOME").or_else(|| std::env::var_os("USERPROFILE"));
    let mut cfg = Config {
        browser: !cfg!(feature = "window"),
        port: 7878,
        open: true,
        data: home.map_or_else(
            || PathBuf::from(".endgrain"),
            |h| PathBuf::from(h).join(".endgrain"),
        ),
        dev: false,
        trace: false,
    };
    let fail = |msg: String| -> ! {
        eprintln!("{msg}\n\n{HELP}");
        std::process::exit(2)
    };
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--browser" => cfg.browser = true,
            "--port" => {
                cfg.browser = true;
                cfg.port = args
                    .next()
                    .and_then(|v| v.parse().ok())
                    .unwrap_or_else(|| fail("--port needs a number".into()))
            }
            "--no-open" => {
                cfg.browser = true;
                cfg.open = false
            }
            "--data" => {
                cfg.data = args
                    .next()
                    .map(PathBuf::from)
                    .unwrap_or_else(|| fail("--data needs a folder".into()))
            }
            "--dev" => cfg.dev = true,
            "--trace" => cfg.trace = true,
            "-h" | "--help" => {
                print!("{HELP}");
                std::process::exit(0)
            }
            // Finder can pass a process serial number when it launches an app.
            other if other.starts_with("-psn") => {}
            other => fail(format!("Unknown option: {other}")),
        }
    }
    cfg
}

/// Started from a terminal on Windows, borrow that terminal so the help and
/// any error messages have somewhere to go. Opened by double-click there is
/// none, and this does nothing.
#[cfg(all(windows, feature = "window"))]
fn use_parent_console() {
    #[link(name = "kernel32")]
    extern "system" {
        fn AttachConsole(process_id: u32) -> i32;
    }
    const PARENT_PROCESS: u32 = u32::MAX;
    // SAFETY: a plain Windows call with no pointers; failing is harmless.
    unsafe {
        AttachConsole(PARENT_PROCESS);
    }
}

/// Stop with a message. On Windows the app has no console to print to when
/// it is opened by double-click, so the message is shown in a box as well.
pub fn fatal(message: &str) -> ! {
    eprintln!("{message}");
    #[cfg(all(windows, feature = "window"))]
    {
        #[link(name = "user32")]
        extern "system" {
            fn MessageBoxW(
                window: *mut std::ffi::c_void,
                text: *const u16,
                caption: *const u16,
                kind: u32,
            ) -> i32;
        }
        const MB_ICONERROR: u32 = 0x10;
        let wide = |s: &str| s.encode_utf16().chain([0]).collect::<Vec<u16>>();
        let (text, caption) = (wide(message), wide("End Grain Calc"));
        // SAFETY: both strings are NUL-terminated and outlive the call.
        unsafe {
            MessageBoxW(
                std::ptr::null_mut(),
                text.as_ptr(),
                caption.as_ptr(),
                MB_ICONERROR,
            );
        }
    }
    std::process::exit(1)
}

fn main() {
    #[cfg(all(windows, feature = "window"))]
    use_parent_console();
    let cfg = parse_args();
    let store = Store::open(cfg.data.clone()).unwrap_or_else(|e| {
        fatal(&format!(
            "Couldn't use {} for saved boards: {e}",
            cfg.data.display()
        ))
    });
    let app = App {
        store,
        dev: cfg.dev,
        trace: cfg.trace,
    };

    #[cfg(feature = "window")]
    if !cfg.browser {
        window::run(app)
    }
    server::serve(app, cfg.port, cfg.open)
}
