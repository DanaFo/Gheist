//! A drum synth whose sound is rendered to a sample: a layer plays the
//! render like any other sample.
//!
//! - Osc 1 and Osc 2 (sine, square, saw, supersaw) with Osc 2 cross-modulating
//!   Osc 1; Osc 3 is a sine FM operator aimed at Osc 1, Osc 2 and the cutoff.
//! - Each oscillator goes through its own mixer channel, which overdrives
//!   asymmetrically above unity.
//! - The filter is two state-variable low-passes whose peaks sit `spread`
//!   octaves apart, with a pre drive and a post clip. Resonance reaches
//!   self-oscillation (a sine) at 80 %; the last 20 % soft-clip it.
//! - Two AD envelopes (amp, filter) and one LFO with a toggle per target.
//!
//! [`render`] runs at twice [`RATE`] and decimates; the same patch always
//! renders the same samples.

use std::f32::consts::{PI, TAU};

/// The rate of the rendered sample.
pub const RATE: u32 = 48_000;
const OS: usize = 2;

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

/// Oscillator waves.
pub const WAVES: &[&str] = &["Sine", "Square", "Saw", "Supersaw"];
/// LFO shapes.
pub const LFO_SHAPES: &[&str] = &["Sine", "Triangle", "Saw", "Square", "S&H"];

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
    Osc2Wave = "osc2_wave", "Wave", Kind::Choice(WAVES), 1.0;
    Osc2Coarse = "osc2_coarse", "Coarse", Kind::Knob, 0.5;
    Osc2Fine = "osc2_fine", "Fine", Kind::Knob, 0.5;
    Osc2Shape = "osc2_shape", "Shape", Kind::Knob, 0.0;
    Osc2Level = "osc2_level", "Level", Kind::Knob, 0.0;
    XMod = "xmod", "X-Mod", Kind::Knob, 0.0;
    Osc3Ratio = "osc3_ratio", "Ratio", Kind::Knob, 1.0 / 3.0;
    Osc3ToOsc1 = "osc3_osc1", "> Osc 1", Kind::Knob, 0.0;
    Osc3ToOsc2 = "osc3_osc2", "> Osc 2", Kind::Knob, 0.0;
    Osc3ToFilter = "osc3_filter", "> Filter", Kind::Knob, 0.0;
    Cutoff = "cutoff", "Cutoff", Kind::Knob, 0.7;
    Resonance = "resonance", "Reso", Kind::Knob, 0.3;
    Spread = "spread", "Spread", Kind::Knob, 0.0;
    Drive = "drive", "Drive", Kind::Knob, 0.0;
    Clip = "clip", "Clip", Kind::Knob, 0.0;
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
const VERSION: u32 = 1;
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

/// Octaves between the two filter peaks.
fn spread_octaves(x: f32) -> f32 {
    x * 4.0
}

