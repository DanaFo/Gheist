//! A drum synth whose sound is rendered to a sample: a layer plays the
//! render like any other sample.
//!
//! - Osc 1 and Osc 2 (sine, square, saw, supersaw) with Osc 2 cross-modulating
//!   Osc 1 (Osc 2 also plays white, pink or brown noise); each has a pitch AD
//!   envelope. Osc 3 is an FM operator (sine, square, triangle, saw, noise)
//!   aimed at Osc 1, Osc 2 and the cutoff, its depth shaped by an AD
//!   envelope.
//! - A noise source blends white to pink to brown and joins before or after
//!   the filter.
//! - Each oscillator goes through its own mixer channel, which overdrives
//!   asymmetrically above unity.
//! - The filter is two state-variable low-passes whose peaks sit `spread`
//!   octaves apart, with a pre drive and a post clip. Resonance reaches
//!   self-oscillation (a sine) at 80 %; the last 20 % soft-clip it. Mix
//!   blends the filter's input back in before the post clip.
//! - Two AD envelopes (amp, filter) and one LFO with a toggle and a
//!   unipolar / bipolar switch per target.
//!
//! [`render`] runs at four times [`RATE`] and decimates, so the folder,
//! FM, cross-modulation and drive alias little; the same patch always
//! renders the same samples.

use std::f32::consts::{PI, TAU};

/// The rate of the rendered sample.
pub const RATE: u32 = 48_000;
/// The render's oversampling.
const OS: usize = 4;
/// The rate noise is made at (it holds between): the noise's colours and
/// level do not follow the oversampling.
const NOISE_RATE: usize = 2 * RATE as usize;

/// The note an oscillator plays with its coarse and fine at the centre.
pub const BASE_NOTE: f32 = 48.0;

/// How a parameter is set.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Kind {
    /// 0..1.
    Knob,
    /// One of the options, stored as its index.
    Choice(&'static [&'static str]),
    /// Off (0) or on (1).
    Toggle,
}

/// One parameter of [`PARAMS`].
#[derive(Clone, Copy, Debug)]
pub struct ParamInfo {
    /// Its key in saved patches.
    pub id: &'static str,
    /// Its label on the panel.
    pub label: &'static str,
    pub kind: Kind,
    pub default: f32,
}

/// Osc 1's waves.
pub const WAVES: &[&str] = &["Sine", "Square", "Saw", "Supersaw"];
/// Osc 2's waves: Osc 1's and noises.
pub const OSC2_WAVES: &[&str] = &["Sine", "Square", "Saw", "Supersaw", "White", "Pink", "Brown"];
/// LFO shapes.
pub const LFO_SHAPES: &[&str] = &["Sine", "Triangle", "Saw", "Square", "S&H"];
/// Osc 3's waves.
pub const OSC3_WAVES: &[&str] = &["Sine", "Square", "Triangle", "Saw", "Noise"];

macro_rules! params {
    ($($name:ident = $id:literal, $label:literal, $kind:expr, $default:expr;)*) => {
        /// A parameter, indexing [`PARAMS`] and [`Patch::v`].
        #[derive(Clone, Copy, Debug, PartialEq, Eq)]
        #[repr(usize)]
        pub enum P { $($name),* }
        /// Every parameter, in [`P`] order.
        pub const PARAMS: &[ParamInfo] = &[$(ParamInfo { id: $id, label: $label, kind: $kind, default: $default }),*];
        impl P {
            /// Every parameter, in order.
            pub const ALL: &[P] = &[$(P::$name),*];
        }
    };
}

params! {
    Osc1Wave = "osc1_wave", "Wave", Kind::Choice(WAVES), 2.0;
    Osc1Coarse = "osc1_coarse", "Coarse", Kind::Knob, 0.5;
    Osc1Fine = "osc1_fine", "Fine", Kind::Knob, 0.5;
    Osc1Shape = "osc1_shape", "Shape", Kind::Knob, 0.0;
    Osc1Level = "osc1_level", "Level", Kind::Knob, 0.5;
    Osc1PitchAttack = "osc1_pitch_attack", "Attack", Kind::Knob, 0.0;
    Osc1PitchDecay = "osc1_pitch_decay", "Decay", Kind::Knob, 0.5;
    Osc1PitchAmount = "osc1_pitch_amount", "Amount", Kind::Knob, 0.5;
    Osc2Wave = "osc2_wave", "Wave", Kind::Choice(OSC2_WAVES), 1.0;
    Osc2Coarse = "osc2_coarse", "Coarse", Kind::Knob, 0.5;
    Osc2Fine = "osc2_fine", "Fine", Kind::Knob, 0.5;
    Osc2Shape = "osc2_shape", "Shape", Kind::Knob, 0.0;
    Osc2Level = "osc2_level", "Level", Kind::Knob, 0.0;
    Osc2PitchAttack = "osc2_pitch_attack", "Attack", Kind::Knob, 0.0;
    Osc2PitchDecay = "osc2_pitch_decay", "Decay", Kind::Knob, 0.5;
    Osc2PitchAmount = "osc2_pitch_amount", "Amount", Kind::Knob, 0.5;
    XMod = "xmod", "X-Mod", Kind::Knob, 0.0;
    Osc3Wave = "osc3_wave", "Wave", Kind::Choice(OSC3_WAVES), 0.0;
    Osc3Ratio = "osc3_ratio", "Ratio", Kind::Knob, 1.0 / 3.0;
    Osc3ToOsc1 = "osc3_osc1", "> Osc 1", Kind::Knob, 0.0;
    Osc3ToOsc2 = "osc3_osc2", "> Osc 2", Kind::Knob, 0.0;
    Osc3ToFilter = "osc3_filter", "> Filter", Kind::Knob, 0.0;
    Osc3EnvAttack = "osc3_env_attack", "Attack", Kind::Knob, 0.0;
    Osc3EnvDecay = "osc3_env_decay", "Decay", Kind::Knob, 0.6;
    Osc3EnvAmount = "osc3_env_amount", "Amount", Kind::Knob, 0.0;
    NoiseColour = "noise_colour", "Colour", Kind::Knob, 0.0;
    NoiseLevel = "noise_level", "Level", Kind::Knob, 0.0;
    NoisePost = "noise_post", "Post", Kind::Toggle, 0.0;
    Cutoff = "cutoff", "Cutoff", Kind::Knob, 0.7;
    Resonance = "resonance", "Reso", Kind::Knob, 0.3;
    Spread = "spread", "Spread", Kind::Knob, 0.0;
    Drive = "drive", "Drive", Kind::Knob, 0.0;
    Clip = "clip", "Clip", Kind::Knob, 0.0;
    FilterMix = "filter_mix", "Mix", Kind::Knob, 1.0;
    FilterEnv = "filter_env", "Env", Kind::Knob, 0.5;
    AmpAttack = "amp_attack", "Attack", Kind::Knob, 0.0;
    AmpDecay = "amp_decay", "Decay", Kind::Knob, 0.8;
    FilterAttack = "filter_attack", "Attack", Kind::Knob, 0.0;
    FilterDecay = "filter_decay", "Decay", Kind::Knob, 0.6;
    LfoRate = "lfo_rate", "Rate", Kind::Knob, 0.5;
    LfoShape = "lfo_shape", "Shape", Kind::Choice(LFO_SHAPES), 0.0;
    LfoDepth = "lfo_depth", "Depth", Kind::Knob, 0.0;
    LfoOsc1 = "lfo_osc1", "Osc 1", Kind::Toggle, 0.0;
    LfoOsc2 = "lfo_osc2", "Osc 2", Kind::Toggle, 0.0;
    LfoCutoff = "lfo_cutoff", "Cutoff", Kind::Toggle, 0.0;
    LfoResonance = "lfo_resonance", "Reso", Kind::Toggle, 0.0;
    LfoSpread = "lfo_spread", "Spread", Kind::Toggle, 0.0;
    LfoOsc1Bi = "lfo_osc1_bi", "Bi", Kind::Toggle, 1.0;
    LfoOsc2Bi = "lfo_osc2_bi", "Bi", Kind::Toggle, 1.0;
    LfoCutoffBi = "lfo_cutoff_bi", "Bi", Kind::Toggle, 1.0;
    LfoResonanceBi = "lfo_resonance_bi", "Bi", Kind::Toggle, 1.0;
    LfoSpreadBi = "lfo_spread_bi", "Bi", Kind::Toggle, 1.0;
    Volume = "volume", "Volume", Kind::Knob, 0.6;
}

