//! The engine: turns a board layout into glue-up panels, a milled cutting
//! list and a rough sawn order list.
//!
//! How an end grain board goes together, and what the numbers mean:
//!
//! 1. Rough stock is milled into strips.
//! 2. Strips are glued edge to edge into a panel (the first glue-up) and the
//!    panel is flattened.
//! 3. The panel is crosscut into slices, each a little longer than the
//!    finished board thickness. Every cut loses one kerf.
//! 4. Slices are turned end grain up and glued side by side. Each slice is one
//!    column (or row) of the finished board, and each strip in it is one cell.
//!
//! Columns that share a width and the same run of timbers come from the same
//! panel. A column that is another one upside down also shares its panel (the
//! slice is flipped end for end), as long as the row sizes are symmetrical.

use crate::model::*;

const EPS: f64 = 1e-6;

/// Rough stock thicknesses a mill normally carries, in quarters of an inch.
pub const QUARTERS: [u32; 7] = [4, 5, 6, 8, 10, 12, 16];

/// Sizes are compared to a ten-thousandth so float noise never splits a group.
fn key(v: f64) -> i64 {
    (v * 10_000.0).round() as i64
}

fn panel_label(mut i: usize) -> String {
    let mut s = String::new();
    loop {
        s.insert(0, (b'A' + (i % 26) as u8) as char);
        if i < 26 {
            break;
        }
        i = i / 26 - 1;
    }
    s
}

/// Work out the real size of every row or column. Sized tracks keep their
/// size; unsized ones split what is left of `total` evenly.
pub fn resolve_tracks(
    tracks: &[Track],
    total: f64,
    axis: &'static str,
) -> (Vec<f64>, Option<LayoutIssue>) {
    let fixed_sum: f64 = tracks
        .iter()
        .filter_map(|t| t.size)
        .map(|s| s.max(0.0))
        .sum();
    let autos = tracks.iter().filter(|t| t.size.is_none()).count();

    if autos == 0 {
        let sizes = tracks
            .iter()
            .map(|t| t.size.unwrap_or(0.0).max(0.0))
            .collect();
        let issue = ((fixed_sum - total).abs() > 1e-4).then_some(LayoutIssue {
            axis,
            kind: "mismatch",
            fixed_sum,
            target: total,
        });
        return (sizes, issue);
    }

    let left = total - fixed_sum;
    if left <= 1e-4 {
        let sizes = tracks
            .iter()
            .map(|t| t.size.map_or(0.0, |s| s.max(0.0)))
            .collect();
        return (
            sizes,
            Some(LayoutIssue {
                axis,
                kind: "over",
                fixed_sum,
                target: total,
            }),
        );
    }
    let each = left / autos as f64;
    let sizes = tracks
        .iter()
        .map(|t| t.size.map_or(each, |s| s.max(0.0)))
        .collect();
    (sizes, None)
}

/// Thick stock is dearer and harder to find, so it has to save a little
/// timber to be chosen over thinner stock: 2.5% per quarter.
fn thin_lean(q: u32) -> f64 {
    1.0 + q as f64 * 0.025
}

/// Choose the rough stock for one strip. A strip can be cut either way up:
/// flat (its glue-up thickness comes from the stock thickness) or on edge.
/// Returns (quarters, on_edge, rough cross-section in square inches), or
/// `None` if no allowed stock is thick enough. All arguments are in inches.
fn pick_stock(
    thickness: f64,
    width: f64,
    s: &Settings,
    fixed: Option<u32>,
) -> Option<(u32, bool, f64)> {
    let mut best: Option<(u32, bool, f64, f64)> = None;
    for (thick, wide, on_edge) in [(thickness, width, false), (width, thickness, true)] {
        let need = thick + s.rough_thickness;
        let fits = |q: u32| q as f64 / 4.0 + EPS >= need;
        let q = match fixed {
            Some(q) => fits(q).then_some(q),
            None => QUARTERS.iter().copied().find(|q| fits(*q)),
        };
        if let Some(q) = q {
            // Least rough timber wins, with a slight lean towards thinner
            // stock, which is cheaper and easier to find.
            let area = q as f64 / 4.0 * (wide + s.rough_width + s.kerf);
            let score = area * thin_lean(q);
            if best.map_or(true, |b| score < b.3 - EPS) {
                best = Some((q, on_edge, area, score));
            }
        }
    }
    best.map(|b| (b.0, b.1, b.2))
}

/// Choose the sawn section thickness for one strip, when ordering by sawn
/// size. Sizes are in millimetres. Returns (sawn thickness, on_edge, board
/// width the strip takes up), or `None` if no section is big enough.
fn pick_sawn(
    t: f64,
    w: f64,
    sizes: &[&SawnSize],
    rough_t: f64,
    rough_w: f64,
) -> Option<(f64, bool, f64)> {
    let mut best: Option<(f64, bool, f64, f64)> = None;
    for z in sizes {
        for (thick, rip, on_edge) in [(t, w, false), (w, t, true)] {
            let need = rip + rough_w;
            if thick + rough_t > z.thickness + 0.01 || need > z.width + 0.01 {
                continue;
            }
            // This strip's share of the board's price per metre.
            let share = need * z.price_per_m / z.width;
            let better = best.map_or(true, |b| {
                share < b.3 - 1e-9 || ((share - b.3).abs() <= 1e-9 && z.thickness < b.0)
            });
            if better {
                best = Some((z.thickness, on_edge, need, share));
            }
        }
    }
    best.map(|b| (b.0, b.1, b.2))
}

/// Avoiding ripping: find a sawn section this strip can have to itself,
/// planed down to size, without wasting more than `max_waste` (0..1) of the
/// board. Returns (sawn thickness, on_edge, width the strip takes, board
/// width), choosing the cheapest such board.
fn pick_whole(
    t: f64,
    w: f64,
    sizes: &[&SawnSize],
    rough_t: f64,
    rough_w: f64,
    max_waste: f64,
) -> Option<(f64, bool, f64, f64)> {
    let mut best: Option<(f64, bool, f64, f64, f64, f64)> = None;
    for z in sizes {
        for (thick, rip, on_edge) in [(t, w, false), (w, t, true)] {
            let (deep, need) = (thick + rough_t, rip + rough_w);
            if deep > z.thickness + 0.01 || need > z.width + 0.01 {
                continue;
            }
            let waste = 1.0 - (deep * need) / (z.thickness * z.width);
            if waste > max_waste + 1e-9 {
                continue;
            }
            let better = best.map_or(true, |b| {
                z.price_per_m < b.4 - 1e-9 || ((z.price_per_m - b.4).abs() <= 1e-9 && waste < b.5)
            });
            if better {
                best = Some((z.thickness, on_edge, need, z.width, z.price_per_m, waste));
            }
        }
    }
    best.map(|b| (b.0, b.1, b.2, b.3))
}

/// One strip waiting to be cut from a sawn board. Lengths in millimetres.
struct SawnItem {
    len_key: i64,
    length_mm: f64,
    /// Rough strip length in the board's units, for display.
    length: f64,
    /// Board width it takes up, in millimetres.
    need: f64,
    /// Finished rip size, and the width to rip it at, in the board's units.
    rip: f64,
    rip_at: f64,
    /// Width of the board this strip has to itself, if it is not ripped.
    whole: Option<f64>,
    /// Finished strip thickness and width in the board's units, for display.
    strip: [f64; 2],
}

/// One strip-length of board, ripped into strips side by side.
#[derive(Clone)]
struct Layer {
    len_key: i64,
    length_mm: f64,
    length: f64,
    used: f64,
    rips: Vec<f64>,
    rip_at: Vec<f64>,
    /// Set for one strip planed from a whole board rather than ripped: its
    /// finished thickness and width.
    planed: Option<[f64; 2]>,
}

/// First-fit packing of strips (widest first) across boards `cap` mm wide.
/// Strips only share a layer with strips of the same length.
fn pack_layers(items: &[SawnItem], cap: f64, kerf: f64) -> Option<Vec<Layer>> {
    let mut layers: Vec<Layer> = Vec::new();
    for item in items {
        if item.need > cap + 0.01 {
            return None;
        }
        match layers
            .iter_mut()
            .find(|l| l.len_key == item.len_key && l.used + kerf + item.need <= cap + 0.01)
        {
            Some(layer) => {
                layer.used += kerf + item.need;
                layer.rips.push(item.rip);
                layer.rip_at.push(item.rip_at);
            }
            None => layers.push(Layer {
                len_key: item.len_key,
                length_mm: item.length_mm,
                length: item.length,
                used: item.need,
                rips: vec![item.rip],
                rip_at: vec![item.rip_at],
                planed: None,
            }),
        }
    }
    Some(layers)
}

/// Join layers end to end into boards no longer than `cap` mm, with a saw
/// cut between them. Returns each board's length in millimetres.
fn boards_from_layers(layers: &[Layer], cap: f64, kerf: f64) -> Vec<f64> {
    let mut lengths: Vec<f64> = layers.iter().map(|l| l.length_mm).collect();
    lengths.sort_by(|a, b| b.total_cmp(a));
    let mut boards: Vec<f64> = Vec::new();
    for len in lengths {
        match boards.iter_mut().find(|b| **b + kerf + len <= cap + 0.01) {
            Some(board) => *board += kerf + len,
            None => boards.push(len),
        }
    }
    boards
}

