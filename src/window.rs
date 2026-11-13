//! App mode: the designer in its own window, with no server and no port.
//!
//! The page is the same one browser mode serves. Its requests go to a private
//! `endgrain://` address that the web view hands straight to [`App::handle`].
//! tao (window) and wry (web view) cover macOS, Windows and Linux; only the
//! menu bar below is macOS-specific.

use endgrain::api::App;
use std::borrow::Cow;
use tao::{
    dpi::LogicalSize,
    event::{Event, WindowEvent},
    event_loop::{ControlFlow, EventLoopBuilder},
    window::WindowBuilder,
};
use wry::{
    http::{
        header::{CACHE_CONTROL, CONTENT_TYPE},
        Response,
    },
    WebViewBuilder,
};

/// Windows' web view only allows custom addresses in this http form.
#[cfg(target_os = "windows")]
const START_URL: &str = "http://endgrain.localhost/";
#[cfg(not(target_os = "windows"))]
const START_URL: &str = "endgrain://localhost/";

/// Paper margins, in points (72 to the inch): about 14 mm all round.
#[cfg(target_os = "macos")]
const PAGE_MARGIN: f32 = 40.0;

/// Open the print dialog. On the Mac the web view prints with no margins at
/// all unless it is given some, which runs the lists off the paper's edge.
#[cfg(target_os = "macos")]
fn print(webview: &wry::WebView) -> wry::Result<()> {
    use wry::{PrintMargin, PrintOptions, WebViewExtMacOS};
    webview.print_with_options(&PrintOptions {
        margins: PrintMargin {
            top: PAGE_MARGIN,
            right: PAGE_MARGIN,
            bottom: PAGE_MARGIN,
            left: PAGE_MARGIN,
        },
    })
}

#[cfg(not(target_os = "macos"))]
fn print(webview: &wry::WebView) -> wry::Result<()> {
    webview.print()
}

/// Things the page (or the menu bar) asks the window to do. Two of them come
/// only from the menu bar, which only the Mac has.
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
enum Host {
    /// Print whatever the page has set up to print.
    Print,
    /// The menu's Print Lists: have the page set up the lists, then print.
    PrintLists,
    /// Close, unless the board has unsaved changes: then ask in the page.
    AskQuit,
    Quit,
    Dirty(bool),
    Title(String),
    /// Open the maker's website in the usual browser.
    OpenSite,
}

const MAKER_SITE: &str = "https://teajayhan.com";

#[cfg(target_os = "macos")]
fn mac_menu(proxy: tao::event_loop::EventLoopProxy<Host>) -> muda::Menu {
    use muda::{
        accelerator::{Accelerator, Code, CMD_OR_CTRL},
        Menu, MenuEvent, MenuItem, PredefinedMenuItem, Submenu,
    };
    let sep = PredefinedMenuItem::separator;
    let key = |code| Some(Accelerator::new(CMD_OR_CTRL, code));

    let app_menu = Submenu::with_items(
        "End Grain Calc",
        true,
        &[
            &PredefinedMenuItem::about(
                Some("About End Grain Calc"),
                Some(muda::AboutMetadata {
                    name: Some("End Grain Calc".into()),
                    version: Some(env!("CARGO_PKG_VERSION").into()),
                    copyright: Some(
                        "\u{a9} 2026 TEA JAY HAN. Shared under the MIT licence.".into(),
                    ),
                    credits: Some("Made with love for woodworkers. teajayhan.com".into()),
                    ..Default::default()
                }),
            ),
            &sep(),
            &PredefinedMenuItem::hide(None),
            &PredefinedMenuItem::hide_others(None),
            &PredefinedMenuItem::show_all(None),
            &sep(),
            &MenuItem::with_id("quit", "Quit End Grain Calc", true, key(Code::KeyQ)),
        ],
    );
    let file = Submenu::with_items(
        "File",
        true,
        &[
            &MenuItem::with_id("print", "Print Lists…", true, key(Code::KeyP)),
            &sep(),
            &PredefinedMenuItem::close_window(None),
        ],
    );
    // Without an Edit menu, copy and paste shortcuts don't reach text fields.
    let edit = Submenu::with_items(
        "Edit",
        true,
        &[
            &PredefinedMenuItem::undo(None),
            &PredefinedMenuItem::redo(None),
            &sep(),
            &PredefinedMenuItem::cut(None),
            &PredefinedMenuItem::copy(None),
            &PredefinedMenuItem::paste(None),
            &PredefinedMenuItem::select_all(None),
        ],
    );
    let window = Submenu::with_items(
        "Window",
        true,
        &[
            &PredefinedMenuItem::minimize(None),
            &PredefinedMenuItem::fullscreen(None),
        ],
    );

    let menu = Menu::new();
    if let (Ok(app_menu), Ok(file), Ok(edit), Ok(window)) = (app_menu, file, edit, window) {
        let _ = menu.append_items(&[&app_menu, &file, &edit, &window]);
        menu.init_for_nsapp();
        window.set_as_windows_menu_for_nsapp();
    }

    let proxy = std::sync::Mutex::new(proxy);
    MenuEvent::set_event_handler(Some(move |event: MenuEvent| {
        let host = if event.id == "quit" {
            Host::AskQuit
        } else if event.id == "print" {
            Host::PrintLists
        } else {
            return;
        };
        if let Ok(proxy) = proxy.lock() {
            let _ = proxy.send_event(host);
        }
    }));
    menu
}