/// The number of parameters.
pub const COUNT: usize = P::ALL.len();

/// A synth patch: one value per [`P`].
#[derive(Clone, Debug, PartialEq)]
pub struct Patch {
    pub v: [f32; COUNT],
}

impl Default for Patch {
    fn default() -> Self {
        let mut v = [0.0; COUNT];
        for (i, p) in PARAMS.iter().enumerate() {
            v[i] = p.default;
        }
        Patch { v }
    }
}

impl Patch {
    pub fn get(&self, p: P) -> f32 {
        self.v[p as usize]
    }

    /// Sets `p`, clamped to its range.
    pub fn set(&mut self, p: P, x: f32) {
        self.v[p as usize] = clamp(p, x);
    }

    fn choice(&self, p: P) -> usize {
        self.get(p).round().max(0.0) as usize
    }

    fn on(&self, p: P) -> bool {
        self.get(p) >= 0.5
    }

    /// The patch as `(id, value)` pairs.
    pub fn values(&self) -> impl Iterator<Item = (&'static str, f32)> + '_ {
        PARAMS.iter().zip(self.v).map(|(p, v)| (p.id, v))
    }

    /// A patch from `(id, value)` pairs; missing or unknown ids keep their
    /// defaults.
    pub fn from_values<'a>(values: impl IntoIterator<Item = (&'a str, f32)>) -> Patch {
        let mut patch = Patch::default();
        for (id, v) in values {
            if let Some(i) = PARAMS.iter().position(|p| p.id == id) {
                if v.is_finite() {
                    patch.set(P::ALL[i], v);
                }
            }
        }
        patch
    }

    /// A stable hash of the patch and the renderer's version, naming its
    /// render.
    pub fn hash(&self) -> u64 {
        let mut h: u64 = 0xcbf2_9ce4_8422_2325;
        for b in VERSION.to_le_bytes().into_iter().chain(self.v.iter().flat_map(|v| v.to_bits().to_le_bytes())) {
            h ^= b as u64;
            h = h.wrapping_mul(0x0100_0000_01b3);
        }
        h
    }

    /// The rendered length in seconds.
    pub fn length(&self) -> f32 {
        attack_time(self.get(P::AmpAttack)) + decay_time(self.get(P::AmpDecay)) + FADE
    }
}

/// Bumped when a change to the renderer changes its output, so old renders
/// are not reused.
const VERSION: u32 = 4;
const FADE: f32 = 0.005;

fn clamp(p: P, x: f32) -> f32 {
    match PARAMS[p as usize].kind {
        Kind::Knob => x.clamp(0.0, 1.0),
        Kind::Choice(o) => x.round().clamp(0.0, (o.len() - 1) as f32),
        Kind::Toggle => {
            if x >= 0.5 {
                1.0
            } else {
                0.0
            }
        }
    }
}

/// Attack in seconds: about 0.2 ms to 0.7 s.
pub fn attack_time(x: f32) -> f32 {
    0.0002 * 3500f32.powf(x)
}

/// Decay in seconds: 2 ms to 1.5 s.
pub fn decay_time(x: f32) -> f32 {
    0.002 * 750f32.powf(x)
}

fn semitones(coarse: f32, fine: f32) -> f32 {
    ((coarse - 0.5) * 48.0).round() + (fine - 0.5) * 2.0
}

fn cutoff_hz(x: f32) -> f32 {
    20.0 * 1000f32.powf(x)
}

fn ratio(x: f32) -> f32 {
    0.25 * 64f32.powf(x)
}

fn lfo_hz(x: f32) -> f32 {
    0.05 * 600f32.powf(x)
}

/// LFO depth eases in: the first quarter of the knob reaches about 10 %.
pub fn lfo_depth(x: f32) -> f32 {
    x.max(0.0).powf(1.66)
}

/// A pitch envelope's amount in semitones: ±48, finer near the centre.
fn pitch_amount(x: f32) -> f32 {
    let u = 2.0 * x - 1.0;
    u * u.abs() * 48.0
}

/// Octaves between the two filter peaks.
fn spread_octaves(x: f32) -> f32 {
    x * 4.0
}

/// The text a panel shows for `p` at `v`.
/// The sine's gain into the folder at `shape`.
fn fold_gain(shape: f32) -> f32 {
    1.0 + 4.0 * shape
}

/// The square's pulse width (of a cycle) at `shape`.
fn pulse_width(shape: f32) -> f32 {
    0.5 - 0.45 * shape
}

/// The supersaw's outer voices' detune in semitones at `shape`.
fn super_detune(shape: f32) -> f32 {
    shape * shape * 0.6 + 0.04
}

/// What an oscillator's Shape does on `wave` (Osc 1 and Osc 2's waves):
/// its knob's label.
pub fn shape_label(wave: usize) -> &'static str {
    match wave {
        0 => "Fold",
        1 => "Width",
        2 => "Tri",
        3 => "Detune",
        _ => "Shape",
    }
}

