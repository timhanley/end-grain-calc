//! Data that is saved to disk and sent between the browser and the engine.
//! Every dimension is in the project's own units (inches or millimetres);
//! board feet are always board feet.

use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Units {
    #[default]
    In,
    Mm,
}

impl Units {
    pub fn to_inches(self, v: f64) -> f64 {
        match self {
            Units::In => v,
            Units::Mm => v / 25.4,
        }
    }
}

/// Which way the first glue-up is crosscut. `Cols` means every column of the
/// finished board is one slice (so the strips run along the rows).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SliceAxis {
    #[default]
    Auto,
    Cols,
    Rows,
}

/// How the order list is written: by the board foot, or as the fixed sawn
/// sections a mill sells by the metre.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum OrderBy {
    #[default]
    Bf,
    Sawn,
}

/// One row or column. `size: None` means "share whatever is left over".
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Track {
    #[serde(default)]
    pub size: Option<f64>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    /// Saw blade kerf, lost at every rip and crosscut.
    pub kerf: f64,
    /// Extra slice length for flattening the finished end grain board.
    pub board_flatten: f64,
    /// Extra strip thickness for flattening the first glue-up.
    pub panel_flatten: f64,
    /// Extra width on each outer strip for squaring the finished board.
    pub edge_trim: f64,
    /// Spare panel length for a squaring cut and a safe last slice.
    pub end_waste: f64,
    /// Strips are never shorter than this (planer / jointer safety).
    pub min_strip_length: f64,
    /// Rough sawn allowances: what milling square takes off.
    pub rough_thickness: f64,
    pub rough_width: f64,
    pub rough_length: f64,
    /// Extra to order for knots, checks and sapwood, in percent.
    pub waste_pct: f64,
    /// Longest piece of timber you can carry home. Zero means no limit.
    /// Glue-ups are split so no strip, and no board on the order, is longer.
    pub max_length: f64,
    /// Sawn sizes: the mill sells length in steps of this many metres.
    pub sawn_step: f64,
    /// Sawn sizes: sales tax (VAT, GST...) added to the mill's prices, in
    /// percent. The field keeps its first name so saved boards still load.
    pub vat_pct: f64,
    /// Sawn sizes: avoid ripping by giving a strip a whole board to itself,
    /// as long as no more than `sawn_whole_waste` percent of that board
    /// would be planed away.
    pub sawn_whole: bool,
    pub sawn_whole_waste: f64,
    /// Sawn sizes: order strips that are too big for every sawn size of
    /// their timber by the board foot, instead of leaving them off.
    pub sawn_big_as_bf: bool,
    /// The mill this board is ordered from (its id), if one was chosen.
    /// The calculation itself never looks at it: the page sends the prices
    /// and step of the chosen mill.
    #[serde(skip_serializing_if = "String::is_empty")]
    pub mill: String,
}

impl Default for Settings {
    fn default() -> Self {
        Settings {
            kerf: 0.125,
            board_flatten: 0.125,
            panel_flatten: 0.0625,
            edge_trim: 0.0,
            end_waste: 2.0,
            min_strip_length: 12.0,
            rough_thickness: 0.25,
            rough_width: 0.25,
            rough_length: 1.0,
            waste_pct: 20.0,
            max_length: 0.0,
            sawn_step: 0.3,
            vat_pct: 20.0,
            sawn_whole: false,
            sawn_whole_waste: 50.0,
            sawn_big_as_bf: false,
            mill: String::new(),
        }
    }
}

impl Settings {
    /// For boards in millimetres: every allowance that ends up in a size
    /// you cut or set is taken to the nearest whole millimetre (and never
    /// rounded away to nothing), so no list asks for part of one. The saw
    /// kerf is left alone: that is a property of the blade.
    pub fn whole_mm(&self) -> Settings {
        let w = |v: f64| if v > 0.0 { v.round().max(1.0) } else { 0.0 };
        Settings {
            board_flatten: w(self.board_flatten),
            panel_flatten: w(self.panel_flatten),
            edge_trim: w(self.edge_trim),
            end_waste: w(self.end_waste),
            min_strip_length: w(self.min_strip_length),
            rough_thickness: w(self.rough_thickness),
            rough_width: w(self.rough_width),
            rough_length: w(self.rough_length),
            max_length: w(self.max_length),
            ..self.clone()
        }
    }

