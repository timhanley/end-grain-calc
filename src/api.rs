//! Answers the designer's requests. It knows nothing about how a request
//! arrived, so the app window and the local web server share it unchanged.

use crate::model::{MillCatalogue, Project, Species};
use crate::store::{is_slug, Prefs, SaveError, Store};
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::path::PathBuf;

const INDEX_HTML: &str = include_str!("../ui/index.html");
const APP_CSS: &str = include_str!("../ui/app.css");
const APP_JS: &str = include_str!("../ui/app.js");

/// Biggest price list that will be read, in bytes.
const MAX_PRICE_LIST: usize = 2_000_000;

pub struct Reply {
    pub status: u16,
    pub content_type: &'static str,
    pub body: Vec<u8>,
}

pub struct App {
    pub store: Store,
    /// Serve the ui/ folder from disk instead of the built-in copy.
    pub dev: bool,
    /// Print each request and its status to the terminal.
    pub trace: bool,
}

#[derive(Deserialize)]
struct SaveRequest {
    project: Project,
    previous: Option<String>,
    #[serde(default)]
    overwrite: bool,
}

fn send_json<T: Serialize>(status: u16, value: &T) -> Reply {
    match serde_json::to_vec(value) {
        Ok(body) => Reply {
            status,
            content_type: "application/json",
            body,
        },
        Err(e) => fail(500, &e.to_string()),
    }
}

fn fail(status: u16, message: &str) -> Reply {
    Reply {
        status,
        content_type: "application/json",
        body: json!({ "error": message }).to_string().into_bytes(),
    }
}

impl App {
    fn asset(&self, file: &str, built_in: &str, content_type: &'static str) -> Reply {
        let body = if self.dev {
            std::fs::read_to_string(PathBuf::from("ui").join(file))
                .unwrap_or_else(|_| built_in.to_string())
        } else {
            built_in.to_string()
        };
        Reply {
            status: 200,
            content_type,
            body: body.into_bytes(),
        }
    }

    /// `method` is an upper-case HTTP verb and `path` has no query string.
    pub fn handle(&self, method: &str, path: &str, body: &str) -> Reply {
        let reply = self.route(method, path, body);
        if self.trace {
            eprintln!("{method} {path} -> {}", reply.status);
        }
        reply
    }

    fn route(&self, method: &str, path: &str, body: &str) -> Reply {
        let store = &self.store;
        match (method, path) {
            ("GET", "/") | ("GET", "/index.html") => {
                self.asset("index.html", INDEX_HTML, "text/html; charset=utf-8")
            }
            ("GET", "/app.css") => self.asset("app.css", APP_CSS, "text/css; charset=utf-8"),
            ("GET", "/app.js") => self.asset("app.js", APP_JS, "text/javascript; charset=utf-8"),

            ("GET", "/api/info") => send_json(
                200,
                &json!({ "data_dir": store.dir.display().to_string(), "version": env!("CARGO_PKG_VERSION") }),
            ),

            ("POST", "/api/calc") => match crate::calculate_json(body) {
                Ok(result) => Reply {
                    status: 200,
                    content_type: "application/json",
                    body: result.into_bytes(),
                },
                Err(e) => fail(400, &e),
            },

            ("GET", "/api/library") => send_json(200, &store.library()),
            ("GET", "/api/prefs") => send_json(200, &store.prefs()),
            ("PUT", "/api/prefs") => match serde_json::from_str::<Prefs>(body) {
                Ok(prefs) => {
                    let prefs = prefs.cleaned();
                    match store.save_prefs(&prefs) {
                        Ok(()) => send_json(200, &prefs),
                        Err(e) => fail(500, &format!("Couldn't save your preferences: {e}")),
                    }
                }
                Err(e) => fail(400, &format!("Couldn't read the preferences: {e}")),
            },
            // Your mill's price list, or null if none has been imported.
            ("GET", "/api/mill") => send_json(200, &store.mill()),
            ("PUT", "/api/mill") if body.len() > MAX_PRICE_LIST => {
                fail(413, "That file is too big to be a price list.")
            }
            ("PUT", "/api/mill") => {
                let list = serde_json::from_str::<MillCatalogue>(body)
                    .map_err(|e| e.to_string())
                    .and_then(MillCatalogue::cleaned);
                match list {
                    Ok(list) => match store.save_mill(&list) {
                        Ok(()) => send_json(200, &list),
                        Err(e) => fail(500, &format!("Couldn't save the price list: {e}")),
                    },
                    Err(e) => fail(
                        400,
                        &format!("That isn't a price list this app can read: {e}."),
                    ),
                }
            }
            ("PUT", "/api/library") => match serde_json::from_str::<Vec<Species>>(body) {
                Ok(lib) => match store.save_library(&lib) {
                    Ok(()) => send_json(200, &json!({ "ok": true })),
                    Err(e) => fail(500, &format!("Couldn't save the timber list: {e}")),
                },
                Err(e) => fail(400, &format!("Couldn't read the timber list: {e}")),
            },

            ("GET", "/api/boards") => send_json(200, &store.list()),
            ("POST", "/api/boards") => match serde_json::from_str::<SaveRequest>(body) {
                Ok(r) if r.project.name.trim().is_empty() => {
                    fail(400, "Name the board to save it.")
                }
                Ok(r) => match store.save(&r.project, r.previous.as_deref(), r.overwrite) {
                    Ok(slug) => send_json(200, &json!({ "slug": slug })),
                    Err(SaveError::Exists(slug)) => {
                        send_json(409, &json!({ "error": "exists", "slug": slug }))
                    }
                    Err(SaveError::Io(e)) => fail(500, &format!("Couldn't save the board: {e}")),
                },
                Err(e) => fail(400, &format!("Couldn't read the board: {e}")),
            },

            (m, p) if p.starts_with("/api/boards/") => {
                let slug = &p["/api/boards/".len()..];
                if !is_slug(slug) {
                    return fail(404, "No such board");
                }
                match m {
                    "GET" => match store.load(slug) {
                        Some(project) => send_json(200, &project),
                        None => fail(404, "No such board"),
                    },
                    "DELETE" => match store.delete(slug) {
                        Ok(()) => send_json(200, &json!({ "ok": true })),
                        Err(_) => fail(404, "No such board"),
                    },
                    _ => fail(405, "Not allowed"),
                }
            }

            _ => fail(404, "Not found"),
        }
    }
}
