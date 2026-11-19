//! The synth panel: edits a layer's synth patch (`einst::synth`) and puts
//! its render on the layer. Opened from the pad menu (Synth on layer ▸) or
//! a layer row's menu (Synth...).
//!
//! - Knobs drag up and down (the wheel steps them); a double click resets
//!   one. Each change renders on release and, with Audition on, plays the
//!   render.
//! - Wave and LFO shape boxes open a menu; the LFO's target buttons toggle.
//! - Presets are `.einstsynth` files in the documents folder's
//!   `Synth Presets`: Load lists them (Browse... picks any file), Save asks
//!   where.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use dreist_formats::Node;
use dreist_gui::app::{App, MouseKind, UiEvent};
use dreist_gui::paint::{Painter, RectF};
use dreist_gui::render::Renderer;
use dreist_gui::style::{HAlign, VAlign};
use dreist_gui::{Colour, Ctrl, Rect};
use einst_dsp::synth::{self as dsp, Kind, P, PARAMS, Patch};

use crate::command::Command;
use crate::model::Model;

pub const OVERLOAD: &str = "SynthPanel";
const OVERLAY: &str = "SynthOverlay";
/// The preset file extension.
pub const PRESET_EXT: &str = "einstsynth";
const PRESET_ROOT: &str = "SynthPreset";

const W: f32 = 700.0;
const H: f32 = 438.0;
const TITLE: f32 = 28.0;
const ROW: f32 = 100.0;
const KNOB_W: f32 = 56.0;
const CHOICE_W: f32 = 76.0;
const TOGGLE_W: f32 = 54.0;
const BUTTON_W: f32 = 72.0;
const PAD: f32 = 8.0;
const GAP: f32 = 10.0;
/// Pixels of drag for the whole range.
const DRAG_RANGE: f32 = 200.0;

const BG: Colour = Colour::rgba(34, 36, 40, 255);
const SECTION: Colour = Colour::rgba(45, 47, 52, 255);
const BORDER: Colour = Colour::rgba(70, 72, 78, 255);
const TEXT: Colour = Colour::rgba(214, 214, 214, 255);
const DIM: Colour = Colour::rgba(140, 142, 148, 255);
const ACCENT: Colour = Colour::rgba(232, 131, 58, 255);
const TRACK: Colour = Colour::rgba(72, 74, 80, 255);
const PRESSED: Colour = Colour::rgba(112, 115, 124, 255);

/// The panel's buttons.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Button {
    Audition,
    Play,
    Init,
    Load,
    Save,
    Close,
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
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum Cell {
    Knob(P),
    Choice(P),
    Toggle(P),
    Button(Button),
}

impl Cell {
    fn width(self) -> f32 {
        match self {
            Cell::Knob(_) => KNOB_W,
            Cell::Choice(_) => CHOICE_W,
            Cell::Toggle(_) => TOGGLE_W,
            Cell::Button(_) => BUTTON_W,
        }
    }
}

use Cell::{Button as B, Choice as C, Knob as K, Toggle as T};

const OSC1: &[Cell] = &[C(P::Osc1Wave), K(P::Osc1Coarse), K(P::Osc1Fine), K(P::Osc1Shape), K(P::Osc1Level)];
const OSC2: &[Cell] = &[C(P::Osc2Wave), K(P::Osc2Coarse), K(P::Osc2Fine), K(P::Osc2Shape), K(P::Osc2Level)];
const OSC3: &[Cell] = &[C(P::Osc3Wave), K(P::Osc3Ratio), K(P::Osc3ToOsc1), K(P::Osc3ToOsc2), K(P::Osc3ToFilter)];
const XMOD: &[Cell] = &[K(P::XMod)];
const AMP: &[Cell] = &[K(P::AmpAttack), K(P::AmpDecay)];
const FENV: &[Cell] = &[K(P::FilterAttack), K(P::FilterDecay)];
const FILTER: &[Cell] = &[K(P::Cutoff), K(P::Resonance), K(P::Spread), K(P::Drive), K(P::Clip), K(P::FilterMix), K(P::FilterEnv)];
const LFO: &[Cell] = &[C(P::LfoShape), K(P::LfoRate), K(P::LfoDepth), T(P::LfoOsc1), T(P::LfoOsc2), T(P::LfoCutoff), T(P::LfoResonance), T(P::LfoSpread)];
const OUT: &[Cell] = &[K(P::Volume)];
/// Buttons: two rows of three.
const PATCH_TOP: &[Cell] = &[B(Button::Audition), B(Button::Play), B(Button::Init)];
const PATCH_BOTTOM: &[Cell] = &[B(Button::Load), B(Button::Save), B(Button::Close)];

