//! Browser mode: serve the designer on 127.0.0.1 and open it in a browser.

use endgrain::api::{App, Reply};
use std::io::Read;
use std::process::Command;
use tiny_http::{Header, Method, Request, Response, Server};

/// Open an address in the computer's usual web browser.
pub fn open_browser(url: &str) {
    #[cfg(target_os = "macos")]
    let result = Command::new("open").arg(url).spawn();
    #[cfg(target_os = "windows")]
    let result = {
        use std::os::windows::process::CommandExt;
        // Without this flag a console window flashes up behind the app.
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        Command::new("cmd")
            .args(["/C", "start", "", url])
            .creation_flags(CREATE_NO_WINDOW)
            .spawn()
    };
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    let result = Command::new("xdg-open").arg(url).spawn();
    if result.is_err() {
        eprintln!("Couldn't open a browser. Go to {url}");
    }
}

fn refuse(status: u16, message: &str) -> Reply {
    Reply {
        status,
        content_type: "application/json",
        body: format!("{{\"error\":\"{message}\"}}").into_bytes(),
    }
}

fn answer(req: &mut Request, app: &App, port: u16) -> Reply {
    let header = |name: &'static str| {
        req.headers()
            .iter()
            .find(|h| h.field.equiv(name))
            .map(|h| h.value.as_str().to_string())
    };
    let (host, origin, content_type) = (header("Host"), header("Origin"), header("Content-Type"));
    let method = req.method().clone();
    let path = req.url().split('?').next().unwrap_or("/").to_string();

    // Only this machine's own pages may talk to the server: refuse requests
    // aimed at another host name or sent by another site's page.
    let hosts = [format!("127.0.0.1:{port}"), format!("localhost:{port}")];
    if !host.is_some_and(|h| hosts.contains(&h)) {
        return refuse(403, "Unexpected host");
    }
    if method != Method::Get {
        if let Some(origin) = origin {
            if !hosts.iter().any(|h| origin == format!("http://{h}")) {
                return refuse(403, "Unexpected origin");
            }
        }
    }

    let mut body = String::new();
    if matches!(method, Method::Post | Method::Put) {
        if !content_type.is_some_and(|c| c.starts_with("application/json")) {
            return refuse(415, "Send JSON");
        }
        if req
            .as_reader()
            .take(4 << 20)
            .read_to_string(&mut body)
            .is_err()
        {
            return refuse(400, "Couldn't read the request");
        }
    }
    app.handle(method.as_str(), &path, &body)
}

pub fn serve(app: App, first_port: u16, open: bool) -> ! {
    let bound = (first_port..=first_port.saturating_add(20))
        .find_map(|port| Server::http(("127.0.0.1", port)).ok().map(|s| (s, port)));
    let Some((server, port)) = bound else {
        eprintln!("Couldn't find a free port from {first_port} up. Try --port.");
        std::process::exit(1)
    };

    let url = format!("http://127.0.0.1:{port}/");
    println!("End Grain Calc is running at {url}");
    println!("Boards are saved in {}", app.store.dir.display());
    println!("Press Ctrl+C to stop.");
    if open {
        open_browser(&url);
    }

    for mut req in server.incoming_requests() {
        let reply = answer(&mut req, &app, port);
        let response = Response::from_data(reply.body)
            .with_status_code(reply.status)
            .with_header(Header::from_bytes("Content-Type", reply.content_type).unwrap())
            .with_header(Header::from_bytes("Cache-Control", "no-store").unwrap());
        let _ = req.respond(response);
    }
    std::process::exit(0)
}