/// Shape `v` on `wave`, as its knob shows it.
pub fn shape_display(wave: usize, v: f32) -> String {
    match wave {
        0 => format!("x{:.1}", fold_gain(v)),
        1 => format!("{:.0} %", pulse_width(v) * 100.0),
        2 => format!("{:.0} %", v * 100.0),
        3 => format!("{:.0} ct", super_detune(v) * 100.0),
        // Noise has no shape.
        _ => "-".into(),
    }
}

pub fn display(p: P, v: f32) -> String {
    use P::*;
    let info = PARAMS[p as usize];
    match info.kind {
        Kind::Choice(o) => return o[(v.round().max(0.0) as usize).min(o.len() - 1)].to_string(),
        Kind::Toggle => return if v >= 0.5 { "On" } else { "Off" }.to_string(),
        Kind::Knob => {}
    }
    let ms = |s: f32| if s < 1.0 { format!("{:.0} ms", s * 1000.0) } else { format!("{s:.2} s") };
    let hz = |f: f32| if f < 1000.0 { format!("{f:.0} Hz") } else { format!("{:.2} kHz", f / 1000.0) };
    match p {
        Osc1Coarse | Osc2Coarse => format!("{:+} st", ((v - 0.5) * 48.0).round() as i32),
        Osc1Fine | Osc2Fine => format!("{:+} ct", ((v - 0.5) * 200.0).round() as i32),
        Osc3Ratio => format!("x{:.2}", ratio(v)),
        Osc1PitchAmount | Osc2PitchAmount => format!("{:+.1} st", pitch_amount(v)),
        LfoDepth => format!("{:.0} %", lfo_depth(v) * 100.0),
        NoiseColour => match v {
            v if v < 0.17 => "White".into(),
            v if v < 0.42 => "White-Pink".into(),
            v if v < 0.58 => "Pink".into(),
            v if v < 0.83 => "Pink-Brown".into(),
            _ => "Brown".into(),
        },
        Cutoff => hz(cutoff_hz(v)),
        Spread => format!("{:.1} oct", spread_octaves(v)),
        FilterEnv => format!("{:+.1} oct", (v - 0.5) * 12.0),
        AmpAttack | FilterAttack | Osc1PitchAttack | Osc2PitchAttack | Osc3EnvAttack => {
            let s = attack_time(v);
            if s < 0.01 { format!("{:.1} ms", s * 1000.0) } else { ms(s) }
        }
        AmpDecay | FilterDecay | Osc1PitchDecay | Osc2PitchDecay | Osc3EnvDecay => ms(decay_time(v)),
        LfoRate => format!("{:.2} Hz", lfo_hz(v)),
        Osc1Level | Osc2Level | NoiseLevel => {
            let g = level_gain(v);
            if g <= 0.0 { "-inf dB".into() } else { format!("{:+.1} dB", 20.0 * g.log10()) }
        }
        Volume => {
            let g = volume_gain(v);
            if g <= 0.0 { "-inf dB".into() } else { format!("{:+.1} dB", 20.0 * g.log10()) }
        }
        _ => format!("{:.0} %", v * 100.0),
    }
}

/// A mixer channel's gain: unity at the centre, +12 dB at the top.
fn level_gain(x: f32) -> f32 {
    4.0 * x * x
}

fn volume_gain(x: f32) -> f32 {
    2.0 * x * x
}

/// A hot mixer channel: clean at low levels, asymmetric soft saturation as
/// it is driven past unity.
fn channel(x: f32) -> f32 {
    const B: f32 = 0.2;
    let t = B.tanh();
    3.0 * ((x / 3.0 + B).tanh() - t) / (1.0 - t * t)
}

/// `tanh(g x) / tanh(g)`: nearly linear for small `g`, a soft clip at ±1 for
/// large.
fn soft(x: f32, g: f32) -> f32 {
    (g * x).tanh() / g.tanh()
}

/// A drive amount (0..1) as the gain of [`soft`].
fn drive_gain(x: f32) -> f32 {
    0.25 * 64f32.powf(x)
}

fn poly_blep(t: f32, dt: f32) -> f32 {
    if t < dt {
        let t = t / dt;
        t + t - t * t - 1.0
    } else if t > 1.0 - dt {
        let t = (t - 1.0) / dt;
        t * t + t + t + 1.0
    } else {
        0.0
    }
}

fn saw(p: f32, dt: f32) -> f32 {
    2.0 * p - 1.0 - poly_blep(p, dt)
}

/// The residual of a band-limited ramp (a corner) `t` cycles from it, for
/// a slope change of one per sample.
fn poly_blamp(t: f32, dt: f32) -> f32 {
    if t < dt {
        let t = t / dt - 1.0;
        -t * t * t / 6.0
    } else if t > 1.0 - dt {
        let t = (t - 1.0) / dt + 1.0;
        t * t * t / 6.0
    } else {
        0.0
    }
}

/// A triangle (-1 at phase 0, 1 at a half) with rounded corners: its slope
/// turns by 8 per cycle at each.
fn triangle(p: f32, dt: f32) -> f32 {
    let naive = 1.0 - 4.0 * (p - 0.5).abs();
    naive + 8.0 * dt * (poly_blamp(p, dt) - poly_blamp(wrap(p + 0.5), dt))
}

fn wrap(x: f32) -> f32 {
    x - x.floor()
}

/// Folds `x` back into ±1 like a triangle.
fn fold(x: f32) -> f32 {
    4.0 * (wrap((x - 1.0) / 4.0) - 0.5).abs() - 1.0
}

/// White, pink and brown noise, each about ±1 at its peaks, made every
/// `hold` ticks.
struct Noise {
    seed: u32,
    pink: [f32; 7],
    brown: f32,
    hold: usize,
    count: usize,
    held: [f32; 3],
}

impl Noise {
    fn new(seed: u32) -> Noise {
        Noise { seed, pink: [0.0; 7], brown: 0.0, hold: 1, count: 0, held: [0.0; 3] }
    }

    fn tick(&mut self) -> [f32; 3] {
        if self.count == 0 {
            self.held = self.next();
        }
        self.count = (self.count + 1) % self.hold;
        self.held
    }

    fn next(&mut self) -> [f32; 3] {
        self.seed = self.seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
        let w = (self.seed >> 8) as f32 / (1u32 << 23) as f32 - 1.0;
        // Pink: a sum of one-pole filters (-3 dB per octave).
        let b = &mut self.pink;
        b[0] = 0.99886 * b[0] + w * 0.055_517_9;
        b[1] = 0.99332 * b[1] + w * 0.075_075_9;
        b[2] = 0.96900 * b[2] + w * 0.153_852;
        b[3] = 0.86650 * b[3] + w * 0.310_485_6;
        b[4] = 0.55000 * b[4] + w * 0.532_952_2;
        b[5] = -0.7616 * b[5] - w * 0.016_898;
        let pink = (b[0] + b[1] + b[2] + b[3] + b[4] + b[5] + b[6] + w * 0.5362) * 0.11;
        b[6] = w * 0.115_926;
        // Brown: leaky integrated white (-6 dB per octave).
        self.brown = (self.brown + 0.02 * w) / 1.02;
        [w, pink, self.brown * 3.5]
    }
}