    /// Negative allowances make no sense; treat them as zero.
    pub fn sanitized(&self) -> Settings {
        let p = |v: f64| if v.is_finite() && v > 0.0 { v } else { 0.0 };
        Settings {
            kerf: p(self.kerf),
            board_flatten: p(self.board_flatten),
            panel_flatten: p(self.panel_flatten),
            edge_trim: p(self.edge_trim),
            end_waste: p(self.end_waste),
            min_strip_length: p(self.min_strip_length),
            rough_thickness: p(self.rough_thickness),
            rough_width: p(self.rough_width),
            rough_length: p(self.rough_length),
            waste_pct: p(self.waste_pct),
            max_length: p(self.max_length),
            sawn_step: p(self.sawn_step),
            vat_pct: p(self.vat_pct),
            sawn_whole: self.sawn_whole,
            sawn_whole_waste: p(self.sawn_whole_waste).min(100.0),
            sawn_big_as_bf: self.sawn_big_as_bf,
            mill: self.mill.clone(),
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Project {
    pub name: String,
    #[serde(default)]
    pub units: Units,
    /// Finished size across the columns (left to right).
    pub width: f64,
    /// Finished size down the rows (top to bottom).
    pub depth: f64,
    /// Finished board thickness (the end grain height).
    pub thickness: f64,
    pub cols: Vec<Track>,
    pub rows: Vec<Track>,
    /// Timber id for each cell, `cells[row][col]`.
    pub cells: Vec<Vec<String>>,
    #[serde(default)]
    pub slice_axis: SliceAxis,
    #[serde(default)]
    pub order_by: OrderBy,
    #[serde(default)]
    pub settings: Settings,
}

pub const MM_PER_INCH: f64 = 25.4;
pub const M_PER_FOOT: f64 = 0.3048;

/// The unit a mill gives the length of its sawn boards in.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Lengths {
    /// Metres.
    #[default]
    M,
    /// Feet.
    Ft,
    /// Inches, for boards sold in set lengths such as 36" or 48".
    In,
}

impl Lengths {
    pub fn metres(self, v: f64) -> f64 {
        match self {
            Lengths::M => v,
            Lengths::Ft => v * M_PER_FOOT,
            Lengths::In => v * MM_PER_INCH / 1000.0,
        }
    }
}

/// One length a mill stocks a sawn size in, already cut, with its price.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SetLength {
    /// The board's length in metres.
    #[serde(default)]
    pub length_m: f64,
    /// Price of one board this long, before tax.
    #[serde(default)]
    pub price: f64,
    /// The length as a price list gave it, in the list's own unit. It
    /// becomes `length_m` when the list is tidied, and is never written out.
    #[serde(default, skip_serializing)]
    pub length: f64,
}

/// A sawn section the mill sells by length, e.g. 50 x 150 mm or 1 x 6 in.
/// It is always kept in millimetres and per metre; the page shows it in the
/// mill's own units.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SawnSize {
    /// Nominal sawn thickness and width in millimetres.
    pub thickness: f64,
    pub width: f64,
    /// Price per metre of length, before tax.
    #[serde(default)]
    pub price_per_m: f64,
    /// A price per foot, read from a price list that gives one. It becomes
    /// `price_per_m` when the list is tidied, and is never written out.
    #[serde(default, skip_serializing)]
    pub price_per_ft: f64,
    /// For a mill that sells this size only in set lengths (36", 2.4 m...),
    /// each length it stocks and its price. When there are any, they are
    /// the price and `price_per_m` is not used.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub lengths: Vec<SetLength>,
}

impl SawnSize {
    /// The set lengths with a usable length and price, shortest first.
    pub fn stocked(&self) -> Vec<&SetLength> {
        let mut v: Vec<&SetLength> = self
            .lengths
            .iter()
            .filter(|l| {
                l.length_m > 0.0 && l.length_m.is_finite() && l.price > 0.0 && l.price.is_finite()
            })
            .collect();
        v.sort_by(|a, b| a.length_m.total_cmp(&b.length_m));
        v
    }

    /// Sold only in set lengths.
    pub fn in_set_lengths(&self) -> bool {
        !self.stocked().is_empty()
    }

    /// Has a price of either kind.
    pub fn priced(&self) -> bool {
        self.price_per_m > 0.0 || self.in_set_lengths()
    }

    /// A price per metre to compare sizes by: the price per metre, or for
    /// set lengths the cheapest per metre of them.
    pub fn rate(&self) -> f64 {
        match self
            .stocked()
            .iter()
            .map(|l| l.price / l.length_m)
            .reduce(f64::min)
        {
            Some(r) => r,
            None => self.price_per_m,
        }
    }

