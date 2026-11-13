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

/// A sawn section the mill sells by the metre, e.g. 50 x 150 mm.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SawnSize {
    /// Nominal sawn thickness and width in millimetres.
    pub thickness: f64,
    pub width: f64,
    /// Price per metre of length, before tax.
    #[serde(default)]
    pub price_per_m: f64,
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
            t.sizes
                .retain(|z| size_ok(z.thickness) && size_ok(z.width) && size_ok(z.price_per_m));
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
