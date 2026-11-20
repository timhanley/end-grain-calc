//! Saved boards and the timber list, kept as plain JSON files.

use crate::model::{default_library, MillCatalogue, OrderBy, Project, Species, Units};
use serde::{Deserialize, Serialize};
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::time::UNIX_EPOCH;

/// Name of the refreshable price list in the data folder.
pub const MILL_FILE: &str = "mill-prices.json";

/// Preferences that apply to every board, and the choices new boards
/// start with. They are asked for once, on first run, and can be changed
/// under Settings.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct Prefs {
    /// Symbol shown in front of every price.
    pub currency: String,
    /// Units a new board starts in.
    pub units: Units,
    /// How a new board's order is written: the way your mill sells timber.
    pub order_by: OrderBy,
    /// Sales tax a new board adds to the mill's prices, in percent.
    pub tax_pct: f64,
    /// False until the first-run setup has been finished or skipped.
    pub setup_done: bool,
    /// Ask GitHub at start-up whether a newer version is out.
    pub check_updates: bool,
}

impl Default for Prefs {
    fn default() -> Self {
        Prefs {
            currency: "\u{a3}".into(),
            units: Units::Mm,
            order_by: OrderBy::Bf,
            tax_pct: 0.0,
            setup_done: false,
            check_updates: true,
        }
    }
}

impl Prefs {
    /// A symbol is at most four characters, and an empty one falls back to
    /// the default; tax is kept between 0 and 100 percent.
    pub fn cleaned(mut self) -> Prefs {
        self.currency = self.currency.trim().chars().take(4).collect();
        if self.currency.is_empty() {
            self.currency = Prefs::default().currency;
        }
        if !(self.tax_pct.is_finite() && (0.0..=100.0).contains(&self.tax_pct)) {
            self.tax_pct = 0.0;
        }
        self
    }
}

pub struct Store {
    pub dir: PathBuf,
}

#[derive(Serialize)]
pub struct BoardSummary {
    pub slug: String,
    pub name: String,
    pub units: Units,
    pub width: f64,
    pub depth: f64,
    pub thickness: f64,
    /// Seconds since 1970.
    pub modified: u64,
}

pub enum SaveError {
    /// Another board already uses this name.
    Exists(String),
    Io(io::Error),
}

/// File-safe version of a board name: lowercase letters, digits and dashes.
pub fn slugify(name: &str) -> String {
    let mut slug = String::new();
    for ch in name.trim().chars() {
        if ch.is_ascii_alphanumeric() {
            slug.push(ch.to_ascii_lowercase());
        } else if !slug.ends_with('-') {
            slug.push('-');
        }
    }
    let slug: String = slug.trim_matches('-').chars().take(80).collect();
    if slug.is_empty() {
        "board".into()
    } else {
        slug
    }
}

pub fn is_slug(s: &str) -> bool {
    !s.is_empty()
        && s.len() <= 80
        && s.bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
}

fn write_atomic(path: &Path, text: &str) -> io::Result<()> {
    let tmp = path.with_extension("json.tmp");
    fs::write(&tmp, text)?;
    fs::rename(&tmp, path)
}

impl Store {
    pub fn open(dir: PathBuf) -> io::Result<Store> {
        fs::create_dir_all(dir.join("boards"))?;
        Ok(Store { dir })
    }

    fn board_path(&self, slug: &str) -> PathBuf {
        self.dir.join("boards").join(format!("{slug}.json"))
    }

    pub fn list(&self) -> Vec<BoardSummary> {
        let mut out = Vec::new();
        let Ok(entries) = fs::read_dir(self.dir.join("boards")) else {
            return out;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            let Some(slug) = path
                .file_stem()
                .and_then(|s| s.to_str())
                .map(str::to_string)
            else {
                continue;
            };
            if path.extension().and_then(|e| e.to_str()) != Some("json") || !is_slug(&slug) {
                continue;
            }
            let Some(p) = fs::read_to_string(&path)
                .ok()
                .and_then(|t| serde_json::from_str::<Project>(&t).ok())
            else {
                continue;
            };
            let modified = entry
                .metadata()
                .and_then(|m| m.modified())
                .ok()
                .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
                .map_or(0, |d| d.as_secs());
            out.push(BoardSummary {
                slug,
                name: p.name,
                units: p.units,
                width: p.width,
                depth: p.depth,
                thickness: p.thickness,
                modified,
            });
        }
        out.sort_by(|a, b| b.modified.cmp(&a.modified).then(a.name.cmp(&b.name)));
        out
    }

    pub fn load(&self, slug: &str) -> Option<Project> {
        let text = fs::read_to_string(self.board_path(slug)).ok()?;
        serde_json::from_str(&text).ok()
    }

    /// Save under the board's name. `previous` is the file this board was
    /// loaded from, if any: saving under a new name renames it.
    pub fn save(
        &self,
        project: &Project,
        previous: Option<&str>,
        overwrite: bool,
    ) -> Result<String, SaveError> {
        let slug = slugify(&project.name);
        let path = self.board_path(&slug);
        if path.exists() && previous != Some(slug.as_str()) && !overwrite {
            return Err(SaveError::Exists(slug));
        }
        let text = serde_json::to_string_pretty(project)
            .map_err(|e| SaveError::Io(io::Error::other(e)))?;
        write_atomic(&path, &text).map_err(SaveError::Io)?;
        if let Some(prev) = previous {
            if prev != slug && is_slug(prev) {
                let _ = fs::remove_file(self.board_path(prev));
            }
        }
        Ok(slug)
    }

    pub fn delete(&self, slug: &str) -> io::Result<()> {
        fs::remove_file(self.board_path(slug))
    }