    /// Whether a strip this long (millimetres) can come out of one board.
    /// Sizes cut to length can always supply it.
    pub fn long_enough(&self, len_mm: f64) -> bool {
        match self.stocked().last() {
            Some(l) => l.length_m * 1000.0 + 0.01 >= len_mm,
            None => true,
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Species {
    pub id: String,
    pub name: String,
    pub color: String,
    #[serde(default)]
    pub price_per_bf: f64,
    /// Rough stock thickness in quarters (8 = 8/4). `None` picks the cheapest fit.
    #[serde(default)]
    pub stock: Option<u32>,
    /// Sawn sections this timber can be bought in, for ordering by sawn size.
    #[serde(default)]
    pub sawn: Vec<SawnSize>,
}

/// One timber in a mill's price list.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct MillTimber {
    pub name: String,
    /// A typical colour for the timber, as #rrggbb, used when it is added to
    /// the timber list.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub color: Option<String>,
    /// Price per board foot, for mills that sell that way. Zero means none.
    #[serde(default)]
    pub price_per_bf: f64,
    /// Sawn sections sold by the metre, for mills that sell that way.
    #[serde(default)]
    pub sizes: Vec<SawnSize>,
}

fn default_step() -> f64 {
    0.3
}

/// A mill's price list: the file you import, or write to the data folder.
/// Nothing is built into the app; `examples/` has one to copy from.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct MillCatalogue {
    #[serde(default)]
    pub mill: String,
    #[serde(default)]
    pub source: String,
    /// The day the prices were read (YYYY-MM-DD), if known.
    #[serde(default)]
    pub fetched: String,
    /// The mill sells length in steps of this many metres.
    #[serde(default = "default_step")]
    pub step_m: f64,
    pub timbers: Vec<MillTimber>,
    /// A list from outside can give its sizes in inches, its lengths in
    /// feet, its step in feet and its prices per foot. `cleaned` turns all
    /// of these into millimetres and metres, so they are never written out.
    #[serde(default, skip_serializing)]
    pub sizes_in: Option<Units>,
    #[serde(default, skip_serializing)]
    pub lengths_in: Option<Lengths>,
    #[serde(default, skip_serializing)]
    pub step_ft: Option<f64>,
    /// The prices in the list already include sales tax.
    #[serde(default)]
    pub prices_include_tax: bool,
}

/// Trimmed, and no longer than `max` characters.
fn short(s: &str, max: usize) -> String {
    s.trim().chars().take(max).collect()
}

fn is_hex_colour(c: &str) -> bool {
    c.len() == 7 && c.starts_with('#') && c[1..].bytes().all(|b| b.is_ascii_hexdigit())
}

impl MillCatalogue {
    /// Tidies a price list that came from outside the app: drops sizes and
    /// timbers with no usable price, and fails if nothing is left.
    pub fn cleaned(mut self) -> Result<MillCatalogue, String> {
        self.mill = short(&self.mill, 60);
        if self.mill.is_empty() {
            self.mill = "Your mill".into();
        }
        self.source = short(&self.source, 300);
        let dated = self.fetched.len() == 10
            && self.fetched.bytes().enumerate().all(|(i, b)| {
                if i == 4 || i == 7 {
                    b == b'-'
                } else {
                    b.is_ascii_digit()
                }
            });
        if !dated {
            self.fetched = String::new();
        }
        // Inches and feet become millimetres and metres.
        let lengths = self.lengths_in.take().unwrap_or_default();
        let in_feet = lengths == Lengths::Ft;
        match self.step_ft.take() {
            Some(ft) if ft.is_finite() && ft > 0.0 => self.step_m = ft * M_PER_FOOT,
            _ if in_feet && (self.step_m - default_step()).abs() < 1e-9 => self.step_m = M_PER_FOOT,
            _ => {}
        }
        let scale = if self.sizes_in.take() == Some(Units::In) {
            MM_PER_INCH
        } else {
            1.0
        };
        // To a thousandth of a millimetre, so 6 in is 152.4, not 152.39999...
        let mm = |v: f64| (v * scale * 1000.0).round() / 1000.0;
        for z in self.timbers.iter_mut().flat_map(|t| t.sizes.iter_mut()) {
            z.thickness = mm(z.thickness);
            z.width = mm(z.width);
            if !(z.price_per_m > 0.0) && z.price_per_ft > 0.0 {
                z.price_per_m = z.price_per_ft / M_PER_FOOT;
            }
            z.price_per_ft = 0.0;
            // Set lengths in the list's own unit become metres.
            for l in &mut z.lengths {
                if l.length > 0.0 {
                    l.length_m = (lengths.metres(l.length) * 1e6).round() / 1e6;
                    l.length = 0.0;
                }
            }
            z.lengths.retain(|l| {
                l.length_m.is_finite()
                    && l.length_m > 0.0
                    && l.length_m < 100.0
                    && l.price.is_finite()
                    && l.price > 0.0
            });
            z.lengths.truncate(40);
        }
        if !(self.step_m.is_finite() && (0.0..=10.0).contains(&self.step_m)) {
            self.step_m = default_step();
        }
        let size_ok = |v: f64| v.is_finite() && v > 0.0 && v < 100_000.0;
        for t in &mut self.timbers {
            t.name = short(&t.name, 60);
            t.color = t.color.take().filter(|c| is_hex_colour(c));
            if !size_ok(t.price_per_bf) {
                t.price_per_bf = 0.0;
            }
            t.sizes.retain(|z| {
                size_ok(z.thickness)
                    && size_ok(z.width)
                    && (size_ok(z.price_per_m) || z.in_set_lengths())
            });
            t.sizes.truncate(400);
        }
        self.timbers
            .retain(|t| !t.name.is_empty() && (!t.sizes.is_empty() || t.price_per_bf > 0.0));
        self.timbers.truncate(200);
        if self.timbers.is_empty() {
            return Err("it has no timbers with prices".into());
        }
        Ok(self)
    }
}

/// How a mill sells its sawn sizes.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Sells {
    /// Cut to the length you ask for, at a price per metre or per foot.
    #[default]
    Cut,
    /// Only in set lengths, each with its own price.
    Set,
}