const ROWS: &[&[(&str, &[Cell])]] = &[
    &[("OSC 1", OSC1), ("OSC 2", OSC2)],
    &[("OSC 3  FM", OSC3), ("X-MOD", XMOD), ("AMP ENV", AMP), ("FILTER ENV", FENV)],
    &[("DUAL-PEAK FILTER", FILTER), ("PATCH", PATCH_TOP)],
    &[("LFO", LFO), ("OUT", OUT)],
];

struct Layout {
    sections: Vec<(&'static str, RectF)>,
    cells: Vec<(Cell, RectF)>,
}

/// Where the sections and cells sit in the panel.
fn layout() -> Layout {
    let mut sections = Vec::new();
    let mut cells = Vec::new();
    for (r, row) in ROWS.iter().enumerate() {
        let y = TITLE + 6.0 + r as f32 * ROW;
        let mut x = GAP;
        for &(title, row_cells) in row.iter() {
            let w: f32 = row_cells.iter().map(|c| c.width()).sum::<f32>() + 2.0 * PAD;
            sections.push((title, RectF::new(x, y, w, ROW - 6.0)));
            let mut cx = x + PAD;
            for &c in row_cells {
                cells.push((c, RectF::new(cx, y + 16.0, c.width(), ROW - 22.0)));
                cx += c.width();
            }
            // The patch buttons' second row sits under the first.
            if title == "PATCH" {
                let mut cx = x + PAD;
                for &c in PATCH_BOTTOM {
                    cells.push((c, RectF::new(cx, y + 16.0 + 36.0, c.width(), 36.0)));
                    cx += c.width();
                }
            }
            x += w + GAP;
        }
    }
    // The patch buttons' first row is 36 px high too.
    for (c, r) in &mut cells {
        if matches!(c, Cell::Button(_)) && r.h > 36.0 {
            r.h = 36.0;
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

fn box_rect(r: &RectF) -> RectF {
    RectF::new(r.x + 4.0, r.y + 24.0, r.w - 8.0, 22.0)
}

fn button_rect(r: &RectF) -> RectF {
    RectF::new(r.x + 3.0, r.y + 6.0, r.w - 6.0, 24.0)
}

enum Menu {
    Choice(P),
    Presets(Vec<PathBuf>),
}

/// Asks the editor for a file dialog.
#[derive(Debug, PartialEq)]
pub enum Request {
    SavePreset,
    OpenPreset,
}

pub struct SynthPanel {
    overlay: Option<Vec<usize>>,
    /// The layer being edited: engine, pad, layer.
    pub open: Option<(usize, usize, usize)>,
    patch: Patch,
    /// The knob being dragged, the press's y and the knob's value then.
    drag: Option<(P, f32, f32)>,
    /// Whether the drag moved the knob.
    dragged: bool,
    last_down: (f32, f32),
    pub audition: bool,
    menu: Option<Menu>,
    pub request: Option<Request>,
    status: String,
    /// The last render, for Play.
    render: Option<PathBuf>,
    /// The button held down.
    pressed: Option<Button>,
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
        SynthPanel {
            overlay: Some(vec![root.children.len() - 1]),
            open: None,
            patch: Patch::default(),
            drag: None,
            dragged: false,
            last_down: (0.0, 0.0),
            audition: crate::options::Options::load().bool("SynthAudition", true),
            menu: None,
            request: None,
            status: String::new(),
            render: None,
            pressed: None,
        }
    }

    /// Opens on layer `l` of pad `p` in engine `e`: its patch, or the
    /// default one (the layer keeps its sample until a change).
    pub fn show(&mut self, app: &mut App, model: &Model, e: usize, p: usize, l: usize) {
        let layer = model.layer_node(e, p, l);
        self.patch = layer.and_then(einst::synth::patch_of).unwrap_or_default();
        self.render = layer.filter(|n| einst::synth::patch_of(n).is_some()).and_then(|n| n.attr("AudioFileName")).map(PathBuf::from);
        self.open = Some((e, p, l));
        self.drag = None;
        self.menu = None;
        self.status.clear();
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
        (self.open, self.drag.map(|(p, ..)| p as usize), self.audition, self.pressed.map(|b| b as usize), &self.status, &self.render).hash(&mut h);
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
        self.render = Some(path);
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

    /// A preset chosen from the menu or a dialog.
    pub fn load(&mut self, model: &mut Model, path: &Path, out: &mut Vec<Command>) {
        match load_preset(path) {
            Some(p) => {
                self.patch = p;
                self.commit(model, out);
                self.status = format!("Loaded {}", path.file_stem().map(|s| s.to_string_lossy()).unwrap_or_default());
            }
            None => self.status = "Not a synth preset".into(),
        }
    }

    /// Saves the patch to `path` (with the preset extension).
    pub fn save(&mut self, path: &Path) {
        let path = path.with_extension(PRESET_EXT);
        self.status = match save_preset(&path, &self.patch) {
            Ok(()) => format!("Saved {}", path.file_stem().map(|s| s.to_string_lossy()).unwrap_or_default()),
            Err(e) => format!("Save failed: {e}"),
        };
    }

    fn press(&mut self, app: &mut App, model: &mut Model, x: f32, y: f32, out: &mut Vec<Command>) {
        self.last_down = (x, y);
        if x >= W - 30.0 && y < TITLE {
            self.close();
            return;
        }
        let Some((cell, _)) = Self::cell_at(x, y) else { return };
        let anchor = self.panel_path().unwrap_or_default();
        match cell {
            Cell::Knob(p) => {
                self.drag = Some((p, y, self.patch.get(p)));
                self.dragged = false;
            }
            Cell::Choice(p) => {
                let Kind::Choice(options) = PARAMS[p as usize].kind else { return };
                let cur = self.patch.get(p).round() as usize;
                app.open_menu(&anchor, options.iter().map(|s| s.to_string()).collect(), Some(cur));
                self.menu = Some(Menu::Choice(p));
            }
            Cell::Toggle(p) => {
                let v = 1.0 - self.patch.get(p);
                self.set(p, v);
                self.commit(model, out);
            }
            Cell::Button(b) => {
                self.pressed = Some(b);
                self.button(app, model, b, &anchor, out);
            }
        }
    }

    fn button(&mut self, app: &mut App, model: &mut Model, b: Button, anchor: &[usize], out: &mut Vec<Command>) {
        match b {
            Button::Audition => {
                self.audition = !self.audition;
                let mut o = crate::options::Options::load();
                o.set("SynthAudition", self.audition);
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
            }
            Button::Load => {
                let presets = presets_in(&presets_dir());
                let mut items: Vec<String> = presets.iter().map(|p| p.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default()).collect();
                if !items.is_empty() {
                    items.push(String::new());
                }
                items.push("Browse...".into());
                app.open_menu(anchor, items, None);
                self.menu = Some(Menu::Presets(presets));
            }
            Button::Save => self.request = Some(Request::SavePreset),
            Button::Close => self.close(),
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
                    MouseKind::Drag => {
                        if let Some((p, y0, v0)) = self.drag {
                            let v = v0 + (y0 - y) / DRAG_RANGE;
                            self.dragged |= self.set(p, v);
                        }
                    }
                    MouseKind::Up => {
                        self.pressed = None;
                        if self.drag.take().is_some() && std::mem::take(&mut self.dragged) {
                            self.commit(model, out);
                        }
                    }
                    MouseKind::RightDown => {}
                }
                true
            }
            UiEvent::DoubleClicked { path, .. } if self.panel_path().as_deref() == Some(path.as_slice()) => {
                let (x, y) = self.last_down;
                if let Some((Cell::Knob(p), _)) = Self::cell_at(x, y) {
                    self.drag = None;
                    if self.set(p, PARAMS[p as usize].default) {
                        self.commit(model, out);
                    }
                }
                true
            }
            UiEvent::Wheel { path, dy, .. } if self.mine(path) => {
                // The wheel steps the knob under the last hover position.
                if let Some((Cell::Knob(p), _)) = Self::cell_at(self.last_down.0, self.last_down.1) {
                    let v = self.patch.get(p) + dy.signum() * 0.01;
                    if self.set(p, v) {
                        self.commit(model, out);
                    }
                }
                true
            }
            UiEvent::Hover { path, x, y, .. } if self.mine(path) && self.panel_path().as_deref() == Some(path.as_slice()) => {
                self.last_down = (*x, *y);
                false
            }
            UiEvent::MenuSelected { anchor, index, .. } if self.panel_path().as_deref() == Some(anchor.as_slice()) => {
                match self.menu.take() {
                    Some(Menu::Choice(p)) => {
                        if self.set(p, *index as f32) {
                            self.commit(model, out);
                        }
                    }
                    Some(Menu::Presets(list)) => match list.get(*index) {
                        Some(f) => self.load(model, &f.clone(), out),
                        None => self.request = Some(Request::OpenPreset),
                    },
                    None => {}
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
            match *cell {
                Cell::Knob(k) => self.draw_knob(p, &font, k, rc),
                Cell::Choice(k) => {
                    p.text(RectF::new(rc.x, rc.y, rc.w, 14.0), PARAMS[k as usize].label, DIM, &font, HAlign::Center, VAlign::Center);
                    let b = box_rect(rc);
                    p.fill_rounded(b, 3.0, [true; 4], TRACK);
                    p.text(b, &dsp::display(k, self.patch.get(k)), TEXT, &font, HAlign::Center, VAlign::Center);
                }
                Cell::Toggle(k) => {
                    let on = self.patch.get(k) >= 0.5;
                    let b = RectF::new(rc.x + 3.0, rc.y + 24.0, rc.w - 6.0, 22.0);
                    p.fill_rounded(b, 3.0, [true; 4], if on { ACCENT } else { TRACK });
                    p.text(b, PARAMS[k as usize].label, if on { BG } else { TEXT }, &font, HAlign::Center, VAlign::Center);
                    if k == P::LfoOsc1 {
                        p.text(RectF::new(rc.x, rc.y, rc.w * 5.0, 14.0), "Targets", DIM, &font, HAlign::Center, VAlign::Center);
                    }
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
                    p.text(br, b.label(), if on { BG } else { TEXT }, &font, HAlign::Center, VAlign::Center);
                }
            }
        }
        true
    }

    fn draw_knob(&self, p: &mut Painter, font: &dreist_gui::style::FontSpec, k: P, rc: &RectF) {
        let v = self.patch.get(k);
        p.text(RectF::new(rc.x, rc.y, rc.w, 14.0), PARAMS[k as usize].label, DIM, font, HAlign::Center, VAlign::Center);
        let kr = knob_rect(rc);
        p.fill_ellipse(RectF::new(kr.x + 5.0, kr.y + 5.0, kr.w - 10.0, kr.h - 10.0), TRACK);
        p.stroke_arc(kr, 135.0, 270.0, true, TRACK, 3.0);
        // Bipolar knobs light from the centre.
        let bipolar = matches!(k, P::FilterEnv | P::Osc1Coarse | P::Osc2Coarse | P::Osc1Fine | P::Osc2Fine);
        let (start, sweep) = if bipolar { (270.0, (v - 0.5) * 270.0) } else { (135.0, v * 270.0) };
        if sweep < 0.0 {
            p.stroke_arc(kr, start + sweep, -sweep, true, ACCENT, 3.0);
        } else {
            p.stroke_arc(kr, start, sweep, true, ACCENT, 3.0);
        }
        let a = (135.0 + v * 270.0f32).to_radians();
        let (cx, cy) = (kr.x + kr.w / 2.0, kr.y + kr.h / 2.0);
        p.line(cx + 4.0 * a.cos(), cy + 4.0 * a.sin(), cx + 10.0 * a.cos(), cy + 10.0 * a.sin(), TEXT, 2.0);
        let active = self.drag.is_some_and(|(d, ..)| d == k);
        p.text(RectF::new(rc.x - 4.0, kr.y + kr.h + 2.0, rc.w + 8.0, 14.0), &dsp::display(k, v), if active { ACCENT } else { TEXT }, font, HAlign::Center, VAlign::Center);
    }
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
            assert!(r.x >= 0.0 && r.y >= TITLE && r.x + r.w <= W && r.y + r.h <= H, "{c:?} at {r:?}");
        }
        for (i, (a, ra)) in lay.cells.iter().enumerate() {
            for (b, rb) in &lay.cells[i + 1..] {
                let overlap = ra.x < rb.x + rb.w && rb.x < ra.x + ra.w && ra.y < rb.y + rb.h && rb.y < ra.y + ra.h;
                assert!(!overlap, "{a:?} overlaps {b:?}");
            }
        }
        // Every parameter has a control.
        for p in P::ALL {
            assert!(lay.cells.iter().any(|(c, _)| matches!(c, Cell::Knob(k) | Cell::Choice(k) | Cell::Toggle(k) if k == p)), "{p:?}");
        }
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
}