    pub fn library(&self) -> Vec<Species> {
        fs::read_to_string(self.dir.join("timber.json"))
            .ok()
            .and_then(|t| serde_json::from_str(&t).ok())
            .unwrap_or_else(default_library)
    }

    /// Your mill's price list, if you have imported one (or put a
    /// `mill-prices.json` in the data folder yourself). The app has none of
    /// its own.
    pub fn mill(&self) -> Option<MillCatalogue> {
        let text = fs::read_to_string(self.dir.join(MILL_FILE)).ok()?;
        serde_json::from_str::<MillCatalogue>(&text)
            .ok()?
            .cleaned()
            .ok()
    }

    pub fn save_mill(&self, mill: &MillCatalogue) -> io::Result<()> {
        let text = serde_json::to_string_pretty(mill).map_err(io::Error::other)?;
        write_atomic(&self.dir.join(MILL_FILE), &text)
    }

    pub fn prefs(&self) -> Prefs {
        fs::read_to_string(self.dir.join("prefs.json"))
            .ok()
            .and_then(|t| serde_json::from_str::<Prefs>(&t).ok())
            .unwrap_or_default()
            .cleaned()
    }

    pub fn save_prefs(&self, prefs: &Prefs) -> io::Result<()> {
        let text = serde_json::to_string_pretty(prefs).map_err(io::Error::other)?;
        write_atomic(&self.dir.join("prefs.json"), &text)
    }

    pub fn save_library(&self, lib: &[Species]) -> io::Result<()> {
        let text = serde_json::to_string_pretty(lib).map_err(io::Error::other)?;
        write_atomic(&self.dir.join("timber.json"), &text)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_become_safe_file_names() {
        assert_eq!(slugify("Walnut & Cherry stripes"), "walnut-cherry-stripes");
        assert_eq!(slugify("  ../../etc/passwd "), "etc-passwd");
        assert_eq!(slugify("***"), "board");
        assert!(is_slug("walnut-cherry-2"));
        assert!(!is_slug("../x"));
        assert!(!is_slug(""));
    }

    #[test]
    fn the_currency_symbol_is_remembered_and_kept_short() {
        let dir = std::env::temp_dir().join(format!("endgrain-prefs-{}", std::process::id()));
        let store = Store::open(dir.clone()).unwrap();
        assert_eq!(store.prefs().currency, "\u{a3}");
        let with = |currency: &str| Prefs {
            currency: currency.into(),
            ..Prefs::default()
        };
        store.save_prefs(&with("  kr ").cleaned()).unwrap();
        assert_eq!(store.prefs().currency, "kr");
        assert_eq!(with("dollars").cleaned().currency, "doll");
        assert_eq!(with("   ").cleaned().currency, "\u{a3}");
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn setup_choices_are_remembered_and_older_files_still_read() {
        let dir = std::env::temp_dir().join(format!("endgrain-setup-{}", std::process::id()));
        let store = Store::open(dir.clone()).unwrap();
        // A new installation: millimetres, no tax, setup still to do.
        let fresh = store.prefs();
        assert!(!fresh.setup_done && fresh.units == Units::Mm && fresh.tax_pct == 0.0);

        // A file from before setup existed has only the currency.
        fs::write(dir.join("prefs.json"), r#"{ "currency": "$" }"#).unwrap();
        let old = store.prefs();
        assert!(old.currency == "$" && !old.setup_done && old.order_by == OrderBy::Bf);
        assert!(old.check_updates, "older files check for new versions");

        let chosen = Prefs {
            currency: "$".into(),
            units: Units::In,
            order_by: OrderBy::Sawn,
            tax_pct: 250.0,
            setup_done: true,
            check_updates: false,
        }
        .cleaned();
        store.save_prefs(&chosen).unwrap();
        let back = store.prefs();
        assert!(back.setup_done && back.units == Units::In && back.order_by == OrderBy::Sawn);
        assert!(!back.check_updates);
        assert_eq!(back.tax_pct, 0.0, "tax outside 0 to 100 is dropped");
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn there_is_no_price_list_until_one_is_imported() {
        let dir = std::env::temp_dir().join(format!("endgrain-test-{}", std::process::id()));
        let store = Store::open(dir.clone()).unwrap();

        // Nothing there: no list, and starter timbers with no prices.
        assert!(store.mill().is_none());
        assert!(store
            .library()
            .iter()
            .all(|t| t.price_per_bf == 0.0 && t.sawn.is_empty()));

        // An imported list is kept and read back.
        let list: MillCatalogue = serde_json::from_str(
            r#"{ "mill": "Hill Sawmill", "fetched": "2026-11-12", "step_m": 0.5, "timbers": [
                { "name": "Ash", "sizes": [{ "thickness": 27, "width": 150, "price_per_m": 9.5 }] }] }"#,
        )
        .unwrap();
        store.save_mill(&list.cleaned().unwrap()).unwrap();
        let back = store.mill().unwrap();
        assert_eq!(back.mill, "Hill Sawmill");
        assert_eq!(back.step_m, 0.5);
        assert_eq!(back.timbers[0].sizes[0].price_per_m, 9.5);
        // It does not touch the timber list: prices are copied in on request.
        assert!(store.library().iter().all(|t| t.sawn.is_empty()));

        // A file that can't be read, or has no prices, counts as no list.
        fs::write(dir.join(MILL_FILE), "not a price list").unwrap();
        assert!(store.mill().is_none());
        fs::write(dir.join(MILL_FILE), r#"{ "timbers": [] }"#).unwrap();
        assert!(store.mill().is_none());

        let _ = fs::remove_dir_all(dir);
    }
}