fn millimetres() -> Units {
    Units::Mm
}

/// One of your timbers' prices at a mill.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct MillPrice {
    /// The timber's id on your timber list.
    pub timber: String,
    #[serde(default)]
    pub price_per_bf: f64,
    #[serde(default)]
    pub sawn: Vec<SawnSize>,
}

/// A mill you buy from: how it sells, and its prices for your timbers.
/// Sizes are kept in millimetres, lengths in metres and prices per metre,
/// whatever the mill uses; `sizes_in` and `lengths_in` say how to show them.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Mill {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub source: String,
    #[serde(default)]
    pub fetched: String,
    #[serde(default = "millimetres")]
    pub sizes_in: Units,
    #[serde(default)]
    pub sells: Sells,
    #[serde(default)]
    pub lengths_in: Lengths,
    /// For sizes cut to length: the step it sells length in, in metres.
    #[serde(default = "default_step")]
    pub step_m: f64,
    /// For a mill that sells in set lengths: the lengths it stocks, in
    /// metres, which the price editor offers a column for.
    #[serde(default)]
    pub set_lengths_m: Vec<f64>,
    /// Its prices already include sales tax (VAT, GST...), so none is added
    /// to an order from it. The page sends a tax rate of zero for such a
    /// mill and shows how much of the total is tax.
    #[serde(default)]
    pub prices_include_tax: bool,
    /// Your timbers' prices here.
    #[serde(default)]
    pub prices: Vec<MillPrice>,
    /// The mill's whole price list as imported, including timbers that are
    /// not on your list, so they can be added or matched later.
    #[serde(default)]
    pub list: Vec<MillTimber>,
}

impl Mill {
    /// Tidies a mill that came from the page.
    pub fn cleaned(mut self) -> Mill {
        self.id = self
            .id
            .chars()
            .filter(|c| c.is_ascii_alphanumeric() || *c == '-' || *c == '_')
            .take(40)
            .collect();
        self.name = short(&self.name, 60);
        if self.name.is_empty() {
            self.name = "My mill".into();
        }
        self.source = short(&self.source, 300);
        if !(self.step_m.is_finite() && (0.0..=10.0).contains(&self.step_m)) {
            self.step_m = default_step();
        }
        self.set_lengths_m
            .retain(|l| l.is_finite() && *l > 0.0 && *l < 100.0);
        self.set_lengths_m.sort_by(f64::total_cmp);
        self.set_lengths_m.dedup_by(|a, b| (*a - *b).abs() < 1e-6);
        self.set_lengths_m.truncate(12);
        self.prices.retain(|p| !p.timber.is_empty());
        for p in &mut self.prices {
            if !(p.price_per_bf.is_finite() && p.price_per_bf >= 0.0) {
                p.price_per_bf = 0.0;
            }
            p.sawn.retain(|z| {
                z.thickness.is_finite() && z.thickness > 0.0 && z.width.is_finite() && z.width > 0.0
            });
            p.sawn.truncate(400);
        }
        self.list.truncate(200);
        self
    }
}