/// Add the layers of `plan` to the boards already `settled`, section by section.
fn with_settled<'a>(
    settled: &[(&'a SawnSize, Vec<Layer>)],
    plan: Vec<(&'a SawnSize, Vec<Layer>)>,
) -> Vec<(&'a SawnSize, Vec<Layer>)> {
    let mut all = settled.to_vec();
    for (z, layers) in plan.into_iter().filter(|(_, layers)| !layers.is_empty()) {
        match all
            .iter_mut()
            .find(|(have, _)| key(have.width) == key(z.width))
        {
            Some((_, list)) => list.extend(layers),
            None => all.push((z, layers)),
        }
    }
    all
}

/// Round up to the mill's selling step. A step of zero means no rounding.
fn round_up(metres: f64, step: f64) -> f64 {
    if step > 0.0 {
        // Rounded again so 3 x 0.3 comes out as 0.9, not 0.8999...
        (((metres / step - 1e-9).ceil() * step) * 1e6).round() / 1e6
    } else {
        (metres * 100.0 - 1e-9).ceil() / 100.0
    }
}

/// What to order of one sawn section to get these layers out of it:
/// (metres needed, metres to order, lengths to ask for).
///
/// Lengths are rounded up to the mill's selling step where that still fits
/// in the car, and the extra for defects is added to them, so the lengths to
/// ask for always add up to the metres to order.
fn section_order(
    layers: &[Layer],
    cap: f64,
    kerf: f64,
    waste: f64,
    step: f64,
) -> (f64, f64, Vec<SawnPiece>) {
    let boards = boards_from_layers(layers, cap, kerf);
    let metres_net = boards.iter().sum::<f64>() / 1000.0;
    let unit = if step > 0.0 { step * 1000.0 } else { 100.0 };
    // Each board, rounded up to a length the mill sells.
    let mut asks: Vec<f64> = boards
        .iter()
        .map(|&len| {
            let ask = (len / unit - 1e-6).ceil() * unit / 1000.0;
            if ask * 1000.0 > cap + 0.01 && len <= cap + 0.01 {
                (len / 10.0 - 1e-6).ceil() / 100.0
            } else {
                ask
            }
        })
        .collect();

    // The extra for defects is part of what you ask for, so the lengths
    // add up to the metres on the order. It goes onto the shortest board
    // that can still grow within the longest length allowed.
    let mut extra = round_up(metres_net * waste, step) - asks.iter().sum::<f64>();
    let inc = unit / 1000.0;
    while extra > 1e-6 {
        let add = inc.min(extra);
        let room = asks
            .iter_mut()
            .filter(|a| (**a + add) * 1000.0 <= cap + 0.01)
            .min_by(|a, b| a.total_cmp(b));
        match room {
            Some(ask) => {
                *ask += add;
                extra -= add;
            }
            None => {
                // Every board is as long as it may be: the spare comes as
                // one more board, long enough to make the shortest strip.
                let shortest = layers
                    .iter()
                    .map(|l| l.length_mm)
                    .fold(f64::INFINITY, f64::min);
                let useful = (shortest / unit - 1e-6).ceil() * unit / 1000.0;
                let wanted = (extra / inc - 1e-6).ceil() * inc;
                asks.push(wanted.max(useful).min(cap / 1000.0));
                extra = 0.0;
            }
        }
    }

    let mut pieces: Vec<SawnPiece> = Vec::new();
    for ask in asks {
        let ask = (ask * 1000.0).round() / 1000.0;
        match pieces.iter_mut().find(|q| (q.length_m - ask).abs() < 1e-6) {
            Some(q) => q.qty += 1,
            None => pieces.push(SawnPiece {
                qty: 1,
                length_m: ask,
            }),
        }
    }
    pieces.sort_by(|a, b| b.length_m.total_cmp(&a.length_m));
    let metres: f64 = pieces.iter().map(|q| q.qty as f64 * q.length_m).sum();
    (metres_net, metres, pieces)
}

/// Sawn boards assumed to be available up to this length when you have not
/// set a longest board of your own.
const USUAL_BOARD_MM: f64 = 3000.0;

/// Work out which sawn sections to order for every strip that was given a
/// sawn thickness. `carry_mm` is the longest board you can carry, or zero.
fn sawn_order(
    p: &Project,
    s: &Settings,
    lib: &[Species],
    panels: &[Panel],
    carry_mm: f64,
) -> Vec<SawnLine> {
    let mm = |v: f64| p.units.to_inches(v) * 25.4;
    let kerf = mm(s.kerf);
    let cap = if carry_mm > 0.0 {
        carry_mm
    } else {
        USUAL_BOARD_MM
    };
    let waste = 1.0 + s.waste_pct / 100.0;

    // Strips grouped by timber and sawn thickness.
    let mut sets: Vec<(&str, f64, Vec<SawnItem>)> = Vec::new();
    for st in panels.iter().flat_map(|panel| panel.strips.iter()) {
        let Some(thickness) = st.sawn else { continue };
        let item = SawnItem {
            len_key: key(st.rough_length),
            length_mm: mm(st.rough_length),
            length: st.rough_length,
            need: mm(st.rough_width),
            rip: if st.on_edge { st.thickness } else { st.width },
            rip_at: st.rip_at,
            whole: st.sawn_width,
            strip: [st.thickness, st.width],
        };
        match sets
            .iter_mut()
            .find(|(id, t, _)| *id == st.species && key(*t) == key(thickness))
        {
            Some((_, _, items)) => items.push(item),
            None => sets.push((st.species.as_str(), thickness, vec![item])),
        }
    }

    let mut lines: Vec<SawnLine> = Vec::new();
    for (id, thickness, mut items) in sets {
        let Some(species) = lib.iter().find(|sp| sp.id == id) else {
            continue;
        };
        let mut sizes: Vec<&SawnSize> = species
            .sawn
            .iter()
            .filter(|z| key(z.thickness) == key(thickness) && z.width > 0.0 && z.price_per_m > 0.0)
            .collect();
        sizes.sort_by(|a, b| a.width.total_cmp(&b.width));
        items.sort_by(|a, b| b.need.total_cmp(&a.need));
        type Plan<'a> = Vec<(&'a SawnSize, Vec<Layer>)>;

        // Strips that have a board to themselves are settled already; only
        // the rest need packing.
        let (alone, items): (Vec<SawnItem>, Vec<SawnItem>) =
            items.into_iter().partition(|item| item.whole.is_some());
        let mut settled: Plan = Vec::new();
        for item in &alone {
            let Some(z) = sizes
                .iter()
                .copied()
                .find(|z| Some(key(z.width)) == item.whole.map(key))
            else {
                continue;
            };
            let layer = Layer {
                len_key: item.len_key,
                length_mm: item.length_mm,
                length: item.length,
                used: item.need,
                rips: vec![item.rip],
                rip_at: vec![item.rip_at],
                planed: Some(item.strip),
            };
            match settled
                .iter_mut()
                .find(|(have, _)| key(have.width) == key(z.width))
            {
                Some((_, list)) => list.push(layer),
                None => settled.push((z, vec![layer])),
            }
        }
        // A plan is a list of (section, layers cut from it). Try every width
        // on its own, and the same packing with each layer moved down to the
        // narrowest board it fits, and keep the cheapest once lengths are
        // rounded up to what the mill sells.
        let price = |plan: &Plan| -> f64 {
            plan.iter()
                .map(|(z, layers)| {
                    section_order(layers, cap, kerf, waste, s.sawn_step).1 * z.price_per_m
                })
                .sum()
        };
        let mut best: Option<(f64, Plan)> = None;
        for &size in &sizes {
            let Some(layers) = pack_layers(&items, size.width, kerf) else {
                continue;
            };
            let mut fitted: Plan = Vec::new();
            for layer in &layers {
                let z = sizes
                    .iter()
                    .copied()
                    .find(|z| z.width + 0.01 >= layer.used)
                    .unwrap_or(size);
                match fitted
                    .iter_mut()
                    .find(|(have, _)| key(have.width) == key(z.width))
                {
                    Some((_, list)) => list.push(layer.clone()),
                    None => fitted.push((z, vec![layer.clone()])),
                }
            }
            for plan in [vec![(size, layers)], fitted] {
                let plan = with_settled(&settled, plan);
                let cost = price(&plan);
                let better = best.as_ref().map_or(true, |(have, old)| {
                    cost < have - 0.005 || ((cost - have).abs() <= 0.005 && plan.len() < old.len())
                });
                if better {
                    best = Some((cost, plan));
                }
            }
        }

        for (z, layers) in best.map(|b| b.1).unwrap_or_default() {
            let (metres_net, metres, pieces) =
                section_order(&layers, cap, kerf, waste, s.sawn_step);

            let mut layouts: Vec<SawnLayout> = Vec::new();
            for layer in &layers {
                let same = |l: &SawnLayout| {
                    key(l.length) == key(layer.length)
                        && l.planed.map(|d| d.map(key)) == layer.planed.map(|d| d.map(key))
                        && l.rips.len() == layer.rips.len()
                        && l.rips
                            .iter()
                            .zip(&layer.rips)
                            .all(|(a, b)| key(*a) == key(*b))
                };
                match layouts.iter_mut().find(|l| same(l)) {
                    Some(l) => l.qty += 1,
                    None => layouts.push(SawnLayout {
                        qty: 1,
                        length: layer.length,
                        rips: layer.rips.clone(),
                        rip_at: layer.rip_at.clone(),
                        planed: layer.planed,
                    }),
                }
            }

            lines.push(SawnLine {
                species: id.to_string(),
                thickness: z.thickness,
                width: z.width,
                strips: layers.iter().map(|l| l.rips.len()).sum(),
                metres_net,
                metres,
                price_per_m: z.price_per_m,
                cost: metres * z.price_per_m,
                pieces,
                layouts,
            });
        }
    }

    let rank = |id: &str| lib.iter().position(|sp| sp.id == id).unwrap_or(usize::MAX);
    lines.sort_by(|a, b| {
        rank(&a.species)
            .cmp(&rank(&b.species))
            .then(a.thickness.total_cmp(&b.thickness))
            .then(a.width.total_cmp(&b.width))
    });
    lines
}

fn push_once(list: &mut Vec<String>, msg: String) {
    if !list.contains(&msg) {
        list.push(msg);
    }
}

fn build_plan(
    p: &Project,
    s: &Settings,
    lib: &[Species],
    axis: SliceAxis,
    row_sizes: &[f64],
    col_sizes: &[f64],
) -> Plan {
    let by_cols = axis == SliceAxis::Cols;
    let (strip_sizes, slice_sizes) = if by_cols {
        (row_sizes, col_sizes)
    } else {
        (col_sizes, row_sizes)
    };
    let cell = |strip: usize, slice: usize| -> &str {
        let (r, c) = if by_cols {
            (strip, slice)
        } else {
            (slice, strip)
        };
        p.cells
            .get(r)
            .and_then(|row| row.get(c))
            .map_or("", |id| id.as_str())
    };
    let n_strips = strip_sizes.len();
    let symmetrical =
        (0..n_strips).all(|i| key(strip_sizes[i]) == key(strip_sizes[n_strips - 1 - i]));

    // ---- group slices into panels
    struct Group<'a> {
        size: f64,
        pattern: Vec<&'a str>,
        members: Vec<Member>,
    }
    let mut groups: Vec<Group> = Vec::new();
    for (j, &size) in slice_sizes.iter().enumerate() {
        let pattern: Vec<&str> = (0..n_strips).map(|i| cell(i, j)).collect();
        let hit = groups.iter().enumerate().find_map(|(gi, g)| {
            if key(g.size) != key(size) {
                None
            } else if g.pattern == pattern {
                Some((gi, false))
            } else if symmetrical && g.pattern.iter().rev().eq(pattern.iter()) {
                Some((gi, true))
            } else {
                None
            }
        });
        match hit {
            Some((gi, flipped)) => groups[gi].members.push(Member { index: j, flipped }),
            None => groups.push(Group {
                size,
                pattern,
                members: vec![Member {
                    index: j,
                    flipped: false,
                }],
            }),
        }
    }

    // ---- keep every strip short enough to carry: split long glue-ups
    let slice_cut = p.thickness + s.board_flatten;
    let mut warnings: Vec<String> = Vec::new();
    let mut carry = s.max_length;
    if carry > 0.0 {
        let most =
            ((carry - s.rough_length - s.end_waste - s.kerf) / (slice_cut + s.kerf) + EPS).floor();
        if most < 1.0 || s.min_strip_length + s.rough_length > carry + EPS {
            push_once(
                &mut warnings,
                "Your longest board is too short for even one slice plus your allowances, so that limit has been ignored.".into(),
            );
            carry = 0.0;
        } else {
            let most = most as usize;
            let mut split: Vec<Group> = Vec::new();
            for g in groups {
                let n = g.members.len();
                let parts = n.div_ceil(most);
                if parts <= 1 {
                    split.push(g);
                    continue;
                }
                push_once(
                    &mut warnings,
                    "Long glue-ups have been split so no strip is longer than your longest board."
                        .into(),
                );
                let mut members = g.members.into_iter();
                for part in 0..parts {
                    let take = n / parts + usize::from(part < n % parts);
                    split.push(Group {
                        size: g.size,
                        pattern: g.pattern.clone(),
                        members: members.by_ref().take(take).collect(),
                    });
                }
            }
            groups = split;
        }
    }

    let inch = |v: f64| p.units.to_inches(v);
    let mm = |v: f64| inch(v) * 25.4;
    let s_in = Settings {
        kerf: inch(s.kerf),
        rough_thickness: inch(s.rough_thickness),
        rough_width: inch(s.rough_width),
        ..s.clone()
    };
    let lib_index = |id: &str| lib.iter().position(|sp| sp.id == id);
    let lib_name = |id: &str| {
        lib_index(id).map_or_else(|| "Unknown timber".to_string(), |i| lib[i].name.clone())
    };

    // Milled size of strip `i` in a panel whose slices finish `size` thick:
    // (thickness, width). The outer strips carry the edge trim.
    let strip_dims = |size: f64, i: usize| -> (f64, f64) {
        let mut width = strip_sizes[i];
        if i == 0 {
            width += s.edge_trim;
        }
        if i == n_strips - 1 {
            width += s.edge_trim;
        }
        (size + s.panel_flatten, width)
    };

    // Panel length for `n` slices: one squaring cut, then one cut per slice.
    let panel_length = |n: usize| -> (f64, bool) {
        let needed = n as f64 * slice_cut + (n as f64 + 1.0) * s.kerf + s.end_waste;
        // Nobody sets a stop to a fraction of a millimetre: round the length
        // up, which only adds to the end waste.
        let needed = if p.units == Units::Mm {
            (needed - 1e-9).ceil()
        } else {
            needed
        };
        let padded = needed + EPS < s.min_strip_length;
        (if padded { s.min_strip_length } else { needed }, padded)
    };

    // ---- ordering by sawn size: give each strip a sawn thickness if the
    // mill sells that timber in a section big enough. The rest fall back to
    // the board foot. sawn_pick[panel][strip] = (thickness mm, on_edge, mm of
    // board width taken, board width if the strip has a board to itself).
    let sawn_sizes = |id: &str| -> Vec<&SawnSize> {
        match (p.order_by, lib_index(id)) {
            (OrderBy::Sawn, Some(i)) => lib[i]
                .sawn
                .iter()
                .filter(|z| z.thickness > 0.0 && z.width > 0.0 && z.price_per_m > 0.0)
                .collect(),
            _ => Vec::new(),
        }
    };
    let sawn_pick: Vec<Vec<Option<(f64, bool, f64, Option<f64>)>>> = groups
        .iter()
        .map(|g| {
            g.pattern
                .iter()
                .enumerate()
                .map(|(i, &id)| {
                    let (t, w) = strip_dims(g.size, i);
                    let (sizes, rough_t, rough_w) =
                        (sawn_sizes(id), mm(s.rough_thickness), mm(s.rough_width));
                    let alone = if s.sawn_whole {
                        pick_whole(
                            mm(t),
                            mm(w),
                            &sizes,
                            rough_t,
                            rough_w,
                            s.sawn_whole_waste / 100.0,
                        )
                    } else {
                        None
                    };
                    match alone {
                        Some((thick, on_edge, need, board)) => {
                            Some((thick, on_edge, need, Some(board)))
                        }
                        None => pick_sawn(mm(t), mm(w), &sizes, rough_t, rough_w)
                            .map(|(thick, on_edge, need)| (thick, on_edge, need, None)),
                    }
                })
                .collect()
        })
        .collect();

    // One stock thickness per timber keeps a board foot order simple. For
    // each timber left on "best fit", find the single thickness all of its
    // strips can come from, and use it unless mixing saves real timber.
    // (thickness, width, length) in inches of every strip, by timber.
    let mut by_species: Vec<(&str, Vec<(f64, f64, f64)>)> = Vec::new();
    for (gi, g) in groups.iter().enumerate() {
        let length = inch(panel_length(g.members.len()).0);
        for (i, &id) in g.pattern.iter().enumerate() {
            // Sawn, or too big for sawn and left off: not bought by the board foot.
            if sawn_pick[gi][i].is_some() || (!sawn_sizes(id).is_empty() && !s.sawn_big_as_bf) {
                continue;
            }
            let (t, w) = strip_dims(g.size, i);
            let dims = (inch(t), inch(w), length);
            match by_species.iter_mut().find(|(sid, _)| *sid == id) {
                Some((_, list)) => list.push(dims),
                None => by_species.push((id, vec![dims])),
            }
        }
    }
    const MIXING_MUST_SAVE: f64 = 0.10;
    let shared_stock: Vec<(&str, Option<u32>)> = by_species
        .iter()
        .map(|(id, dims)| {
            // Rough volume, leaning thin, if every strip came from `fixed`
            // (or from its own best fit when `fixed` is `None`).
            let volume = |fixed: Option<u32>| -> Option<f64> {
                dims.iter()
                    .map(|&(t, w, len)| {
                        pick_stock(t, w, &s_in, fixed).map(|(q, _, area)| area * len * thin_lean(q))
                    })
                    .sum()
            };
            let mut best: Option<(u32, f64)> = None;
            for q in QUARTERS {
                if let Some(v) = volume(Some(q)) {
                    if best.map_or(true, |b| v < b.1 - EPS) {
                        best = Some((q, v));
                    }
                }
            }
            let single = best.filter(|&(_, v)| {
                volume(None).map_or(true, |mixed| v <= mixed * (1.0 + MIXING_MUST_SAVE))
            });
            (*id, single.map(|b| b.0))
        })
        .collect();
    let shared = |id: &str| {
        shared_stock
            .iter()
            .find(|(sid, _)| *sid == id)
            .and_then(|(_, q)| *q)
    };

    let mut panels: Vec<Panel> = Vec::new();
    let mut too_big: Vec<TooBig> = Vec::new();

    for (gi, g) in groups.into_iter().enumerate() {
        let label = panel_label(gi);
        let n = g.members.len();
        let (length, padded) = panel_length(n);
        let thickness = g.size + s.panel_flatten;

        let mut strips = Vec::with_capacity(n_strips);
        for (i, &id) in g.pattern.iter().enumerate() {
            let (_, width) = strip_dims(g.size, i);

            if let Some((sawn_thickness, on_edge, _, sawn_width)) = sawn_pick[gi][i] {
                strips.push(PanelStrip {
                    species: id.to_string(),
                    cell: strip_sizes[i],
                    thickness,
                    width,
                    length,
                    stock: 0,
                    sawn: Some(sawn_thickness),
                    sawn_width,
                    too_big: false,
                    on_edge,
                    rip_at: (if on_edge { thickness } else { width }) + s.rough_width,
                    rough_width: (if on_edge { thickness } else { width }) + s.rough_width,
                    rough_length: length + s.rough_length,
                    bf: 0.0,
                });
                continue;
            }
            if p.order_by == OrderBy::Sawn && lib_index(id).is_some() {
                let sizes = sawn_sizes(id);
                if sizes.is_empty() {
                    push_once(
                        &mut warnings,
                        format!(
                            "No sawn sizes are entered for {}, so it is listed by the board foot.",
                            lib_name(id)
                        ),
                    );
                } else {
                    // The timber is sold in sawn sizes but none is big enough
                    // for this strip. Say why, and leave it off the order
                    // unless you have asked for such strips by the board foot.
                    push_once(
                        &mut warnings,
                        format!(
                            "Some {} strips are too big for the sawn sizes the mill lists, so they are {}. The order list says why.",
                            lib_name(id),
                            if s.sawn_big_as_bf { "ordered by the board foot" } else { "left off this order" }
                        ),
                    );
                    let (small, large) =
                        (mm(thickness).min(mm(width)), mm(thickness).max(mm(width)));
                    let need_thickness = small + mm(s.rough_thickness);
                    let need_width = large + mm(s.rough_width);
                    let widest = |min_thickness: f64| {
                        sizes
                            .iter()
                            .filter(|z| z.thickness + 0.01 >= min_thickness)
                            .map(|z| z.width)
                            .fold(0.0, f64::max)
                    };
                    let max_thickness = sizes.iter().map(|z| z.thickness).fold(0.0, f64::max);
                    let max_width = if max_thickness + 0.01 >= need_thickness {
                        widest(need_thickness)
                    } else {
                        widest(0.0)
                    };
                    match too_big.iter_mut().find(|t: &&mut TooBig| {
                        t.species == id
                            && key(t.thickness) == key(thickness)
                            && key(t.width) == key(width)
                    }) {
                        Some(t) => t.qty += 1,
                        None => too_big.push(TooBig {
                            species: id.to_string(),
                            qty: 1,
                            thickness,
                            width,
                            need_thickness,
                            need_width,
                            max_thickness,
                            max_width,
                        }),
                    }
                    if !s.sawn_big_as_bf {
                        strips.push(PanelStrip {
                            species: id.to_string(),
                            cell: strip_sizes[i],
                            thickness,
                            width,
                            length,
                            stock: 0,
                            sawn: None,
                            sawn_width: None,
                            too_big: true,
                            on_edge: width < thickness,
                            rip_at: thickness.max(width) + s.rough_width,
                            rough_width: thickness.max(width) + s.rough_width,
                            rough_length: length + s.rough_length,
                            bf: 0.0,
                        });
                        continue;
                    }
                }
            }

            // `fixed` is what you pinned in the timber list, if anything.
            let fixed = match lib_index(id) {
                Some(li) => lib[li].stock,
                None => {
                    push_once(
                        &mut warnings,
                        "A timber on this board is missing from your timber list, so it is priced at zero.".into(),
                    );
                    None
                }
            };
            let (t_in, w_in) = (inch(thickness), inch(width));
            let (stock, on_edge) = match pick_stock(t_in, w_in, &s_in, fixed.or_else(|| shared(id)))
            {
                Some(pick) => (pick.0, pick.1),
                None => match pick_stock(t_in, w_in, &s_in, None) {
                    Some((q, on_edge, _)) => {
                        let pick = (q, on_edge);
                        push_once(
                            &mut warnings,
                            format!(
                                "{} strips in panel {} are too thick for {}/4 stock, so they are listed as {}/4.",
                                lib_name(id),
                                label,
                                fixed.unwrap_or(0),
                                pick.0
                            ),
                        );
                        pick
                    }
                    None => {
                        push_once(
                            &mut warnings,
                            format!(
                                "{} strips in panel {} are thicker than 16/4 stock allows. Split that row or column, or laminate the strips.",
                                lib_name(id),
                                label
                            ),
                        );
                        let on_edge = w_in < t_in;
                        let need = t_in.min(w_in) + s_in.rough_thickness;
                        ((need * 4.0 - EPS).ceil() as u32, on_edge)
                    }
                },
            };

            let rip = if on_edge { thickness } else { width };
            let rough_width = rip + s.rough_width + s.kerf;
            let rough_length = length + s.rough_length;
            let bf = stock as f64 / 4.0 * inch(rough_width) * inch(rough_length) / 144.0;
            strips.push(PanelStrip {
                species: id.to_string(),
                cell: strip_sizes[i],
                thickness,
                width,
                length,
                stock,
                sawn: None,
                sawn_width: None,
                too_big: false,
                on_edge,
                rip_at: rip + s.rough_width,
                rough_width,
                rough_length,
                bf,
            });
        }

        if padded {
            push_once(
                &mut warnings,
                format!(
                    "Panel {} only makes {} slice{}, so its strips are padded to your shortest strip length.",
                    label,
                    n,
                    if n == 1 { "" } else { "s" }
                ),
            );
        }

        panels.push(Panel {
            label,
            slices: n,
            members: g.members,
            slice_cut,
            slice_final: g.size,
            width: strips.iter().map(|st| st.width).sum(),
            thickness,
            length,
            padded,
            strips,
        });
    }

    // ---- milled cutting list: identical strips counted together
    let mut cut_list: Vec<CutItem> = Vec::new();
    for panel in &panels {
        for st in &panel.strips {
            let found = cut_list.iter_mut().find(|c| {
                c.species == st.species
                    && key(c.thickness) == key(st.thickness)
                    && key(c.width) == key(st.width)
                    && key(c.length) == key(st.length)
                    && c.stock == st.stock
                    && c.sawn.map(key) == st.sawn.map(key)
                    && c.sawn_width.map(key) == st.sawn_width.map(key)
                    && c.too_big == st.too_big
                    && c.on_edge == st.on_edge
            });
            match found {
                Some(c) => {
                    c.qty += 1;
                    if !c.panels.contains(&panel.label) {
                        c.panels.push(panel.label.clone());
                    }
                }
                None => cut_list.push(CutItem {
                    qty: 1,
                    species: st.species.clone(),
                    thickness: st.thickness,
                    width: st.width,
                    length: st.length,
                    stock: st.stock,
                    sawn: st.sawn,
                    sawn_width: st.sawn_width,
                    too_big: st.too_big,
                    on_edge: st.on_edge,
                    rip_at: st.rip_at,
                    rough_length: st.rough_length,
                    panels: vec![panel.label.clone()],
                }),
            }
        }
    }
    let rank = |id: &str| lib_index(id).unwrap_or(usize::MAX);
    cut_list.sort_by(|a, b| {
        rank(&a.species)
            .cmp(&rank(&b.species))
            .then(key(b.thickness).cmp(&key(a.thickness)))
            .then(key(b.width).cmp(&key(a.width)))
            .then(key(b.length).cmp(&key(a.length)))
    });

    // ---- order by the board foot: one line per timber and stock thickness
    let waste = 1.0 + s.waste_pct / 100.0;
    let mut order: Vec<OrderLine> = Vec::new();
    for st in panels
        .iter()
        .flat_map(|panel| panel.strips.iter())
        .filter(|st| st.sawn.is_none() && !st.too_big)
    {
        let pos = order
            .iter()
            .position(|o| o.species == st.species && o.stock == st.stock);
        let line = match pos {
            Some(i) => &mut order[i],
            None => {
                order.push(OrderLine {
                    species: st.species.clone(),
                    stock: st.stock,
                    strips: 0,
                    bf_net: 0.0,
                    bf_order: 0.0,
                    price_per_bf: lib_index(&st.species)
                        .map_or(0.0, |i| lib[i].price_per_bf.max(0.0)),
                    cost: 0.0,
                    min_width: 0.0,
                    min_length: 0.0,
                    blanks: Vec::new(),
                });
                order.last_mut().unwrap()
            }
        };
        line.strips += 1;
        line.bf_net += st.bf;
        line.min_width = line.min_width.max(st.rough_width);
        line.min_length = line.min_length.max(st.rough_length);
        match line
            .blanks
            .iter_mut()
            .find(|b| key(b.width) == key(st.rough_width) && key(b.length) == key(st.rough_length))
        {
            Some(b) => b.qty += 1,
            None => line.blanks.push(Blank {
                qty: 1,
                width: st.rough_width,
                length: st.rough_length,
            }),
        }
    }
    for line in &mut order {
        line.bf_order = line.bf_net * waste;
        line.cost = line.bf_order * line.price_per_bf;
        line.blanks.sort_by(|a, b| {
            key(b.width)
                .cmp(&key(a.width))
                .then(key(b.length).cmp(&key(a.length)))
        });
    }
    order.sort_by(|a, b| {
        rank(&a.species)
            .cmp(&rank(&b.species))
            .then(a.stock.cmp(&b.stock))
    });

    // ---- order by sawn size: sections by the metre
    let sawn = sawn_order(p, s, lib, &panels, mm(carry));

    let cost = order.iter().map(|o| o.cost).sum::<f64>() + sawn.iter().map(|l| l.cost).sum::<f64>();
    let vat = if p.order_by == OrderBy::Sawn {
        cost * s.vat_pct / 100.0
    } else {
        0.0
    };
    let totals = Totals {
        bf_net: order.iter().map(|o| o.bf_net).sum(),
        bf_order: order.iter().map(|o| o.bf_order).sum(),
        sawn_metres: sawn.iter().map(|l| l.metres).sum(),
        cost,
        vat,
        total: cost + vat,
        whole_strips: None,
        whole_extra: None,
        unpriced: if s.sawn_big_as_bf {
            0
        } else {
            too_big.iter().map(|t| t.qty).sum()
        },
        board_bf: inch(row_sizes.iter().sum::<f64>())
            * inch(col_sizes.iter().sum::<f64>())
            * inch(p.thickness)
            / 144.0,
        panels: panels.len(),
        slices: panels.iter().map(|panel| panel.slices).sum(),
        strips: panels.iter().map(|panel| panel.strips.len()).sum(),
    };

    Plan {
        axis,
        panels,
        cut_list,
        order,
        sawn,
        too_big,
        totals,
        warnings,
    }
}