pub fn run(app: App) -> ! {
    let event_loop = EventLoopBuilder::<Host>::with_user_event().build();
    let window = WindowBuilder::new()
        .with_title("End Grain Calc")
        .with_inner_size(LogicalSize::new(1480.0, 940.0))
        .with_min_inner_size(LogicalSize::new(860.0, 600.0))
        .build(&event_loop)
        .unwrap_or_else(|e| {
            eprintln!("Couldn't open a window: {e}. Try --browser.");
            std::process::exit(1)
        });

    #[cfg(target_os = "macos")]
    let _menu = mac_menu(event_loop.create_proxy());

    let from_page = event_loop.create_proxy();
    let builder = WebViewBuilder::new()
        .with_custom_protocol("endgrain".into(), move |_id, request| {
            let body = String::from_utf8_lossy(request.body());
            let reply = app.handle(request.method().as_str(), request.uri().path(), &body);
            Response::builder()
                .status(reply.status)
                .header(CONTENT_TYPE, reply.content_type)
                .header(CACHE_CONTROL, "no-store")
                .body(Cow::Owned(reply.body))
                .unwrap_or_else(|_| Response::new(Cow::Borrowed(&[][..])))
        })
        .with_ipc_handler(move |message| {
            let host = match message.body().as_str() {
                "print" => Host::Print,
                "quit" => Host::Quit,
                "dirty:1" => Host::Dirty(true),
                "dirty:0" => Host::Dirty(false),
                "open-site" => Host::OpenSite,
                other => match other.strip_prefix("title:") {
                    Some(title) => Host::Title(title.to_string()),
                    None => return,
                },
            };
            let _ = from_page.send_event(host);
        })
        .with_url(START_URL);

    #[cfg(any(target_os = "windows", target_os = "macos"))]
    let built = builder.build(&window);
    #[cfg(not(any(target_os = "windows", target_os = "macos")))]
    let built = {
        use tao::platform::unix::WindowExtUnix;
        use wry::WebViewBuilderExtUnix;
        builder.build_gtk(window.default_vbox().expect("window has a GTK box"))
    };
    let webview = built.unwrap_or_else(|e| {
        eprintln!("Couldn't start the web view: {e}. Try --browser.");
        std::process::exit(1)
    });

    let mut dirty = false;
    event_loop.run(move |event, _, control_flow| {
        *control_flow = ControlFlow::Wait;
        match event {
            Event::WindowEvent {
                event: WindowEvent::CloseRequested,
                ..
            }
            | Event::UserEvent(Host::AskQuit) => {
                // With unsaved changes the page asks what to do and answers
                // with "quit". If it can't be asked, just close.
                if !dirty
                    || webview
                        .evaluate_script("window.endgrainConfirmQuit()")
                        .is_err()
                {
                    *control_flow = ControlFlow::Exit;
                }
            }
            Event::UserEvent(Host::Quit) => *control_flow = ControlFlow::Exit,
            Event::UserEvent(Host::Dirty(now)) => dirty = now,
            Event::UserEvent(Host::Title(title)) => window.set_title(&title),
            Event::UserEvent(Host::OpenSite) => crate::server::open_browser(MAKER_SITE),
            Event::UserEvent(Host::PrintLists) => {
                let _ = webview.evaluate_script("window.endgrainPrintLists()");
            }
            Event::UserEvent(Host::Print) => {
                let _ = print(&webview);
            }
            _ => {}
        }
    })
}