/// White (0) through pink (0.5) to brown (1).
fn colour(c: f32, n: [f32; 3]) -> f32 {
    if c < 0.5 { n[0] + (n[1] - n[0]) * c * 2.0 } else { n[1] + (n[2] - n[1]) * (c - 0.5) * 2.0 }
}

const SUPER: usize = 7;
const SUPER_SPREAD: [f32; SUPER] = [-1.0, -0.62, -0.28, 0.0, 0.3, 0.6, 1.0];

struct Osc {
    phase: f32,
    supers: [f32; SUPER],
    /// The supersaw voices' pitch ratios.
    ratios: [f32; SUPER],
    last: f32,
    noise: Noise,
}

impl Osc {
    /// An oscillator whose supersaw detune follows `shape`.
    fn new(seed: u32, shape: f32) -> Osc {
        let mut s = seed;
        let mut supers = [0.0; SUPER];
        for p in &mut supers {
            s = s.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
            *p = (s >> 8) as f32 / (1u32 << 24) as f32;
        }
        let detune = super_detune(shape);
        let ratios = SUPER_SPREAD.map(|s| 2f32.powf(s * detune / 12.0));
        Osc { phase: 0.0, supers, ratios, last: 0.0, noise: Noise::new(seed.wrapping_mul(7919)) }
    }

    /// One sample of `wave` at `inc` cycles per sample, phase-modulated by
    /// `pm` cycles.
    fn tick(&mut self, wave: usize, inc: f32, pm: f32, shape: f32) -> f32 {
        let p = wrap(self.phase + pm);
        let moved = p - self.last;
        let dt = (moved - moved.round()).abs().clamp(1e-5, 0.5);
        self.last = p;
        self.phase = wrap(self.phase + inc);
        match wave {
            0 => fold((TAU * p).sin() * fold_gain(shape)),
            1 => {
                let pw = pulse_width(shape);
                let mut y = if p < pw { 1.0 } else { -1.0 };
                y += poly_blep(p, dt);
                y -= poly_blep(wrap(p - pw + 1.0), dt);
                y
            }
            2 => {
                if shape <= 0.0 {
                    saw(p, dt)
                } else {
                    (1.0 - shape) * saw(p, dt) + shape * triangle(p, dt)
                }
            }
            3 => {
                let mut sum = 0.0;
                for (i, sp) in self.supers.iter_mut().enumerate() {
                    let r = self.ratios[i];
                    let q = wrap(*sp + pm);
                    let w = if i == SUPER / 2 { 1.0 } else { 0.7 };
                    sum += w * saw(q, (dt * r).min(0.5));
                    *sp = wrap(*sp + inc * r);
                }
                sum * 0.35
            }
            // White, pink, brown.
            w => self.noise.tick()[(w - 4).min(2)],
        }
    }
}

/// A state-variable filter (trapezoidal integrators) whose damping drops
/// below zero for self-oscillation; damping that grows with the band
/// state's level holds the oscillation at about ±1.
struct Svf {
    ic1: f32,
    ic2: f32,
}

impl Default for Svf {
    /// A small charge in the band state: at full resonance the filter rings
    /// up from the note's start even without input.
    fn default() -> Self {
        Svf { ic1: 0.01, ic2: 0.0 }
    }
}

impl Svf {
    fn lowpass(&mut self, x: f32, g: f32, k: f32) -> f32 {
        let k = k + 2.0 * NEG_DAMP * self.ic1 * self.ic1;
        let a1 = 1.0 / (1.0 + g * (g + k));
        let a2 = g * a1;
        let a3 = g * a2;
        let v3 = x - self.ic2;
        let v1 = a1 * self.ic1 + a2 * v3;
        let v2 = self.ic2 + a2 * self.ic1 + a3 * v3;
        self.ic1 = 2.0 * v1 - self.ic1;
        self.ic2 = 2.0 * v2 - self.ic2;
        v2
    }
}

/// The damping at full resonance: just below zero, so the filter starts
/// oscillating by itself.
const NEG_DAMP: f32 = 0.1;
/// Where resonance self-oscillates.
pub const SELF_OSC: f32 = 0.8;

/// SVF damping for a resonance of 0..1.
fn damping(r: f32) -> f32 {
    let r = (r / SELF_OSC).clamp(0.0, 1.0);
    (2.0 + NEG_DAMP) * (1.0 - r) - NEG_DAMP
}

/// Gain of the soft clip on the resonance above [`SELF_OSC`].
fn reso_clip(r: f32) -> f32 {
    drive_gain(((r - SELF_OSC) / (1.0 - SELF_OSC)).clamp(0.0, 1.0))
}

/// An attack-decay envelope: a linear rise over `a` seconds, then an
/// exponential fall to -60 dB over `d`.
struct Ad {
    a: f32,
    d: f32,
}

impl Ad {
    fn at(&self, t: f32) -> f32 {
        if t < self.a { t / self.a } else { (-6.907_755 * (t - self.a) / self.d).exp() }
    }
}

struct Lfo {
    phase: f32,
    held: f32,
    seed: u32,
}

impl Lfo {
    fn tick(&mut self, shape: usize, inc: f32) -> f32 {
        let p = self.phase;
        let y = match shape {
            0 => (TAU * p).sin(),
            1 => 1.0 - 4.0 * (p - 0.5).abs(),
            2 => 1.0 - 2.0 * p,
            3 => {
                if p < 0.5 {
                    1.0
                } else {
                    -1.0
                }
            }
            _ => self.held,
        };
        self.phase += inc;
        if self.phase >= 1.0 {
            self.phase -= 1.0;
            self.seed = self.seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
            self.held = (self.seed >> 8) as f32 / (1u32 << 23) as f32 - 1.0;
        }
        y
    }
}

fn note_hz(semis: f32) -> f32 {
    440.0 * 2f32.powf((BASE_NOTE + semis - 69.0) / 12.0)
}

/// Renders `patch` as a mono sample at [`RATE`].
pub fn render(patch: &Patch) -> Vec<f32> {
    render_at(patch, OS)
}