/// The timber list a new installation starts with: two timbers and no
/// prices, because prices and sizes differ from mill to mill.
pub fn default_library() -> Vec<Species> {
    let s = |id: &str, name: &str, color: &str| Species {
        id: id.into(),
        name: name.into(),
        color: color.into(),
        price_per_bf: 0.0,
        stock: None,
        sawn: Vec::new(),
    };
    vec![
        s("walnut", "Walnut", "#5a3d2b"),
        s("cherry", "Cherry", "#b9693c"),
    ]
}

// ---------------------------------------------------------------- results

#[derive(Clone, Debug, Serialize)]
pub struct LayoutIssue {
    /// "rows" or "cols"
    pub axis: &'static str,
    /// "mismatch": every track is sized but they don't add up to the board.
    /// "over": the sized tracks leave nothing for the unsized ones.
    pub kind: &'static str,
    pub fixed_sum: f64,
    pub target: f64,
}

#[derive(Clone, Debug, Serialize)]
pub struct PanelStrip {
    pub species: String,
    /// Finished size of this strip's cells in the board.
    pub cell: f64,
    /// Milled size, as it sits in the glue-up.
    pub thickness: f64,
    pub width: f64,
    pub length: f64,
    /// Rough stock in quarters. Zero when it comes from a sawn section.
    pub stock: u32,
    /// Thickness in millimetres of the sawn section it comes from, if any.
    pub sawn: Option<f64>,
    /// Width in millimetres of that section when the strip has a whole board
    /// to itself and needs no ripping.
    pub sawn_width: Option<f64>,
    /// Ordering by sawn size: the timber has sawn sizes but none is big enough
    /// for this strip, so it is left off the order.
    pub too_big: bool,
    /// True when the strip is ripped so its width comes from the stock thickness.
    pub on_edge: bool,
    /// Width to rip the strip at, before planing it to its finished size:
    /// the finished size across the rip plus the rough width loss.
    pub rip_at: f64,
    pub rough_width: f64,
    pub rough_length: f64,
    pub bf: f64,
}

#[derive(Clone, Debug, Serialize)]
pub struct Member {
    /// Zero-based row or column this slice becomes.
    pub index: usize,
    /// Turn the slice end for end before gluing.
    pub flipped: bool,
}

#[derive(Clone, Debug, Serialize)]
pub struct Panel {
    pub label: String,
    pub slices: usize,
    pub members: Vec<Member>,
    /// Crosscut each slice at this length (finished thickness + flattening).
    pub slice_cut: f64,
    /// Panel thickness after flattening = finished row/column size.
    pub slice_final: f64,
    /// Glued panel size before flattening.
    pub width: f64,
    pub thickness: f64,
    pub length: f64,
    /// Length was raised to the minimum strip length.
    pub padded: bool,
    pub strips: Vec<PanelStrip>,
}

#[derive(Clone, Debug, Serialize)]
pub struct CutItem {
    pub qty: usize,
    pub species: String,
    pub thickness: f64,
    pub width: f64,
    pub length: f64,
    pub stock: u32,
    pub sawn: Option<f64>,
    pub sawn_width: Option<f64>,
    pub too_big: bool,
    pub on_edge: bool,
    /// Rip the strip this wide and crosscut it this long, then plane it to
    /// the finished size above.
    pub rip_at: f64,
    pub rough_length: f64,
    pub panels: Vec<String>,
}

#[derive(Clone, Debug, Serialize)]
pub struct Blank {
    pub qty: usize,
    pub width: f64,
    pub length: f64,
}

#[derive(Clone, Debug, Serialize)]
pub struct OrderLine {
    pub species: String,
    pub stock: u32,
    pub strips: usize,
    pub bf_net: f64,
    pub bf_order: f64,
    pub price_per_bf: f64,
    pub cost: f64,
    /// Narrowest and shortest rough board that still yields every strip.
    pub min_width: f64,
    pub min_length: f64,
    pub blanks: Vec<Blank>,
}

