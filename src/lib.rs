//! End grain chopping board calculator.
//!
//! The crate is layered so the same code can ship three ways:
//!
//! - [`model`] and [`calc`] are the engine: plain data in, plain data out, no
//!   files, network or windows. They build for any target, wasm32 included.
//! - [`store`] keeps boards as JSON files and [`api`] answers the designer's
//!   requests. They need a file system, so they are left out of wasm builds.
//! - The binary (`src/main.rs`) puts a transport in front of [`api`]: the
//!   app's own window, or a local web server for use in a browser.
//!
//! A hosted web version would compile the engine to WebAssembly, call
//! [`calculate_json`] directly, and keep boards in the browser's storage.

pub mod calc;
pub mod model;

#[cfg(not(target_arch = "wasm32"))]
pub mod api;
#[cfg(not(target_arch = "wasm32"))]
pub mod store;

use serde::Deserialize;

#[derive(Deserialize)]
struct CalcRequest {
    project: model::Project,
    library: Vec<model::Species>,
}

/// The whole engine behind one call: takes `{"project": ..., "library": [...]}`
/// as JSON and returns the result as JSON. Every front end goes through this.
pub fn calculate_json(request: &str) -> Result<String, String> {
    let req: CalcRequest =
        serde_json::from_str(request).map_err(|e| format!("Couldn't read the board: {e}"))?;
    serde_json::to_string(&calc::calculate(&req.project, &req.library)).map_err(|e| e.to_string())
}