/// [`render`] at `os` times [`RATE`] (even).
fn render_at(patch: &Patch, os: usize) -> Vec<f32> {
    use P::*;
    let fs = (RATE as usize * os) as f32;
    let hold = (RATE as usize * os / NOISE_RATE).max(1);
    let n = (patch.length() * fs).ceil() as usize;
    let g = |p| patch.get(p);
    let (w1, w2) = (patch.choice(Osc1Wave), patch.choice(Osc2Wave));
    let s1 = semitones(g(Osc1Coarse), g(Osc1Fine));
    let s2 = semitones(g(Osc2Coarse), g(Osc2Fine));
    let f3 = note_hz(s1) * ratio(g(Osc3Ratio));
    let (l1, l2) = (level_gain(g(Osc1Level)), level_gain(g(Osc2Level)));
    let xmod = g(XMod) * g(XMod) * 2.0;
    let pm3 = [g(Osc3ToOsc1), g(Osc3ToOsc2)].map(|a| a * a * 2.0);
    let fm3 = g(Osc3ToFilter) * g(Osc3ToFilter) * 4.0;
    let env_oct = (g(FilterEnv) - 0.5) * 12.0;
    let penv = [(Osc1PitchAttack, Osc1PitchDecay), (Osc2PitchAttack, Osc2PitchDecay), (Osc3EnvAttack, Osc3EnvDecay)].map(|(a, d)| Ad { a: attack_time(g(a)), d: decay_time(g(d)) });
    let (pa1, pa2) = (pitch_amount(g(Osc1PitchAmount)), pitch_amount(g(Osc2PitchAmount)));
    let env3 = g(Osc3EnvAmount);
    let (n_colour, n_level, n_post) = (g(NoiseColour), level_gain(g(NoiseLevel)), patch.on(NoisePost));
    let pre = drive_gain(g(Drive));
    let post = drive_gain(g(Clip));
    let amp = Ad { a: attack_time(g(AmpAttack)), d: decay_time(g(AmpDecay)) };
    let fenv = Ad { a: attack_time(g(FilterAttack)), d: decay_time(g(FilterDecay)) };
    let lfo_shape = patch.choice(LfoShape);
    let lfo_inc = lfo_hz(g(LfoRate)) / fs;
    let depth = lfo_depth(g(LfoDepth));
    // Each target's depth and whether it swings both ways.
    let to = |p, bi| (if patch.on(p) { depth } else { 0.0 }, patch.on(bi));
    let targets = [to(LfoOsc1, LfoOsc1Bi), to(LfoOsc2, LfoOsc2Bi), to(LfoCutoff, LfoCutoffBi), to(LfoResonance, LfoResonanceBi), to(LfoSpread, LfoSpreadBi)];
    let volume = volume_gain(g(Volume));

    let (mut o1, mut o2) = (Osc::new(1, g(Osc1Shape)), Osc::new(2, g(Osc2Shape)));
    // Osc 3 as an oscillator wave and shape: the triangle is the saw's
    // full triangle blend.
    let (w3, sh3) = [(0, 0.0), (1, 0.0), (2, 1.0), (2, 0.0), (4, 0.0)][patch.choice(Osc3Wave).min(4)];
    let mut noise_src = Noise::new(5);
    let mut o3 = Osc::new(3, 0.0);
    for n in [&mut noise_src, &mut o1.noise, &mut o2.noise, &mut o3.noise] {
        n.hold = hold;
    }
    let mix = g(FilterMix);
    let mut lfo = Lfo { phase: 0.0, held: 0.0, seed: 3 };
    let (mut f1, mut f2) = (Svf::default(), Svf::default());
    let (mut dc_x, mut dc_y) = (0.0f32, 0.0f32);
    let dc_r = 1.0 - TAU * 10.0 / fs;
    let mut noise = 0x1234_5678u32;
    let fade_from = n.saturating_sub((FADE * fs) as usize);
    let mut out = Vec::with_capacity(n);
    let mut last2 = 0.0f32;
    for i in 0..n {
        let t = i as f32 / fs;
        let l = lfo.tick(lfo_shape, lfo_inc);
        // Each target's LFO: -1..1, or 0..1 when unipolar.
        let [lo1, lo2, lcut, lres, lspread] = targets.map(|(d, bi)| d * if bi { l } else { 0.5 * (l + 1.0) });
        let depth3 = 1.0 - env3 + env3 * penv[2].at(t);
        let m3 = o3.tick(w3, f3 / fs, 0.0, sh3) * depth3;
        let inc1 = note_hz(s1 + lo1 * 12.0 + pa1 * penv[0].at(t)) / fs;
        let inc2 = note_hz(s2 + lo2 * 12.0 + pa2 * penv[1].at(t)) / fs;
        let y2 = o2.tick(w2, inc2, pm3[1] * m3, g(Osc2Shape));
        let y1 = o1.tick(w1, inc1, pm3[0] * m3 + xmod * last2, g(Osc1Shape));
        last2 = y2;
        let nz = if n_level > 0.0 { channel(colour(n_colour, noise_src.tick()) * n_level) } else { 0.0 };
        let mixed = channel(y1 * l1) + channel(y2 * l2) + if n_post { 0.0 } else { nz };
        // The channels' asymmetry leaves an offset: a 10 Hz high-pass.
        dc_y = mixed - dc_x + dc_r * dc_y;
        dc_x = mixed;
        noise = noise.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
        let x = soft(dc_y, pre) + (noise >> 9) as f32 * 1e-12;
        let oct = env_oct * fenv.at(t) + lcut * 4.0 + fm3 * m3;
        let fc = cutoff_hz(g(Cutoff)) * 2f32.powf(oct);
        let half = 0.5 * (spread_octaves(g(Spread)) + lspread * 2.0).max(0.0);
        let r = (g(Resonance) + lres * 0.4).clamp(0.0, 1.0);
        let k = damping(r);
        let gain = |f: f32| (PI * f.clamp(10.0, 0.45 * fs) / fs).tan();
        let a = f1.lowpass(x, gain(fc * 2f32.powf(-half)), k);
        let b = f2.lowpass(x, gain(fc * 2f32.powf(half)), k);
        let rc = reso_clip(r);
        let y = 0.5 * (soft(a, rc) + soft(b, rc));
        let y = x + mix * (y - x) + if n_post { nz } else { 0.0 };
        let mut y = soft(y, post) * amp.at(t) * volume;
        if i >= fade_from {
            y *= (n - i) as f32 / (n - fade_from) as f32;
        }
        out.push(y);
    }
    decimate(&out, os)
}

/// Takes the rate down `os` times with a windowed-sinc low-pass at 0.9 of
/// the new Nyquist.
fn decimate(x: &[f32], os: usize) -> Vec<f32> {
    let taps = 32 * os - 1;
    let mid = (taps / 2) as f32;
    let fc = 0.9 / os as f32;
    let h: Vec<f32> = (0..taps)
        .map(|i| {
            let t = i as f32 - mid;
            let sinc = if t == 0.0 { 1.0 } else { (PI * t * fc).sin() / (PI * t * fc) };
            let w = 0.42 - 0.5 * (TAU * i as f32 / (taps - 1) as f32).cos() + 0.08 * (2.0 * TAU * i as f32 / (taps - 1) as f32).cos();
            fc * sinc * w
        })
        .collect();
    let norm: f32 = h.iter().sum();
    (0..x.len() / os)
        .map(|o| {
            let c = (o * os) as isize;
            let mut s = 0.0;
            for (j, hj) in h.iter().enumerate() {
                let k = c + j as isize - mid as isize;
                if k >= 0 && (k as usize) < x.len() {
                    s += hj * x[k as usize];
                }
            }
            s / norm
        })
        .collect()
}

