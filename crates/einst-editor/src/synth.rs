//! The synth panel: edits a layer's synth patch (`einst::synth`) and puts
//! its render on the layer. Opened from the pad menu (Synth on layer ▸) or
//! a layer row's menu (Synth...).
//!
//! - Knobs drag up and down (the wheel steps them); a double click resets
//!   one. Each change renders on release and, with Audition on, plays the
//!   render.
//! - Wave and LFO shape boxes open a menu; toggles switch on a click, and
//!   each LFO target has its own Uni / Bi switch.
//! - The dot under a control includes it in Randomize, which moves the
//!   included controls towards random values by Amount; Dice dots sets the
//!   dots at random.
//! - The preset list holds the factory presets and the `.einstsynth` files
//!   in the documents folder's `Synth Presets`; a click (or < and >) loads
//!   one. Load picks any file, Save asks where.
//! - The scope shows the render: all of it, and its first 40 ms.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use dreist_formats::Node;
use dreist_gui::app::{App, MouseKind, UiEvent};
use dreist_gui::paint::{Painter, RectF};
use dreist_gui::render::Renderer;
use dreist_gui::style::{FontSpec, HAlign, VAlign};
use dreist_gui::{Colour, Ctrl, Rect};
use einst_dsp::synth::{self as dsp, COUNT, Kind, P, PARAMS, Patch};

use crate::command::Command;
use crate::model::Model;

pub const OVERLOAD: &str = "SynthPanel";
const OVERLAY: &str = "SynthOverlay";
/// The preset file extension.
pub const PRESET_EXT: &str = "einstsynth";
const PRESET_ROOT: &str = "SynthPreset";

const W: f32 = 1060.0;
const H: f32 = 530.0;
const TITLE: f32 = 28.0;
const ROW: f32 = 96.0;
const KNOB_W: f32 = 52.0;
const CHOICE_W: f32 = 70.0;
const TOGGLE_W: f32 = 48.0;
const BUTTON_W: f32 = 72.0;
const PAD: f32 = 8.0;
const GAP: f32 = 10.0;
/// Pixels of drag for the whole range.
const DRAG_RANGE: f32 = 200.0;
/// The right column (presets over the scope) starts here.
const RX: f32 = 720.0;
const RW: f32 = W - RX - GAP;
const ITEM_H: f32 = 18.0;
/// The scope's columns.
const SCOPE_COLS: usize = (RW - 2.0 * PAD) as usize;
/// The scope's close-up of the start, in seconds.
const ZOOM: f32 = 0.04;

const OPT_AUDITION: &str = "SynthAudition";
/// The parameters left out of Randomize (ids, comma-separated).
const OPT_RANDOM_OFF: &str = "SynthRandomOff";
/// Randomize's amount, in percent.
const OPT_RANDOM_AMOUNT: &str = "SynthRandomAmount";

const BG: Colour = Colour::rgba(34, 36, 40, 255);
const SECTION: Colour = Colour::rgba(45, 47, 52, 255);
const BORDER: Colour = Colour::rgba(70, 72, 78, 255);
const TEXT: Colour = Colour::rgba(214, 214, 214, 255);
const DIM: Colour = Colour::rgba(140, 142, 148, 255);
const ACCENT: Colour = Colour::rgba(232, 131, 58, 255);
const TRACK: Colour = Colour::rgba(72, 74, 80, 255);
const PRESSED: Colour = Colour::rgba(112, 115, 124, 255);
const WELL: Colour = Colour::rgba(26, 27, 30, 255);

/// The factory presets: changes from the default patch.
const FACTORY: &[(&str, &[(P, f32)])] = &[
    ("Init", &[]),
    ("Kick", &[(P::Osc1Wave, 0.0), (P::Osc1Coarse, 0.25), (P::Osc1PitchAmount, 0.85), (P::Osc1PitchDecay, 0.35), (P::AmpDecay, 0.6), (P::Cutoff, 0.8), (P::Drive, 0.3)]),
    ("Tom", &[(P::Osc1Wave, 0.0), (P::Osc1Coarse, 0.42), (P::Osc1PitchAmount, 0.72), (P::Osc1PitchDecay, 0.45), (P::AmpDecay, 0.62), (P::NoiseLevel, 0.15), (P::NoiseColour, 0.6)]),
    (
        "Snare",
        &[
            (P::Osc1Wave, 0.0),
            (P::Osc1Coarse, 0.6),
            (P::Osc1PitchAmount, 0.7),
            (P::Osc1PitchDecay, 0.3),
            (P::AmpDecay, 0.5),
            (P::NoiseLevel, 0.6),
            (P::NoiseColour, 0.2),
            (P::NoisePost, 1.0),
            (P::Cutoff, 0.85),
        ],
    ),
    ("Hat", &[(P::Osc1Level, 0.0), (P::NoiseLevel, 0.5), (P::Cutoff, 0.97), (P::Resonance, 0.4), (P::AmpDecay, 0.3)]),
    ("Zap", &[(P::Osc1Wave, 1.0), (P::Osc1PitchAmount, 1.0), (P::Osc1PitchDecay, 0.3), (P::AmpDecay, 0.45), (P::Resonance, 0.6), (P::Volume, 0.5)]),
    ("Sub Bass", &[(P::Osc1Wave, 0.0), (P::Osc1Coarse, 0.25), (P::Osc2Wave, 1.0), (P::Osc2Coarse, 0.25), (P::Osc2Level, 0.25), (P::Cutoff, 0.45), (P::AmpDecay, 0.75), (P::Drive, 0.2)]),
    (
        "Acid",
        &[(P::Osc1Wave, 2.0), (P::Osc1Coarse, 0.25), (P::Cutoff, 0.35), (P::Resonance, 0.75), (P::FilterEnv, 0.85), (P::FilterDecay, 0.4), (P::AmpDecay, 0.65), (P::Drive, 0.4), (P::Volume, 0.48)],
    ),
    (
        "Pluck",
        &[(P::Osc1Wave, 2.0), (P::Osc2Wave, 2.0), (P::Osc2Fine, 0.54), (P::Osc2Level, 0.5), (P::Cutoff, 0.3), (P::FilterEnv, 0.9), (P::FilterDecay, 0.35), (P::AmpDecay, 0.55), (P::Volume, 0.48)],
    ),
    ("Supersaw Stab", &[(P::Osc1Wave, 3.0), (P::Osc1Shape, 0.6), (P::Cutoff, 0.6), (P::FilterEnv, 0.7), (P::FilterDecay, 0.5), (P::AmpDecay, 0.6), (P::Volume, 0.75)]),
    (
        "FM Bell",
        &[
            (P::Osc1Wave, 0.0),
            (P::Osc1Coarse, 0.75),
            (P::Osc3Wave, 0.0),
            (P::Osc3Ratio, 0.6),
            (P::Osc3ToOsc1, 0.45),
            (P::Osc3EnvAmount, 1.0),
            (P::Osc3EnvDecay, 0.55),
            (P::Cutoff, 1.0),
            (P::AmpDecay, 0.9),
        ],
    ),
    (
        "Noise Sweep",
        &[
            (P::Osc1Level, 0.0),
            (P::NoiseLevel, 0.75),
            (P::NoiseColour, 0.5),
            (P::Cutoff, 0.25),
            (P::Resonance, 0.7),
            (P::FilterEnv, 1.0),
            (P::FilterAttack, 0.5),
            (P::FilterDecay, 0.7),
            (P::AmpAttack, 0.3),
            (P::AmpDecay, 0.85),
        ],
    ),
    (
        "Wobble",
        &[
            (P::Osc1Wave, 2.0),
            (P::Osc1Coarse, 0.25),
            (P::Cutoff, 0.35),
            (P::Resonance, 0.6),
            (P::LfoRate, 0.45),
            (P::LfoDepth, 0.7),
            (P::LfoCutoff, 1.0),
            (P::LfoCutoffBi, 0.0),
            (P::AmpDecay, 0.9),
            (P::Volume, 0.52),
        ],
    ),
];