/// Boards to ask the mill for, e.g. 2 lengths of 2.1 m.
#[derive(Clone, Debug, Serialize)]
pub struct SawnPiece {
    pub qty: usize,
    pub length_m: f64,
    /// Price of one board, for a size sold in set lengths.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub price: Option<f64>,
}

/// What one length of a sawn board is ripped into.
#[derive(Clone, Debug, Serialize)]
pub struct SawnLayout {
    pub qty: usize,
    /// Rough strip length, in the board's units.
    pub length: f64,
    /// Finished rip sizes across the board, in the board's units.
    pub rips: Vec<f64>,
    /// The width to rip each of those at, before planing.
    pub rip_at: Vec<f64>,
    /// Set when the board is one strip, planed to size rather than ripped:
    /// the strip's finished thickness and width, in the board's units.
    pub planed: Option<[f64; 2]>,
}

/// One line of an order by sawn size: a section and how many metres of it.
#[derive(Clone, Debug, Serialize)]
pub struct SawnLine {
    pub species: String,
    /// Nominal section in millimetres.
    pub thickness: f64,
    pub width: f64,
    pub strips: usize,
    /// Metres actually needed, and metres to order after adding the extra
    /// for defects and rounding up to the mill's step.
    pub metres_net: f64,
    pub metres: f64,
    pub price_per_m: f64,
    pub cost: f64,
    pub pieces: Vec<SawnPiece>,
    pub layouts: Vec<SawnLayout>,
    /// Bought as boards of set lengths, each with its own price, rather
    /// than cut to length at a price per metre.
    pub set_lengths: bool,
}

/// Strips that no sawn size of their timber is big enough for.
#[derive(Clone, Debug, Serialize)]
pub struct TooBig {
    pub species: String,
    pub qty: usize,
    /// Finished strip size, in the board's units.
    pub thickness: f64,
    pub width: f64,
    /// The smallest sawn board the strip could come from, in millimetres:
    /// its thickness, and the width the strip would take up.
    pub need_thickness: f64,
    pub need_width: f64,
    /// The thickest board the mill lists for this timber, and the widest
    /// board at the thickness needed (or at all, if none is that thick).
    pub max_thickness: f64,
    pub max_width: f64,
}

#[derive(Clone, Debug, Serialize)]
pub struct Totals {
    pub bf_net: f64,
    pub bf_order: f64,
    /// Metres of sawn board on the order.
    pub sawn_metres: f64,
    /// Everything on the order before tax, the tax, and the two together.
    /// Tax is only added when ordering by sawn size.
    pub cost: f64,
    pub vat: f64,
    pub total: f64,
    /// Ordering by sawn size: how many strips get a board to themselves when
    /// ripping is avoided, and what that adds to the cost before tax compared
    /// with ripping (negative if it is cheaper). `None` by the board foot.
    pub whole_strips: Option<usize>,
    pub whole_extra: Option<f64>,
    /// Strips left off the order because no sawn size fits them. Zero when
    /// they are being ordered by the board foot instead.
    pub unpriced: usize,
    /// Volume of the finished board, for comparison.
    pub board_bf: f64,
    pub panels: usize,
    pub slices: usize,
    pub strips: usize,
}

#[derive(Clone, Debug, Serialize)]
pub struct Plan {
    /// Resolved axis: `Cols` or `Rows`, never `Auto`.
    pub axis: SliceAxis,
    pub panels: Vec<Panel>,
    pub cut_list: Vec<CutItem>,
    /// Timber ordered by the board foot.
    pub order: Vec<OrderLine>,
    /// Timber ordered as sawn sections by the metre.
    pub sawn: Vec<SawnLine>,
    /// Ordering by sawn size: strips too big for any size their timber is
    /// sold in. They are left off the order and the cost, unless you chose
    /// to order them by the board foot, in which case they are in `order`.
    pub too_big: Vec<TooBig>,
    pub totals: Totals,
    pub warnings: Vec<String>,
}

#[derive(Clone, Debug, Serialize)]
pub struct AxisOption {
    pub axis: SliceAxis,
    pub panels: usize,
    pub bf_order: f64,
    pub cost: f64,
}

#[derive(Clone, Debug, Serialize)]
pub struct CalcResult {
    pub col_sizes: Vec<f64>,
    pub row_sizes: Vec<f64>,
    pub actual_width: f64,
    pub actual_depth: f64,
    pub issues: Vec<LayoutIssue>,
    pub errors: Vec<String>,
    pub plan: Option<Plan>,
    pub options: Vec<AxisOption>,
}