/// `samples` as a 24-bit mono WAV file at [`RATE`].
pub fn wav(samples: &[f32]) -> Vec<u8> {
    let data = samples.len() * 3;
    let mut b = Vec::with_capacity(44 + data);
    b.extend_from_slice(b"RIFF");
    b.extend_from_slice(&(36 + data as u32).to_le_bytes());
    b.extend_from_slice(b"WAVEfmt ");
    b.extend_from_slice(&16u32.to_le_bytes());
    b.extend_from_slice(&1u16.to_le_bytes());
    b.extend_from_slice(&1u16.to_le_bytes());
    b.extend_from_slice(&RATE.to_le_bytes());
    b.extend_from_slice(&(RATE * 3).to_le_bytes());
    b.extend_from_slice(&3u16.to_le_bytes());
    b.extend_from_slice(&24u16.to_le_bytes());
    b.extend_from_slice(b"data");
    b.extend_from_slice(&(data as u32).to_le_bytes());
    for s in samples {
        let v = (s.clamp(-1.0, 1.0) * 8_388_607.0).round() as i32;
        b.extend_from_slice(&v.to_le_bytes()[..3]);
    }
    b
}

#[cfg(test)]
mod tests {
    use super::*;

    fn peak(x: &[f32]) -> f32 {
        x.iter().fold(0.0, |m, v| m.max(v.abs()))
    }

    /// The dominant frequency of `x` by zero crossings.
    fn crossings_hz(x: &[f32]) -> f32 {
        let n = x.windows(2).filter(|w| w[0] < 0.0 && w[1] >= 0.0).count();
        n as f32 * RATE as f32 / x.len() as f32
    }

    /// RMS of `a - b` over RMS of `b`.
    fn rel_error(a: &[f32], b: &[f32]) -> f32 {
        let d: f32 = a.iter().zip(b).map(|(x, y)| (x - y) * (x - y)).sum();
        let r: f32 = b.iter().map(|y| y * y).sum();
        (d / r).sqrt()
    }

    /// The share of `x`'s power off the harmonics of a tone `period` bins
    /// apart (`x` holds a whole number of its cycles): its aliasing.
    fn alias_share(x: &[f32], period: usize) -> f32 {
        let n = x.len();
        let table: Vec<(f32, f32)> = (0..n).map(|k| (TAU * k as f32 / n as f32).sin_cos()).collect();
        let (mut off, mut all) = (0.0f32, 0.0f32);
        for b in 1..n / 2 {
            let (mut re, mut im) = (0.0f32, 0.0f32);
            for (i, v) in x.iter().enumerate() {
                let (s, c) = table[(b * i) % n];
                re += v * c;
                im -= v * s;
            }
            let pw = re * re + im * im;
            all += pw;
            if b % period != 0 {
                off += pw;
            }
        }
        off / all
    }

    /// The rounded corners cut a high triangle's aliasing.
    #[test]
    fn triangle_corners_alias_less() {
        // 2990 Hz: 299 whole cycles in 4800 samples, harmonics every 299
        // bins, the aliases between them.
        let dt = 2990.0f32 / 48_000.0;
        let ps: Vec<f32> = (0..4800).map(|i| wrap(i as f32 * dt)).collect();
        let naive: Vec<f32> = ps.iter().map(|&p| 1.0 - 4.0 * (p - 0.5).abs()).collect();
        let smooth: Vec<f32> = ps.iter().map(|&p| triangle(p, dt)).collect();
        let (a, b) = (alias_share(&naive, 299), alias_share(&smooth, 299));
        assert!(b < 0.1 * a, "rounded {b} vs naive {a}");
    }

    /// A folded sine swept high aliases less at four times the rate than at
    /// two (measured against sixteen times).
    #[test]
    fn oversampling_cuts_aliasing() {
        let mut p = Patch::default();
        for (k, v) in [(P::Osc1Wave, 0.0), (P::Osc1Shape, 1.0), (P::Osc1Coarse, 1.0), (P::Osc1PitchAmount, 1.0), (P::Osc1PitchDecay, 1.0), (P::FilterMix, 0.0), (P::AmpDecay, 1.0)] {
            p.set(k, v);
        }
        let window = |x: Vec<f32>| x[2400..14_400].to_vec();
        let reference = window(render_at(&p, 16));
        let (e2, e4) = (rel_error(&window(render_at(&p, 2)), &reference), rel_error(&window(render_at(&p, 4)), &reference));
        assert!(e4 < 0.5 * e2, "4x {e4} vs 2x {e2}");
    }

    /// Noise keeps its level and colour whatever the oversampling.
    #[test]
    fn noise_does_not_follow_the_oversampling() {
        let mut p = Patch::default();
        for (k, v) in [(P::Osc1Level, 0.0), (P::NoiseLevel, 0.5), (P::NoiseColour, 0.5), (P::FilterMix, 0.0), (P::AmpDecay, 1.0)] {
            p.set(k, v);
        }
        let rms = |x: &[f32]| (x[2400..24_000].iter().map(|v| v * v).sum::<f32>() / 21_600.0).sqrt();
        let (a, b) = (rms(&render_at(&p, 2)), rms(&render_at(&p, 4)));
        assert!((20.0 * (b / a).log10()).abs() < 0.5, "2x {a} vs 4x {b}");
    }

    #[test]
    fn shape_names_follow_the_wave() {
        let names: Vec<_> = (0..OSC2_WAVES.len()).map(shape_label).collect();
        assert_eq!(names, ["Fold", "Width", "Tri", "Detune", "Shape", "Shape", "Shape"]);
        assert_eq!(shape_display(0, 1.0), "x5.0");
        assert_eq!((shape_display(1, 0.0), shape_display(1, 1.0)), ("50 %".into(), "5 %".into()));
        assert_eq!(shape_display(3, 0.0), "4 ct");
        assert_eq!(shape_display(5, 0.3), "-");
    }

    #[test]
    fn params_line_up() {
        assert_eq!(PARAMS.len(), COUNT);
        for (i, p) in P::ALL.iter().enumerate() {
            assert_eq!(*p as usize, i);
        }
        let mut ids: Vec<_> = PARAMS.iter().map(|p| p.id).collect();
        ids.sort();
        ids.dedup();
        assert_eq!(ids.len(), COUNT, "ids are unique");
    }