/// Factory preset `i`.
pub fn factory_patch(i: usize) -> Patch {
    let mut p = Patch::default();
    for &(k, v) in FACTORY[i].1 {
        p.set(k, v);
    }
    p
}

/// The panel's buttons.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Button {
    Audition,
    Play,
    Init,
    Load,
    Save,
    Close,
    Randomize,
    Dice,
    Prev,
    Next,
}

impl Button {
    fn label(self) -> &'static str {
        match self {
            Button::Audition => "Audition",
            Button::Play => "Play",
            Button::Init => "Init",
            Button::Load => "Load",
            Button::Save => "Save",
            Button::Close => "Close",
            Button::Randomize => "Randomize",
            Button::Dice => "Dice dots",
            Button::Prev => "<",
            Button::Next => ">",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum Cell {
    Knob(P),
    Choice(P),
    Toggle(P),
    /// An LFO target: on / off over Uni / Bi.
    Target(P, P),
    Button(Button),
    /// Randomize's amount.
    Amount,
}

impl Cell {
    fn width(self) -> f32 {
        match self {
            Cell::Knob(_) | Cell::Target(..) | Cell::Amount => KNOB_W,
            Cell::Choice(_) => CHOICE_W,
            Cell::Toggle(_) => TOGGLE_W,
            Cell::Button(_) => BUTTON_W,
        }
    }

    /// The parameters the cell's randomize dot covers.
    fn params(self) -> Vec<P> {
        match self {
            Cell::Knob(p) | Cell::Choice(p) | Cell::Toggle(p) => vec![p],
            Cell::Target(a, b) => vec![a, b],
            Cell::Button(_) | Cell::Amount => Vec::new(),
        }
    }
}

use Cell::{Amount as AMT, Button as B, Choice as C, Knob as K, Target as TG, Toggle as T};

/// A section: its columns, each one cell or cells stacked.
type Section = (&'static str, &'static [&'static [Cell]]);

const OSC1: &[&[Cell]] = &[&[C(P::Osc1Wave)], &[K(P::Osc1Coarse)], &[K(P::Osc1Fine)], &[K(P::Osc1Shape)], &[K(P::Osc1Level)]];
const PENV1: &[&[Cell]] = &[&[K(P::Osc1PitchAttack)], &[K(P::Osc1PitchDecay)], &[K(P::Osc1PitchAmount)]];
const OSC2: &[&[Cell]] = &[&[C(P::Osc2Wave)], &[K(P::Osc2Coarse)], &[K(P::Osc2Fine)], &[K(P::Osc2Shape)], &[K(P::Osc2Level)]];
const PENV2: &[&[Cell]] = &[&[K(P::Osc2PitchAttack)], &[K(P::Osc2PitchDecay)], &[K(P::Osc2PitchAmount)]];
const OSC3: &[&[Cell]] = &[&[C(P::Osc3Wave)], &[K(P::Osc3Ratio)], &[K(P::Osc3ToOsc1)], &[K(P::Osc3ToOsc2)], &[K(P::Osc3ToFilter)]];
const DENV3: &[&[Cell]] = &[&[K(P::Osc3EnvAttack)], &[K(P::Osc3EnvDecay)], &[K(P::Osc3EnvAmount)]];
const NOISE: &[&[Cell]] = &[&[K(P::NoiseColour)], &[K(P::NoiseLevel)], &[T(P::NoisePost)]];
const XMOD: &[&[Cell]] = &[&[K(P::XMod)]];
const AMP: &[&[Cell]] = &[&[K(P::AmpAttack)], &[K(P::AmpDecay)]];
const FENV: &[&[Cell]] = &[&[K(P::FilterAttack)], &[K(P::FilterDecay)]];
const FILTER: &[&[Cell]] = &[&[K(P::Cutoff)], &[K(P::Resonance)], &[K(P::Spread)], &[K(P::Drive)], &[K(P::Clip)], &[K(P::FilterMix)], &[K(P::FilterEnv)]];
const LFO: &[&[Cell]] = &[
    &[C(P::LfoShape)],
    &[K(P::LfoRate)],
    &[K(P::LfoDepth)],
    &[TG(P::LfoOsc1, P::LfoOsc1Bi)],
    &[TG(P::LfoOsc2, P::LfoOsc2Bi)],
    &[TG(P::LfoCutoff, P::LfoCutoffBi)],
    &[TG(P::LfoResonance, P::LfoResonanceBi)],
    &[TG(P::LfoSpread, P::LfoSpreadBi)],
];
const OUT: &[&[Cell]] = &[&[K(P::Volume)]];
const RANDOM: &[&[Cell]] = &[&[AMT], &[B(Button::Randomize), B(Button::Dice)]];
const PATCH: &[&[Cell]] = &[&[B(Button::Audition), B(Button::Load)], &[B(Button::Play), B(Button::Save)], &[B(Button::Init), B(Button::Close)]];

const ROWS: &[&[Section]] = &[
    &[("OSC 1", OSC1), ("PITCH ENV 1", PENV1), ("AMP ENV", AMP), ("OUT", OUT)],
    &[("OSC 2", OSC2), ("PITCH ENV 2", PENV2), ("FILTER ENV", FENV), ("X-MOD", XMOD)],
    &[("OSC 3  FM", OSC3), ("DEPTH ENV 3", DENV3), ("NOISE", NOISE)],
    &[("DUAL-PEAK FILTER", FILTER), ("RANDOM", RANDOM)],
    &[("LFO", LFO), ("PATCH", PATCH)],
];

struct Layout {
    sections: Vec<(&'static str, RectF)>,
    cells: Vec<(Cell, RectF)>,
}

fn column_width(col: &[Cell]) -> f32 {
    col.iter().map(|c| c.width()).fold(0.0, f32::max)
}

/// Where the sections and cells sit in the panel.
fn layout() -> Layout {
    let mut sections = Vec::new();
    let mut cells = Vec::new();
    for (r, row) in ROWS.iter().enumerate() {
        let y = TITLE + 6.0 + r as f32 * ROW;
        let mut x = GAP;
        for &(title, cols) in row.iter() {
            let w: f32 = cols.iter().map(|c| column_width(c)).sum::<f32>() + 2.0 * PAD;
            sections.push((title, RectF::new(x, y, w, ROW - 6.0)));
            let mut cx = x + PAD;
            for col in cols.iter() {
                let (cw, ch) = (column_width(col), (ROW - 22.0) / col.len() as f32);
                for (i, &c) in col.iter().enumerate() {
                    cells.push((c, RectF::new(cx, y + 16.0 + i as f32 * ch, cw, ch)));
                }
                cx += cw;
            }
            x += w + GAP;
        }
    }
    Layout { sections, cells }
}

fn contains(r: &RectF, x: f32, y: f32) -> bool {
    x >= r.x && x < r.x + r.w && y >= r.y && y < r.y + r.h
}

/// A cell's knob circle.
fn knob_rect(r: &RectF) -> RectF {
    RectF::new(r.x + r.w / 2.0 - 16.0, r.y + 18.0, 32.0, 32.0)
}

/// Where the knob of `p` sits in the panel (tests).
#[cfg(test)]
pub(crate) fn knob_centre(p: P) -> (f32, f32) {
    let (_, r) = layout().cells.into_iter().find(|(c, _)| *c == Cell::Knob(p)).expect("a knob");
    let k = knob_rect(&r);
    (k.x + k.w / 2.0, k.y + k.h / 2.0)
}

fn box_rect(r: &RectF) -> RectF {
    RectF::new(r.x + 4.0, r.y + 24.0, r.w - 8.0, 22.0)
}

fn button_rect(r: &RectF) -> RectF {
    RectF::new(r.x + 3.0, r.y + 4.0, r.w - 6.0, r.h - 8.0)
}

/// A target's on / off switch and its Uni / Bi switch.
fn target_rects(r: &RectF) -> (RectF, RectF) {
    (RectF::new(r.x + 3.0, r.y + 16.0, r.w - 6.0, 22.0), RectF::new(r.x + 3.0, r.y + 42.0, r.w - 6.0, 18.0))
}

/// The randomize dot's hit area (the dot is its centre).
fn dot_rect(r: &RectF) -> RectF {
    RectF::new(r.x + r.w / 2.0 - 8.0, r.y + r.h - 12.0, 16.0, 12.0)
}

fn presets_rect() -> RectF {
    RectF::new(RX, TITLE + 6.0, RW, 3.0 * ROW - 6.0)
}

fn scope_rect() -> RectF {
    RectF::new(RX, TITLE + 6.0 + 3.0 * ROW, RW, 2.0 * ROW - 6.0)
}

/// The preset list's rows.
fn list_rect() -> RectF {
    let s = presets_rect();
    RectF::new(s.x + PAD, s.y + 20.0, s.w - 2.0 * PAD, s.h - 26.0)
}

fn visible_items() -> usize {
    (list_rect().h / ITEM_H) as usize
}

fn header_button(b: Button) -> RectF {
    let s = presets_rect();
    let x = s.x + s.w - PAD - if b == Button::Prev { 52.0 } else { 24.0 };
    RectF::new(x, s.y + 2.0, 24.0, 16.0)
}

/// The scope's two views: all of the render and its start.
fn scope_views() -> (RectF, RectF) {
    let s = scope_rect();
    let h = (s.h - 44.0) / 2.0;
    (RectF::new(s.x + PAD, s.y + 18.0, s.w - 2.0 * PAD, h), RectF::new(s.x + PAD, s.y + 36.0 + h, s.w - 2.0 * PAD, h))
}

/// Asks the editor for a file dialog.
#[derive(Debug, PartialEq)]
pub enum Request {
    SavePreset,
    OpenPreset,
}

/// A preset in the list.
#[derive(Clone, Debug, PartialEq)]
enum Source {
    Factory(usize),
    File(PathBuf),
}

#[derive(Clone, Debug)]
struct Item {
    name: String,
    source: Source,
}

impl Item {
    fn patch(&self) -> Option<Patch> {
        match &self.source {
            Source::Factory(i) => Some(factory_patch(*i)),
            Source::File(f) => load_preset(f),
        }
    }
}

/// The factory presets, then the user's.
fn preset_items() -> Vec<Item> {
    let factory = FACTORY.iter().enumerate().map(|(i, (n, _))| Item { name: n.to_string(), source: Source::Factory(i) });
    let user = presets_in(&presets_dir()).into_iter().map(|f| Item { name: stem(&f), source: Source::File(f) });
    factory.chain(user).collect()
}

/// What the scope shows: per column, the lowest and highest sample.
#[derive(Debug, Default)]
struct Scope {
    all: Vec<(f32, f32)>,
    start: Vec<(f32, f32)>,
    secs: f32,
}

fn bins(x: &[f32], cols: usize) -> Vec<(f32, f32)> {
    (0..cols)
        .map(|c| {
            let a = c * x.len() / cols;
            let b = ((c + 1) * x.len() / cols).max(a + 1).min(x.len());
            x.get(a..b).filter(|s| !s.is_empty()).map_or((0.0, 0.0), |s| s.iter().fold((f32::MAX, f32::MIN), |(lo, hi), &v| (lo.min(v), hi.max(v))))
        })
        .collect()
}

fn scope_of(x: &[f32], sr: f32) -> Scope {
    let n = ((ZOOM * sr) as usize).min(x.len());
    Scope { all: bins(x, SCOPE_COLS), start: bins(&x[..n], SCOPE_COLS), secs: x.len() as f32 / sr }
}

/// Moves the parameters not `off` towards random values by `amount`
/// (0..1): a knob that far towards a random value, a choice or a toggle
/// picked at random with that chance.
fn randomize(patch: &mut Patch, off: &[bool], amount: f32, rand: &mut impl FnMut() -> f32) {
    for &p in P::ALL {
        if off[p as usize] {
            continue;
        }
        let v = patch.get(p);
        let nv = match PARAMS[p as usize].kind {
            Kind::Knob => v + amount * (rand() - v),
            Kind::Choice(o) => {
                if rand() < amount {
                    (rand() * o.len() as f32).floor()
                } else {
                    v
                }
            }
            Kind::Toggle => {
                if rand() < amount {
                    (rand() < 0.5) as u8 as f32
                } else {
                    v
                }
            }
        };
        patch.set(p, nv);
    }
}

/// The knob being dragged.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Drag {
    Param(P),
    Amount,
}

pub struct SynthPanel {
    overlay: Option<Vec<usize>>,
    /// The layer being edited: engine, pad, layer.
    pub open: Option<(usize, usize, usize)>,
    patch: Patch,
    /// The knob being dragged, the press's y and the knob's value then.
    drag: Option<(Drag, f32, f32)>,
    /// Whether the drag moved the knob.
    dragged: bool,
    last_down: (f32, f32),
    pub audition: bool,
    /// The choice whose menu is open.
    menu: Option<P>,
    pub request: Option<Request>,
    status: String,
    /// The last render, for Play and the scope.
    render: Option<PathBuf>,
    scope: Option<Scope>,
    /// The button held down.
    pressed: Option<Button>,
    items: Vec<Item>,
    /// The preset loaded (or matching the patch).
    current: Option<usize>,
    /// The first preset row shown.
    scroll: usize,
    /// Per parameter: left out of Randomize.
    random_off: [bool; COUNT],
    amount: f32,
    seed: u64,
}

/// The renders folder: `Synth Renders` in the settings folder (as
/// `einst::synth::render_dir`; a unit test's own settings folder).
fn render_dir() -> Option<PathBuf> {
    if let Some(d) = std::env::var_os(einst::synth::RENDERS_ENV) {
        return Some(PathBuf::from(d));
    }
    crate::options::settings_dir().map(|d| d.join("Synth Renders"))
}

/// The presets folder.
pub fn presets_dir() -> PathBuf {
    crate::options::documents_dir().join("Synth Presets")
}

/// The presets in `dir`, sorted by name.
pub fn presets_in(dir: &Path) -> Vec<PathBuf> {
    let mut v: Vec<PathBuf> = std::fs::read_dir(dir).into_iter().flatten().flatten().map(|e| e.path()).filter(|p| p.extension().is_some_and(|e| e.eq_ignore_ascii_case(PRESET_EXT))).collect();
    v.sort_by_key(|p| p.file_stem().map(|s| s.to_string_lossy().to_lowercase()));
    v
}

/// Writes `patch` as a preset.
pub fn save_preset(path: &Path, patch: &Patch) -> std::io::Result<()> {
    let mut root = Node::new(PRESET_ROOT, None);
    root.set("Ver", 1);
    root.children.push(einst::synth::patch_node(patch));
    if let Some(d) = path.parent() {
        std::fs::create_dir_all(d)?;
    }
    std::fs::write(path, root.to_xml())
}

/// Reads a preset.
pub fn load_preset(path: &Path) -> Option<Patch> {
    let root = Node::parse(&std::fs::read_to_string(path).ok()?).ok()?;
    if root.class != PRESET_ROOT {
        return None;
    }
    einst::synth::patch_of(&root)
}

fn stem(path: &Path) -> String {
    path.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default()
}

impl SynthPanel {
    pub fn build(root: &mut Ctrl) -> SynthPanel {
        let panel = Ctrl {
            class: "CtrlPanel".into(),
            name: OVERLOAD.into(),
            overload: OVERLOAD.into(),
            style: "InvisibleStyle".into(),
            rect: Rect::new(0, 0, W as i32, H as i32),
            visible: true,
            enabled: true,
            ..Ctrl::default()
        };
        let overlay = Ctrl {
            class: "CtrlPanel".into(),
            name: OVERLAY.into(),
            overload: "ModalOverlay".into(),
            style: "InvisibleStyle".into(),
            rect: root.rect,
            zorder: 1001,
            visible: false,
            enabled: true,
            children: vec![panel],
            ..Ctrl::default()
        };
        root.children.push(overlay);
        let o = crate::options::Options::load();
        let off_ids = o.string(OPT_RANDOM_OFF, PARAMS[P::Volume as usize].id);
        let mut random_off = [false; COUNT];
        for id in off_ids.split(',') {
            if let Some(p) = PARAMS.iter().position(|i| i.id == id) {
                random_off[p] = true;
            }
        }
        let seed = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(1, |d| d.as_nanos() as u64) | 1;
        SynthPanel {
            overlay: Some(vec![root.children.len() - 1]),
            open: None,
            patch: Patch::default(),
            drag: None,
            dragged: false,
            last_down: (0.0, 0.0),
            audition: o.bool(OPT_AUDITION, true),
            menu: None,
            request: None,
            status: String::new(),
            render: None,
            scope: None,
            pressed: None,
            items: Vec::new(),
            current: None,
            scroll: 0,
            random_off,
            amount: (o.int(OPT_RANDOM_AMOUNT, 50).clamp(0, 100) as f32) / 100.0,
            seed,
        }
    }

    /// Opens on layer `l` of pad `p` in engine `e`: its patch, or the
    /// default one (the layer keeps its sample until a change).
    pub fn show(&mut self, app: &mut App, model: &Model, e: usize, p: usize, l: usize) {
        let layer = model.layer_node(e, p, l);
        self.patch = layer.and_then(einst::synth::patch_of).unwrap_or_default();
        let render = layer.filter(|n| einst::synth::patch_of(n).is_some()).and_then(|n| n.attr("AudioFileName")).map(PathBuf::from);
        self.set_render(render);
        self.open = Some((e, p, l));
        self.drag = None;
        self.menu = None;
        self.status.clear();
        self.items = preset_items();
        self.current = self.matching();
        self.reveal();
        let (rw, rh) = (app.root.rect.w, app.root.rect.h);
        if let Some(o) = self.overlay.clone().and_then(|p| app.ctrl_mut(&p)) {
            o.set_rect(Rect::new(0, 0, rw, rh));
            if let Some(panel) = o.children.first_mut() {
                let (w, h) = (W as i32, H as i32);
                panel.set_rect(Rect::new(((rw - w) / 2).max(0), ((rh - h) / 2).max(0), w, h));
            }
        }
    }

    pub fn close(&mut self) {
        self.open = None;
        self.drag = None;
        self.menu = None;
    }

    pub fn patch(&self) -> &Patch {
        &self.patch
    }

    pub fn sync(&self, app: &mut App) {
        let open = self.open.is_some();
        let stamp = self.stamp();
        if let Some(o) = self.overlay.clone().and_then(|p| app.ctrl_mut(&p)) {
            if o.visible != open {
                o.visible = open;
            }
            // The panel draws from this state, not from its control: the
            // stamp changes the control's drawn state, so the app repaints
            // the panel whenever what it shows changes.
            if let Some(panel) = o.children.first_mut() {
                if panel.attr("state") != Some(stamp.as_str()) {
                    panel.set_attr("state", stamp);
                }
            }
        }
    }

    /// A fingerprint of everything the panel shows.
    fn stamp(&self) -> String {
        use std::hash::{Hash, Hasher};
        let mut h = std::collections::hash_map::DefaultHasher::new();
        self.patch.v.iter().for_each(|v| v.to_bits().hash(&mut h));
        (self.open, self.drag.map(|(d, ..)| format!("{d:?}")), self.audition, self.pressed.map(|b| b as usize), &self.status, &self.render).hash(&mut h);
        (self.items.len(), self.current, self.scroll, self.random_off, self.amount.to_bits()).hash(&mut h);
        format!("{:016x}", h.finish())
    }

    pub(crate) fn panel_path(&self) -> Option<Vec<usize>> {
        self.overlay.as_ref().map(|o| o.iter().copied().chain([0]).collect())
    }

    fn mine(&self, path: &[usize]) -> bool {
        self.open.is_some() && self.overlay.as_ref().is_some_and(|p| path.starts_with(p))
    }

    fn cell_at(x: f32, y: f32) -> Option<(Cell, RectF)> {
        layout().cells.into_iter().find(|(_, r)| contains(r, x, y))
    }

    fn set_render(&mut self, path: Option<PathBuf>) {
        self.scope = path.as_deref().and_then(|f| dreist_engine::audio::load(f).ok()).map(|d| scope_of(&d.left, d.sample_rate));
        self.render = path;
    }

    /// Renders the patch, puts the render on the layer and rebuilds; with
    /// Audition on, plays it.
    fn commit(&mut self, model: &mut Model, out: &mut Vec<Command>) {
        let Some((e, p, l)) = self.open else { return };
        if crate::padmenu::layer_locked(model, e, p, l) {
            self.status = "The layer is locked".into();
            return;
        }
        let Some(dir) = render_dir() else {
            self.status = "No folder for renders".into();
            return;
        };
        let path = match einst::synth::render_to(&dir, &self.patch) {
            Ok(p) => p,
            Err(err) => {
                self.status = format!("Render failed: {err}");
                return;
            }
        };
        assign(model, e, p, l, &self.patch, &path);
        out.push(model.rebuild_command());
        self.set_render(Some(path));
        self.status.clear();
        if self.audition {
            self.play(out);
        }
    }

    fn play(&self, out: &mut Vec<Command>) {
        if let Some(d) = self.render.as_deref().and_then(|f| dreist_engine::audio::load(f).ok()) {
            out.push(Command::Preview(Some(Arc::new(d))));
        }
    }

    fn set(&mut self, p: P, v: f32) -> bool {
        let before = self.patch.get(p);
        self.patch.set(p, v);
        self.patch.get(p) != before
    }

    /// The preset whose patch is the panel's.
    fn matching(&self) -> Option<usize> {
        self.items.iter().position(|it| it.patch().as_ref() == Some(&self.patch))
    }

    /// Scrolls the list to show the current preset.
    fn reveal(&mut self) {
        let n = visible_items();
        if let Some(c) = self.current {
            if c < self.scroll {
                self.scroll = c;
            } else if c >= self.scroll + n {
                self.scroll = c + 1 - n;
            }
        }
        self.scroll = self.scroll.min(self.items.len().saturating_sub(n));
    }

    /// Loads preset `i` of the list.
    fn load_item(&mut self, model: &mut Model, i: usize, out: &mut Vec<Command>) {
        let Some(item) = self.items.get(i).cloned() else { return };
        match item.patch() {
            Some(p) => {
                self.patch = p;
                self.commit(model, out);
                self.current = Some(i);
                self.reveal();
                if self.status.is_empty() {
                    self.status = format!("Loaded {}", item.name);
                }
            }
            None => self.status = format!("{} is not a synth preset", item.name),
        }
    }

    /// A preset chosen from a dialog.
    pub fn load(&mut self, model: &mut Model, path: &Path, out: &mut Vec<Command>) {
        match load_preset(path) {
            Some(p) => {
                self.patch = p;
                self.commit(model, out);
                self.items = preset_items();
                self.current = self.items.iter().position(|it| it.source == Source::File(path.to_path_buf())).or_else(|| self.matching());
                self.reveal();
                if self.status.is_empty() {
                    self.status = format!("Loaded {}", stem(path));
                }
            }
            None => self.status = "Not a synth preset".into(),
        }
    }

    /// Saves the patch to `path` (with the preset extension).
    pub fn save(&mut self, path: &Path) {
        let path = path.with_extension(PRESET_EXT);
        self.status = match save_preset(&path, &self.patch) {
            Ok(()) => format!("Saved {}", stem(&path)),
            Err(e) => format!("Save failed: {e}"),
        };
        self.items = preset_items();
        self.current = self.items.iter().position(|it| it.source == Source::File(path.clone()));
        self.reveal();
    }

    fn rand(&mut self) -> f32 {
        // xorshift64*
        self.seed ^= self.seed >> 12;
        self.seed ^= self.seed << 25;
        self.seed ^= self.seed >> 27;
        (self.seed.wrapping_mul(0x2545_f491_4f6c_dd1d) >> 40) as f32 / (1u64 << 24) as f32
    }

    fn save_random(&self) {
        let mut o = crate::options::Options::load();
        let off: Vec<&str> = P::ALL.iter().filter(|p| self.random_off[**p as usize]).map(|p| PARAMS[*p as usize].id).collect();
        o.set(OPT_RANDOM_OFF, off.join(","));
        o.set(OPT_RANDOM_AMOUNT, (self.amount * 100.0).round() as i64);
        o.save();
    }

    /// Whether the cell's dot is on (it covers all its parameters).
    fn dot_on(&self, cell: Cell) -> bool {
        cell.params().iter().all(|p| !self.random_off[*p as usize])
    }

    fn set_dot(&mut self, cell: Cell, on: bool) {
        for p in cell.params() {
            self.random_off[p as usize] = !on;
        }
    }

    fn press(&mut self, app: &mut App, model: &mut Model, x: f32, y: f32, out: &mut Vec<Command>) {
        self.last_down = (x, y);
        if x >= W - 30.0 && y < TITLE {
            self.close();
            return;
        }
        for b in [Button::Prev, Button::Next] {
            if contains(&header_button(b), x, y) {
                self.pressed = Some(b);
                let n = self.items.len();
                if n > 0 {
                    let i = match (self.current, b) {
                        (None, Button::Prev) => n - 1,
                        (None, _) => 0,
                        (Some(c), Button::Prev) => (c + n - 1) % n,
                        (Some(c), _) => (c + 1) % n,
                    };
                    self.load_item(model, i, out);
                }
                return;
            }
        }
        let list = list_rect();
        if contains(&list, x, y) {
            let i = self.scroll + ((y - list.y) / ITEM_H) as usize;
            if i < self.items.len() && i < self.scroll + visible_items() {
                self.load_item(model, i, out);
            }
            return;
        }
        let lay = layout();
        if let Some(&(cell, _)) = lay.cells.iter().find(|(c, r)| !c.params().is_empty() && contains(&dot_rect(r), x, y)) {
            let on = !self.dot_on(cell);
            self.set_dot(cell, on);
            self.save_random();
            return;
        }
        let Some((cell, rc)) = Self::cell_at(x, y) else { return };
        let anchor = self.panel_path().unwrap_or_default();
        match cell {
            Cell::Knob(p) => {
                self.drag = Some((Drag::Param(p), y, self.patch.get(p)));
                self.dragged = false;
            }
            Cell::Amount => {
                self.drag = Some((Drag::Amount, y, self.amount));
                self.dragged = false;
            }
            Cell::Choice(p) => {
                let Kind::Choice(options) = PARAMS[p as usize].kind else { return };
                let cur = self.patch.get(p).round() as usize;
                app.open_menu(&anchor, options.iter().map(|s| s.to_string()).collect(), Some(cur));
                self.menu = Some(p);
            }
            Cell::Toggle(p) => {
                let v = 1.0 - self.patch.get(p);
                self.set(p, v);
                self.commit(model, out);
            }
            Cell::Target(on, bi) => {
                let (r_on, r_bi) = target_rects(&rc);
                let p = if contains(&r_bi, x, y) {
                    bi
                } else if contains(&r_on, x, y) {
                    on
                } else {
                    return;
                };
                let v = 1.0 - self.patch.get(p);
                self.set(p, v);
                self.commit(model, out);
            }
            Cell::Button(b) => {
                self.pressed = Some(b);
                self.button(model, b, out);
            }
        }
    }

    fn button(&mut self, model: &mut Model, b: Button, out: &mut Vec<Command>) {
        match b {
            Button::Audition => {
                self.audition = !self.audition;
                let mut o = crate::options::Options::load();
                o.set(OPT_AUDITION, self.audition);
                o.save();
            }
            Button::Play => {
                if self.render.is_none() {
                    self.commit(model, out);
                }
                self.play(out);
            }
            Button::Init => {
                self.patch = Patch::default();
                self.commit(model, out);
                self.current = self.matching();
                self.reveal();
            }
            Button::Randomize => {
                let (off, amount) = (self.random_off, self.amount);
                let mut patch = self.patch.clone();
                randomize(&mut patch, &off, amount, &mut || self.rand());
                self.patch = patch;
                self.commit(model, out);
                self.current = self.matching();
            }
            Button::Dice => {
                for (cell, _) in layout().cells {
                    if !cell.params().is_empty() {
                        let on = self.rand() < 0.5;
                        self.set_dot(cell, on);
                    }
                }
                self.save_random();
            }
            Button::Load => self.request = Some(Request::OpenPreset),
            Button::Save => self.request = Some(Request::SavePreset),
            Button::Close => self.close(),
            Button::Prev | Button::Next => {}
        }
    }

    /// Returns whether the event was the panel's.
    pub fn handle(&mut self, app: &mut App, model: &mut Model, ev: &UiEvent, out: &mut Vec<Command>) -> bool {
        if self.open.is_none() {
            return false;
        }
        match ev {
            UiEvent::Mouse { path, overload, kind, x, y } if overload == OVERLOAD && self.mine(path) => {
                match kind {
                    MouseKind::Down => self.press(app, model, *x, *y, out),
                    MouseKind::Drag => match self.drag {
                        Some((Drag::Param(p), y0, v0)) => {
                            let v = v0 + (y0 - y) / DRAG_RANGE;
                            self.dragged |= self.set(p, v);
                        }
                        Some((Drag::Amount, y0, v0)) => {
                            let v = (v0 + (y0 - y) / DRAG_RANGE).clamp(0.0, 1.0);
                            self.dragged |= v != self.amount;
                            self.amount = v;
                        }
                        None => {}
                    },
                    MouseKind::Up => {
                        self.pressed = None;
                        let drag = self.drag.take();
                        if std::mem::take(&mut self.dragged) {
                            match drag {
                                Some((Drag::Param(_), ..)) => {
                                    self.commit(model, out);
                                    self.current = self.matching();
                                }
                                Some((Drag::Amount, ..)) => self.save_random(),
                                None => {}
                            }
                        }
                    }
                    MouseKind::RightDown => {}
                }
                true
            }
            UiEvent::DoubleClicked { path, .. } if self.panel_path().as_deref() == Some(path.as_slice()) => {
                let (x, y) = self.last_down;
                match Self::cell_at(x, y) {
                    Some((Cell::Knob(p), r)) if !contains(&dot_rect(&r), x, y) => {
                        self.drag = None;
                        if self.set(p, PARAMS[p as usize].default) {
                            self.commit(model, out);
                            self.current = self.matching();
                        }
                    }
                    Some((Cell::Amount, _)) => {
                        self.drag = None;
                        self.amount = 0.5;
                        self.save_random();
                    }
                    _ => {}
                }
                true
            }
            UiEvent::Wheel { path, dy, .. } if self.mine(path) => {
                // The wheel scrolls the list, or steps the knob, under the
                // last hover position.
                let (x, y) = self.last_down;
                if contains(&list_rect(), x, y) {
                    let max = self.items.len().saturating_sub(visible_items());
                    self.scroll = if *dy > 0.0 { self.scroll.saturating_sub(1) } else { (self.scroll + 1).min(max) };
                    return true;
                }
                match Self::cell_at(x, y) {
                    Some((Cell::Knob(p), _)) => {
                        let v = self.patch.get(p) + dy.signum() * 0.01;
                        if self.set(p, v) {
                            self.commit(model, out);
                            self.current = self.matching();
                        }
                    }
                    Some((Cell::Amount, _)) => {
                        self.amount = (self.amount + dy.signum() * 0.01).clamp(0.0, 1.0);
                        self.save_random();
                    }
                    _ => {}
                }
                true
            }
            UiEvent::Hover { path, x, y, .. } if self.mine(path) && self.panel_path().as_deref() == Some(path.as_slice()) => {
                self.last_down = (*x, *y);
                false
            }
            UiEvent::MenuSelected { anchor, index, .. } if self.panel_path().as_deref() == Some(anchor.as_slice()) => {
                if let Some(p) = self.menu.take() {
                    if self.set(p, *index as f32) {
                        self.commit(model, out);
                        self.current = self.matching();
                    }
                }
                true
            }
            UiEvent::Mouse { path, overload, kind: MouseKind::Down, .. } if overload == "ModalOverlay" && self.overlay.as_deref() == Some(path.as_slice()) => {
                self.close();
                true
            }
            UiEvent::Mouse { path, .. } | UiEvent::Wheel { path, .. } | UiEvent::Pressed { path, .. } | UiEvent::Clicked { path, .. } | UiEvent::ValueChanged { path, .. } => self.mine(path),
            _ => false,
        }
    }

    pub fn draw(&self, model: &Model, r: &Renderer, p: &mut Painter, c: &Ctrl) -> bool {
        if c.overload != OVERLOAD {
            return false;
        }
        let font = r.styles.font("ApplicationStyle");
        p.fill_rounded(RectF::new(0.0, 0.0, W, H), 6.0, [true; 4], BG);
        p.stroke_rounded(RectF::new(0.5, 0.5, W - 1.0, H - 1.0), 6.0, [true; 4], BORDER, 1.0);
        let title = match self.open {
            Some((e, pad, l)) => {
                let name = model.pad_name(e, pad).unwrap_or_else(|| format!("Pad {}", pad + 1));
                format!("SYNTH   {name}  ·  Layer {}", l + 1)
            }
            None => "SYNTH".into(),
        };
        p.text(RectF::new(12.0, 0.0, W - 60.0, TITLE), &title, TEXT, &font, HAlign::Left, VAlign::Center);
        if !self.status.is_empty() {
            p.text(RectF::new(W / 2.0, 0.0, W / 2.0 - 40.0, TITLE), &self.status, ACCENT, &font, HAlign::Right, VAlign::Center);
        }
        p.text(RectF::new(W - 30.0, 0.0, 24.0, TITLE), "x", DIM, &font, HAlign::Center, VAlign::Center);
        let lay = layout();
        for (t, s) in &lay.sections {
            p.fill_rounded(*s, 4.0, [true; 4], SECTION);
            p.text(RectF::new(s.x + PAD, s.y + 2.0, s.w - 2.0 * PAD, 14.0), t, ACCENT, &font, HAlign::Left, VAlign::Center);
        }
        for (cell, rc) in &lay.cells {
            self.draw_cell(p, &font, *cell, rc);
            if !cell.params().is_empty() {
                let d = dot_rect(rc);
                let dot = RectF::new(d.x + d.w / 2.0 - 3.0, d.y + d.h / 2.0 - 3.0, 6.0, 6.0);
                if self.dot_on(*cell) {
                    p.fill_ellipse(dot, ACCENT);
                } else {
                    p.stroke_ellipse(dot, DIM, 1.0);
                }
            }
        }
        self.draw_presets(p, &font);
        self.draw_scope(p, &font);
        true
    }

    fn draw_cell(&self, p: &mut Painter, font: &FontSpec, cell: Cell, rc: &RectF) {
        match cell {
            Cell::Knob(k) => {
                let v = self.patch.get(k);
                let bipolar = matches!(k, P::FilterEnv | P::Osc1Coarse | P::Osc2Coarse | P::Osc1Fine | P::Osc2Fine | P::Osc1PitchAmount | P::Osc2PitchAmount);
                let active = self.drag.is_some_and(|(d, ..)| d == Drag::Param(k));
                draw_knob(p, font, rc, PARAMS[k as usize].label, v, &dsp::display(k, v), bipolar, active);
            }
            Cell::Amount => {
                let active = self.drag.is_some_and(|(d, ..)| d == Drag::Amount);
                draw_knob(p, font, rc, "Amount", self.amount, &format!("{:.0} %", self.amount * 100.0), false, active);
            }
            Cell::Choice(k) => {
                p.text(RectF::new(rc.x, rc.y, rc.w, 14.0), PARAMS[k as usize].label, DIM, font, HAlign::Center, VAlign::Center);
                let b = box_rect(rc);
                p.fill_rounded(b, 3.0, [true; 4], TRACK);
                p.text(b, &dsp::display(k, self.patch.get(k)), TEXT, font, HAlign::Center, VAlign::Center);
            }
            Cell::Toggle(k) => {
                let on = self.patch.get(k) >= 0.5;
                let b = box_rect(rc);
                // Noise joins before or after the filter: a two-way switch.
                let (label, lit) = match k {
                    P::NoisePost => (if on { "Post" } else { "Pre" }, on),
                    _ => (PARAMS[k as usize].label, on),
                };
                if k == P::NoisePost {
                    p.text(RectF::new(rc.x, rc.y, rc.w, 14.0), "Filter", DIM, font, HAlign::Center, VAlign::Center);
                }
                p.fill_rounded(b, 3.0, [true; 4], if lit { ACCENT } else { TRACK });
                p.text(b, label, if lit { BG } else { TEXT }, font, HAlign::Center, VAlign::Center);
            }
            Cell::Target(on_p, bi_p) => {
                let (on, bi) = (self.patch.get(on_p) >= 0.5, self.patch.get(bi_p) >= 0.5);
                let (r_on, r_bi) = target_rects(rc);
                if on_p == P::LfoOsc1 {
                    p.text(RectF::new(rc.x, rc.y, rc.w * 5.0, 14.0), "Targets", DIM, font, HAlign::Center, VAlign::Center);
                }
                p.fill_rounded(r_on, 3.0, [true; 4], if on { ACCENT } else { TRACK });
                p.text(r_on, PARAMS[on_p as usize].label, if on { BG } else { TEXT }, font, HAlign::Center, VAlign::Center);
                p.stroke_rounded(r_bi, 3.0, [true; 4], TRACK, 1.0);
                let c = if on { TEXT } else { DIM };
                p.text(r_bi, if bi { "Bi" } else { "Uni" }, c, font, HAlign::Center, VAlign::Center);
            }
            Cell::Button(b) => {
                let on = b == Button::Audition && self.audition;
                let br = button_rect(rc);
                let fill = if self.pressed == Some(b) {
                    PRESSED
                } else if on {
                    ACCENT
                } else {
                    TRACK
                };
                p.fill_rounded(br, 3.0, [true; 4], fill);
                p.text(br, b.label(), if on { BG } else { TEXT }, font, HAlign::Center, VAlign::Center);
            }
        }
    }

    fn draw_presets(&self, p: &mut Painter, font: &FontSpec) {
        let s = presets_rect();
        p.fill_rounded(s, 4.0, [true; 4], SECTION);
        p.text(RectF::new(s.x + PAD, s.y + 2.0, s.w - 2.0 * PAD, 16.0), "PRESETS", ACCENT, font, HAlign::Left, VAlign::Center);
        for b in [Button::Prev, Button::Next] {
            let r = header_button(b);
            p.fill_rounded(r, 3.0, [true; 4], if self.pressed == Some(b) { PRESSED } else { TRACK });
            p.text(r, b.label(), TEXT, font, HAlign::Center, VAlign::Center);
        }
        let list = list_rect();
        p.fill_rounded(list, 3.0, [true; 4], WELL);
        for (row, (i, item)) in self.items.iter().enumerate().skip(self.scroll).take(visible_items()).enumerate() {
            let r = RectF::new(list.x, list.y + row as f32 * ITEM_H, list.w, ITEM_H);
            let cur = self.current == Some(i);
            if cur {
                p.fill_rect(r, TRACK);
            }
            let text = RectF::new(r.x + 6.0, r.y, r.w - 12.0, r.h);
            p.text(text, &item.name, if cur { ACCENT } else { TEXT }, font, HAlign::Left, VAlign::Center);
            let tag = if matches!(item.source, Source::Factory(_)) { "Factory" } else { "User" };
            p.text(text, tag, DIM, font, HAlign::Right, VAlign::Center);
        }
        // How far down the list is.
        let n = visible_items();
        if self.items.len() > n {
            let t = list.h * n as f32 / self.items.len() as f32;
            let y = list.y + (list.h - t) * self.scroll as f32 / (self.items.len() - n) as f32;
            p.fill_rect(RectF::new(list.x + list.w - 3.0, y, 2.0, t), DIM);
        }
    }

    fn draw_scope(&self, p: &mut Painter, font: &FontSpec) {
        let s = scope_rect();
        p.fill_rounded(s, 4.0, [true; 4], SECTION);
        p.text(RectF::new(s.x + PAD, s.y + 2.0, s.w - 2.0 * PAD, 14.0), "SCOPE", ACCENT, font, HAlign::Left, VAlign::Center);
        let (all, start) = scope_views();
        if let Some(sc) = &self.scope {
            let len = if sc.secs < 1.0 { format!("{:.0} ms", sc.secs * 1000.0) } else { format!("{:.2} s", sc.secs) };
            p.text(RectF::new(s.x + PAD, s.y + 2.0, s.w - 2.0 * PAD, 14.0), &len, DIM, font, HAlign::Right, VAlign::Center);
        }
        p.text(RectF::new(start.x, start.y - 17.0, start.w, 16.0), &format!("First {:.0} ms", ZOOM * 1000.0), DIM, font, HAlign::Left, VAlign::Center);
        for (view, cols) in [(all, self.scope.as_ref().map(|s| &s.all)), (start, self.scope.as_ref().map(|s| &s.start))] {
            p.fill_rect(view, WELL);
            let mid = view.y + view.h / 2.0;
            p.line(view.x, mid, view.x + view.w, mid, TRACK, 1.0);
            let half = view.h / 2.0 - 1.0;
            for (i, &(lo, hi)) in cols.into_iter().flatten().enumerate() {
                let x = view.x + i as f32 + 0.5;
                let y0 = mid - hi.clamp(-1.0, 1.0) * half;
                let y1 = (mid - lo.clamp(-1.0, 1.0) * half).max(y0 + 1.0);
                p.line(x, y0, x, y1, ACCENT, 1.0);
            }
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn draw_knob(p: &mut Painter, font: &FontSpec, rc: &RectF, label: &str, v: f32, value: &str, bipolar: bool, active: bool) {
    p.text(RectF::new(rc.x, rc.y, rc.w, 14.0), label, DIM, font, HAlign::Center, VAlign::Center);
    let kr = knob_rect(rc);
    p.fill_ellipse(RectF::new(kr.x + 5.0, kr.y + 5.0, kr.w - 10.0, kr.h - 10.0), TRACK);
    p.stroke_arc(kr, 135.0, 270.0, true, TRACK, 3.0);
    // Bipolar knobs light from the centre.
    let (start, sweep) = if bipolar { (270.0, (v - 0.5) * 270.0) } else { (135.0, v * 270.0) };
    if sweep < 0.0 {
        p.stroke_arc(kr, start + sweep, -sweep, true, ACCENT, 3.0);
    } else {
        p.stroke_arc(kr, start, sweep, true, ACCENT, 3.0);
    }
    let a = (135.0 + v * 270.0f32).to_radians();
    let (cx, cy) = (kr.x + kr.w / 2.0, kr.y + kr.h / 2.0);
    p.line(cx + 4.0 * a.cos(), cy + 4.0 * a.sin(), cx + 10.0 * a.cos(), cy + 10.0 * a.sin(), TEXT, 2.0);
    p.text(RectF::new(rc.x - 4.0, kr.y + kr.h + 2.0, rc.w + 8.0, 14.0), value, if active { ACCENT } else { TEXT }, font, HAlign::Center, VAlign::Center);
}

/// Puts the render `file` of `patch` on layer `l` of pad `p`, as a sample
/// load does (the sample's markers go, the hold fits the sample), and
/// stores the patch.
pub fn assign(model: &mut Model, e: usize, p: usize, l: usize, patch: &Patch, file: &Path) {
    let layer = model.layer_mut(e, p, l);
    layer.children.retain(|c| c.class != "AudioFileMarkers");
    layer.set("AudioFileName", file.to_string_lossy());
    layer.child_mut_or_insert("FixedString", "LayerName").set("string", einst::synth::LAYER_NAME);
    einst::synth::set_patch(layer, patch);
    if let Some((frames, sr)) = dreist_engine::audio::info(file) {
        let n = layer.clone();
        let get = |q: dreist_formats::params::LayerParam| n.f32(q.name()).unwrap_or(q.default_value()).clamp(0.0, 1.0);
        let bound = n.attr("PlaybackMode").map(dreist_dsp::voice::PlaybackMode::canonical) == Some("OneShotLengthBound");
        let h = dreist_engine::hold::reset_hold_time(&get, bound, frames as f64, frames as f64, sr, false);
        layer.set("AmpEnvHold", h).set("FreeEnvHold", h);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cells_fit_the_panel_and_do_not_overlap() {
        let lay = layout();
        for (c, r) in &lay.cells {
            assert!(r.x >= 0.0 && r.y >= TITLE && r.x + r.w <= RX && r.y + r.h <= H, "{c:?} at {r:?}");
        }
        for (t, s) in &lay.sections {
            assert!(s.x + s.w <= RX - GAP + 0.01 && s.y + s.h <= H, "{t} at {s:?}");
        }
        for (i, (a, ra)) in lay.cells.iter().enumerate() {
            for (b, rb) in &lay.cells[i + 1..] {
                let overlap = ra.x < rb.x + rb.w && rb.x < ra.x + ra.w && ra.y < rb.y + rb.h && rb.y < ra.y + ra.h;
                assert!(!overlap, "{a:?} overlaps {b:?}");
            }
        }
        // Every parameter has a control.
        for &p in P::ALL {
            assert!(lay.cells.iter().any(|(c, _)| c.params().contains(&p)), "{p:?}");
        }
        // The right column fits too.
        for r in [presets_rect(), scope_rect()] {
            assert!(r.x + r.w <= W && r.y + r.h <= H, "{r:?}");
        }
        assert!(presets_rect().y + presets_rect().h <= scope_rect().y);
    }

    #[test]
    fn presets_round_trip() {
        let dir = std::env::temp_dir().join(format!("einst-synth-presets-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let mut p = Patch::default();
        p.set(P::Osc1Wave, 3.0);
        p.set(P::Resonance, 0.85);
        save_preset(&dir.join(format!("Bass.{PRESET_EXT}")), &p).unwrap();
        std::fs::write(dir.join("other.txt"), "x").unwrap();
        let list = presets_in(&dir);
        assert_eq!(list.len(), 1);
        assert_eq!(load_preset(&list[0]), Some(p));
        assert_eq!(load_preset(&dir.join("other.txt")), None);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn factory_presets_differ_and_sound() {
        assert_eq!(factory_patch(0), Patch::default());
        for i in 0..FACTORY.len() {
            for j in i + 1..FACTORY.len() {
                assert_ne!(factory_patch(i), factory_patch(j), "{} and {}", FACTORY[i].0, FACTORY[j].0);
            }
            let x = dsp::render(&factory_patch(i));
            let peak = x.iter().fold(0f32, |m, v| m.max(v.abs()));
            assert!(peak > 0.05 && peak <= 0.9, "{}: peak {peak}", FACTORY[i].0);
        }
    }

    #[test]
    fn randomize_moves_only_dotted_controls_by_the_amount() {
        let mut seed = 7u32;
        let mut rand = || {
            seed = seed.wrapping_mul(1_103_515_245).wrapping_add(12_345);
            (seed >> 8) as f32 / (1u32 << 24) as f32
        };
        let base = Patch::default();
        let mut off = [false; COUNT];
        off[P::Cutoff as usize] = true;
        let mut p = base.clone();
        randomize(&mut p, &off, 0.0, &mut rand);
        assert_eq!(p, base, "amount 0 changes nothing");
        let mut p = base.clone();
        randomize(&mut p, &off, 0.3, &mut rand);
        assert_eq!(p.get(P::Cutoff), base.get(P::Cutoff), "an undotted control stays");
        for &k in P::ALL {
            if matches!(PARAMS[k as usize].kind, Kind::Knob) {
                assert!((p.get(k) - base.get(k)).abs() <= 0.3 + 1e-6, "{k:?} moved too far");
            }
        }
        let mut p = base.clone();
        randomize(&mut p, &[false; COUNT], 1.0, &mut rand);
        assert!(P::ALL.iter().filter(|&&k| p.get(k) != base.get(k)).count() > COUNT / 2, "full amount moves most controls");
    }

    #[test]
    fn the_scope_bins_the_render() {
        let x: Vec<f32> = (0..48_000).map(|i| if i < 24_000 { 0.5 } else { -0.25 }).collect();
        let s = scope_of(&x, 48_000.0);
        assert_eq!(s.all.len(), SCOPE_COLS);
        assert_eq!(s.all[0], (0.5, 0.5));
        assert_eq!(*s.all.last().unwrap(), (-0.25, -0.25));
        assert_eq!(s.start.len(), SCOPE_COLS);
        assert!((s.secs - 1.0).abs() < 1e-6);
        // Shorter than a column each.
        let s = scope_of(&[0.1, -0.1], 48_000.0);
        assert_eq!(s.all.len(), SCOPE_COLS);
    }
}