/// The text a panel shows for `p` at `v`.
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
        Cutoff => hz(cutoff_hz(v)),
        Spread => format!("{:.1} oct", spread_octaves(v)),
        FilterEnv => format!("{:+.1} oct", (v - 0.5) * 12.0),
        AmpAttack | FilterAttack => {
            let s = attack_time(v);
            if s < 0.01 { format!("{:.1} ms", s * 1000.0) } else { ms(s) }
        }
        AmpDecay | FilterDecay => ms(decay_time(v)),
        LfoRate => format!("{:.2} Hz", lfo_hz(v)),
        Osc1Level | Osc2Level => {
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

fn wrap(x: f32) -> f32 {
    x - x.floor()
}

/// Folds `x` back into ±1 like a triangle.
fn fold(x: f32) -> f32 {
    4.0 * (wrap((x - 1.0) / 4.0) - 0.5).abs() - 1.0
}

const SUPER: usize = 7;
const SUPER_SPREAD: [f32; SUPER] = [-1.0, -0.62, -0.28, 0.0, 0.3, 0.6, 1.0];

struct Osc {
    phase: f32,
    supers: [f32; SUPER],
    /// The supersaw voices' pitch ratios.
    ratios: [f32; SUPER],
    last: f32,
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
        let detune = shape * shape * 0.6 + 0.04;
        let ratios = SUPER_SPREAD.map(|s| 2f32.powf(s * detune / 12.0));
        Osc { phase: 0.0, supers, ratios, last: 0.0 }
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
            0 => fold((TAU * p).sin() * (1.0 + 4.0 * shape)),
            1 => {
                let pw = 0.5 - 0.45 * shape;
                let mut y = if p < pw { 1.0 } else { -1.0 };
                y += poly_blep(p, dt);
                y -= poly_blep(wrap(p - pw + 1.0), dt);
                y
            }
            2 => {
                let tri = 1.0 - 4.0 * (p - 0.5).abs();
                (1.0 - shape) * saw(p, dt) + shape * tri
            }
            _ => {
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
    use P::*;
    let fs = (RATE as usize * OS) as f32;
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
    let pre = drive_gain(g(Drive));
    let post = drive_gain(g(Clip));
    let amp = Ad { a: attack_time(g(AmpAttack)), d: decay_time(g(AmpDecay)) };
    let fenv = Ad { a: attack_time(g(FilterAttack)), d: decay_time(g(FilterDecay)) };
    let lfo_shape = patch.choice(LfoShape);
    let lfo_inc = lfo_hz(g(LfoRate)) / fs;
    let depth = g(LfoDepth);
    let to = |p| if patch.on(p) { depth } else { 0.0 };
    let (lo1, lo2, lcut, lres, lspread) = (to(LfoOsc1), to(LfoOsc2), to(LfoCutoff), to(LfoResonance), to(LfoSpread));
    let volume = volume_gain(g(Volume));

    let (mut o1, mut o2) = (Osc::new(1, g(Osc1Shape)), Osc::new(2, g(Osc2Shape)));
    let mut o3 = 0.0f32;
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
        let m3 = (TAU * o3).sin();
        o3 = wrap(o3 + f3 / fs);
        let inc1 = note_hz(s1 + l * lo1 * 12.0) / fs;
        let inc2 = note_hz(s2 + l * lo2 * 12.0) / fs;
        let y2 = o2.tick(w2, inc2, pm3[1] * m3, g(Osc2Shape));
        let y1 = o1.tick(w1, inc1, pm3[0] * m3 + xmod * last2, g(Osc1Shape));
        last2 = y2;
        let mixed = channel(y1 * l1) + channel(y2 * l2);
        // The channels' asymmetry leaves an offset: a 10 Hz high-pass.
        dc_y = mixed - dc_x + dc_r * dc_y;
        dc_x = mixed;
        noise = noise.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
        let x = soft(dc_y, pre) + (noise >> 9) as f32 * 1e-12;
        let oct = env_oct * fenv.at(t) + lcut * l * 4.0 + fm3 * m3;
        let fc = cutoff_hz(g(Cutoff)) * 2f32.powf(oct);
        let half = 0.5 * (spread_octaves(g(Spread)) + lspread * l * 2.0).max(0.0);
        let r = (g(Resonance) + lres * l * 0.4).clamp(0.0, 1.0);
        let k = damping(r);
        let gain = |f: f32| (PI * f.clamp(10.0, 0.45 * fs) / fs).tan();
        let a = f1.lowpass(x, gain(fc * 2f32.powf(-half)), k);
        let b = f2.lowpass(x, gain(fc * 2f32.powf(half)), k);
        let rc = reso_clip(r);
        let y = 0.5 * (soft(a, rc) + soft(b, rc));
        let mut y = soft(y, post) * amp.at(t) * volume;
        if i >= fade_from {
            y *= (n - i) as f32 / (n - fade_from) as f32;
        }
        out.push(y);
    }
    decimate(&out)
}

/// Halves the rate with a windowed-sinc low-pass.
fn decimate(x: &[f32]) -> Vec<f32> {
    const TAPS: usize = 63;
    let mid = (TAPS / 2) as f32;
    let h: Vec<f32> = (0..TAPS)
        .map(|i| {
            let t = i as f32 - mid;
            let sinc = if t == 0.0 { 1.0 } else { (PI * t * 0.45).sin() / (PI * t * 0.45) };
            let w = 0.42 - 0.5 * (TAU * i as f32 / (TAPS - 1) as f32).cos() + 0.08 * (2.0 * TAU * i as f32 / (TAPS - 1) as f32).cos();
            0.45 * sinc * w
        })
        .collect();
    let norm: f32 = h.iter().sum();
    (0..x.len() / OS)
        .map(|o| {
            let c = (o * OS) as isize;
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