    #[test]
    fn the_default_patch_sounds_and_ends_in_silence() {
        let p = Patch::default();
        let x = render(&p);
        assert!((x.len() as f32 / RATE as f32 - p.length()).abs() < 1e-3);
        assert!(peak(&x) > 0.1, "peak {}", peak(&x));
        assert!(peak(&x) <= 1.5);
        assert!(x.last().unwrap().abs() < 1e-3);
        assert!(x.iter().all(|v| v.is_finite()));
    }

    #[test]
    fn renders_repeat_exactly() {
        let mut p = Patch::default();
        p.set(P::Osc1Wave, 3.0);
        p.set(P::LfoShape, 4.0);
        p.set(P::LfoDepth, 0.5);
        p.set(P::LfoCutoff, 1.0);
        assert_eq!(render(&p), render(&p));
        assert_eq!(p.hash(), p.clone().hash());
        let mut q = p.clone();
        q.set(P::Cutoff, 0.3);
        assert_ne!(p.hash(), q.hash());
    }

    #[test]
    fn envelopes_span_their_ranges() {
        assert!(attack_time(0.0) < 0.001);
        assert!((attack_time(1.0) - 0.7).abs() < 1e-3);
        assert!(decay_time(0.0) <= 0.002);
        assert!((decay_time(1.0) - 1.5).abs() < 1e-3);
        let mut p = Patch::default();
        p.set(P::AmpAttack, 1.0);
        p.set(P::AmpDecay, 1.0);
        assert!((p.length() - 2.205).abs() < 1e-3);
    }

    /// With the oscillators silent, resonance at 80 % rings as a sine at the
    /// cutoff by itself; below it the filter stays silent.
    #[test]
    fn resonance_self_oscillates_at_eighty_percent() {
        let mut p = Patch::default();
        p.set(P::Osc1Level, 0.0);
        p.set(P::Cutoff, (1000f32 / 20.0).log10() / 3.0);
        p.set(P::AmpAttack, 0.0);
        p.set(P::AmpDecay, 1.0);
        // 20 ms at 0.5 s, where the amp envelope barely moves.
        let at = |x: &[f32]| x[RATE as usize / 2..RATE as usize / 2 + RATE as usize / 50].to_vec();
        p.set(P::Resonance, 0.7);
        let quiet = at(&render(&p));
        assert!(peak(&quiet) < 1e-4, "no oscillation at 70 %: {}", peak(&quiet));
        p.set(P::Resonance, SELF_OSC);
        let x = render(&p);
        let window = at(&x);
        let f = crossings_hz(&window);
        assert!((f - 1000.0).abs() < 60.0, "rings at {f} Hz");
        let pk = peak(&window);
        assert!(pk > 0.05, "peak {pk}");
        // A sine: its crest factor is √2.
        let rms = (window.iter().map(|v| v * v).sum::<f32>() / window.len() as f32).sqrt();
        assert!((pk / rms - 2f32.sqrt()).abs() < 0.1, "crest {}", pk / rms);
        // Past 80 %, the soft clip squares it up.
        p.set(P::Resonance, 1.0);
        let w = at(&render(&p));
        let rms2 = (w.iter().map(|v| v * v).sum::<f32>() / w.len() as f32).sqrt();
        assert!(peak(&w) / rms2 < pk / rms - 0.1, "crest {}", peak(&w) / rms2);
    }

    #[test]
    fn spread_splits_the_peaks() {
        let mut p = Patch::default();
        p.set(P::Osc1Level, 0.0);
        p.set(P::Cutoff, (1000f32 / 20.0).log10() / 3.0);
        p.set(P::Resonance, SELF_OSC);
        p.set(P::AmpDecay, 1.0);
        p.set(P::Spread, 0.25);
        let x = render(&p);
        let w = &x[RATE as usize / 4..RATE as usize / 2];
        // Peaks at 707 Hz and 1414 Hz: the crossings count no longer reads 1 kHz.
        assert!((crossings_hz(w) - 1000.0).abs() > 60.0);
    }

    /// Osc 3's waves give Osc 1 different FM spectra; at zero amount they
    /// change nothing.
    #[test]
    fn osc3_waves_modulate_differently() {
        let mut p = Patch::default();
        p.set(P::Osc1Wave, 0.0);
        p.set(P::Cutoff, 1.0);
        let plain = render(&p);
        p.set(P::Osc3Wave, 2.0);
        assert_eq!(render(&p), plain, "no amount, no change");
        p.set(P::Osc3ToOsc1, 0.6);
        let renders: Vec<Vec<f32>> = (0..4)
            .map(|w| {
                p.set(P::Osc3Wave, w as f32);
                render(&p)
            })
            .collect();
        for i in 0..4 {
            assert!(renders[i].iter().all(|v| v.is_finite()));
            assert_ne!(renders[i], plain);
            for j in i + 1..4 {
                assert_ne!(renders[i], renders[j], "waves {i} and {j}");
            }
        }
    }

    /// Mix at 0 bypasses the filter: a closed filter lets the saw through.
    #[test]
    fn mix_blends_the_dry_signal_back() {
        let energy = |x: &[f32]| x.iter().map(|v| v * v).sum::<f32>();
        let mut p = Patch::default();
        p.set(P::Cutoff, 0.0);
        p.set(P::Resonance, 0.0);
        let closed = energy(&render(&p));
        p.set(P::FilterMix, 0.0);
        let dry = energy(&render(&p));
        p.set(P::FilterMix, 0.5);
        let half = energy(&render(&p));
        assert!(dry > closed * 20.0, "dry {dry} closed {closed}");
        assert!(half > closed && half < dry);
    }

    fn energy(x: &[f32]) -> f32 {
        x.iter().map(|v| v * v).sum::<f32>() / x.len().max(1) as f32
    }

    /// How much of `x` is high frequencies: the energy of its first
    /// difference over its own.
    fn brightness(x: &[f32]) -> f32 {
        let d: Vec<f32> = x.windows(2).map(|w| w[1] - w[0]).collect();
        energy(&d) / energy(x)
    }

    /// A pitch envelope sweeps the oscillator down to its pitch, as a kick.
    #[test]
    fn pitch_envelopes_sweep() {
        let mut p = Patch::default();
        p.set(P::Osc1Wave, 0.0);
        p.set(P::Cutoff, 1.0);
        p.set(P::AmpDecay, 1.0);
        p.set(P::Osc1PitchAmount, 1.0);
        p.set(P::Osc1PitchDecay, 0.6);
        let x = render(&p);
        let early = crossings_hz(&x[..RATE as usize / 50]);
        let late = crossings_hz(&x[RATE as usize..RATE as usize + RATE as usize / 5]);
        assert!((late - 130.8).abs() < 10.0, "settles at the note: {late}");
        assert!(early > late * 4.0, "starts high: {early}");
        // Osc 2's envelope moves Osc 2 only.
        p.set(P::Osc1PitchAmount, 0.5);
        let plain = render(&p);
        p.set(P::Osc2PitchAmount, 1.0);
        assert_eq!(render(&p), plain, "Osc 2 is silent");
    }

