'use strict';
// End grain board calculator: the designer. All cutting-list maths happens in
// the Rust engine (/api/calc); this file draws the board, handles painting
// and resizing, and lays out the lists the engine returns.

const $ = (s, el = document) => el.querySelector(s);
const esc = (s) => String(s).replace(/[&<>"']/g, (c) => ({ '&': '&amp;', '<': '&lt;', '>': '&gt;', '"': '&quot;', "'": '&#39;' }[c]));

const DEFAULTS = {
  in: { kerf: 0.125, board_flatten: 0.125, panel_flatten: 0.0625, edge_trim: 0, end_waste: 2, min_strip_length: 12, rough_thickness: 0.25, rough_width: 0.25, rough_length: 1, waste_pct: 20, max_length: 0, sawn_step: 0.3, vat_pct: 20, sawn_whole: false, sawn_whole_waste: 50, sawn_big_as_bf: false },
  mm: { kerf: 3.2, board_flatten: 3, panel_flatten: 2, edge_trim: 0, end_waste: 50, min_strip_length: 300, rough_thickness: 6, rough_width: 6, rough_length: 25, waste_pct: 20, max_length: 0, sawn_step: 0.3, vat_pct: 20, sawn_whole: false, sawn_whole_waste: 50, sawn_big_as_bf: false },
};

const ALLOWANCES = [
  ['kerf', 'Saw kerf', 'Lost at every rip and every crosscut.'],
  ['max_length', 'Longest board I can carry', 'Nothing on the order, and no strip, will be longer. Try 2.4m or 8ft. Leave empty for no limit.'],
  ['board_flatten', 'End grain flattening', 'Extra slice length for flattening the finished board.'],
  ['panel_flatten', 'Panel flattening', 'Extra strip thickness for cleaning up the first glue-up.'],
  ['edge_trim', 'Edge trim', 'Extra on each outer strip for squaring the board. Leave at 0 for none.'],
  ['end_waste', 'Panel end waste', 'Spare length for a squaring cut and a safe last slice.'],
  ['min_strip_length', 'Shortest strip', 'Strips are never shorter than this, so they stay safe to mill.'],
  ['rough_thickness', 'Rough thickness loss', 'Planed off rough stock to get it flat.'],
  ['rough_width', 'Rough width loss', 'Jointed off each strip to get a straight edge.'],
  ['rough_length', 'Rough length loss', 'Trimmed off the ends of rough stock.'],
  ['waste_pct', 'Extra for defects', 'Added to the order for knots, checks and sapwood.'],
];

// Settings that are not sizes in the board's units.
const PLAIN_SETTINGS = ['waste_pct', 'sawn_step', 'vat_pct', 'sawn_whole', 'sawn_whole_waste', 'sawn_big_as_bf'];
const STOCKS = [4, 5, 6, 8, 10, 12, 16];
// Widths a mill's sawn boards usually come in, in millimetres.
const SAWN_WIDTHS = [50, 75, 100, 125, 150, 175, 200, 225];
const MAX_TRACKS = 40;
const GUT = 66; // room for the row tape, left of the board
const RUL = 34; // room for the column tape, above the board
const TAPE = 26;
// A drawing pin, 10 wide by 12 tall: the mark for a pinned size.
const PIN = 'M2.5 0h5v1.6h-.9l.4 3.4 2 2v1H5.6L5 12 4.4 8H1V7l2-2 .4-3.4h-.9z';
const PIN_W = 10;
const pinIcon = '<svg class="pin-i" viewBox="0 0 10 12" aria-hidden="true"><path d="' + PIN + '"/></svg>';

const S = {
  project: null,
  library: [],
  brush: null,
  result: null,
  savedSlug: null,
  savedJson: '',
  undo: [],
  redo: [],
  sel: null, // { axis: 'rows' | 'cols', i }
  openTimber: null,
  boards: [],
  mill: null, // your mill's price list, once one has been imported
  // The currency symbol, and what a new board starts with.
  prefs: { currency: '£', units: 'mm', order_by: 'bf', tax_pct: 0, setup_done: true, check_updates: true },
  setup: null, // the settings screen while it is open
  info: null,
  pending: null, // what to do once unsaved changes are dealt with
  confirmDelete: null,
  geo: null,
};

// ------------------------------------------------------------------ server

// In the desktop app the page can message its own window (print, quit,
// title). In a browser there is no window to message and this is null.
const host = window.ipc ? (msg) => window.ipc.postMessage(msg) : null;

async function api(method, path, body) {
  const res = await fetch(path, {
    method,
    headers: body !== undefined ? { 'Content-Type': 'application/json' } : {},
    body: body !== undefined ? JSON.stringify(body) : undefined,
  });
  const data = await res.json().catch(() => null);
  if (!res.ok) {
    const err = new Error((data && data.error) || res.statusText);
    err.status = res.status;
    throw err;
  }
  return data;
}

// ------------------------------------------------------------------- units

const units = () => S.project.units;
const snapStep = () => (units() === 'in' ? 1 / 16 : 1);
const gcd = (a, b) => (b ? gcd(b, a % b) : a);

/** 1.75 -> 1 3/4" (inches, to the nearest 1/32) or 44.5 mm. */
function fmt(v, { mark = true, u = units() } = {}) {
  if (!isFinite(v)) return '';
  if (u === 'mm') {
    const r = Math.round(v * 10) / 10;
    return (Number.isInteger(r) ? String(r) : r.toFixed(1)) + (mark ? ' mm' : '');
  }
  const neg = v < 0;
  let n = Math.round(Math.abs(v) * 32);
  const whole = Math.floor(n / 32);
  n -= whole * 32;
  let s = String(whole);
  if (n) {
    const g = gcd(n, 32);
    const frac = `${n / g}/${32 / g}`;
    s = whole ? `${whole} ${frac}` : frac;
  }
  return (neg ? '-' : '') + s + (mark ? '"' : '');
}

/** Reads 1 3/4, 1-3/4, 7/8, 1.75 (inches) or plain numbers (mm). */
function parseDim(text, u = units()) {
  // Lengths can be given in metres, centimetres or feet: 2.4m, 240cm, 8ft.
  const long = String(text).trim().toLowerCase().replace(',', '.').match(/^(\d*\.?\d+)\s*(m|cm|ft|')$/);
  if (long) {
    const mm = parseFloat(long[1]) * { m: 1000, cm: 10, ft: 304.8, "'": 304.8 }[long[2]];
    return u === 'mm' ? mm : mm / 25.4;
  }
  const s = String(text).replace(/["”″]|mm|in\b|inch(es)?/gi, '').replace(',', '.').trim();
  if (!s) return null;
  let m;
  if ((m = s.match(/^\d*\.?\d+$/))) return parseFloat(s);
  if (u === 'mm') return null;
  if ((m = s.match(/^(\d+)\s*\/\s*(\d+)$/))) return +m[2] ? +m[1] / +m[2] : null;
  if ((m = s.match(/^(\d+)[\s-]+(\d+)\s*\/\s*(\d+)$/))) return +m[3] ? +m[1] + +m[2] / +m[3] : null;
  return null;
}

const bf = (v) => v.toFixed(1);
/** A price with your currency symbol, safe to put straight into the page. */
const money = (v) => esc(S.prefs.currency) + v.toFixed(2);
const stockLabel = (q) => `${q}/4`;
/** Where a strip comes from: a sawn section or rough stock in quarters. */
const fromLabel = (item) =>
  item.sawn_width
    ? `${item.sawn} \u00d7 ${item.sawn_width} mm, no rip`
    : (item.sawn ? `${item.sawn} mm sawn` : stockLabel(item.stock)) + (item.on_edge ? ' on edge' : '');
const metres = (v) => `${Number(v.toFixed(2))} m`;
const plural = (n, one, many) => `${n} ${n === 1 ? one : many || one + 's'}`;

// Allowances that end up in a size you cut or set. In millimetres they are
// kept whole; the saw kerf is the blade's own and keeps its decimal.
const WHOLE_MM = ['board_flatten', 'panel_flatten', 'edge_trim', 'end_waste', 'min_strip_length', 'rough_thickness', 'rough_width', 'rough_length', 'max_length'];
/** An allowance as it is kept on a board in these units. */
const tidyAllowance = (key, v, u) => (u === 'mm' && WHOLE_MM.includes(key) ? (v > 0 ? Math.max(1, Math.round(v)) : 0) : v);

function convertUnits(to, p = S.project) {
  if (p.units === to) return;
  const f = to === 'mm' ? (v) => Math.round(v * 25.4) : (v) => Math.round((v / 25.4) * 32) / 32;
  p.units = to;
  p.width = f(p.width);
  p.depth = f(p.depth);
  p.thickness = f(p.thickness);
  for (const t of [...p.rows, ...p.cols]) if (t.size != null) t.size = f(t.size);
  p.settings.max_length = f(p.settings.max_length || 0);
  for (const k of Object.keys(p.settings)) {
    if (PLAIN_SETTINGS.includes(k) || k === 'max_length') continue;
    // In millimetres only the kerf keeps a decimal; in inches the small
    // allowances go to the nearest 1/64.
    p.settings[k] = to === 'mm' ? tidyAllowance(k, Math.round(p.settings[k] * 25.4 * 10) / 10, 'mm') : Math.round((p.settings[k] / 25.4) * 64) / 64;
  }
}

// ----------------------------------------------------------------- timbers

const UNKNOWN = { id: '', name: 'Unknown timber', color: '#9aa4aa', price_per_bf: 0, stock: null };
const timber = (id) => S.library.find((t) => t.id === id) || UNKNOWN;

function mix(hex, amt) {
  const n = parseInt(hex.slice(1), 16);
  const to = amt < 0 ? 0 : 255;
  const a = Math.abs(amt);
  const ch = (shift) => Math.round(((n >> shift) & 255) * (1 - a) + to * a);
  return `rgb(${ch(16)},${ch(8)},${ch(0)})`;
}
function luminance(hex) {
  const n = parseInt(hex.slice(1), 16);
  return (0.299 * ((n >> 16) & 255) + 0.587 * ((n >> 8) & 255) + 0.114 * (n & 255)) / 255;
}
const validHex = (c) => (/^#[0-9a-f]{6}$/i.test(c) ? c : UNKNOWN.color);

let libraryTimer = null;
function saveLibrarySoon() {
  clearTimeout(libraryTimer);
  libraryTimer = setTimeout(() => api('PUT', '/api/library', S.library).catch((e) => toast(e.message)), 400);
}

// ---------------------------------------------------------------- projects

/** What every new board takes from Settings and the mill's price list. */
function boardBasics() {
  return { order_by: S.prefs.order_by, mill: { vat_pct: S.prefs.tax_pct, ...(S.mill ? { sawn_step: S.mill.step_m } : {}) } };
}

function sampleProject() {
  const a = S.library[0] ? S.library[0].id : '';
  const b = S.library[1] ? S.library[1].id : a;
  const mm = S.prefs.units !== 'in';
  const sizes = mm ? [null, null, null, null, 25, 12, 25, 12, 25, 12, 25, 20] : [null, null, null, null, 1, 0.5, 1, 0.5, 1, 0.5, 1, 0.75];
  const ids = [a, a, a, a, b, a, b, a, b, a, b, a];
  const basics = boardBasics();
  return {
    name: 'Walnut and cherry stripes',
    units: mm ? 'mm' : 'in',
    width: mm ? 480 : 18,
    depth: mm ? 360 : 14,
    thickness: mm ? 45 : 1.75,
    cols: Array.from({ length: 12 }, () => ({ size: null })),
    rows: sizes.map((size) => ({ size })),
    cells: ids.map((id) => Array(12).fill(id)),
    slice_axis: 'auto',
    order_by: basics.order_by,
    settings: { ...DEFAULTS[mm ? 'mm' : 'in'], ...basics.mill },
  };
}

/** A new, empty board. It keeps the current board's size and allowances, and
    takes its units, tax and way of ordering from Settings. */
function blankProject() {
  const p = S.project;
  const id = S.brush || (S.library[0] ? S.library[0].id : '');
  const basics = boardBasics();
  const blank = {
    name: '',
    units: p.units,
    width: p.width,
    depth: p.depth,
    thickness: p.thickness,
    cols: Array.from({ length: 12 }, () => ({ size: null })),
    rows: Array.from({ length: 8 }, () => ({ size: null })),
    cells: Array.from({ length: 8 }, () => Array(12).fill(id)),
    slice_axis: 'auto',
    order_by: basics.order_by,
    settings: { ...p.settings, ...basics.mill },
  };
  convertUnits(S.prefs.units === 'in' ? 'in' : 'mm', blank);
  return blank;
}

/** Make sure a loaded board has a cell for every row and column. */
function normalize(p) {
  const fallback = S.library[0] ? S.library[0].id : '';
  if (!p.rows.length) p.rows = [{ size: null }];
  if (!p.cols.length) p.cols = [{ size: null }];
  p.cells = p.rows.map((_, r) => p.cols.map((_, c) => (p.cells[r] && p.cells[r][c] != null ? p.cells[r][c] : fallback)));
  p.settings = { ...DEFAULTS[p.units], ...(p.settings || {}) };
  // A board saved with part-millimetre allowances is tidied as it opens.
  for (const k of WHOLE_MM) p.settings[k] = tidyAllowance(k, p.settings[k], p.units);
  p.order_by = p.order_by === 'sawn' ? 'sawn' : 'bf';
  return p;
}

const isDirty = () => JSON.stringify(S.project) !== S.savedJson;

function setProject(p, slug) {
  S.project = normalize(p);
  S.savedSlug = slug;
  S.savedJson = JSON.stringify(S.project);
  S.undo = [];
  S.redo = [];
  S.sel = null;
  try {
    if (slug) localStorage.setItem('endgrain:last', slug);
  } catch (_) { /* private window: nothing to remember */ }
  afterChange();
}

function pushUndo() {
  S.undo.push(JSON.stringify(S.project));
  if (S.undo.length > 100) S.undo.shift();
  S.redo.length = 0;
}

/** Every edit to the board goes through here. */
function change(fn, { undo = true } = {}) {
  if (undo) pushUndo();
  fn(S.project);
  afterChange();
}

function afterChange() {
  syncInputs();
  recalc();
}

function stepHistory(from, to) {
  if (!from.length) return;
  to.push(JSON.stringify(S.project));
  S.project = JSON.parse(from.pop());
  afterChange();
}
const undo = () => stepHistory(S.undo, S.redo);
const redo = () => stepHistory(S.redo, S.undo);

/** The header's undo and redo buttons: lit only when there is a step to take. */
function syncHistory() {
  $('#undo').disabled = !S.undo.length;
  $('#redo').disabled = !S.redo.length;
}

function buildHistory() {
  const key = /Mac|iPhone|iPad/.test(navigator.platform) ? ['\u2318Z', '\u21e7\u2318Z'] : ['Ctrl+Z', 'Ctrl+Shift+Z'];
  $('#undo').title = `Undo (${key[0]})`;
  $('#redo').title = `Redo (${key[1]})`;
  $('#undo').addEventListener('click', undo);
  $('#redo').addEventListener('click', redo);
}

let calcSeq = 0;
async function recalc() {
  const mine = ++calcSeq;
  try {
    const result = await api('POST', '/api/calc', { project: S.project, library: S.library });
    if (mine !== calcSeq) return;
    S.result = result;
    render();
  } catch (e) {
    if (mine === calcSeq) toast(`Couldn't calculate: ${e.message}`);
  }
}

// ---------------------------------------------------- rows and columns

const sizesOf = (axis) => (S.result ? (axis === 'cols' ? S.result.col_sizes : S.result.row_sizes) : []);
const names = (axis) => (axis === 'cols' ? { one: 'column', many: 'columns', One: 'Column', dim: 'width', adj: 'wide' } : { one: 'row', many: 'rows', One: 'Row', dim: 'depth', adj: 'deep' });

/** Insert a copy of track `from` at position `at`. */
function addTrack(p, axis, at, from) {
  const tracks = p[axis];
  if (tracks.length >= MAX_TRACKS) return toast(`That's the most ${names(axis).many} a board can have here.`);
  tracks.splice(at, 0, { size: tracks[from].size });
  if (axis === 'rows') {
    p.cells.splice(at, 0, [...p.cells[from]]);
  } else {
    for (const row of p.cells) row.splice(at, 0, row[from]);
  }
}

/** Move track `from` so it sits in gap `slot` (0 = before the first, n = after the last). */
function moveTrack(p, axis, from, slot) {
  const to = slot > from ? slot - 1 : slot;
  if (to === from) return;
  const shift = (list) => list.splice(to, 0, list.splice(from, 1)[0]);
  shift(p[axis]);
  if (axis === 'rows') shift(p.cells);
  else for (const row of p.cells) shift(row);
}

function removeTrack(p, axis, i) {
  if (p[axis].length <= 1) return;
  p[axis].splice(i, 1);
  if (axis === 'rows') p.cells.splice(i, 1);
  else for (const row of p.cells) row.splice(i, 1);
}

// ------------------------------------------------------------ rail inputs

const dimInputs = [];

/** Wire a size field: commit on Enter or blur, arrows nudge by one step. */
function bindDim(el, get, set, { pct = false, positive = false, blank = false } = {}) {
  // `blank` fields may be left empty, which is stored as zero.
  const show = () => (blank && !get() ? '' : pct ? `${get()}%` : fmt(get()));
  const read = (text) => (blank && !String(text).trim() ? 0 : pct ? parseDim(String(text).replace('%', ''), 'mm') : parseDim(text));
  const commit = () => {
    const v = read(el.value);
    if (v == null || v < 0 || (positive && v <= 0)) {
      el.value = show();
      toast(pct ? 'That needs to be a number.' : units() === 'in' ? "Couldn't read that size. Try 1 3/4 or 1.75." : "Couldn't read that size. Try 44.5.");
      return;
    }
    if (Math.abs(v - get()) < 1e-9) {
      el.value = show();
      return;
    }
    change((p) => set(p, v));
  };
  el.addEventListener('change', commit);
  el.addEventListener('focus', () => el.select());
  el.addEventListener('keydown', (e) => {
    if (e.key === 'Enter') el.blur();
    else if (e.key === 'Escape') {
      el.value = show();
      el.blur();
    } else if (e.key === 'ArrowUp' || e.key === 'ArrowDown') {
      e.preventDefault();
      const step = pct ? 1 : snapStep();
      const base = read(el.value);
      const v = Math.max(positive ? step : 0, (base == null ? get() : base) + (e.key === 'ArrowUp' ? step : -step));
      change((p) => set(p, v));
      el.value = show();
      el.select();
    }
  });
  dimInputs.push({ el, show });
}

function buildRail() {
  bindDim($('#f-width'), () => S.project.width, (p, v) => (p.width = v), { positive: true });
  bindDim($('#f-depth'), () => S.project.depth, (p, v) => (p.depth = v), { positive: true });
  bindDim($('#f-thickness'), () => S.project.thickness, (p, v) => (p.thickness = v), { positive: true });

  ALLOWANCES.forEach(([key, label, help], i) => {
    const row = document.createElement('div');
    row.className = 'allow';
    const blank = key === 'max_length';
    row.innerHTML = `<label for="a-${key}">${label}</label><small>${help}</small><input id="a-${key}" class="dim" type="text" inputmode="decimal" autocomplete="off"${blank ? ' placeholder="No limit"' : ''}>`;
    (i < 2 ? $('#allow-main') : $('#allow-rest')).append(row);
    bindDim(
      $('input', row),
      () => S.project.settings[key],
      (p, v) => {
        p.settings[key] = tidyAllowance(key, v, p.units);
        if (p.settings[key] !== v) toast('Allowances are kept to whole millimetres. Only the saw kerf takes a decimal.');
      },
      { pct: key === 'waste_pct', blank },
    );
  });
  // Resetting allowances leaves the car and the mill's terms alone.
  $('#allow-reset').addEventListener('click', () =>
    change((p) => {
      const { max_length, sawn_step, vat_pct, sawn_whole, sawn_whole_waste, sawn_big_as_bf } = p.settings;
      p.settings = { ...DEFAULTS[p.units], max_length, sawn_step, vat_pct, sawn_whole, sawn_whole_waste, sawn_big_as_bf };
    }),
  );

  for (const stepper of document.querySelectorAll('.stepper')) {
    stepper.addEventListener('click', (e) => {
      const d = e.target.closest('button') && +e.target.closest('button').dataset.d;
      if (!d) return;
      const axis = stepper.dataset.axis;
      const n = S.project[axis].length;
      if (d < 0 && n <= 1) return;
      change((p) => (d > 0 ? addTrack(p, axis, n, n - 1) : removeTrack(p, axis, n - 1)));
    });
  }

  $('#name').addEventListener('input', (e) => {
    S.project.name = e.target.value;
    syncChrome();
  });
  $('#name').addEventListener('keydown', (e) => e.key === 'Enter' && e.target.blur());

  $('#units').addEventListener('click', (e) => {
    const u = e.target.dataset.u;
    if (u && u !== units()) change(() => convertUnits(u));
  });
}

function syncInputs() {
  for (const { el, show } of dimInputs) if (document.activeElement !== el) el.value = show();
  $('#n-cols').textContent = S.project.cols.length;
  $('#n-rows').textContent = S.project.rows.length;
  if (document.activeElement !== $('#name')) $('#name').value = S.project.name;
  for (const b of document.querySelectorAll('#units button')) b.classList.toggle('on', b.dataset.u === units());
  syncChrome();
}

let toldDirty = null;
function syncChrome() {
  syncHistory();
  const dirty = isDirty();
  const name = S.project.name.trim() || 'Untitled board';
  $('#dirty').hidden = !dirty;
  if (document.title !== `${name} | End Grain Calc`) {
    document.title = `${name} | End Grain Calc`;
    if (host) host(`title:${name}`);
  }
  if (host && dirty !== toldDirty) {
    toldDirty = dirty;
    host(dirty ? 'dirty:1' : 'dirty:0');
  }
}

/** The app window calls this when it is closed with unsaved changes. */
window.endgrainConfirmQuit = () => {
  $('#conflict').innerHTML = `<span><b>${esc(S.project.name.trim() || 'This board')}</b> has changes that aren't saved.</span>
    <button type="button" class="btn small" data-act="save-quit">Save and quit</button>
    <button type="button" class="btn small" data-act="quit">Quit without saving</button>
    <button type="button" class="btn small" data-act="stay">Keep working</button>`;
  // Red, because this is what is stopping the app from closing.
  $('#conflict').classList.add('urgent');
  $('#conflict').hidden = false;
};

async function copyText(text) {
  try {
    await navigator.clipboard.writeText(text);
    return true;
  } catch (_) {
    // The clipboard API isn't offered everywhere; fall back to a selection.
    const area = document.createElement('textarea');
    area.value = text;
    area.style.cssText = 'position:fixed;opacity:0';
    document.body.append(area);
    area.select();
    const ok = document.execCommand('copy');
    area.remove();
    return ok;
  }
}

// -------------------------------------------------------------- timber list

function renderTimber() {
  $('#timber').innerHTML = S.library
    .map((t, i) => {
      const open = S.openTimber === t.id;
      return `<li class="timber${t.id === S.brush ? ' on' : ''}" data-id="${esc(t.id)}">
        <button type="button" class="swatch" style="background:${validHex(t.color)}" title="Paint with ${esc(t.name)}" aria-label="Paint with ${esc(t.name)}" aria-pressed="${t.id === S.brush}">${i < 9 ? `<span>${i + 1}</span>` : ''}</button>
        <input class="t-name" type="text" value="${esc(t.name)}" aria-label="Timber name" autocomplete="off" spellcheck="false">
        <button type="button" class="t-more" aria-expanded="${open}" aria-label="Colour, prices and sizes for ${esc(t.name)}" title="Colour, prices and sizes">···</button>
        ${open ? `<div class="t-extra">
          <div class="t-top">
            <label>Colour <input type="color" value="${validHex(t.color)}"></label>
            <button type="button" class="btn small t-del">Remove this timber</button>
          </div>
          <div class="t-sec t-bf">
            <b>By the board foot</b>
            <small>Used when an order is written by the board foot. "Buy as" is the rough thickness to ask for; Best fit picks the one that uses least timber.</small>
            <div class="t-row">
              <label>Price <span class="t-price">${esc(S.prefs.currency)}<input type="text" inputmode="decimal" value="${t.price_per_bf > 0 ? t.price_per_bf.toFixed(2) : ''}" placeholder="none" aria-label="${esc(t.name)} price per board foot" autocomplete="off"></span></label>
              <label>Buy as <select aria-label="Rough thickness to buy ${esc(t.name)} in">${[null, ...STOCKS].map((q) => `<option value="${q || ''}"${(t.stock || null) === q ? ' selected' : ''}>${q ? stockLabel(q) : 'Best fit'}</option>`).join('')}</select></label>
            </div>
          </div>
          ${sawnEditor(t)}
        </div>` : ''}
      </li>`;
    })
    .join('');
}

/** Sawn thicknesses a timber has prices for, thinnest first. */
const sawnThicknesses = (t) => [...new Set((t.sawn || []).map((z) => z.thickness))].sort((a, b) => a - b);
/** The timber in the mill's price list that this one most likely is, going by name. */
const sawnPreset = (t) => {
  const mine = t.name.trim().toLowerCase();
  if (!S.mill || !mine) return null;
  return S.mill.timbers.find((m) => m.sizes.length && (m.name.toLowerCase().includes(mine) || mine.includes(m.name.toLowerCase()))) || null;
};
const millDate = () => (S.mill && S.mill.fetched ? new Date(S.mill.fetched + 'T12:00:00').toLocaleDateString(undefined, { day: 'numeric', month: 'short', year: 'numeric' }) : '');
/** Replace a timber's sawn sizes with the mill's sizes for one of its timbers. */
function fillFromMill(t, name) {
  const m = S.mill && S.mill.timbers.find((x) => x.name === name);
  if (!m || !m.sizes.length) return;
  t.sawn = m.sizes.map((z) => ({ ...z }));
  renderTimber();
  saveLibrarySoon();
  recalc();
}

function sawnEditor(t) {
  const blocks = sawnThicknesses(t)
    .map((th) => {
      // The usual widths, plus any others the mill lists at this thickness.
      const widths = [...new Set([...SAWN_WIDTHS, ...t.sawn.filter((x) => x.thickness === th).map((x) => x.width)])].sort((a, b) => a - b);
      const cells = widths
        .map((w) => {
          const z = t.sawn.find((x) => x.thickness === th && x.width === w);
          return `<label><span>${w}</span><input class="sawn-price" type="text" inputmode="decimal" autocomplete="off" data-t="${th}" data-w="${w}" value="${z && z.price_per_m > 0 ? z.price_per_m.toFixed(2) : ''}" placeholder="none" aria-label="${th} by ${w} mm, price per metre"></label>`;
        })
        .join('');
      return `<div class="sawn-block">
        <div class="sawn-t"><input class="sawn-thick" type="text" inputmode="decimal" autocomplete="off" data-t="${th}" value="${th}" aria-label="Sawn thickness in millimetres"> mm thick, by width in mm
          <button type="button" class="sawn-del" data-t="${th}" aria-label="Remove the ${th} mm sizes">&times;</button></div>
        <div class="sawn-grid">${cells}</div>
      </div>`;
    })
    .join('');
  const guess = sawnPreset(t);
  const listed = S.mill ? S.mill.timbers.filter((m) => m.sizes.length) : [];
  const fill = listed.length
    ? `<select class="sawn-fill" aria-label="Fill in from ${esc(S.mill.mill)}'s price list">
        <option value="">Fill in from ${esc(S.mill.mill)}…</option>
        ${listed.map((m) => `<option value="${esc(m.name)}">${esc(m.name)}${guess && guess.name === m.name ? ' (likely match)' : ''}</option>`).join('')}
      </select>`
    : '';
  return `<div class="t-sec sawn">
    <b>By sawn size</b>
    <small>What the mill charges per metre for each sawn size, before tax. Leave a width empty if they don't stock it.${listed.length ? ` ${esc(S.mill.mill)}'s price list ${millDate() ? `was read on ${millDate()}` : 'is loaded'}.` : ''}</small>
    ${blocks}
    <div class="sawn-acts">
      ${fill}
      <button type="button" class="btn small sawn-add">Add a thickness</button>
    </div>
  </div>`;
}

/** A pointer that carries a swatch of the timber it will paint. */
function brushCursor() {
  const c = validHex(timber(S.brush).color);
  const art = `<svg xmlns='http://www.w3.org/2000/svg' width='24' height='24'><path d='M1 9V1h8' fill='none' stroke='white' stroke-width='4'/><path d='M1 9V1h8' fill='none' stroke='black' stroke-width='2'/><rect x='8' y='8' width='15' height='15' rx='3' fill='white'/><rect x='9.5' y='9.5' width='12' height='12' rx='2' fill='${c}' stroke='black'/></svg>`;
  return `url("data:image/svg+xml,${encodeURIComponent(art).replace(/'/g, '%27')}") 1 1, crosshair`;
}

let timberIds = 0;
const newTimberId = () => 't' + Date.now().toString(36) + (timberIds++).toString(36) + Math.floor(Math.random() * 1296).toString(36);

/** Open a timber's ··· editor, bring it into view and put the cursor in it. */
function showTimber(id, sel) {
  if (!S.library.some((t) => t.id === id)) return;
  pickBrush(id);
  S.openTimber = id;
  renderTimber();
  const li = [...document.querySelectorAll('.timber')].find((x) => x.dataset.id === id);
  li.scrollIntoView({ block: 'center' });
  const el = sel && $(sel, li);
  if (!el) return;
  el.focus({ preventScroll: true });
  if (el.select) el.select();
}

function pickBrush(id) {
  if (S.brush === id) return;
  S.brush = id;
  if ($('#cells')) $('#cells').style.cursor = brushCursor();
  for (const li of document.querySelectorAll('.timber')) {
    li.classList.toggle('on', li.dataset.id === id);
    $('.swatch', li).setAttribute('aria-pressed', li.dataset.id === id);
  }
  renderPop();
}

function buildTimber() {
  const list = $('#timber');
  const at = (e) => {
    const li = e.target.closest('.timber');
    return li && S.library.find((t) => t.id === li.dataset.id);
  };
  list.addEventListener('focusin', (e) => {
    const t = at(e);
    if (t) pickBrush(t.id);
  });
  list.addEventListener('click', (e) => {
    const t = at(e);
    if (!t) return;
    pickBrush(t.id);
    if (e.target.closest('.t-more')) {
      S.openTimber = S.openTimber === t.id ? null : t.id;
      renderTimber();
    } else if (e.target.closest('.sawn-add, .sawn-del')) {
      t.sawn = t.sawn || [];
      if (e.target.closest('.sawn-del')) t.sawn = t.sawn.filter((z) => z.thickness !== +e.target.closest('.sawn-del').dataset.t);
      else {
        const have = sawnThicknesses(t);
        const next = [26, 50, 32, 38, 75, 100].find((th) => !have.includes(th)) || Math.max(...have) + 25;
        for (const w of SAWN_WIDTHS) t.sawn.push({ thickness: next, width: w, price_per_m: 0 });
      }
      renderTimber();
      saveLibrarySoon();
      recalc();
    } else if (e.target.closest('.t-del')) {
      if (S.project.cells.some((row) => row.includes(t.id))) return toast(`${t.name} is still on this board. Repaint those cells first.`);
      if (S.library.length <= 1) return toast('Keep at least one timber.');
      S.library = S.library.filter((x) => x !== t);
      S.brush = S.library[0].id;
      renderTimber();
      saveLibrarySoon();
      recalc();
    }
  });
  list.addEventListener('input', (e) => {
    const t = at(e);
    if (!t) return;
    const el = e.target;
    if (el.classList.contains('t-name')) t.name = el.value;
    else if (el.type === 'color') {
      t.color = el.value;
      $('.swatch', el.closest('.timber')).style.background = el.value;
    } else if (el.classList.contains('sawn-fill')) return; // applied below, on change
    else if (el.tagName === 'SELECT') t.stock = el.value ? +el.value : null;
    else if (el.classList.contains('sawn-thick')) return; // applied when you leave the field
    else if (el.classList.contains('sawn-price')) {
      const v = parseFloat(el.value.replace(',', '.'));
      const z = t.sawn.find((x) => x.thickness === +el.dataset.t && x.width === +el.dataset.w);
      if (z) z.price_per_m = isFinite(v) && v > 0 ? v : 0;
      else t.sawn.push({ thickness: +el.dataset.t, width: +el.dataset.w, price_per_m: isFinite(v) && v > 0 ? v : 0 });
    } else if (el.closest('.t-price')) {
      const v = parseFloat(el.value.replace(',', '.'));
      t.price_per_bf = isFinite(v) && v >= 0 ? v : 0;
    } else return;
    saveLibrarySoon();
    recalc();
  });
  list.addEventListener('change', (e) => {
    const t = at(e);
    if (!t) return;
    const el = e.target;
    if (el.closest('.t-price')) el.value = t.price_per_bf > 0 ? t.price_per_bf.toFixed(2) : '';
    else if (el.classList.contains('sawn-fill')) {
      if (el.value) fillFromMill(t, el.value);
    } else if (el.classList.contains('sawn-thick')) {
      const from = +el.dataset.t;
      const to = parseFloat(el.value.replace(',', '.'));
      if (isFinite(to) && to > 0 && to !== from && !sawnThicknesses(t).includes(to)) {
        for (const z of t.sawn) if (z.thickness === from) z.thickness = to;
        saveLibrarySoon();
        recalc();
      } else if (to !== from) toast(isFinite(to) && to > 0 ? `${t.name} already has ${to} mm sizes.` : 'Thickness needs to be a number of millimetres.');
      renderTimber();
    }
  });
  list.addEventListener('keydown', (e) => e.key === 'Enter' && e.target.tagName === 'INPUT' && e.target.blur());

  $('#import-prices').addEventListener('click', () => openSetup(false, true));

  $('#add-timber').addEventListener('click', () => {
    const t = { id: newTimberId(), name: 'New timber', color: '#a8784c', price_per_bf: 0, stock: null, sawn: [] };
    S.library.push(t);
    S.brush = t.id;
    S.openTimber = t.id;
    renderTimber();
    saveLibrarySoon();
    const input = $(`.timber[data-id="${t.id}"] .t-name`);
    input.focus();
    input.select();
  });
}

// -------------------------------------------------------------------- board

function hash(str) {
  let h = 2166136261;
  for (let i = 0; i < str.length; i++) h = Math.imul(h ^ str.charCodeAt(i), 16777619);
  return h >>> 0;
}

/** Growth rings for one cell. The pith sits past a corner, and neighbours
    mirror each other the way bookmatched strips do. */
function rings(w, h, r, c, id, color) {
  if (w < 9 || h < 9) return '';
  const seed = hash(id + ':' + r + ':' + c);
  const j1 = (seed % 97) / 97;
  const j2 = ((seed >>> 8) % 89) / 89;
  const m = Math.max(w, h);
  let cx = w + m * (0.15 + 0.5 * j1);
  let cy = h + m * (0.3 + 0.9 * j2);
  if (c % 2) cx = w - cx;
  if (r % 2) cy = h - cy;
  const far = Math.hypot(Math.max(Math.abs(cx), Math.abs(cx - w)), Math.max(Math.abs(cy), Math.abs(cy - h)));
  const near = Math.hypot(Math.min(Math.abs(cx), Math.abs(cx - w)), Math.min(Math.abs(cy), Math.abs(cy - h)));
  const step = Math.max(3.5, (far - near) / 9);
  const stroke = luminance(color) < 0.3 ? mix(color, 0.2) : mix(color, -0.22);
  let out = '';
  for (let rad = near + step * (0.3 + 0.5 * j1), i = 0; rad < far; rad += step, i++) {
    out += `<circle class="ring" cx="${cx.toFixed(1)}" cy="${cy.toFixed(1)}" r="${rad.toFixed(1)}" stroke="${stroke}" stroke-width="${i % 3 ? 0.8 : 1.4}" stroke-opacity="0.55"/>`;
  }
  return out;
}

function renderBoard() {
  const r = S.result;
  const svg = $('#board');
  const wrap = $('#board-wrap');
  if (!r) return;
  const cs = r.col_sizes;
  const rs = r.row_sizes;
  const W = Math.max(r.actual_width, 1e-6);
  const D = Math.max(r.actual_depth, 1e-6);
  const k = Math.min(Math.max(160, wrap.clientWidth - GUT - 16) / W, Math.max(160, wrap.clientHeight - RUL - 20) / D);
  const edges = (sizes) => sizes.reduce((acc, v) => (acc.push(acc[acc.length - 1] + v * k), acc), [0]);
  const xs = edges(cs);
  const ys = edges(rs);
  const bw = W * k;
  const bh = D * k;
  S.geo = { k, xs, ys };

  const inch = units() === 'in';
  const p = S.project;
  let h = '';

  // Tapes with a tick at every whole unit.
  h += `<rect class="tape-bg" x="${GUT}" y="0" width="${bw}" height="${TAPE}" rx="2"/>`;
  h += `<rect class="tape-bg" x="0" y="${RUL}" width="${GUT - 8}" height="${bh}" rx="2"/>`;
  let unit = inch ? 1 : 10;
  while (unit * k < 5) unit *= 2;
  for (let u = unit; u < W - 1e-6; u += unit) h += `<line class="tick" x1="${GUT + u * k}" x2="${GUT + u * k}" y1="${TAPE - 5}" y2="${TAPE}"/>`;
  for (let u = unit; u < D - 1e-6; u += unit) h += `<line class="tick" y1="${RUL + u * k}" y2="${RUL + u * k}" x1="${GUT - 13}" x2="${GUT - 8}"/>`;

  const track = (axis, i, x, y, w, ht) => {
    const t = p[axis][i];
    const label = fmt(sizesOf(axis)[i], { mark: inch });
    const pinned = t.size != null;
    // A pinned size is bold, on a darker band, with a pin beside it where
    // there is room. The band alone still shows on the narrowest column.
    const wide = label.length * (pinned ? 6.6 : 5.6);
    const fits = axis === 'cols' ? w >= wide + 4 : ht >= 13;
    const sc = Math.min(0.9, Math.max(0.6, (ht - 3) / 12));
    const pin = pinned && fits && w >= wide + PIN_W * sc + 6;
    const cx = x + w / 2 + (pin ? (PIN_W * sc) / 2 + 1 : 0);
    const cy = y + ht / 2;
    const sel = S.sel && S.sel.axis === axis && S.sel.i === i;
    const n = names(axis);
    const box = `x="${x}" y="${y}" width="${Math.max(w, 0)}" height="${Math.max(ht, 0)}"`;
    return `<g class="track${pinned ? ' pinned' : ''}${sel ? ' sel' : ''}" data-axis="${axis}" data-i="${i}">
      <title>${n.One} ${i + 1}: ${fmt(sizesOf(axis)[i])}, ${pinned ? 'pinned' : 'not pinned, so it shares the leftover space'}. Click to size it, drag to move it.</title>
      ${pinned ? `<rect class="pin-band" ${box}/><rect class="pin-edge" x="${x}" y="${y}" width="${axis === 'cols' ? Math.max(w, 0) : 3}" height="${axis === 'cols' ? 3 : Math.max(ht, 0)}"/>` : ''}
      <rect class="seg-hit" ${box}/>
      ${pin ? `<path class="pin" transform="translate(${cx - wide / 2 - PIN_W * sc - 2},${cy - 6 * sc + (axis === 'cols' ? 1 : 0)}) scale(${sc})" d="${PIN}"/>` : ''}
      ${fits ? `<text x="${cx}" y="${cy + 4.5}" text-anchor="middle">${label}</text>` : ''}
    </g>`;
  };
  cs.forEach((_, j) => (h += track('cols', j, GUT + xs[j], 0, xs[j + 1] - xs[j], TAPE)));
  rs.forEach((_, i) => (h += track('rows', i, 0, RUL + ys[i], GUT - 8, ys[i + 1] - ys[i])));
  for (let j = 1; j < cs.length; j++) h += `<line class="join" x1="${GUT + xs[j]}" x2="${GUT + xs[j]}" y1="0" y2="${TAPE}"/>`;
  for (let i = 1; i < rs.length; i++) h += `<line class="join" y1="${RUL + ys[i]}" y2="${RUL + ys[i]}" x1="0" x2="${GUT - 8}"/>`;

  // The board itself.
  h += `<defs><clipPath id="board-clip"><rect width="${bw}" height="${bh}" rx="5"/></clipPath></defs>`;
  h += `<g id="cells" transform="translate(${GUT},${RUL})" style='cursor: ${brushCursor()}'><g clip-path="url(#board-clip)">`;
  h += `<rect id="cells-bg" width="${bw}" height="${bh}" fill="${UNKNOWN.color}"/>`;
  for (let i = 0; i < rs.length; i++) {
    for (let j = 0; j < cs.length; j++) {
      const w = xs[j + 1] - xs[j];
      const ht = ys[i + 1] - ys[i];
      if (w <= 0 || ht <= 0) continue;
      const id = p.cells[i][j];
      const color = validHex(timber(id).color);
      h += `<svg x="${xs[j]}" y="${ys[i]}" width="${w}" height="${ht}"><rect width="100%" height="100%" fill="${color}"/>${rings(w, ht, i, j, id, color)}</svg>`;
    }
  }
  for (let j = 1; j < cs.length; j++) h += `<line class="seam" x1="${xs[j]}" x2="${xs[j]}" y1="0" y2="${bh}"/>`;
  for (let i = 1; i < rs.length; i++) h += `<line class="seam" y1="${ys[i]}" y2="${ys[i]}" x1="0" x2="${bw}"/>`;
  h += `</g><rect class="edge" width="${bw}" height="${bh}" rx="5"/></g>`;

  // Grab zones on the joins, drawn last so they sit on top.
  for (let j = 1; j < cs.length; j++) h += `<rect class="handle h-cols" data-axis="cols" data-i="${j - 1}" x="${GUT + xs[j] - 5}" y="0" width="10" height="${RUL}"><title>Drag to resize</title></rect>`;
  for (let i = 1; i < rs.length; i++) h += `<rect class="handle h-rows" data-axis="rows" data-i="${i - 1}" x="0" y="${RUL + ys[i] - 5}" width="${GUT}" height="10"><title>Drag to resize</title></rect>`;

  svg.setAttribute('width', GUT + bw + 2);
  svg.setAttribute('height', RUL + bh + 2);
  svg.setAttribute('viewBox', `0 0 ${GUT + bw + 2} ${RUL + bh + 2}`);
  svg.innerHTML = h;
}

function cellAt(x, y) {
  const bg = $('#cells-bg');
  if (!bg || !S.geo) return null;
  const box = bg.getBoundingClientRect();
  const px = x - box.left;
  const py = y - box.top;
  if (px < 0 || py < 0 || px >= box.width || py >= box.height) return null;
  const find = (edges, v) => {
    for (let i = 0; i < edges.length - 1; i++) if (v < edges[i + 1]) return i;
    return edges.length - 2;
  };
  return { r: find(S.geo.ys, py), c: find(S.geo.xs, px) };
}

function buildBoard() {
  const svg = $('#board');
  let drag = null;

  // A fast drag reports only a few points, so paint every cell on the line
  // from the last point to this one.
  const paint = (e) => {
    if (S.brush == null) return;
    const from = drag.last || { x: e.clientX, y: e.clientY };
    const steps = Math.max(1, Math.ceil(Math.hypot(e.clientX - from.x, e.clientY - from.y) / 3));
    let touched = false;
    for (let s = 0; s <= steps; s++) {
      const hit = cellAt(from.x + ((e.clientX - from.x) * s) / steps, from.y + ((e.clientY - from.y) * s) / steps);
      if (hit && S.project.cells[hit.r][hit.c] !== S.brush) {
        S.project.cells[hit.r][hit.c] = S.brush;
        touched = true;
      }
    }
    drag.last = { x: e.clientX, y: e.clientY };
    if (touched) {
      drag.changed = true;
      afterChange();
    }
  };

  svg.addEventListener('pointerdown', (e) => {
    if (e.button !== 0) return;
    const handle = e.target.closest('.handle');
    const seg = e.target.closest('.track');
    if (handle) {
      const axis = handle.dataset.axis;
      const i = +handle.dataset.i;
      const sizes = sizesOf(axis);
      drag = { kind: 'resize', axis, i, start: axis === 'cols' ? e.clientX : e.clientY, a: sizes[i], b: sizes[i + 1], before: JSON.stringify(S.project), changed: false };
      closePop();
    } else if (seg) {
      // A press on the tape is a click (open the sizing box) unless the
      // pointer travels, in which case the row or column is being moved.
      drag = { kind: 'move', axis: seg.dataset.axis, i: +seg.dataset.i, x: e.clientX, y: e.clientY, moving: false, slot: null };
    } else if (cellAt(e.clientX, e.clientY)) {
      const hit = cellAt(e.clientX, e.clientY);
      if (e.altKey) return pickBrush(S.project.cells[hit.r][hit.c]);
      drag = { kind: 'paint', before: JSON.stringify(S.project), changed: false };
      preview(null);
      closePop();
      paint(e);
    } else return;
    e.preventDefault();
    svg.setPointerCapture(e.pointerId);
  });

  // With no button down, show what a click here would paint.
  const preview = (e) => {
    const hit = e && S.brush != null ? cellAt(e.clientX, e.clientY) : null;
    const old = $('#cell-preview');
    if (!hit) return old && old.remove();
    if (old && old.dataset.at === `${hit.r},${hit.c}` && old.dataset.brush === S.brush) return;
    if (old) old.remove();
    const { xs, ys } = S.geo;
    const same = S.project.cells[hit.r][hit.c] === S.brush;
    svg.insertAdjacentHTML(
      'beforeend',
      `<rect id="cell-preview" class="${same ? 'same' : ''}" data-at="${hit.r},${hit.c}" data-brush="${esc(S.brush)}" x="${GUT + xs[hit.c]}" y="${RUL + ys[hit.r]}" width="${xs[hit.c + 1] - xs[hit.c]}" height="${ys[hit.r + 1] - ys[hit.r]}" fill="${validHex(timber(S.brush).color)}"/>`,
    );
  };
  svg.addEventListener('pointerleave', () => preview(null));

  svg.addEventListener('pointermove', (e) => {
    if (!drag) return preview(e);
    if (drag.kind === 'paint') return paint(e);
    if (drag.kind === 'move') return moveDrag(e);
    const step = snapStep();
    const delta = ((drag.axis === 'cols' ? e.clientX : e.clientY) - drag.start) / S.geo.k;
    const total = drag.a + drag.b;
    let a = Math.round((drag.a + delta) / step) * step;
    a = Math.min(Math.max(a, step), total - step);
    if (!(a > 0) || total - a <= 0) return;
    const tracks = S.project[drag.axis];
    const round = (v) => Math.round(v * 1e6) / 1e6;
    if (tracks[drag.i].size === round(a) && tracks[drag.i + 1].size === round(total - a)) return;
    tracks[drag.i].size = round(a);
    tracks[drag.i + 1].size = round(total - a);
    drag.changed = true;
    afterChange();
  });

  // Moving a row or column: a tinted copy follows the pointer and a line
  // shows the gap it will drop into.
  const moveDrag = (e) => {
    const cols = drag.axis === 'cols';
    const travel = cols ? e.clientX - drag.x : e.clientY - drag.y;
    if (!drag.moving) {
      if (Math.hypot(e.clientX - drag.x, e.clientY - drag.y) < 5) return;
      drag.moving = true;
      if (S.sel) closePop();
      svg.classList.add('moving');
    }
    const edges = cols ? S.geo.xs : S.geo.ys;
    const box = $('#cells-bg').getBoundingClientRect();
    const at = cols ? e.clientX - box.left : e.clientY - box.top;
    // The gap whose neighbours' midpoints the pointer sits between.
    let slot = 0;
    while (slot < edges.length - 1 && at > (edges[slot] + edges[slot + 1]) / 2) slot++;
    drag.slot = slot;
    const stays = slot === drag.i || slot === drag.i + 1;

    const [a, b] = [edges[drag.i], edges[drag.i + 1]];
    const bw = S.geo.xs[S.geo.xs.length - 1];
    const bh = S.geo.ys[S.geo.ys.length - 1];
    const ghost = cols
      ? `<rect class="move-ghost" x="${GUT + a + travel}" y="0" width="${b - a}" height="${RUL + bh}"/>`
      : `<rect class="move-ghost" x="0" y="${RUL + a + travel}" width="${GUT + bw}" height="${b - a}"/>`;
    const line = stays
      ? ''
      : cols
        ? `<line class="move-line" x1="${GUT + edges[slot]}" x2="${GUT + edges[slot]}" y1="0" y2="${RUL + bh}"/>`
        : `<line class="move-line" y1="${RUL + edges[slot]}" y2="${RUL + edges[slot]}" x1="0" x2="${GUT + bw}"/>`;
    const old = $('#move-marks');
    if (old) old.remove();
    svg.insertAdjacentHTML('beforeend', `<g id="move-marks">${ghost}${line}</g>`);
  };

  // `cancelled` is true when the system takes the pointer away mid-drag.
  const end = (cancelled) => {
    if (!drag) return;
    if (drag.kind === 'move') {
      const { axis, i, moving, slot } = drag;
      drag = null;
      svg.classList.remove('moving');
      const marks = $('#move-marks');
      if (marks) marks.remove();
      if (cancelled) return;
      if (!moving) {
        S.sel = S.sel && S.sel.axis === axis && S.sel.i === i ? null : { axis, i };
        renderBoard();
        renderPop();
      } else if (slot != null && slot !== i && slot !== i + 1) {
        change((p) => moveTrack(p, axis, i, slot));
      }
      return;
    }
    if (drag.changed) {
      S.undo.push(drag.before);
      if (S.undo.length > 100) S.undo.shift();
      S.redo.length = 0;
      syncHistory();
    }
    drag = null;
  };
  svg.addEventListener('pointerup', () => end(false));
  svg.addEventListener('pointercancel', () => end(true));
}

// ----------------------------------------------------- row / column popover

function closePop() {
  if (!S.sel) return;
  S.sel = null;
  renderBoard();
  renderPop();
}

function renderPop() {
  const pop = $('#pop');
  const sel = S.sel;
  if (!sel || !S.result || sel.i >= S.project[sel.axis].length) {
    S.sel = null;
    pop.hidden = true;
    return;
  }
  const n = names(sel.axis);
  const t = S.project[sel.axis][sel.i];
  const size = sizesOf(sel.axis)[sel.i];
  const pinned = t.size != null;
  const brush = timber(S.brush);
  const [before, after] = sel.axis === 'cols' ? ['Add one left', 'Add one right'] : ['Add one above', 'Add one below'];
  const editing = document.activeElement && document.activeElement.id === 'pop-size';
  if (!editing) {
    pop.innerHTML = `
      <div class="pop-head"><strong>${n.One} ${sel.i + 1}</strong><button type="button" data-act="close" aria-label="Close">&times;</button></div>
      <label for="pop-size">${n.dim === 'width' ? 'Width' : 'Depth'}</label>
      <input id="pop-size" class="dim" type="text" inputmode="decimal" autocomplete="off" value="${esc(fmt(size))}">
      <div class="seg pop-pin" role="group" aria-label="Pinned or not">
        <button type="button" data-act="auto" class="${pinned ? '' : 'on'}" aria-pressed="${!pinned}">Not pinned</button>
        <button type="button" data-act="pin" class="${pinned ? 'on' : ''}" aria-pressed="${pinned}">${pinIcon}Pinned</button>
      </div>
      <p class="pop-note">${pinned
        ? `Stays at <b>${fmt(size)}</b> whatever else changes.`
        : `Takes an equal share of the ${n.dim} the pinned ${n.many} leave, so it changes when the board does. Type a size, or choose Pinned, to hold it.`}</p>
      <div class="pop-acts">
        <button type="button" class="wide" data-act="fill"><i style="background:${validHex(brush.color)}"></i>Paint this ${n.one} ${esc(brush.name)}</button>
        <button type="button" data-act="before">${before}</button>
        <button type="button" data-act="after">${after}</button>
        <button type="button" data-act="equal">Make all equal</button>
        <button type="button" data-act="remove"${S.project[sel.axis].length <= 1 ? ' disabled' : ''}>Remove</button>
      </div>`;
  }
  pop.hidden = false;

  const seg = $(`.track[data-axis="${sel.axis}"][data-i="${sel.i}"] .seg-hit`);
  if (!seg) return;
  const box = seg.getBoundingClientRect();
  const pw = pop.offsetWidth;
  const ph = pop.offsetHeight;
  let left, top;
  if (sel.axis === 'cols') {
    left = box.left + box.width / 2 - pw / 2;
    top = box.top - ph - 10 >= 8 ? box.top - ph - 10 : box.bottom + 10;
  } else {
    left = box.left - pw - 10 >= 8 ? box.left - pw - 10 : box.right + 18;
    top = box.top + box.height / 2 - ph / 2;
  }
  pop.style.left = Math.min(Math.max(8, left), innerWidth - pw - 8) + 'px';
  pop.style.top = Math.min(Math.max(8, top), innerHeight - ph - 8) + 'px';
}

function buildPop() {
  const pop = $('#pop');
  pop.addEventListener('click', (e) => {
    const act = e.target.closest('[data-act]') && e.target.closest('[data-act]').dataset.act;
    if (!act || !S.sel) return;
    const { axis, i } = S.sel;
    if (act === 'close') return closePop();
    if (act === 'fill') {
      change((p) => {
        if (axis === 'rows') p.cells[i] = p.cells[i].map(() => S.brush);
        else for (const row of p.cells) row[i] = S.brush;
      });
    } else if (act === 'auto') {
      if (S.project[axis][i].size != null) change((p) => (p[axis][i].size = null));
    } else if (act === 'pin') {
      // Hold it at the size it has now, to the nearest step you can cut to.
      const step = snapStep();
      const v = Math.max(step, Math.round(sizesOf(axis)[i] / step) * step);
      if (S.project[axis][i].size == null) change((p) => (p[axis][i].size = v));
    } else if (act === 'equal') change((p) => p[axis].forEach((t) => (t.size = null)));
    else if (act === 'before') {
      change((p) => addTrack(p, axis, i, i));
      S.sel = { axis, i: i + 1 };
    } else if (act === 'after') change((p) => addTrack(p, axis, i + 1, i));
    else if (act === 'remove') {
      S.sel = null;
      change((p) => removeTrack(p, axis, i));
    }
  });
  pop.addEventListener('change', (e) => {
    if (e.target.id !== 'pop-size' || !S.sel) return;
    const { axis, i } = S.sel;
    const v = parseDim(e.target.value);
    if (v == null || v <= 0) {
      e.target.value = fmt(sizesOf(axis)[i]);
      return toast(units() === 'in' ? "Couldn't read that size. Try 1 3/4 or 1.75." : "Couldn't read that size. Try 44.5.");
    }
    change((p) => (p[axis][i].size = v));
  });
  pop.addEventListener('keydown', (e) => {
    if (e.target.id !== 'pop-size') return;
    if (e.key === 'Enter') e.target.blur();
    else if (e.key === 'ArrowUp' || e.key === 'ArrowDown') {
      e.preventDefault();
      const { axis, i } = S.sel;
      const step = snapStep();
      const base = parseDim(e.target.value) ?? sizesOf(axis)[i];
      const v = Math.max(step, Math.round((base + (e.key === 'ArrowUp' ? step : -step)) / step) * step);
      e.target.value = fmt(v);
      change((p) => (p[axis][i].size = v));
    }
  });
  pop.addEventListener('focusin', (e) => e.target.id === 'pop-size' && e.target.select());
  document.addEventListener('pointerdown', (e) => {
    // The board redraws itself on a click, which detaches e.target, so ask
    // the event where it came from rather than walking up from the target.
    const path = e.composedPath();
    if (S.sel && !path.includes(pop) && !path.includes($('#board'))) closePop();
  });
}

// --------------------------------------------------------- issues and axis

function renderIssues() {
  const r = S.result;
  let h = '';
  for (const is of r.issues) {
    const n = names(is.axis);
    if (is.kind === 'mismatch') {
      h += `<div class="issue"><p>The ${n.many} add up to <b>${fmt(is.fixed_sum)}</b>, but the board is <b>${fmt(is.target)}</b> ${n.adj}.</p>
        <button type="button" class="btn small" data-fix="fit" data-axis="${is.axis}" data-v="${is.fixed_sum}">Make the board ${fmt(is.fixed_sum)} ${n.adj}</button>
        <button type="button" class="btn small" data-fix="slack" data-axis="${is.axis}">Let the last ${n.one} take up the difference</button></div>`;
    } else {
      h += `<div class="issue"><p>The pinned ${n.many} add up to <b>${fmt(is.fixed_sum)}</b>, which leaves nothing for the rest of a board <b>${fmt(is.target)}</b> ${n.adj}. Shrink one, or make the board ${n.adj}r.</p>
        <button type="button" class="btn small" data-fix="equal" data-axis="${is.axis}">Make all ${n.many} equal</button></div>`;
    }
  }
  for (const msg of r.errors) h += `<div class="issue"><p>${esc(msg)}</p></div>`;

  // In millimetres, unpinned rows or columns that share out to a fraction
  // are awkward to mill: offer the nearest board sizes that give whole ones.
  if (units() === 'mm' && !r.issues.length && !r.errors.length) {
    for (const axis of ['cols', 'rows']) {
      const n = names(axis);
      const tracks = S.project[axis];
      const free = tracks.filter((t) => t.size == null).length;
      const each = sizesOf(axis)[tracks.findIndex((t) => t.size == null)];
      if (!free || Math.abs(each - Math.round(each)) < 0.001) continue;
      const pinned = tracks.reduce((sum, t) => sum + (t.size || 0), 0);
      const offer = [Math.ceil(each), Math.floor(each)]
        .filter((whole) => whole >= 1)
        .map((whole) => `<button type="button" class="btn small" data-fix="fit" data-axis="${axis}" data-v="${pinned + free * whole}">${fmt(pinned + free * whole)} ${n.adj}, for ${fmt(whole)} ${free === 1 ? n.one : n.many}</button>`)
        .join('');
      h += `<div class="issue note"><p>The ${free === 1 ? `unpinned ${n.one} comes` : `${free} unpinned ${n.many} come`} to <b>${fmt(each)}</b>${free === 1 ? '' : ' each'}. For whole millimetres, make the board:</p>${offer}</div>`;
    }
  }
  $('#issues').innerHTML = h;
}

function renderAxis() {
  const r = S.result;
  const cur = S.project.slice_axis;
  const count = (axis) => {
    const o = r.options.find((x) => x.axis === axis);
    return o ? ` (${plural(o.panels, 'glue-up')})` : '';
  };
  const btn = (axis, label) => `<button type="button" data-axis="${axis}" class="${cur === axis ? 'on' : ''}">${label}</button>`;
  const resolved = r.plan && r.plan.axis;
  $('#axis').innerHTML = `<span>Crosscut the first glue-up into</span>
    <div class="seg">${btn('auto', 'Whichever needs fewer')}${btn('cols', 'Columns' + count('cols'))}${btn('rows', 'Rows' + count('rows'))}</div>
    <p class="axis-note">${
      resolved === 'cols'
        ? 'Each slice becomes one column of the board. Each strip in the glue-up shows as one row, running left to right.'
        : resolved === 'rows'
        ? 'Each slice becomes one row of the board. Each strip in the glue-up shows as one column, running top to bottom.'
        : ''
    }</p>`;
}

// ------------------------------------------------------------------ results

function ranges(indices) {
  const xs = [...indices].sort((a, b) => a - b);
  const out = [];
  for (let i = 0; i < xs.length; i++) {
    let j = i;
    while (j + 1 < xs.length && xs[j + 1] === xs[j] + 1) j++;
    out.push(j > i ? `${xs[i] + 1}–${xs[j] + 1}` : `${xs[i] + 1}`);
    i = j;
  }
  return out.join(', ');
}

function boardLength(line) {
  const feet = (line.bf_order * 8) / line.stock;
  return units() === 'in' ? `${feet.toFixed(1)} ft of 6" wide board` : `${(feet * 0.3048).toFixed(1)} m of 150 mm wide board`;
}

/** Spread label positions so none sit closer than `gap`, each staying as
    near as it can to where it wants to be, all within `lo` to `hi`. */
function spreadLabels(ideal, gap, lo, hi) {
  // Labels that would collide are merged into a block and moved together.
  const blocks = [];
  for (const y of ideal) {
    blocks.push({ n: 1, sum: y });
    while (blocks.length > 1) {
      const a = blocks[blocks.length - 2];
      const c = blocks[blocks.length - 1];
      if (a.sum / a.n + a.n * gap <= c.sum / c.n) break;
      a.sum += c.sum - c.n * a.n * gap;
      a.n += c.n;
      blocks.pop();
    }
  }
  const out = [];
  for (const b of blocks) for (let j = 0; j < b.n; j++) out.push(b.sum / b.n + j * gap);
  for (let i = 0; i < out.length; i++) out[i] = Math.max(out[i], i ? out[i - 1] + gap : lo);
  for (let i = out.length - 1; i >= 0; i--) out[i] = Math.min(out[i], i < out.length - 1 ? out[i + 1] - gap : hi);
  return out;
}

/** A glue-up, drawn the way it relates to the board picture, with its
    strips numbered in gluing order and labelled with finished width and timber.
    When slices become columns the strips are stacked top to bottom, like the
    board's rows, and labelled down the left. When slices become rows
    (`upright`) the strips stand side by side, like the board's columns, and
    are labelled along the top. */
function panelSvg(pn, upright) {
  const kerf = S.project.settings.kerf;
  const n = pn.strips.length;
  const widths = pn.strips.map((st) => fmt(st.width));
  const timbers = pn.strips.map((st) => {
    const name = timber(st.species).name;
    return name.length > 16 ? name.slice(0, 15).trimEnd() + '…' : name;
  });
  const ROOM = 424; // what the lists panel has across it
  // Labels need this much room each along the row of numbers. Upright
  // panels with a great many strips get smaller numbers so they still fit.
  const GAP = upright ? Math.max(14, Math.min(20, Math.floor(ROOM / n))) : 20;
  const BADGE = GAP / 2 - 1;
  // A label, reading away from the panel: number, finished width, timber.
  // Text sizes are estimated from the number of characters.
  const nameLen = Math.max(...timbers.map((x) => x.length)) * 6.6 + 4;
  const widthLen = Math.max(...widths.map((x) => x.length)) * 7.8;
  const widthEnd = nameLen + 10 + widthLen; // where the widths stop, nearest the panel
  const badgeAt = widthEnd + 9 + BADGE; // centre of the numbers
  const edge = badgeAt + BADGE + 16; // where the panel starts

  // `along` is the panel's length and `across` runs over the strips. Thin
  // strips have less room than their labels need, so the panel is drawn up
  // to twice as big as scale across the strips; past that the labels fan out.
  const k = upright ? Math.min(ROOM / pn.width, 230 / pn.length) : Math.min((ROOM - edge) / pn.length, 230 / pn.width);
  let ka = Math.min(Math.max(k, (n * GAP) / pn.width), k * 2);
  if (upright) ka = Math.max(k, Math.min(ka, ROOM / pn.width));
  const along = pn.length * k;
  const across = pn.width * ka;
  const span = Math.max(across, n * GAP); // room for the row of labels
  const lead = (span - across) / 2;
  const W = upright ? span + 1 : edge + along + 1;
  const H = upright ? edge + along + 1 : span;
  // Panel coordinates (a = along, c = across) to drawing coordinates.
  const box = (a, c, da, dc) => (upright ? `x="${(lead + c).toFixed(2)}" y="${(edge + a).toFixed(2)}" width="${dc.toFixed(2)}" height="${da.toFixed(2)}"` : `x="${(edge + a).toFixed(2)}" y="${(lead + c).toFixed(2)}" width="${da.toFixed(2)}" height="${dc.toFixed(2)}"`);
  const pt = (a, c) => (upright ? `${(lead + c).toFixed(1)} ${(edge + a).toFixed(1)}` : `${(edge + a).toFixed(1)} ${(lead + c).toFixed(1)}`);

  let out = `<svg width="${W.toFixed(1)}" height="${H.toFixed(1)}" viewBox="0 0 ${W.toFixed(1)} ${H.toFixed(1)}" role="img" aria-label="Panel ${pn.label}: ${n} strips numbered from the ${upright ? 'left' : 'top'}, crosscut into ${pn.slices} slices">
    <defs><pattern id="hatch" width="5" height="5" patternUnits="userSpaceOnUse" patternTransform="rotate(45)"><rect width="5" height="5" fill="rgba(255,255,255,0.55)"/><rect width="2" height="5" fill="rgba(28,41,49,0.55)"/></pattern></defs>`;
  const centres = [];
  let c = 0;
  for (const st of pn.strips) {
    const size = st.width * ka;
    out += `<rect ${box(0, c, along, size + 0.4)} fill="${validHex(timber(st.species).color)}"/>`;
    // A glue line, so neighbouring strips of one timber can be told apart.
    if (c > 0) out += `<path class="glue" d="M${pt(0, c)}L${pt(along, c)}"/>`;
    centres.push(lead + c + size / 2);
    c += size;
  }
  const pitch = pn.slice_cut + kerf;
  const kw = Math.max(kerf * k, 1.3);
  for (let i = 0; i <= pn.slices; i++) out += `<rect class="kerf" ${box(i * pitch * k, 0, kw, across)}/>`;
  const wasteAt = (pn.slices * pitch + kerf) * k;
  if (along - wasteAt > 0.5) out += `<rect class="waste" ${box(wasteAt, 0, along - wasteAt, across)}/>`;
  out += `<rect ${box(0.5, 0.5, along - 1, across - 1)} fill="none" stroke="rgba(22,12,6,0.55)"/>`;

  const at = spreadLabels(centres, GAP, BADGE + 1, span - BADGE - 1);
  const small = BADGE < 8 ? ' small' : '';
  pn.strips.forEach((st, i) => {
    const l = at[i];
    const s = centres[i];
    out += `<g class="strip-label"><title>Strip ${i + 1}: ${esc(timber(st.species).name)}, ${widths[i]} wide</title>`;
    if (upright) {
      // Along the top, with the words turned to read upwards.
      out += `<path class="lead" d="M${l.toFixed(1)} ${(badgeAt + BADGE).toFixed(1)}L${s.toFixed(1)} ${(edge - 5).toFixed(1)}V${(edge + 5).toFixed(1)}"/>
        <circle class="lead-end" cx="${s.toFixed(1)}" cy="${(edge + 5).toFixed(1)}" r="1.8"/>
        <text class="lt" transform="translate(${(l + 4).toFixed(1)},${nameLen.toFixed(1)}) rotate(-90)">${esc(timbers[i])}</text>
        <text class="lw" transform="translate(${(l + 5).toFixed(1)},${widthEnd.toFixed(1)}) rotate(-90)">${widths[i]}</text>
        <circle class="badge" cx="${l.toFixed(1)}" cy="${badgeAt.toFixed(1)}" r="${BADGE}"/>
        <text class="ln${small}" x="${l.toFixed(1)}" y="${(badgeAt + 4.2).toFixed(1)}" text-anchor="middle">${i + 1}</text>`;
    } else {
      out += `<path class="lead" d="M${(badgeAt + BADGE).toFixed(1)} ${l.toFixed(1)}L${(edge - 5).toFixed(1)} ${s.toFixed(1)}H${(edge + 5).toFixed(1)}"/>
        <circle class="lead-end" cx="${(edge + 5).toFixed(1)}" cy="${s.toFixed(1)}" r="1.8"/>
        <text class="lt" x="${nameLen.toFixed(1)}" y="${(l + 4).toFixed(1)}" text-anchor="end">${esc(timbers[i])}</text>
        <text class="lw" x="${widthEnd.toFixed(1)}" y="${(l + 5).toFixed(1)}" text-anchor="end">${widths[i]}</text>
        <circle class="badge" cx="${badgeAt.toFixed(1)}" cy="${l.toFixed(1)}" r="${BADGE}"/>
        <text class="ln" x="${badgeAt.toFixed(1)}" y="${(l + 4.2).toFixed(1)}" text-anchor="middle">${i + 1}</text>`;
    }
    out += `</g>`;
  });
  return { svg: out + `</svg>`, stretched: ka > k * 1.05 };
}

function renderResults() {
  const el = $('#results');
  const r = S.result;
  if (!r.plan) {
    el.innerHTML = `<div class="empty"><h2>Lists appear once the layout adds up</h2><p>Sort out the sizes flagged above the board and the order and cutting lists will fill in.</p></div>`;
    return;
  }
  const plan = r.plan;
  const t = plan.totals;
  const p = S.project;
  const byCols = plan.axis === 'cols';
  const slice = byCols ? names('cols') : names('rows');
  const sw = (id) => `<i class="sw" style="background:${validHex(timber(id).color)}"></i>`;

  const sawnMode = p.order_by === 'sawn';
  const carry = p.settings.max_length > 0 ? p.settings.max_length : 0;
  const bySegment = `<div class="seg no-print" id="order-by" role="group" aria-label="How the order is written">
      <button type="button" data-by="bf" class="${sawnMode ? '' : 'on'}">By the board foot</button>
      <button type="button" data-by="sawn" class="${sawnMode ? 'on' : ''}">By sawn size</button>
    </div>`;

  // Timbers on the board foot list with no price yet. Their rows offer to
  // add one, and the totals say they are missing rather than showing a zero.
  const noPrice = [...new Set(plan.order.filter((o) => !(o.price_per_bf > 0) && timber(o.species).id).map((o) => o.species))];
  const noPriceNames = noPrice.map((id) => esc(timber(id).name));
  const lacking = `${noPriceNames.length > 1 ? `${noPriceNames.slice(0, -1).join(', ')} and ` : ''}${noPriceNames[noPriceNames.length - 1]} ${noPrice.length === 1 ? 'has' : 'have'} no price yet`;
  /** The headline cost: `amount` with `label` under it, or what is missing. */
  const costHead = (amount, label) =>
    !noPrice.length
      ? `<b>${money(amount)}</b>${label}`
      : amount > 0
      ? `<b>${money(amount)}</b>so far: ${lacking}`
      : `<b class="quiet">No prices yet</b>add them in the order list below`;

  // The board foot table, also used for timbers with no sawn sizes.
  const bfTable = () => {
    let out = `<table><thead><tr><th>Timber</th><th>Rough</th><th class="n">Board feet</th><th class="n">Price</th><th class="n">Cost</th></tr></thead><tbody>`;
    for (const o of plan.order) {
      const priced = !noPrice.includes(o.species);
      out += `<tr class="main"><td>${sw(o.species)}${esc(timber(o.species).name)}</td><td class="d">${stockLabel(o.stock)}</td><td class="n">${bf(o.bf_order)}</td><td class="n">${priced ? money(o.price_per_bf) : `<button type="button" class="link" data-price="${esc(o.species)}">add a price</button>`}</td><td class="n">${priced ? money(o.cost) : '&ndash;'}</td></tr>
        <tr class="detail"><td colspan="5">Yields ${plural(o.strips, 'strip')}. Boards need to be at least ${fmt(o.min_width)} wide and ${fmt(o.min_length)} long${carry ? `, and no longer than ${fmt(carry)}` : ''}. Roughly ${boardLength(o)}.</td></tr>`;
    }
    return out;
  };

  let h;
  if (!sawnMode) {
    h = `<div class="totals">
        <div><b>${bf(t.bf_order)}</b>board feet to order</div>
        <div>${costHead(t.cost, 'at your prices')}</div>
      </div>
      <p class="sub">That includes ${p.settings.waste_pct}% extra for defects. The finished board itself is ${bf(t.board_bf)} board feet.</p>`;
  } else {
    h = `<div class="order-total${t.unpriced ? ' incomplete' : ''}">
      ${t.unpriced ? `<p class="missing"><b>Not the whole board.</b> ${plural(t.unpriced, 'strip')} could not be fitted to a sawn size, so ${t.unpriced === 1 ? 'it is' : 'they are'} missing from this order and this price. The order list below says why.</p>` : ''}
      <div class="totals">
        <div><b>${metres(t.sawn_metres)}</b>of sawn board to order${plan.order.length ? `, plus ${bf(t.bf_order)} board feet` : ''}</div>
        <div>${costHead(t.total, t.vat > 0 ? 'including tax' : 'at the prices entered')}</div>
      </div>
      <p class="sub">Lengths include ${p.settings.waste_pct}% extra for defects${p.settings.sawn_step > 0 ? ` and are rounded up to the next ${p.settings.sawn_step === 1 ? 'whole metre' : metres(p.settings.sawn_step)}` : ''}.</p>
    </div>`;
  }
  // On paper the left-hand panel is gone, so the sheet says what it is for.
  const printed = new Date().toLocaleDateString(undefined, { day: 'numeric', month: 'short', year: 'numeric' });
  h = `<div class="print-bar no-print"><span>Order, cutting list and glue-ups for the workshop</span><button type="button" class="btn small tape" id="print-lists">Print these lists</button></div>
    <p class="print-only sheet-head">Finished board <b>${fmt(r.actual_width)} &times; ${fmt(r.actual_depth)} &times; ${fmt(p.thickness)}</b>, ${plural(p.cols.length, 'column')} by ${plural(p.rows.length, 'row')}. Saw kerf ${fmt(p.settings.kerf)}. Printed ${printed}.</p>` + h;
  for (const w of plan.warnings) h += `<p class="warn">${esc(w)}</p>`;

  h += `<section><div class="sec-head"><h2>Order from the mill</h2><button type="button" class="btn small no-print" id="copy-order">Copy order</button></div>
    ${bySegment}`;
  if (!sawnMode) {
    h += `<p class="help">Rough sawn, by timber and thickness.</p>${bfTable()}
      <tr class="sum"><td colspan="2">Total</td><td class="n">${bf(t.bf_order)}</td><td></td><td class="n">${noPrice.length && !(t.cost > 0) ? '&ndash;' : money(t.cost)}</td></tr></tbody></table>`;
  } else {
    const missing = [...new Set(plan.order.map((o) => o.species))].map(timber).filter((x) => x.id && !(x.sawn || []).some((z) => z.price_per_m > 0));
    h += `<p class="help">Sawn boards in the mill's sizes, by the metre. <span class="no-print">Enter a timber's sizes and prices under its ··· button in the timber list. </span>Each length is cut, ripped into strips a little oversize, and the strips are planed to their finished width.</p>
      <p class="rip-opt no-print"><label><input type="checkbox" id="sawn-whole"${p.settings.sawn_whole ? ' checked' : ''}> Avoid ripping</label>
        <span>Give a strip a board to itself where no more than <input id="sawn-whole-waste" class="mini" type="text" inputmode="decimal" autocomplete="off" value="${p.settings.sawn_whole_waste}" aria-label="Most of a board to plane away, in percent">% of that board would be planed away. Thinner strips are still ripped from wider boards. <b>${ripNote(t, p.settings.sawn_whole)}</b></span></p>`;
    for (const x of missing) {
      const m = sawnPreset(x);
      h += `<p class="no-print acts">${m ? `<button type="button" class="btn small" data-preset="${esc(x.id)}">Use ${esc(S.mill.mill)}'s ${esc(m.name.toLowerCase())} prices for ${esc(x.name)}</button>` : ''}<button type="button" class="btn small" data-sawn="${esc(x.id)}">Enter sawn sizes for ${esc(x.name)}</button></p>`;
    }
    if (missing.length && !S.mill) h += `<p class="help no-print">Or <button type="button" class="link" data-settings="prices">import your mill's whole price list</button> and fill them all in at once.</p>`;
    if (plan.sawn.length) {
      h += `<table><thead><tr><th>Timber</th><th>Sawn size</th><th class="n">Metres</th><th class="n">Per metre</th><th class="n">Cost</th></tr></thead><tbody>`;
      for (const l of plan.sawn) {
        const ask = l.pieces.map((q) => `${q.qty} &times; ${metres(q.length_m)}`).join(', ');
        // Plain instructions, in the order you do them: what to ask the
        // mill for, then each crosscut and what that piece is ripped into.
        const spare = l.metres - l.metres_net > 0.005 ? ` <span class="lay-note">(${metres(l.metres_net)} is cut into strips; the other ${metres(l.metres - l.metres_net)} is spare for defects)</span>` : '';
        const steps = l.layouts
          .map((y, i) => {
            const one = y.qty === 1;
            const what = `${y.qty}${i ? ' more' : ''} ${one ? 'piece' : 'pieces'}, ${fmt(y.length)} long`;
            if (y.planed) return `<p>Crosscut <b>${what}</b>. No ripping: plane ${one ? 'it' : 'each'} to <b>${fmt(y.planed[0])} &times; ${fmt(y.planed[1])}</b>.</p>`;
            const chips = ripGroups(y)
              .map(
                (g) =>
                  `<span class="chip rp" title="${g.n > 1 ? `${g.n} strips: rip` : 'Rip'} at ${fmt(g.at)}, then plane to ${fmt(g.to)}">${g.n > 1 ? `<b>${g.n} &times;</b>` : ''}<small>rip</small>${fmt(g.at)}<small>&rarr; plane</small>${fmt(g.to)}</span>`,
              )
              .join('');
            return `<p>Crosscut <b>${what}</b>, and rip, then plane ${one ? 'it' : 'each'} into:</p><span class="chips">${chips}</span>`;
          })
          .join('');
        const cuts = `<div class="layouts"><p>Ask the mill for <b>${ask}</b>${spare}</p>${steps}</div>`;
        h += `<tr class="main"><td>${sw(l.species)}${esc(timber(l.species).name)}</td><td class="d">${l.thickness} &times; ${l.width} mm</td><td class="n">${metres(l.metres)}</td><td class="n">${money(l.price_per_m)}</td><td class="n">${money(l.cost)}</td></tr>
          <tr class="detail"><td colspan="5">${cuts}</td></tr>`;
      }
      h += `</tbody></table>`;
    }
    if (plan.too_big.length) {
      // Strips of a timber the mill does sell sawn, but not big enough.
      const toUnits = (mm) => (units() === 'mm' ? mm : mm / 25.4);
      const asBf = p.settings.sawn_big_as_bf;
      h += `<div class="sub-head with-check"><h3>Too big for the mill's sawn sizes</h3>
          <label class="no-print"><input type="checkbox" id="big-as-bf"${asBf ? ' checked' : ''}> Order these by the board foot</label></div>
        <p class="help">${asBf ? 'These strips are on the board foot list below, at your price per board foot.' : 'These strips are not on the order and not in the cost. Tick the box to order them by the board foot instead.'}</p>`;
      for (const x of plan.too_big) {
        const name = esc(timber(x.species).name);
        const why =
          x.need_thickness > x.max_thickness + 0.01
            ? `Even on its side, each needs a sawn board <b>${Number(x.need_thickness.toFixed(1))} mm</b> thick once your ${fmt(p.settings.rough_thickness)} rough thickness loss is added. The thickest ${name.toLowerCase()} the mill lists is <b>${x.max_thickness} mm</b>, so the most that fits is <b>${fmt(toUnits(x.max_thickness) - p.settings.rough_thickness)}</b> on the strip's smaller side.`
            : `Each needs a sawn board ${Number(x.need_thickness.toFixed(1))} mm thick and <b>${Number(x.need_width.toFixed(1))} mm</b> wide once your rough losses are added. The widest ${name.toLowerCase()} that thick is <b>${x.max_width} mm</b>.`;
        h += `<div class="issue toobig${asBf ? ' note' : ''}"><p>${sw(x.species)}<b>${plural(x.qty, `${name} strip`)}</b> finishing ${fmt(x.thickness)} &times; ${fmt(x.width)}. ${why}</p></div>`;
      }
      h += `<p class="help">To buy them as sawn boards${asBf ? ' instead' : ''}, make the rows or columns behind them smaller, or lower the rough thickness loss under "More allowances" if your boards need less planing.</p>`;
    }
    if (plan.order.length) {
      // Two things can land here: timbers with no sawn sizes entered, and
      // strips too big for the mill's sizes when you have asked for those.
      const big = new Set(p.settings.sawn_big_as_bf ? plan.too_big.map((x) => x.species) : []);
      const unlisted = plan.order.some((o) => !big.has(o.species) || !(timber(o.species).sawn || []).some((z) => z.price_per_m > 0));
      const what = [unlisted ? 'timbers with no sawn sizes entered' : '', big.size ? "strips too big for the mill's sizes" : ''].filter(Boolean).join(', and ');
      h += `<h3 class="sub-head">By the board foot</h3><p class="help">${what.charAt(0).toUpperCase() + what.slice(1)}.</p>${bfTable()}</tbody></table>`;
    }
    h += `<table class="sums"><tbody>
        <tr><td>Before tax</td><td class="n">${money(t.cost)}</td></tr>
        <tr><td>Tax at <input id="vat-pct" class="mini" type="text" inputmode="decimal" autocomplete="off" value="${p.settings.vat_pct}" aria-label="Tax percent">%</td><td class="n">${money(t.vat)}</td></tr>
        <tr class="sum"><td>Total</td><td class="n">${money(t.total)}</td></tr>
      </tbody></table>
      <p class="help mill-terms no-print">The mill sells length in steps of <input id="sawn-step" class="mini" type="text" inputmode="decimal" autocomplete="off" value="${p.settings.sawn_step}" aria-label="Metres per step"> m.${carry ? ` No board on this order is longer than ${metres(carry * (units() === 'in' ? 0.0254 : 0.001))}.` : ''}</p>`;
  }
  h += `</section>`;

  h += `<section><h2>Milled cutting list</h2>
    <p class="help">Finished strip sizes, ready for the first glue-up. Thick is the panel's thickness; wide is measured across the panel. Under each is the oversize piece to cut first, which leaves your rough allowances for planing. "On edge" means the strip's width comes out of the stock's thickness.</p>
    <table><thead><tr><th class="n">Qty</th><th>Timber</th><th class="n">Thick</th><th class="n">Wide</th><th class="n">Long</th><th>Panel</th></tr></thead><tbody>`;
  for (const c of plan.cut_list) {
    const how = c.too_big
      ? `No sawn size the mill lists is big enough for this strip, so it is not on the order.`
      : c.sawn_width
      ? `From a ${c.sawn} &times; ${c.sawn_width} mm board cut ${fmt(c.rough_length)} long. No ripping: plane it to size.`
      : `From ${fromLabel(c)}: rip at ${fmt(c.rip_at)} and cut ${fmt(c.rough_length)} long, then plane to size.`;
    h += `<tr class="main"><td class="n">${c.qty}</td><td>${sw(c.species)}${esc(timber(c.species).name)}</td><td class="n">${fmt(c.thickness)}</td><td class="n">${fmt(c.width)}</td><td class="n">${fmt(c.length)}</td><td class="d wrap">${c.panels.join(', ')}</td></tr>
      <tr class="detail"><td></td><td colspan="5">${how}</td></tr>`;
  }
  h += `</tbody></table></section>`;

  h += `<section><h2>Glue-ups</h2>
    <div class="legend"><span class="l-kerf">Saw cut</span><span class="l-waste">End waste</span></div>`;
  for (const pn of plan.panels) {
    const flipped = pn.members.filter((m) => m.flipped).map((m) => m.index);
    const which = `${pn.slices === 1 ? slice.one : slice.many} ${ranges(pn.members.map((m) => m.index))}`;
    const drawing = panelSvg(pn, !byCols);
    h += `<div class="panel"><h3>Panel ${pn.label}</h3>
      <p>Glue ${plural(pn.strips.length, 'strip')} into a panel ${fmt(pn.width)} wide, ${fmt(pn.thickness)} thick and ${fmt(pn.length)} long. Flatten it to ${fmt(pn.slice_final)}, then crosscut ${plural(pn.slices, 'slice')} at ${fmt(pn.slice_cut)}. ${pn.slices === 1 ? 'It becomes' : 'They become'} ${which}.${flipped.length ? ` Turn the slices for ${flipped.length === 1 ? slice.one : slice.many} ${ranges(flipped)} end for end.` : ''}</p>
      <p class="chips-cap">${
        byCols
          ? "The strips are numbered in gluing order, from the top of the drawing down, the same way up as the board's rows. Beside each number is the strip's finished width."
          : "The strips are numbered in gluing order, from left to right, the same way round as the board's columns. Above each is its number, finished width and timber."
      }${drawing.stretched ? ` The drawing is ${byCols ? 'taller' : 'wider'} than scale so that every strip can be labelled.` : ''}</p>
      ${drawing.svg}
    </div>`;
  }
  h += `</section>`;

  h += `<section><h2>Order of work</h2><ol class="steps">
    <li>Rip and crosscut each strip oversize, as the cutting list shows, then plane it to its finished size.</li>
    <li>Glue up each panel with the strips in the order shown, ${byCols ? 'top to bottom' : 'left to right'}, then flatten it.</li>
    <li>Square one end and crosscut the slices at ${fmt(pn0(plan).slice_cut)}. Each cut loses a ${fmt(p.settings.kerf)} kerf.</li>
    <li>Stand the slices end grain up, arrange them to match the board, and glue.</li>
    <li>Flatten to ${fmt(p.thickness)} thick and trim the two ragged edges to finish at ${fmt(r.actual_width)} by ${fmt(r.actual_depth)}.</li>
  </ol></section>`;

  el.innerHTML = h;
}
const pn0 = (plan) => plan.panels[0];

/** The strips ripped from one length, with identical ones counted together:
    how many, the width to rip at, and the width to plane down to. */
function ripGroups(layout) {
  const groups = [];
  layout.rips.forEach((to, k) => {
    const at = layout.rip_at[k];
    const same = groups.find((g) => Math.abs(g.at - at) < 1e-6 && Math.abs(g.to - to) < 1e-6);
    if (same) same.n++;
    else groups.push({ n: 1, at, to });
  });
  return groups;
}

/** What avoiding ripping does to this order, in a sentence. */
function ripNote(t, on) {
  if (t.whole_strips == null) return '';
  if (!t.whole_strips) return 'No strip on this board suits a board of its own at that limit.';
  const one = t.whole_strips === 1;
  const x = t.whole_extra;
  const same = Math.abs(x) < 0.005;
  if (on) {
    const cost = same ? 'costs the same as ripping' : `${x > 0 ? 'adds' : 'saves'} ${money(Math.abs(x))} before tax`;
    return `${plural(t.whole_strips, 'strip')} ${one ? 'has a board to itself' : 'have a board to themselves'}, which ${cost}.`;
  }
  const cost = same ? 'cost the same as ripping' : `${x > 0 ? 'add' : 'save'} ${money(Math.abs(x))} before tax`;
  return `Here it would give ${plural(t.whole_strips, 'strip')} a board of ${one ? 'its' : 'their'} own and ${cost}.`;
}

/** Print either the board with its lists, or the guide. The page's print
    styles show only the one asked for. */
function printPage(what) {
  const guide = $('#guide');
  const reopen = guide.open;
  // A dialog on top of the page doesn't print cleanly, so it is closed and
  // printed as an ordinary block.
  if (reopen) guide.close();
  document.body.dataset.print = what;
  if (host) return host('print');
  window.print();
  if (reopen) guide.showModal();
}

/** The app's File > Print Lists menu item calls this. */
window.endgrainPrintLists = () => printPage('lists');

function orderText() {
  const r = S.result;
  const p = S.project;
  const plan = r.plan;
  const carry = p.settings.max_length > 0 ? p.settings.max_length : 0;
  const lines = [`Timber order for ${p.name.trim() || 'a chopping board'}`];
  if (plan.sawn.length) {
    lines.push('Sawn boards:');
    for (const l of plan.sawn) {
      const ask = l.pieces.map((q) => `${q.qty} x ${metres(q.length_m)}`).join(', ');
      lines.push(`- ${timber(l.species).name} ${l.thickness} x ${l.width} mm: ${metres(l.metres)}, as ${ask}`);
    }
  }
  if (plan.order.length) {
    lines.push('Rough sawn by the board foot:');
    for (const o of plan.order) {
      lines.push(`- ${timber(o.species).name} ${stockLabel(o.stock)}: ${bf(o.bf_order)} bf, boards at least ${fmt(o.min_width)} wide x ${fmt(o.min_length)} long`);
    }
    lines.push(`Total: ${bf(plan.totals.bf_order)} bf`);
  }
  if (carry) lines.push(`No length over ${metres(carry * (units() === 'in' ? 0.0254 : 0.001))} please.`);
  return lines.join('\n');
}

function render() {
  renderBoard();
  renderIssues();
  renderAxis();
  renderResults();
  renderPop();
}

// ----------------------------------------------- settings and price lists

/** A likely currency and units for where this computer is set up. Only used
    to fill in the first-run screen, where you can change them. */
function localeGuess() {
  const region = ((navigator.language || '').split('-')[1] || '').toUpperCase();
  const guess = {};
  if (region === 'GB') guess.currency = '£';
  else if ('AT BE CY DE EE ES FI FR GR HR IE IT LT LU LV MT NL PT SI SK'.split(' ').includes(region)) guess.currency = '€';
  else if (['US', 'CA', 'AU', 'NZ'].includes(region)) guess.currency = '$';
  if (region === 'US') guess.units = 'in';
  return guess;
}

const today = () => {
  const d = new Date();
  return `${d.getFullYear()}-${String(d.getMonth() + 1).padStart(2, '0')}-${String(d.getDate()).padStart(2, '0')}`;
};

/** Typical colours, for timbers added from a price list that gives none. */
const WOOD_COLOURS = [['walnut', '#5a3d2b'], ['cherry', '#b9693c'], ['maple', '#e6cfa5'], ['white oak', '#bf9f6b'], ['oak', '#b99a66'], ['sapele', '#8a4a30'], ['purpleheart', '#6b3a5e'], ['beech', '#d8b48a'], ['birch', '#e3cfa8'], ['sycamore', '#e9dcc0'], ['mahogany', '#8e4b32'], ['iroko', '#a67a3d'], ['teak', '#a5753f'], ['padauk', '#a8412a'], ['wenge', '#3b2a22'], ['elm', '#a9825a'], ['poplar', '#cfc79a'], ['tulip', '#cfc79a'], ['hickory', '#c8a26b'], ['meranti', '#a8644a'], ['utile', '#8f5238'], ['douglas fir', '#d39a5e'], ['pine', '#e2c078'], ['cedar', '#b5653f'], ['zebrano', '#b8935a'], ['bubinga', '#8b3f2f'], ['ebony', '#2b221e'], ['idigbo', '#cdb36a'], ['ash', '#d9c49a'], ['fir', '#d8a86c']];
const woodColour = (name) => (WOOD_COLOURS.find(([k]) => name.toLowerCase().includes(k)) || [0, '#a8784c'])[1];

/** The prompt to give an AI assistant so it writes a price list this app reads. */
function aiPrompt(site) {
  return `I use a woodworking app called End Grain Calc to plan chopping boards. It can read a timber mill's price list as JSON. Please build that list for my mill.

My mill: ${site.trim() || "[put the mill's name and website here, or attach its price list]"}

Read the mill's current prices for sawn or rough sawn boards, then reply with one JSON code block in exactly this shape. The numbers below only show the shape.

{
  "mill": "The mill's name",
  "source": "The web page the prices came from",
  "fetched": "${today()}",
  "step_m": 0.3,
  "timbers": [
    {
      "name": "American black walnut",
      "color": "#5a3d2b",
      "price_per_bf": 0,
      "sizes": [
        { "thickness": 26, "width": 150, "price_per_m": 24.5 },
        { "thickness": 50, "width": 150, "price_per_m": 47 }
      ]
    }
  ]
}

Rules:
- One entry in "timbers" for each species. "color" is a typical hex colour for that wood; the app uses it to draw the board.
- If the mill sells fixed sawn sizes by length, list each one under "sizes". "thickness" and "width" are the nominal sawn size in millimetres (25.4 mm to the inch). "price_per_m" is the price of one metre of length (a price per foot times 3.281). Use the price for the smallest quantity. Leave out sizes with no price or that are out of stock.
- If the mill sells by the board foot, put that price in "price_per_bf" and make "sizes" an empty list. A price per cubic foot divided by 12, or a price per cubic metre divided by 424, is a price per board foot.
- "step_m" is the smallest step of length the mill sells, in metres. Use 0.3 if it sells by the foot or does not say.
- Prices are plain numbers in the mill's own currency, with no currency symbol. If the mill shows prices with and without sales tax or VAT, use the price without.
- "fetched" is today's date, year-month-day.
- Only use prices you actually read from the mill. Do not estimate or fill gaps. After the JSON, tell me what you left out and anything you were unsure of.`;
}

/** Numbers that arrive as text ("24.50", "£24.50") are read as numbers, so a
    slightly loose list still imports. Anything unreadable becomes zero and
    is dropped by the app. */
function tidyList(list) {
  const num = (v) => {
    const n = typeof v === 'string' ? parseFloat(v.replace(/[^0-9.]/g, '')) : v;
    return typeof n === 'number' && isFinite(n) ? n : 0;
  };
  const str = (v) => (typeof v === 'string' ? v : v == null ? '' : String(v));
  return {
    mill: str(list.mill),
    source: str(list.source),
    fetched: str(list.fetched) || today(),
    step_m: list.step_m == null ? 0.3 : num(list.step_m),
    timbers: list.timbers
      .filter((t) => t && typeof t === 'object')
      .map((t) => ({
        name: str(t.name),
        ...(typeof t.color === 'string' ? { color: t.color } : {}),
        price_per_bf: num(t.price_per_bf),
        sizes: (Array.isArray(t.sizes) ? t.sizes : []).filter((z) => z && typeof z === 'object').map((z) => ({ thickness: num(z.thickness), width: num(z.width), price_per_m: num(z.price_per_m) })),
      })),
  };
}

/** Each timber in the price list, with the timber of yours it would update
    (matched by name, exact names first) and whether you already have its prices. */
function millMatches() {
  const norm = (x) => x.trim().toLowerCase();
  const taken = new Set();
  const rows = S.mill.timbers.map((m) => ({ m, t: null }));
  const claim = (fits) => {
    for (const row of rows) {
      if (row.t) continue;
      row.t = S.library.find((t) => !taken.has(t.id) && norm(t.name) && fits(norm(row.m.name), norm(t.name))) || null;
      if (row.t) taken.add(row.t.id);
    }
  };
  claim((a, b) => a === b);
  claim((a, b) => a.includes(b) || b.includes(a));
  const sizes = (list) => JSON.stringify(list.map((z) => [z.thickness, z.width, z.price_per_m]));
  for (const row of rows) {
    const { m, t } = row;
    row.same = !!t && (!m.sizes.length || sizes(m.sizes) === sizes(t.sawn || [])) && (!(m.price_per_bf > 0) || m.price_per_bf === t.price_per_bf);
  }
  return rows;
}

function renderSetupPrices() {
  const st = S.setup;
  if (!st) return;
  const m = S.mill;
  let h = `<h3>Your mill's prices</h3>
    <p class="help">The app comes with no prices, because every mill's are different. Type them in under each timber's ··· button as you need them, or bring in a whole price list here.</p>`;
  if (m) {
    const rows = millMatches();
    if (!st.picks) st.picks = new Set(rows.map((x, i) => (x.t && !x.same ? i : -1)).filter((i) => i >= 0));
    const what = (x) => [x.sizes.length ? plural(x.sizes.length, 'sawn size') : '', x.price_per_bf > 0 ? `${esc(st.draft.currency.trim() || S.prefs.currency)}${x.price_per_bf.toFixed(2)} a board foot` : ''].filter(Boolean).join(', ');
    h += `<div class="set-have">
        <p><b>${esc(m.mill)}</b>: ${plural(m.timbers.length, 'timber')}${millDate() ? `, read on ${millDate()}` : ''}. Tick the ones you use and their prices go onto your timber list.</p>
        <ul class="set-list">${rows
          .map(
            (x, i) =>
              `<li><label><input type="checkbox" data-i="${i}"${st.picks.has(i) ? ' checked' : ''}><span><b>${esc(x.m.name)}</b><small>${what(x.m)}</small></span></label><em>${
                x.same ? `already on ${esc(x.t.name)}` : x.t ? `updates ${esc(x.t.name)}` : 'adds a new timber'
              }</em></li>`,
          )
          .join('')}</ul>
        <div class="set-line"><button type="button" id="set-apply" class="btn small tape">Update my timber list</button><button type="button" id="set-all" class="btn small">Tick all</button></div>
      </div>`;
  }
  const steps = `<ol class="set-steps">
      <li><b>Ask your AI assistant to build the list.</b>
        <p>Copy this prompt into an assistant that can read web pages. It says what to look for and the format this app reads.</p>
        <div class="set-line"><input id="set-site" type="text" autocomplete="off" spellcheck="false" placeholder="Your mill's website (optional)" aria-label="Your mill's website" value="${esc(st.site)}"><button type="button" id="set-copy" class="btn small">Copy the prompt</button></div></li>
      <li><b>Paste what it gives you here, or choose the file.</b>
        <textarea id="set-json" rows="4" spellcheck="false" aria-label="Price list" placeholder='{ "mill": "…", "timbers": [ … ] }'></textarea>
        <div class="set-line"><button type="button" id="set-import" class="btn small tape">Read this price list</button><button type="button" id="set-file-btn" class="btn small">Choose a file…</button><input id="set-file" type="file" accept=".json,.txt,application/json,text/plain" hidden></div>
        <p id="set-error" class="warn" role="alert" hidden></p></li>
      <li><b>Check a few prices against the mill's own list.</b> Assistants can misread a page.</li>
    </ol>`;
  h += m ? `<details id="set-more"><summary>Import a different price list</summary><p class="help">It replaces ${esc(m.mill)}'s list. Prices already on your timbers stay until you update them.</p>${steps}</details>` : steps;
  $('#set-prices').innerHTML = h;
}

/** Read a pasted or chosen price list, save it and offer its timbers. */
async function importMill(text) {
  const say = (msg) => {
    $('#set-error').textContent = msg;
    $('#set-error').hidden = !msg;
  };
  say('');
  // Assistants often wrap the list in a code block or a sentence, so only
  // what is between the outer braces is read.
  const a = text.indexOf('{');
  const b = text.lastIndexOf('}');
  if (a < 0) return say('Paste the price list first. It starts with { and ends with }.');
  if (b <= a) return say('That text is cut off: it has no closing }. Ask the assistant to send the whole list again.');
  let list;
  try {
    list = JSON.parse(text.slice(a, b + 1));
  } catch (e) {
    return say(`That text isn't a complete list, so it can't be read (${e.message}). Ask the assistant to send the whole list again.`);
  }
  if (!list || !Array.isArray(list.timbers)) return say('That has no "timbers" list, so it isn\'t a price list. The prompt above asks for the right format.');
  try {
    S.mill = await api('PUT', '/api/mill', tidyList(list));
  } catch (e) {
    return say(e.message);
  }
  S.setup.picks = null;
  renderSetupPrices();
  renderTimber();
  if (S.result) render();
  toast(`Read ${S.mill.mill}'s price list: ${plural(S.mill.timbers.length, 'timber')}.`);
}

/** Copy the ticked timbers' prices onto the timber list, adding new ones. */
function applyMill() {
  let updated = 0;
  let added = 0;
  millMatches().forEach((x, i) => {
    if (!S.setup.picks.has(i)) return;
    const sawn = x.m.sizes.map((z) => ({ thickness: z.thickness, width: z.width, price_per_m: z.price_per_m }));
    if (x.t) {
      // A list with only a board foot price leaves your sawn sizes alone.
      if (sawn.length) x.t.sawn = sawn;
      if (x.m.price_per_bf > 0) x.t.price_per_bf = x.m.price_per_bf;
      updated++;
    } else {
      S.library.push({ id: newTimberId(), name: x.m.name, color: x.m.color ? validHex(x.m.color) : woodColour(x.m.name), price_per_bf: x.m.price_per_bf || 0, stock: null, sawn });
      added++;
    }
  });
  if (!updated && !added) return toast('Tick at least one timber first.');
  S.setup.picks = null;
  renderTimber();
  saveLibrarySoon();
  recalc();
  renderSetupPrices();
  toast([updated ? `Updated ${plural(updated, 'timber')}` : '', added ? `${updated ? 'added' : 'Added'} ${plural(added, 'timber')}` : ''].filter(Boolean).join(', ') + '.');
}

function syncSetup() {
  const d = S.setup.draft;
  for (const b of document.querySelectorAll('#set-units button')) b.classList.toggle('on', b.dataset.v === d.units);
  for (const b of document.querySelectorAll('#set-order button')) b.classList.toggle('on', b.dataset.v === d.order_by);
}

/** The settings screen. On first run it greets you and can be skipped. */
function openSetup(first, toPrices = false) {
  if (S.setup) return;
  // What the form starts with: your saved choices. On first run that is the
  // board you already have, if any, or a guess from the computer's region.
  const start = { currency: S.prefs.currency, units: S.prefs.units, order_by: S.prefs.order_by, tax_pct: S.prefs.tax_pct, check_updates: S.prefs.check_updates !== false };
  if (first && S.savedSlug) Object.assign(start, { units: S.project.units, order_by: S.project.order_by, tax_pct: S.project.settings.vat_pct });
  else if (first && !S.boards.length) Object.assign(start, localeGuess());
  S.setup = { first, start, draft: { ...start }, picks: null, site: '' };
  const dlg = $('#setup');
  dlg.innerHTML = `<div class="guide-head">
      <h2 id="setup-title">${first ? 'Welcome to End Grain Calc' : 'Settings'}</h2>
      <span class="guide-acts">
        <button type="button" class="btn small" data-close="${first ? 'skip' : 'cancel'}">${first ? 'Skip for now' : 'Cancel'}</button>
        <button type="button" class="btn small tape" data-close="done">${first ? 'Start designing' : 'Done'}</button>
      </span>
    </div>
    <div class="set-body">
      <p class="help">${
        first
          ? 'A few quick choices so the numbers suit you and your mill. You can skip them: everything here can be changed later under Settings.'
          : 'The currency symbol changes everywhere. The other three are what a new board starts with. A board you have already made keeps its own, and you can change them on the board itself.'
      }</p>
      <div class="set-grid">
        <label for="set-currency">Currency symbol</label>
        <div><input id="set-currency" class="mini" type="text" maxlength="4" autocomplete="off" spellcheck="false" value="${esc(start.currency)}"><small>Shown before every price.</small></div>
        <span id="set-units-l">Measure in</span>
        <div><div class="seg" id="set-units" role="group" aria-labelledby="set-units-l"><button type="button" data-v="mm">Millimetres</button><button type="button" data-v="in">Inches</button></div></div>
        <span id="set-order-l">My mill sells timber</span>
        <div><div class="seg" id="set-order" role="group" aria-labelledby="set-order-l"><button type="button" data-v="bf">By the board foot</button><button type="button" data-v="sawn">In sawn sizes, by the metre</button></div><small>Sets how the order list is written. You can switch on any board.</small></div>
        <label for="set-tax">Tax on timber</label>
        <div><input id="set-tax" class="mini" type="text" inputmode="decimal" autocomplete="off" value="${start.tax_pct}"> %<small>VAT, GST or sales tax, added to orders by sawn size. Leave it at 0 if the prices you enter already include tax.</small></div>
        <span>New versions</span>
        <div><label class="set-check"><input id="set-updates" type="checkbox"${start.check_updates ? ' checked' : ''}> Tell me when a new version is out</label><small>When the app starts it asks GitHub which version is the latest. Nothing about you or your boards is sent.</small></div>
      </div>
      <section id="set-prices"></section>
    </div>`;
  syncSetup();
  renderSetupPrices();
  dlg.showModal();
  if (toPrices) $('#set-prices').scrollIntoView();
}

/** Close the settings screen: 'done' saves what is on it, 'skip' saves what
    it started with, and 'cancel' saves nothing. */
async function closeSetup(how) {
  const st = S.setup;
  if (!st) return;
  if (how !== 'cancel') {
    try {
      S.prefs = await api('PUT', '/api/prefs', { ...(how === 'skip' ? st.start : st.draft), setup_done: true });
    } catch (e) {
      return toast(`Couldn't save your settings: ${e.message}`);
    }
  }
  S.setup = null;
  $('#setup').close();
  if (S.prefs.check_updates === false) $('#update').hidden = true;
  // On first run the sample board is redrawn in the units you chose.
  if (st.first && !S.savedSlug && !isDirty()) setProject(sampleProject(), null);
  else {
    renderTimber();
    if (S.result) render();
  }
  if (st.first) toast('You can change these any time under Settings.');
}

function buildSetup() {
  const dlg = $('#setup');
  $('#settings-btn').addEventListener('click', () => openSetup(false));
  // Escape skips the welcome, or cancels a later visit.
  dlg.addEventListener('cancel', (e) => {
    e.preventDefault();
    if (S.setup) closeSetup(S.setup.first ? 'skip' : 'cancel');
  });
  dlg.addEventListener('click', async (e) => {
    const st = S.setup;
    const btn = e.target.closest('button');
    if (!st || !btn) return;
    if (btn.dataset.close) return closeSetup(btn.dataset.close);
    if (btn.closest('#set-units')) st.draft.units = btn.dataset.v;
    else if (btn.closest('#set-order')) st.draft.order_by = btn.dataset.v;
    else if (btn.id === 'set-copy') return toast((await copyText(aiPrompt(st.site))) ? 'Prompt copied. Paste it to your AI assistant.' : "Couldn't reach the clipboard.");
    else if (btn.id === 'set-import') return importMill($('#set-json').value);
    else if (btn.id === 'set-file-btn') return $('#set-file').click();
    else if (btn.id === 'set-apply') return applyMill();
    else if (btn.id === 'set-all') {
      st.picks = new Set(S.mill.timbers.map((_, i) => i));
      return renderSetupPrices();
    } else return;
    syncSetup();
  });
  dlg.addEventListener('input', (e) => {
    const st = S.setup;
    if (!st) return;
    const el = e.target;
    if (el.id === 'set-currency') st.draft.currency = el.value;
    else if (el.id === 'set-site') st.site = el.value;
    else if (el.id === 'set-updates') st.draft.check_updates = el.checked;
    else if (el.id === 'set-tax') {
      const v = parseFloat(el.value.replace(',', '.'));
      st.draft.tax_pct = isFinite(v) && v >= 0 && v <= 100 ? v : 0;
    } else if (el.type === 'checkbox' && el.dataset.i) st.picks[el.checked ? 'add' : 'delete'](+el.dataset.i);
  });
  dlg.addEventListener('change', async (e) => {
    if (e.target.id !== 'set-file' || !e.target.files[0]) return;
    const text = await e.target.files[0].text().catch(() => '');
    $('#set-json').value = text;
    importMill(text);
  });
}

// ------------------------------------------------------------ new versions

// New versions are published on GitHub. At start-up the page asks GitHub's
// public API which one is the latest; nothing about you or your boards goes
// with the question, and it can be turned off under Settings.
const RELEASES = 'https://github.com/timhanley/end-grain-calc/releases/latest';
const LATEST_RELEASE = 'https://api.github.com/repos/timhanley/end-grain-calc/releases/latest';

/** "v0.10.2" or "0.10.2" as [0, 10, 2], or null for anything else. */
function versionParts(v) {
  const m = /^v?(\d+)\.(\d+)\.(\d+)$/.exec(String(v || '').trim());
  return m ? m.slice(1).map(Number) : null;
}

/** Is version a later than version b? */
function isLater(a, b) {
  for (let i = 0; i < 3; i++) if (a[i] !== b[i]) return a[i] > b[i];
  return false;
}

/** Show the header's small button if GitHub has a newer version. Silent if
    the check is off, the computer is offline or GitHub doesn't answer. */
async function checkForUpdate() {
  const mine = versionParts(S.info && S.info.version);
  if (S.prefs.check_updates === false || !mine) return;
  const stop = new AbortController();
  const timer = setTimeout(() => stop.abort(), 10000);
  try {
    const res = await fetch(LATEST_RELEASE, { signal: stop.signal, cache: 'no-store', credentials: 'omit', referrerPolicy: 'no-referrer' });
    if (!res.ok) return;
    const latest = versionParts((await res.json()).tag_name);
    if (!latest || !isLater(latest, mine) || S.prefs.check_updates === false) return;
    const b = $('#update');
    b.innerHTML = `<svg viewBox="0 0 14 14" aria-hidden="true"><path d="M7 1.5v8M3.5 6 7 9.5 10.5 6M2 12.5h10"/></svg><span class="wordy">Version </span>${latest.join('.')}<span class="wordy"> is out</span>`;
    b.title = `You have version ${mine.join('.')}. Open the download page to see what's new.`;
    b.hidden = false;
  } catch (_) {
    /* offline, or GitHub didn't answer: say nothing */
  } finally {
    clearTimeout(timer);
  }
}

function buildUpdate() {
  // In the app window a link would replace the designer, so the window is
  // asked to open the download page in the usual browser instead.
  $('#update').addEventListener('click', () => {
    if (host) host('open-releases');
    else window.open(RELEASES, '_blank', 'noopener');
  });
}

// ------------------------------------------------------- save, open, boards

let toastTimer = null;
function toast(msg) {
  const el = $('#toast');
  el.textContent = msg;
  el.hidden = false;
  clearTimeout(toastTimer);
  toastTimer = setTimeout(() => (el.hidden = true), 3200);
}

async function refreshBoards() {
  S.boards = await api('GET', '/api/boards');
  renderMenu();
}

async function save({ overwrite = false } = {}) {
  const name = S.project.name.trim();
  if (!name) {
    $('#name').focus();
    toast('Name the board to save it.');
    return false;
  }
  S.project.name = name;
  try {
    const { slug } = await api('POST', '/api/boards', { project: S.project, previous: S.savedSlug, overwrite });
    S.savedSlug = slug;
    S.savedJson = JSON.stringify(S.project);
    try {
      localStorage.setItem('endgrain:last', slug);
    } catch (_) { /* nothing to remember */ }
    $('#conflict').hidden = true;
    syncInputs();
    toast(`Saved ${name}`);
    await refreshBoards();
    return true;
  } catch (e) {
    if (e.status === 409) {
      $('#conflict').classList.remove('urgent');
      $('#conflict').innerHTML = `<span>You already have a board called <b>${esc(name)}</b>.</span>
        <button type="button" class="btn small" data-act="replace">Replace it</button>
        <button type="button" class="btn small" data-act="cancel">Keep both, I'll rename this one</button>`;
      $('#conflict').hidden = false;
    } else toast(e.message);
    return false;
  }
}

/** Open a saved board or start a new one, checking for unsaved work first. */
async function go(target, { force = false } = {}) {
  if (!force && isDirty()) {
    S.pending = target;
    showMenu(true);
    return;
  }
  S.pending = null;
  if (target.kind === 'new') {
    setProject(blankProject(), null);
    $('#name').focus();
  } else {
    try {
      setProject(await api('GET', `/api/boards/${target.slug}`), target.slug);
    } catch (e) {
      toast(`Couldn't open that board: ${e.message}`);
    }
  }
  showMenu(false);
}

function showMenu(open) {
  $('#boards-menu').hidden = !open;
  $('#boards-btn').setAttribute('aria-expanded', open);
  if (open) renderMenu();
  else {
    S.pending = null;
    S.confirmDelete = null;
  }
}

function renderMenu() {
  const menu = $('#boards-menu');
  if (menu.hidden) return;
  let h = '';
  if (S.pending) {
    h += `<div class="menu-warn"><p><b>${esc(S.project.name.trim() || 'This board')}</b> has changes that aren't saved.</p>
      <button type="button" data-act="save-go">Save, then continue</button><button type="button" data-act="discard-go">Continue without saving</button></div>`;
  }
  h += `<div class="menu-top"><button type="button" data-act="new">New board</button><button type="button" data-act="dup">Duplicate this board</button></div>
    <div class="menu-label">Saved boards</div>`;
  if (!S.boards.length) h += `<p class="menu-empty">Nothing saved yet. Name this board and save it to see it here.</p>`;
  for (const b of S.boards) {
    const dims = `${fmt(b.width, { u: b.units })} x ${fmt(b.depth, { u: b.units })} x ${fmt(b.thickness, { u: b.units })}`;
    h += `<div class="menu-row${b.slug === S.savedSlug ? ' cur' : ''}">
      <button type="button" class="open" data-act="open" data-slug="${b.slug}"><b>${esc(b.name)}</b><small>${esc(dims)}</small></button>
      ${S.confirmDelete === b.slug
        ? `<button type="button" class="yes" data-act="del-yes" data-slug="${b.slug}">Delete for good</button><button type="button" data-act="del-no">Keep</button>`
        : `<button type="button" class="del" data-act="del" data-slug="${b.slug}" aria-label="Delete ${esc(b.name)}">&times;</button>`}
    </div>`;
  }
  if (S.info) h += `<p class="menu-foot">Kept in ${esc(S.info.data_dir)}/boards</p>`;
  menu.innerHTML = h;
}

function buildChrome() {
  $('#save').addEventListener('click', () => save());

  // The guide: a picture of how a board goes together, and what the words mean.
  const guide = $('#guide');
  $('#guide-btn').addEventListener('click', () => guide.showModal());
  $('#guide-close').addEventListener('click', () => guide.close());
  $('#guide-print').addEventListener('click', () => printPage('guide'));
  guide.addEventListener('click', (e) => e.target === guide && guide.close());
  // In the app window a link would replace the designer, so the window is
  // asked to open the maker's site in the usual browser instead.
  $('#credit').addEventListener('click', (e) => {
    if (!host) return;
    e.preventDefault();
    host('open-site');
  });
  $('#boards-btn').addEventListener('click', async () => {
    const open = $('#boards-menu').hidden;
    showMenu(open);
    if (open) await refreshBoards().catch((e) => toast(e.message));
  });

  $('#boards-menu').addEventListener('click', async (e) => {
    const btn = e.target.closest('[data-act]');
    if (!btn) return;
    const { act, slug } = btn.dataset;
    if (act === 'new') go({ kind: 'new' });
    else if (act === 'open') go({ kind: 'open', slug });
    else if (act === 'dup') {
      change((p) => (p.name = `${p.name.trim() || 'Board'} copy`), { undo: false });
      S.savedSlug = null;
      showMenu(false);
      $('#name').focus();
      $('#name').select();
    } else if (act === 'save-go') {
      const target = S.pending;
      if (await save()) go(target, { force: true });
      else showMenu(false);
    } else if (act === 'discard-go') go(S.pending, { force: true });
    else if (act === 'del') {
      S.confirmDelete = slug;
      renderMenu();
    } else if (act === 'del-no') {
      S.confirmDelete = null;
      renderMenu();
    } else if (act === 'del-yes') {
      try {
        await api('DELETE', `/api/boards/${slug}`);
        if (S.savedSlug === slug) {
          S.savedSlug = null;
          S.savedJson = '';
          syncChrome();
        }
        S.confirmDelete = null;
        await refreshBoards();
      } catch (err) {
        toast(`Couldn't delete that board: ${err.message}`);
      }
    }
  });

  $('#conflict').addEventListener('click', async (e) => {
    const act = e.target.dataset.act;
    if (act === 'replace') save({ overwrite: true });
    else if (act === 'quit') host('quit');
    else if (act === 'stay') $('#conflict').hidden = true;
    else if (act === 'save-quit') {
      if (await save()) host('quit');
    } else if (act === 'cancel') {
      $('#conflict').hidden = true;
      $('#name').focus();
      $('#name').select();
    }
  });

  $('#issues').addEventListener('click', (e) => {
    const { fix, axis, v } = e.target.dataset;
    if (!fix) return;
    change((p) => {
      if (fix === 'fit') p[axis === 'cols' ? 'width' : 'depth'] = Math.round(+v * 1e6) / 1e6;
      else if (fix === 'slack') p[axis][p[axis].length - 1].size = null;
      else p[axis].forEach((t) => (t.size = null));
    });
  });

  $('#axis').addEventListener('click', (e) => {
    const axis = e.target.dataset.axis;
    if (axis && axis !== S.project.slice_axis) change((p) => (p.slice_axis = axis));
  });

  $('#results').addEventListener('change', (e) => {
    if (e.target.id === 'sawn-whole') return change((p) => (p.settings.sawn_whole = e.target.checked));
    if (e.target.id === 'big-as-bf') return change((p) => (p.settings.sawn_big_as_bf = e.target.checked));
    const key = { 'vat-pct': 'vat_pct', 'sawn-step': 'sawn_step', 'sawn-whole-waste': 'sawn_whole_waste' }[e.target.id];
    if (!key) return;
    const v = parseFloat(e.target.value.replace(',', '.'));
    if (isFinite(v) && v >= 0) change((p) => (p.settings[key] = v));
    else e.target.value = S.project.settings[key];
  });
  $('#results').addEventListener('keydown', (e) => e.key === 'Enter' && e.target.classList.contains('mini') && e.target.blur());
  $('#results').addEventListener('click', async (e) => {
    const by = e.target.dataset.by;
    if (by && by !== S.project.order_by) return change((p) => (p.order_by = by));
    const preset = e.target.dataset.preset;
    if (preset) {
      const t = timber(preset);
      return fillFromMill(t, sawnPreset(t).name);
    }
    if (e.target.dataset.price) return showTimber(e.target.dataset.price, '.t-price input');
    if (e.target.dataset.sawn) return showTimber(e.target.dataset.sawn, '.sawn-add');
    if (e.target.dataset.settings) return openSetup(false, true);
    if (e.target.id === 'print-lists') return printPage('lists');
    if (e.target.id !== 'copy-order') return;
    toast((await copyText(orderText())) ? 'Order copied' : "Couldn't reach the clipboard. Use Print these lists instead.");
  });

  document.addEventListener('pointerdown', (e) => {
    if (!$('#boards-menu').hidden && !e.target.closest('.menu-wrap')) showMenu(false);
  });

  document.addEventListener('keydown', (e) => {
    const typing = /^(INPUT|SELECT|TEXTAREA)$/.test(e.target.tagName);
    const cmd = e.metaKey || e.ctrlKey;
    if (cmd && e.key.toLowerCase() === 's') {
      e.preventDefault();
      if (typing) e.target.blur();
      save();
    } else if (cmd && e.key.toLowerCase() === 'z' && !typing) {
      e.preventDefault();
      if (e.shiftKey) redo();
      else undo();
    } else if (e.key === 'Escape') {
      showMenu(false);
      closePop();
    } else if (!typing && !cmd && /^[1-9]$/.test(e.key) && S.library[+e.key - 1]) {
      pickBrush(S.library[+e.key - 1].id);
    }
  });

  // A browser tab warns before closing with unsaved work; the app window
  // asks through endgrainConfirmQuit instead.
  window.addEventListener('beforeunload', (e) => {
    if (!host && isDirty()) e.preventDefault();
  });

  let resizeTimer = null;
  window.addEventListener('resize', () => {
    clearTimeout(resizeTimer);
    resizeTimer = setTimeout(() => S.result && (renderBoard(), renderPop()), 60);
  });
}

// --------------------------------------------------------------------- start

async function init() {
  buildRail();
  buildTimber();
  buildBoard();
  buildPop();
  buildChrome();
  buildHistory();
  buildUpdate();
  buildSetup();
  try {
    [S.info, S.library, S.boards, S.mill] = await Promise.all([api('GET', '/api/info'), api('GET', '/api/library'), api('GET', '/api/boards'), api('GET', '/api/mill')]);
    S.prefs = await api('GET', '/api/prefs');
    for (const t of S.library) t.sawn = t.sawn || [];
  } catch (e) {
    toast(`Couldn't reach the calculator: ${e.message}`);
    return;
  }
  S.brush = S.library[0] ? S.library[0].id : null;
  renderTimber();

  let last = null;
  try {
    last = localStorage.getItem('endgrain:last');
  } catch (_) { /* nothing remembered */ }
  const start = S.boards.find((b) => b.slug === last) || S.boards[0];
  const saved = start ? await api('GET', `/api/boards/${start.slug}`).catch(() => null) : null;
  if (saved) setProject(saved, start.slug);
  else setProject(sampleProject(), null);
  // First run: a short welcome that asks the basics and can be skipped.
  if (!S.prefs.setup_done) openSetup(true);
  checkForUpdate();
}

init();