pub fn calculate(p: &Project, lib: &[Species]) -> CalcResult {
    let s = match p.units {
        Units::Mm => p.settings.sanitized().whole_mm(),
        Units::In => p.settings.sanitized(),
    };
    let (col_sizes, col_issue) = resolve_tracks(&p.cols, p.width, "cols");
    let (row_sizes, row_issue) = resolve_tracks(&p.rows, p.depth, "rows");
    let issues: Vec<LayoutIssue> = [col_issue, row_issue].into_iter().flatten().collect();

    let mut errors = Vec::new();
    if p.cols.is_empty() || p.rows.is_empty() {
        errors.push("The board needs at least one row and one column.".to_string());
    }
    if !(p.thickness > 0.0) {
        errors.push("Set a board thickness above zero.".to_string());
    }
    let over = issues.iter().any(|i| i.kind == "over");
    if !over && col_sizes.iter().chain(row_sizes.iter()).any(|v| *v <= EPS) {
        errors.push("Every row and column needs a size above zero.".to_string());
    }

    let mut plan = None;
    let mut options = Vec::new();
    if errors.is_empty() && !over {
        let cols = build_plan(p, &s, lib, SliceAxis::Cols, &row_sizes, &col_sizes);
        let rows = build_plan(p, &s, lib, SliceAxis::Rows, &row_sizes, &col_sizes);
        options = [&cols, &rows]
            .iter()
            .map(|pl| AxisOption {
                axis: pl.axis,
                panels: pl.totals.panels,
                bf_order: pl.totals.bf_order,
                cost: pl.totals.cost,
            })
            .collect();
        // Fewer glue-ups wins; on a tie, the cheaper order (or the smaller
        // one when ordering by the board foot, where prices may be unset).
        let cheaper = match p.order_by {
            OrderBy::Bf => rows.totals.bf_order < cols.totals.bf_order - 1e-9,
            OrderBy::Sawn => rows.totals.cost < cols.totals.cost - 0.005,
        };
        let rows_better = rows.totals.panels < cols.totals.panels
            || (rows.totals.panels == cols.totals.panels && cheaper);
        let mut chosen = match p.slice_axis {
            SliceAxis::Cols => cols,
            SliceAxis::Rows => rows,
            SliceAxis::Auto => {
                if rows_better {
                    rows
                } else {
                    cols
                }
            }
        };
        // Ordering by sawn size: price the order the other way round too, so
        // the designer can show what avoiding ripping costs.
        if p.order_by == OrderBy::Sawn {
            let other_s = Settings {
                sawn_whole: !s.sawn_whole,
                ..s.clone()
            };
            let other = build_plan(p, &other_s, lib, chosen.axis, &row_sizes, &col_sizes);
            let (avoiding, ripping) = if s.sawn_whole {
                (&chosen, &other)
            } else {
                (&other, &chosen)
            };
            let alone = avoiding
                .panels
                .iter()
                .flat_map(|panel| &panel.strips)
                .filter(|st| st.sawn_width.is_some())
                .count();
            let extra = avoiding.totals.cost - ripping.totals.cost;
            chosen.totals.whole_strips = Some(alone);
            chosen.totals.whole_extra = Some(extra);
        }
        plan = Some(chosen);
    }

    CalcResult {
        actual_width: col_sizes.iter().sum(),
        actual_depth: row_sizes.iter().sum(),
        col_sizes,
        row_sizes,
        issues,
        errors,
        plan,
        options,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn close(a: f64, b: f64) -> bool {
        (a - b).abs() < 1e-6
    }

    /// An 18 x 14 x 1 3/4 inch board with evenly shared rows and columns.
    fn board(cols: usize, rows: usize, cell: impl Fn(usize, usize) -> &'static str) -> Project {
        Project {
            name: "test".into(),
            units: Units::In,
            width: 18.0,
            depth: 14.0,
            thickness: 1.75,
            cols: vec![Track { size: None }; cols],
            rows: vec![Track { size: None }; rows],
            cells: (0..rows)
                .map(|r| (0..cols).map(|c| cell(r, c).to_string()).collect())
                .collect(),
            slice_axis: SliceAxis::Cols,
            order_by: OrderBy::Bf,
            // Edge trim is off by default; these boards have 1/8 on each
            // outer strip so the tests cover it.
            settings: Settings {
                edge_trim: 0.125,
                ..Settings::default()
            },
        }
    }

    /// The invented mill's price list in `examples/`, used here as test data.
    fn example_catalogue() -> MillCatalogue {
        serde_json::from_str::<MillCatalogue>(include_str!("../examples/hill-sawmill.json"))
            .expect("the example price list is valid JSON")
            .cleaned()
            .expect("the example price list has prices")
    }

    /// Six timbers priced by the board foot; five also have the example
    /// mill's sawn sizes, and purpleheart has none.
    fn library() -> Vec<Species> {
        let mill = example_catalogue();
        let sawn = |name: &str| {
            mill.timbers
                .iter()
                .find(|t| t.name == name)
                .map_or_else(Vec::new, |t| t.sizes.clone())
        };
        let s = |id: &str, color: &str, price: f64, mill_name: &str| Species {
            id: id.into(),
            // "walnut" is shown as "Walnut".
            name: id[..1].to_uppercase() + &id[1..],
            color: color.into(),
            price_per_bf: price,
            stock: None,
            sawn: sawn(mill_name),
        };
        vec![
            s("walnut", "#5a3d2b", 12.50, "American black walnut"),
            s("cherry", "#b9693c", 7.50, "Cherry"),
            s("maple", "#e6cfa5", 6.50, "Maple"),
            s("oak", "#bf9f6b", 7.00, "American white oak"),
            s("sapele", "#8a4a30", 9.00, "Sapele"),
            s("purpleheart", "#6b3a5e", 14.00, "none"),
        ]
    }

    fn plan(p: &Project) -> Plan {
        calculate(p, &library()).plan.expect("plan")
    }

    #[test]
    fn unsized_tracks_share_what_is_left() {
        let tracks = vec![
            Track { size: None },
            Track { size: Some(2.0) },
            Track { size: None },
        ];
        let (sizes, issue) = resolve_tracks(&tracks, 10.0, "rows");
        assert_eq!(sizes, vec![4.0, 2.0, 4.0]);
        assert!(issue.is_none());
    }

    #[test]
    fn fully_sized_tracks_that_miss_the_board_size_are_flagged() {
        let tracks = vec![Track { size: Some(3.0) }, Track { size: Some(4.0) }];
        let (sizes, issue) = resolve_tracks(&tracks, 10.0, "cols");
        assert_eq!(sizes, vec![3.0, 4.0]);
        let issue = issue.unwrap();
        assert_eq!(issue.kind, "mismatch");
        assert!(close(issue.fixed_sum, 7.0));
    }

    #[test]
    fn sized_tracks_that_fill_the_board_leave_no_room() {
        let tracks = vec![Track { size: Some(10.0) }, Track { size: None }];
        let (_, issue) = resolve_tracks(&tracks, 10.0, "cols");
        assert_eq!(issue.unwrap().kind, "over");
        let mut p = board(2, 2, |_, _| "walnut");
        p.cols = tracks;
        p.width = 10.0;
        assert!(calculate(&p, &library()).plan.is_none());
    }

    #[test]
    fn plain_board_is_one_panel_and_kerf_is_counted_per_cut() {
        let p = board(12, 4, |_, _| "walnut");
        let plan = plan(&p);
        assert_eq!(plan.panels.len(), 1);
        let panel = &plan.panels[0];
        assert_eq!(panel.slices, 12);
        // 12 slices at 1 7/8, 13 kerfs at 1/8, 2 of end waste.
        assert!(close(panel.slice_cut, 1.875));
        assert!(close(panel.length, 12.0 * 1.875 + 13.0 * 0.125 + 2.0));
        // Columns are 1 1/2 wide; the panel is milled 1/16 over for flattening.
        assert!(close(panel.thickness, 1.5625));
        // Rows are 3 1/2; the two outer strips carry 1/8 of trim each.
        let widths: Vec<f64> = panel.strips.iter().map(|s| s.width).collect();
        assert!(close(widths[0], 3.625) && close(widths[1], 3.5) && close(widths[3], 3.625));
        assert!(close(panel.width, 14.25));

        // A wider kerf makes the strips longer by 13 times the difference.
        let mut thin = p.clone();
        thin.settings.kerf = 0.0;
        assert!(close(
            panel.length - super::calculate(&thin, &library()).plan.unwrap().panels[0].length,
            13.0 * 0.125
        ));
    }

    #[test]
    fn board_feet_use_nominal_stock_and_rough_allowances() {
        let p = board(12, 4, |_, _| "walnut");
        let plan = plan(&p);
        assert_eq!(plan.order.len(), 1);
        let line = &plan.order[0];
        assert_eq!(line.stock, 8);
        assert_eq!(line.strips, 4);
        // Rough widths: 2 x (3.625 + .25 + .125) + 2 x (3.5 + .25 + .125) = 15.75
        // Rough length 26.125 + 1 = 27.125, nominal thickness 2.
        let net = 2.0 * 15.75 * 27.125 / 144.0;
        assert!(close(line.bf_net, net));
        assert!(close(line.bf_order, net * 1.2));
        assert!(close(line.cost, net * 1.2 * 12.5));
        assert!(close(plan.totals.board_bf, 18.0 * 14.0 * 1.75 / 144.0));
    }

    #[test]
    fn checkerboard_comes_from_one_panel_with_every_other_slice_flipped() {
        let p = board(
            6,
            4,
            |r, c| if (r + c) % 2 == 0 { "walnut" } else { "maple" },
        );
        let plan = plan(&p);
        assert_eq!(plan.panels.len(), 1);
        let flips: Vec<bool> = plan.panels[0].members.iter().map(|m| m.flipped).collect();
        assert_eq!(flips, vec![false, true, false, true, false, true]);
    }

    #[test]
    fn slices_are_not_flipped_when_row_sizes_are_lopsided() {
        let mut p = board(
            6,
            4,
            |r, c| if (r + c) % 2 == 0 { "walnut" } else { "maple" },
        );
        p.rows[0].size = Some(1.0);
        let plan = plan(&p);
        assert_eq!(plan.panels.len(), 2);
        assert!(plan
            .panels
            .iter()
            .all(|panel| panel.members.iter().all(|m| !m.flipped)));
    }

    #[test]
    fn auto_picks_the_direction_with_fewer_panels() {
        let mut stripes_across = board(8, 6, |r, _| if r % 2 == 0 { "walnut" } else { "cherry" });
        stripes_across.slice_axis = SliceAxis::Auto;
        assert_eq!(plan(&stripes_across).axis, SliceAxis::Cols);

        let mut stripes_down = board(8, 6, |_, c| if c % 2 == 0 { "walnut" } else { "cherry" });
        stripes_down.slice_axis = SliceAxis::Auto;
        let pl = plan(&stripes_down);
        assert_eq!(pl.axis, SliceAxis::Rows);
        assert_eq!(pl.panels.len(), 1);
    }

    #[test]
    fn columns_of_different_width_need_their_own_panels() {
        let mut p = board(4, 3, |_, _| "walnut");
        p.cols[0].size = Some(3.0);
        let plan = plan(&p);
        assert_eq!(plan.panels.len(), 2);
        assert_eq!(plan.panels[0].slices, 1);
        assert!(
            plan.panels[0].padded,
            "a single slice is padded to the minimum strip length"
        );
        assert!(close(plan.panels[0].length, 12.0));
        assert_eq!(plan.panels[1].slices, 3);
    }

    #[test]
    fn strips_are_cut_whichever_way_up_uses_the_least_rough_timber() {
        // A 1/2 inch pinstripe row between two wide rows, columns 1 1/2 wide.
        let mut p = board(12, 3, |_, _| "walnut");
        p.rows[1].size = Some(0.5);
        let pl = plan(&p);
        let thin = pl.panels[0]
            .strips
            .iter()
            .find(|s| close(s.cell, 0.5))
            .unwrap();
        // 1/2 x 1 9/16: ripping a 1/2 inch sliver off 8/4 beats 4/4 at 1 9/16 wide.
        assert_eq!(thin.stock, 8);
        assert!(!thin.on_edge);
        assert!(close(thin.rough_width, 0.5 + 0.25 + 0.125));

        // A 1 inch cherry row: 5/4 laid on edge (1.94 wide) beats 8/4 ripped at 1 3/8.
        p.rows[1].size = Some(1.0);
        for cell in &mut p.cells[1] {
            *cell = "cherry".into();
        }
        let pl = plan(&p);
        let strip = pl.panels[0]
            .strips
            .iter()
            .find(|s| close(s.cell, 1.0))
            .unwrap();
        assert_eq!(strip.stock, 5);
        assert!(strip.on_edge);
        assert!(close(strip.rough_width, 1.5625 + 0.25 + 0.125));
    }

    #[test]
    fn each_timber_is_ordered_in_one_thickness_where_possible() {
        // On its own a 3/4 inch walnut row would be a touch cheaper from 5/4,
        // but the wide walnut rows need 8/4, so all the walnut is bought as 8/4.
        let mut p = board(12, 3, |_, _| "walnut");
        p.rows[2].size = Some(0.75);
        let pl = plan(&p);
        assert_eq!(pl.order.len(), 1);
        assert_eq!(pl.order[0].stock, 8);
        assert!(pl.warnings.is_empty());

        // One 2 1/2 x 3 inch cell needs 12/4, but the board is mostly 1 inch
        // rows. Buying all of it in 12/4 would waste a lot, so thicknesses mix.
        let mut p = board(12, 12, |_, _| "walnut");
        p.cols[0].size = Some(2.5);
        p.rows[0].size = Some(3.0);
        let pl = plan(&p);
        let stocks: Vec<u32> = pl.order.iter().map(|o| o.stock).collect();
        assert!(stocks.len() > 1 && stocks.contains(&12), "got {stocks:?}");
    }

    #[test]
    fn pinned_stock_is_respected_and_too_thin_stock_is_flagged() {
        let mut lib = library();
        lib[0].stock = Some(12);
        let p = board(12, 4, |_, _| "walnut");
        let plan12 = calculate(&p, &lib).plan.unwrap();
        assert!(plan12.order.iter().all(|o| o.stock == 12));
        assert!(plan12.warnings.is_empty());

        lib[0].stock = Some(4);
        let plan4 = calculate(&p, &lib).plan.unwrap();
        assert!(plan4.order.iter().all(|o| o.stock == 8));
        assert!(plan4
            .warnings
            .iter()
            .any(|w| w.contains("too thick for 4/4")));
    }

    #[test]
    fn millimetre_boards_give_much_the_same_board_feet() {
        let inch = board(12, 4, |_, _| "walnut");
        let mut mm = inch.clone();
        mm.units = Units::Mm;
        for v in [&mut mm.width, &mut mm.depth, &mut mm.thickness] {
            *v *= 25.4;
        }
        let s = &mut mm.settings;
        for v in [
            &mut s.kerf,
            &mut s.board_flatten,
            &mut s.panel_flatten,
            &mut s.edge_trim,
            &mut s.end_waste,
            &mut s.min_strip_length,
            &mut s.rough_thickness,
            &mut s.rough_width,
            &mut s.rough_length,
        ] {
            *v *= 25.4;
        }
        let (a, b) = (plan(&inch), plan(&mm));
        // In millimetres the allowances and the strip length are taken to
        // whole millimetres, so the two orders differ by a hair, no more.
        let length = b.panels[0].length;
        assert!(close(length, length.round()));
        assert!((length - a.panels[0].length * 25.4).abs() < 2.0);
        assert!((b.totals.bf_order / a.totals.bf_order - 1.0).abs() < 0.02);
    }

    #[test]
    fn in_millimetres_every_size_you_cut_is_a_whole_millimetre() {
        // 420 x 343 x 43 with whole rows and columns, but allowances typed
        // (or converted from inches) with a decimal: 3.2 mm of edge trim and
        // flattening used to give strips 36.2 wide and slices cut at 46.2.
        let mut p = board(10, 15, |r, _| {
            if r > 6 && r % 2 == 1 {
                "cherry"
            } else {
                "walnut"
            }
        });
        p.units = Units::Mm;
        (p.width, p.depth, p.thickness) = (420.0, 343.0, 43.0);
        for (i, row) in p.rows.iter_mut().enumerate() {
            row.size = Some(if i < 7 {
                33.0
            } else if i % 2 == 1 {
                15.0
            } else {
                13.0
            });
        }
        p.settings = Settings {
            kerf: 3.2,
            board_flatten: 3.2,
            panel_flatten: 1.5,
            edge_trim: 3.2,
            end_waste: 50.0,
            min_strip_length: 305.2,
            rough_thickness: 6.35,
            rough_width: 6.35,
            rough_length: 25.4,
            ..Settings::default()
        };
        let pl = plan(&p);
        let whole = |v: f64| close(v, v.round());
        for pn in &pl.panels {
            assert!(whole(pn.slice_cut) && whole(pn.thickness) && whole(pn.length));
            assert!(whole(pn.width), "panel {} is {} wide", pn.label, pn.width);
            for st in &pn.strips {
                assert!(
                    whole(st.width) && whole(st.rip_at) && whole(st.rough_length),
                    "strip {} wide, rip at {}",
                    st.width,
                    st.rip_at
                );
            }
        }
        for c in &pl.cut_list {
            assert!(whole(c.thickness) && whole(c.width) && whole(c.length) && whole(c.rip_at));
        }
        // Outer strips carry 3 mm of edge trim, not 3.2; slices are cut at 46.
        assert!(close(pl.panels[0].strips[0].width, 36.0));
        assert!(close(pl.panels[0].slice_cut, 46.0));
        // The saw kerf is the blade's, and keeps its decimal.
        let kerfs = 11.0 * 3.2;
        assert!(close(
            pl.panels[0].length,
            (10.0 * 46.0 + kerfs + 50.0_f64).ceil()
        ));
    }

    #[test]
    fn unknown_timber_is_priced_at_zero_with_a_warning() {
        let p = board(4, 2, |_, _| "zebrawood");
        let plan = plan(&p);
        assert!(close(plan.totals.cost, 0.0));
        assert!(plan
            .warnings
            .iter()
            .any(|w| w.contains("missing from your timber list")));
    }

    /// Walnut-only board ordered as sawn sections from the example price list.
    fn sawn_board() -> Project {
        let mut p = board(12, 4, |_, _| "walnut");
        p.order_by = OrderBy::Sawn;
        // Whole metres keep the sums in these tests easy to follow.
        p.settings.sawn_step = 1.0;
        p
    }

    #[test]
    fn the_example_price_list_reads() {
        let mill = example_catalogue();
        assert_eq!(mill.mill, "Hill Sawmill");
        assert!(close(mill.step_m, 0.3));
        assert_eq!(mill.timbers.len(), 8);
        // Its walnut is priced at 6,000 a cubic metre in every size.
        let walnut = mill
            .timbers
            .iter()
            .find(|t| t.name == "American black walnut")
            .unwrap();
        let price = |t: f64, w: f64| {
            walnut
                .sizes
                .iter()
                .find(|z| z.thickness == t && z.width == w)
                .unwrap()
                .price_per_m
        };
        assert!(
            close(price(26.0, 50.0), 7.80)
                && close(price(50.0, 100.0), 30.0)
                && close(price(50.0, 200.0), 60.0)
        );
        // The test timbers that the mill sells come with its sizes.
        let lib = library();
        for id in ["walnut", "cherry", "maple", "oak", "sapele"] {
            assert!(
                !lib.iter().find(|sp| sp.id == id).unwrap().sawn.is_empty(),
                "{id} has no sawn sizes"
            );
        }
        assert!(lib
            .iter()
            .find(|sp| sp.id == "purpleheart")
            .unwrap()
            .sawn
            .is_empty());
    }

    #[test]
    fn lengths_are_asked_for_in_the_mills_steps_and_the_order_covers_them() {
        // Two boards of 0.69 m (too long to join within 1 m), sold in 0.3 m
        // steps, no extra for defects:
        // each is asked for as 0.9 m, so 1.8 m is ordered, not 1.5.
        let layer = |mm: f64| Layer {
            len_key: 0,
            length_mm: mm,
            length: 0.0,
            used: 0.0,
            rips: vec![],
            rip_at: vec![],
            planed: None,
        };
        let (net, metres, pieces) =
            section_order(&[layer(689.0), layer(689.0)], 1000.0, 3.2, 1.0, 0.3);
        assert!(close(net, 1.378) && close(metres, 1.8));
        assert_eq!((pieces.len(), pieces[0].qty), (1, 2));
        assert!(close(pieces[0].length_m, 0.9));
    }

    #[test]
    fn sawn_order_lists_sections_by_the_metre_instead_of_board_feet() {
        let pl = plan(&sawn_board());
        assert!(
            pl.order.is_empty(),
            "nothing should be left on the board foot list"
        );
        assert!(!pl.sawn.is_empty());
        // Columns are 1 1/2 in (38.1 mm) + flattening + 1/4 in rough loss: only 50 mm stock is thick enough.
        assert!(pl.panels[0]
            .strips
            .iter()
            .all(|s| s.sawn == Some(50.0) && s.stock == 0));
        assert!(pl.sawn.iter().all(|l| l.thickness == 50.0));
        // Every strip is accounted for, and each line costs metres x price.
        assert_eq!(pl.sawn.iter().map(|l| l.strips).sum::<usize>(), 4);
        for line in &pl.sawn {
            assert!(close(line.cost, line.metres * line.price_per_m));
            // Sold in whole metres by default, and never less than is needed plus 20%.
            assert!(close(line.metres, line.metres.round()));
            assert!(line.metres + 1e-9 >= line.metres_net * 1.2);
            // Every layout fits across its board: rips + 1/4 in each + a kerf between.
            for layout in &line.layouts {
                let across: f64 = layout.rips.iter().map(|r| (r + 0.25) * 25.4).sum::<f64>()
                    + (layout.rips.len() as f64 - 1.0) * 0.125 * 25.4;
                assert!(
                    across <= line.width + 0.01,
                    "{across} mm does not fit a {} mm board",
                    line.width
                );
            }
        }
        // Strips are ripped oversize by the rough width loss (1/4 in), to be
        // planed down afterwards, and the lists say so.
        for st in &pl.panels[0].strips {
            assert!(close(st.rip_at, st.width + 0.25));
        }
        assert!(pl
            .cut_list
            .iter()
            .all(|c| close(c.rip_at, c.width + 0.25) && close(c.rough_length, c.length + 1.0)));
        for layout in pl.sawn.iter().flat_map(|l| &l.layouts) {
            assert_eq!(layout.rips.len(), layout.rip_at.len());
            assert!(layout
                .rips
                .iter()
                .zip(&layout.rip_at)
                .all(|(fin, rip)| close(*rip, fin + 0.25)));
        }
        // Tax is added on top at 20%.
        let ex_vat: f64 = pl.sawn.iter().map(|l| l.cost).sum();
        assert!(close(pl.totals.cost, ex_vat));
        assert!(close(pl.totals.vat, ex_vat * 0.2));
        assert!(close(pl.totals.total, ex_vat * 1.2));
    }

    #[test]
    fn sawn_order_picks_the_cheapest_way_to_buy_the_strips() {
        // Four strips about 98 mm wide by 0.69 m. Cheapest is 3 m of 50 x 100
        // (one strip per length): 4 x 0.689 x 1.2 = 3.3 -> 4 m at 30.00.
        // Two per length on 50 x 200 would be 2 m at 60.00: the same money,
        // so check against a brute-force minimum rather than a fixed answer.
        let pl = plan(&sawn_board());
        let total: f64 = pl.sawn.iter().map(|l| l.cost).sum();
        assert!(total <= 2.0 * 60.0 + 0.005, "found {total}");
    }

    #[test]
    fn timber_without_sawn_sizes_stays_on_the_board_foot_list() {
        let mut p = board(12, 4, |r, _| if r == 0 { "purpleheart" } else { "walnut" });
        p.order_by = OrderBy::Sawn;
        let pl = plan(&p);
        assert!(pl.order.iter().all(|o| o.species == "purpleheart") && pl.order.len() == 1);
        assert!(pl.sawn.iter().all(|l| l.species == "walnut") && !pl.sawn.is_empty());
        assert!(pl
            .warnings
            .iter()
            .any(|w| w.contains("No sawn sizes are entered for Purpleheart")));
    }

    #[test]
    fn strips_too_big_for_any_sawn_size_are_left_off_the_order_with_the_reason() {
        // A board in millimetres: 9 columns of 48 and 5 walnut rows of 45.
        // Strips finish 50 x 45. The mill's thickest walnut is 50 mm, which
        // after 6 mm of rough loss yields 44: one millimetre short.
        let mut p = board(9, 5, |_, _| "walnut");
        p.units = Units::Mm;
        p.order_by = OrderBy::Sawn;
        (p.width, p.depth, p.thickness) = (432.0, 225.0, 43.0);
        p.settings = Settings {
            kerf: 3.2,
            board_flatten: 3.0,
            panel_flatten: 2.0,
            edge_trim: 0.0,
            end_waste: 50.0,
            min_strip_length: 300.0,
            rough_thickness: 6.0,
            rough_width: 6.0,
            rough_length: 25.0,
            ..Settings::default()
        };
        let pl = plan(&p);
        // Not sawn, and not quietly moved to the board foot list either.
        assert!(pl.sawn.is_empty() && pl.order.is_empty());
        assert!(close(pl.totals.cost, 0.0));
        assert_eq!(pl.totals.unpriced, 5);
        assert_eq!(pl.too_big.len(), 1);
        let t = &pl.too_big[0];
        assert_eq!((t.species.as_str(), t.qty), ("walnut", 5));
        assert!(close(t.thickness, 50.0) && close(t.width, 45.0));
        assert!(close(t.need_thickness, 51.0) && close(t.need_width, 56.0));
        assert!(close(t.max_thickness, 50.0));
        assert!(pl.panels[0].strips.iter().all(|s| s.too_big));
        assert!(pl
            .warnings
            .iter()
            .any(|w| w.contains("left off this order")));

        // Asked to, the engine orders those strips by the board foot instead:
        // still listed as too big, but priced and on the board foot list.
        p.settings.sawn_big_as_bf = true;
        let mixed = plan(&p);
        assert_eq!(mixed.too_big.iter().map(|t| t.qty).sum::<usize>(), 5);
        assert_eq!(mixed.totals.unpriced, 0);
        assert!(mixed.sawn.is_empty());
        assert_eq!(mixed.order.iter().map(|o| o.strips).sum::<usize>(), 5);
        assert!(mixed
            .order
            .iter()
            .all(|o| o.species == "walnut" && o.stock >= 10));
        assert!(mixed.totals.cost > 0.0);
        assert!(mixed.panels[0]
            .strips
            .iter()
            .all(|s| !s.too_big && s.stock >= 10));
        assert!(mixed
            .warnings
            .iter()
            .any(|w| w.contains("ordered by the board foot")));
        p.settings.sawn_big_as_bf = false;

        // One millimetre less on the rows and every strip fits, on edge.
        p.depth = 220.0;
        let pl = plan(&p);
        assert!(pl.too_big.is_empty() && pl.totals.unpriced == 0 && !pl.sawn.is_empty());
        assert!(pl.panels[0]
            .strips
            .iter()
            .all(|s| s.sawn == Some(50.0) && s.on_edge));
    }

    #[test]
    fn longest_board_splits_glue_ups_and_caps_what_is_ordered() {
        // 12 slices need a 26 1/8 in panel (27 1/8 rough). With 20 in to play
        // with, a panel holds 8 slices, so it becomes two panels of 6.
        let mut p = sawn_board();
        p.settings.max_length = 20.0;
        let pl = plan(&p);
        assert_eq!(
            pl.panels
                .iter()
                .map(|panel| panel.slices)
                .collect::<Vec<_>>(),
            vec![6, 6]
        );
        assert!(pl
            .panels
            .iter()
            .all(|panel| panel.length + p.settings.rough_length <= 20.0 + 1e-9));
        let slices: Vec<usize> = pl
            .panels
            .iter()
            .flat_map(|panel| panel.members.iter().map(|m| m.index))
            .collect();
        assert_eq!(slices, (0..12).collect::<Vec<_>>());
        // No board on the order is longer than 20 in (508 mm).
        assert!(pl
            .sawn
            .iter()
            .flat_map(|l| l.pieces.iter())
            .all(|piece| piece.length_m <= 0.508 + 1e-9));
        assert!(pl.warnings.iter().any(|w| w.contains("split")));

        // A limit too short for one slice is ignored, with a warning.
        p.settings.max_length = 3.0;
        let pl = plan(&p);
        assert_eq!(pl.panels.len(), 1);
        assert!(pl.warnings.iter().any(|w| w.contains("ignored")));
    }

    #[test]
    fn avoiding_ripping_gives_each_strip_its_own_board() {
        // Rows are 3 1/2 in: with the rough allowance a strip needs about
        // 95 mm of a 50 mm board, so each one takes a 50 x 100 to itself.
        let mut p = sawn_board();
        p.settings.sawn_whole = true;
        let pl = plan(&p);
        assert!(pl.panels[0]
            .strips
            .iter()
            .all(|s| s.sawn == Some(50.0) && s.sawn_width == Some(100.0)));
        assert_eq!(pl.sawn.len(), 1);
        let line = &pl.sawn[0];
        assert_eq!((line.thickness, line.width, line.strips), (50.0, 100.0, 4));
        assert!(line
            .layouts
            .iter()
            .all(|l| l.planed.is_some() && l.rips.len() == 1));
        // Ripping instead would put two strips on each 50 x 200 length:
        // 2 m at 60.00, against 4 m of 50 x 100 at 30.00. The same money.
        assert_eq!(pl.totals.whole_strips, Some(4));
        assert!(close(pl.totals.whole_extra.unwrap(), 0.0));
        // Four lengths of 27 1/8 in is 2.76 m; plus 20% is 3.3, sold as 4 m.
        assert!(close(line.metres, 4.0));
    }

    #[test]
    fn ripping_is_kept_where_a_whole_board_would_waste_too_much() {
        // A 1/2 in row between two wide ones. On edge it fits a 26 x 50 board
        // with a third of the board planed away; flat in a 50 x 50 it would
        // waste two thirds.
        let mut p = board(12, 3, |_, _| "walnut");
        p.order_by = OrderBy::Sawn;
        p.rows[1].size = Some(0.5);
        p.settings.sawn_whole = true;
        let thin = |pl: &Plan| {
            pl.panels[0]
                .strips
                .iter()
                .find(|s| close(s.cell, 0.5))
                .cloned()
                .unwrap()
        };

        p.settings.sawn_whole_waste = 50.0;
        let alone = thin(&plan(&p));
        assert_eq!(
            (alone.sawn, alone.sawn_width, alone.on_edge),
            (Some(26.0), Some(50.0), true)
        );

        // Allowing only 20% waste, no board is close enough: it is ripped
        // from a wider board alongside other strips instead.
        p.settings.sawn_whole_waste = 20.0;
        let ripped = thin(&plan(&p));
        assert_eq!(ripped.sawn_width, None);
        assert!(ripped.sawn.is_some());

        // Switched off, nothing gets a board to itself.
        p.settings.sawn_whole = false;
        assert!(plan(&p).panels[0]
            .strips
            .iter()
            .all(|s| s.sawn_width.is_none()));
    }

    #[test]
    fn boards_are_joined_end_to_end_up_to_the_longest_you_can_carry() {
        let layer = |mm: f64| Layer {
            len_key: 0,
            length_mm: mm,
            length: 0.0,
            used: 0.0,
            rips: vec![],
            rip_at: vec![],
            planed: None,
        };
        let layers = vec![layer(700.0), layer(700.0), layer(700.0), layer(700.0)];
        // 2.2 m takes three 0.7 m lengths and two 3 mm cuts; the fourth starts a new board.
        assert_eq!(
            boards_from_layers(&layers, 2200.0, 3.0),
            vec![2106.0, 700.0]
        );
        assert!(
            close(round_up(2.52, 1.0), 3.0)
                && close(round_up(3.0, 1.0), 3.0)
                && close(round_up(2.52, 0.5), 3.0)
        );
        assert!(close(round_up(2.521, 0.0), 2.53));
    }

    #[test]
    fn panel_labels_run_past_z() {
        assert_eq!(panel_label(0), "A");
        assert_eq!(panel_label(25), "Z");
        assert_eq!(panel_label(26), "AA");
        assert_eq!(panel_label(27), "AB");
    }

    #[test]
    fn the_lengths_to_ask_for_include_the_spare_and_add_up_to_the_metres() {
        let layer = |mm: f64| Layer {
            len_key: (mm * 100.0) as i64,
            length_mm: mm,
            length: mm,
            used: 0.0,
            rips: vec![],
            rip_at: vec![],
            planed: None,
        };
        let total = |pieces: &[SawnPiece]| {
            pieces
                .iter()
                .map(|q| q.qty as f64 * q.length_m)
                .sum::<f64>()
        };
        // Two 573 mm lengths end to end need 1.15 m. With 20% for defects
        // that is 1.38 m, sold as 1.5 m: ask for one board of 1.5 m, not 1.2.
        let two = [layer(573.0), layer(573.0)];
        let (net, metres, pieces) = section_order(&two, 3000.0, 3.2, 1.2, 0.3);
        assert!(close(net, 1.1492) && close(metres, 1.5));
        assert_eq!((pieces.len(), pieces[0].qty), (1, 1));
        assert!(close(pieces[0].length_m, 1.5) && close(total(&pieces), metres));

        // Nothing longer than 1.2 m fits in the car, so the board cannot
        // grow: the spare comes as a second board long enough for a strip.
        let (_, metres, pieces) = section_order(&two, 1200.0, 3.2, 1.2, 0.3);
        assert_eq!(pieces.len(), 2);
        assert!(close(pieces[0].length_m, 1.2) && close(pieces[1].length_m, 0.6));
        assert!(close(metres, 1.8) && close(total(&pieces), metres));

        // Spare is shared out, shortest board first, and never past the limit.
        let four = [layer(900.0), layer(900.0), layer(900.0), layer(900.0)];
        let (_, metres, pieces) = section_order(&four, 2000.0, 3.2, 1.2, 0.3);
        assert!(pieces.iter().all(|q| q.length_m <= 2.0 + 1e-9));
        assert!(close(total(&pieces), metres) && metres + 1e-9 >= 3.6 * 1.2);

        // No selling step: lengths still add up to the metres.
        let (_, metres, pieces) = section_order(&two, 3000.0, 3.2, 1.2, 0.0);
        assert!(close(total(&pieces), metres) && close(metres, 1.38));
    }

    #[test]
    fn a_new_installation_starts_with_two_timbers_and_no_prices() {
        let lib = default_library();
        assert_eq!(lib.len(), 2);
        assert!(lib
            .iter()
            .all(|t| t.price_per_bf == 0.0 && t.sawn.is_empty()));
        // The sample board still plans, with nothing priced.
        let p = board(12, 4, |r, _| if r % 2 == 0 { "walnut" } else { "cherry" });
        let pl = calculate(&p, &lib).plan.expect("plan");
        assert!(pl.totals.bf_order > 0.0 && close(pl.totals.cost, 0.0));
    }

    #[test]
    fn an_imported_price_list_is_tidied_and_an_empty_one_refused() {
        let read = |text: &str| {
            serde_json::from_str::<MillCatalogue>(text)
                .unwrap()
                .cleaned()
        };
        // Only the timbers are required. Sizes with no price are dropped,
        // and so is a timber left with nothing; a bad colour is forgotten.
        let m = read(
            r##"{ "timbers": [
                { "name": "  Ash ", "color": "red", "sizes": [
                    { "thickness": 27, "width": 150, "price_per_m": 9.5 },
                    { "thickness": 27, "width": 200 },
                    { "thickness": 0, "width": 200, "price_per_m": 4 } ] },
                { "name": "Hickory", "color": "#C8A26B", "price_per_bf": 6.25 },
                { "name": "Unpriced", "sizes": [] } ] }"##,
        )
        .unwrap();
        assert_eq!(m.mill, "Your mill");
        assert!(m.fetched.is_empty() && close(m.step_m, 0.3));
        assert_eq!(m.timbers.len(), 2);
        assert_eq!(m.timbers[0].name, "Ash");
        assert!(m.timbers[0].color.is_none() && m.timbers[0].sizes.len() == 1);
        assert_eq!(m.timbers[1].color.as_deref(), Some("#C8A26B"));
        assert!(close(m.timbers[1].price_per_bf, 6.25) && m.timbers[1].sizes.is_empty());

        assert!(read(r#"{ "timbers": [] }"#).is_err());
        assert!(read(r#"{ "timbers": [{ "name": "Oak" }] }"#).is_err());
        assert!(serde_json::from_str::<MillCatalogue>(r#"{ "mill": "No timbers" }"#).is_err());
    }
}