    /// Osc 3's depth envelope fades its FM: late in the note the sound
    /// matches the unmodulated one.
    #[test]
    fn osc3_depth_follows_its_envelope() {
        let mut p = Patch::default();
        p.set(P::Osc1Wave, 0.0);
        p.set(P::Cutoff, 1.0);
        p.set(P::AmpDecay, 1.0);
        let plain = render(&p);
        p.set(P::Osc3ToOsc1, 0.8);
        p.set(P::Osc3Ratio, 0.5);
        let always = render(&p);
        p.set(P::Osc3EnvAmount, 1.0);
        p.set(P::Osc3EnvDecay, 0.3);
        let shaped = render(&p);
        let late = RATE as usize..RATE as usize + 4800;
        let diff = |a: &[f32], b: &[f32]| energy(&a.iter().zip(b).map(|(x, y)| x - y).collect::<Vec<_>>());
        let base = energy(&plain[late.clone()]);
        assert!(diff(&always[late.clone()], &plain[late.clone()]) > base * 0.1);
        assert!(diff(&shaped[late.clone()], &plain[late.clone()]) < base * 1e-4, "the FM has gone");
        assert_ne!(shaped[..2400], plain[..2400], "early on it modulates");
    }

    /// White is brighter than pink, pink than brown, from the noise source
    /// and from Osc 2's noise waves.
    #[test]
    fn noise_colours_darken() {
        let mut p = Patch::default();
        p.set(P::Osc1Level, 0.0);
        p.set(P::Cutoff, 1.0);
        p.set(P::Resonance, 0.0);
        p.set(P::AmpDecay, 1.0);
        p.set(P::NoiseLevel, 0.5);
        let b: Vec<f32> = [0.0, 0.5, 1.0]
            .map(|c| {
                p.set(P::NoiseColour, c);
                let x = render(&p);
                assert!(energy(&x) > 1e-4, "colour {c} sounds");
                brightness(&x[..RATE as usize / 2])
            })
            .to_vec();
        assert!(b[0] > b[1] * 1.5 && b[1] > b[2] * 1.5, "{b:?}");
        p.set(P::NoiseLevel, 0.0);
        p.set(P::Osc2Level, 0.5);
        let w: Vec<f32> = [4.0, 5.0, 6.0]
            .map(|w| {
                p.set(P::Osc2Wave, w);
                brightness(&render(&p)[..RATE as usize / 2])
            })
            .to_vec();
        assert!(w[0] > w[1] && w[1] > w[2], "{w:?}");
    }

    /// Post-filter noise skips the filter: a closed filter only hides the
    /// pre-filter noise.
    #[test]
    fn noise_joins_before_or_after_the_filter() {
        let mut p = Patch::default();
        p.set(P::Osc1Level, 0.0);
        p.set(P::Cutoff, 0.0);
        p.set(P::Resonance, 0.0);
        p.set(P::NoiseLevel, 0.5);
        let pre = energy(&render(&p));
        p.set(P::NoisePost, 1.0);
        let post = energy(&render(&p));
        assert!(post > pre * 50.0, "pre {pre} post {post}");
    }

    #[test]
    fn lfo_depth_eases_in() {
        assert!((lfo_depth(0.25) - 0.1).abs() < 0.01);
        assert!((lfo_depth(1.0) - 1.0).abs() < 1e-6);
        assert_eq!(display(P::LfoDepth, 0.25), "10 %");
    }

    /// A unipolar target only moves one way: the cutoff only opens.
    #[test]
    fn unipolar_targets_move_one_way() {
        let mut p = Patch::default();
        p.set(P::Cutoff, 0.3);
        p.set(P::Resonance, 0.0);
        p.set(P::AmpDecay, 1.0);
        p.set(P::LfoShape, 3.0);
        // A 1 Hz square: low from 0.5 s to 1 s.
        p.set(P::LfoRate, 20f32.ln() / 600f32.ln());
        p.set(P::LfoDepth, 1.0);
        p.set(P::LfoCutoff, 1.0);
        let bi = render(&p);
        p.set(P::LfoCutoffBi, 0.0);
        let uni = render(&p);
        p.set(P::LfoCutoff, 0.0);
        let none = render(&p);
        // The square's low half: bipolar closes the filter, unipolar leaves it.
        let low = RATE as usize * 3 / 4..RATE as usize;
        let e = |x: &[f32]| energy(&x[low.clone()]);
        assert!(e(&bi) < e(&none) * 0.8, "bipolar closes: {} {}", e(&bi), e(&none));
        assert!((e(&uni) / e(&none) - 1.0).abs() < 0.05, "unipolar stays: {} {}", e(&uni), e(&none));
    }

    #[test]
    fn hot_channels_saturate_asymmetrically() {
        assert!((channel(0.01) - 0.01).abs() < 1e-3);
        let (hi, lo) = (channel(8.0), -channel(-8.0));
        assert!(hi < 8.0 * 0.5 && lo < 8.0 * 0.6);
        assert!((hi - lo).abs() > 0.2, "{hi} {lo}");
    }

    #[test]
    fn values_round_trip() {
        let mut p = Patch::default();
        p.set(P::Osc2Wave, 3.0);
        p.set(P::Spread, 0.3);
        p.set(P::LfoSpread, 1.0);
        let q = Patch::from_values(p.values().collect::<Vec<_>>());
        assert_eq!(p, q);
        assert_eq!(Patch::from_values([("nope", 1.0), ("osc1_wave", 9.0)]).get(P::Osc1Wave), 3.0);
    }

    /// The longest, busiest patch renders quickly enough to follow a knob.
    #[test]
    fn renders_are_quick() {
        let mut p = Patch::default();
        for (k, v) in [(P::Osc1Wave, 3.0), (P::Osc2Wave, 3.0), (P::Osc2Level, 0.7), (P::AmpAttack, 1.0), (P::AmpDecay, 1.0), (P::LfoDepth, 1.0), (P::LfoCutoff, 1.0), (P::Spread, 0.5)] {
            p.set(k, v);
        }
        let t = std::time::Instant::now();
        let x = render(&p);
        let ms = t.elapsed().as_secs_f32() * 1000.0;
        eprintln!("{} samples in {ms:.1} ms", x.len());
        assert!(x.iter().all(|v| v.is_finite()));
    }

    #[test]
    fn wav_has_the_samples() {
        let b = wav(&[0.0, 1.0, -1.0]);
        assert_eq!(b.len(), 44 + 9);
        assert_eq!(&b[44..53], &[0, 0, 0, 0xff, 0xff, 0x7f, 0x01, 0x00, 0x80]);
    }
}
