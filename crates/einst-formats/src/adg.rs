//! Drum rack presets of another DAW (`.adg`, gzipped XML) as kits.
//!
//! - A drum rack's pads become pads: a pad's note `n` (`128 − ReceivingNote`)
//!   is pad `n − 36`; pads outside the 16 fill the free ones in note order.
//! - Each sampler on a pad (classic or multi-zone sampler, drum sampler)
//!   gives layers: its zones that play at the pad's sending note, at most
//!   eight, split by velocity when their ranges differ.
//! - An eight-slot drum instrument preset gives pads 1 to 8.
//! - Carried over: sample, start and end, reverse, volume, pan, transpose,
//!   attack, velocity to volume, choke group (global cut groups 1 to 8),
//!   pad volume, pan, mute and name. Pads with synths and effects are not.
//! - Sample paths are relative to the library holding the preset: they
//!   resolve against its folders, the stored absolute path and the
//!   default library locations.

use std::io::Read;
use std::path::{Path, PathBuf};

use dreist_formats::Node;
use dreist_formats::params::{LayerParam, PadParam, db_to_value01};

use crate::Error;

/// The file extension.
pub const EXTENSION: &str = "adg";

/// Pads per engine.
const PADS: usize = 16;
/// Layers per pad.
const LAYERS: usize = 8;
/// The note of pad 1.
const FIRST_NOTE: i32 = 36;

/// Whether `path` names a drum rack preset (by extension).
pub fn is_drum_rack(path: &Path) -> bool {
    path.extension().is_some_and(|e| e.eq_ignore_ascii_case(EXTENSION))
}

/// The kit (`<root>` with `Pad0`..`Pad15`, v2's tree) of the drum rack at
/// `path`.
pub fn convert(path: &Path) -> Result<Node, Error> {
    let bytes = std::fs::read(path)?;
    let text = gunzip(&bytes)?;
    let doc = crate::parse(&text)?;
    let roots = library_roots(path);
    let pads = drum_pads(&doc, &roots);
    if pads.iter().all(Option::is_none) {
        return Err(Error::Shape("drum rack", "no pad with a sample".into()));
    }
    let mut root = Node::new("root", None);
    root.version = Some(3000);
    for (i, pad) in pads.into_iter().enumerate() {
        if let Some(p) = pad {
            root.children.push(p.node(i));
        }
    }
    Ok(root)
}

fn gunzip(bytes: &[u8]) -> Result<String, Error> {
    if !bytes.starts_with(&[0x1f, 0x8b]) {
        return String::from_utf8(bytes.to_vec()).map_err(|_| Error::Shape("drum rack", "not UTF-8".into()));
    }
    let mut text = String::new();
    flate2::read::GzDecoder::new(bytes).read_to_string(&mut text)?;
    Ok(text)
}

/// One sample zone as a layer.
#[derive(Debug, Clone, Default)]
struct Zone {
    file: PathBuf,
    db: f32,
    pan: f32,
    semitones: f32,
    reverse: bool,
    attack_secs: f32,
    /// Start and end frame, and the sample's length, when not the whole sample.
    markers: Option<(u32, u32, u32)>,
    velocity: (u32, u32),
}

/// One pad.
#[derive(Debug, Clone, Default)]
struct Pad {
    name: String,
    gain_db: f32,
    pan: f32,
    mute: bool,
    choke: u32,
    vel_to_vol: Option<f32>,
    zones: Vec<Zone>,
}

impl Pad {
    fn node(&self, index: usize) -> Node {
        let mut pad = Node::new("Pad", Some(&format!("Pad{index}")));
        if self.gain_db.abs() > 0.01 {
            pad.set(PadParam::Gain.name(), db_to_value01(self.gain_db));
        }
        if self.pan.abs() > 1e-4 {
            pad.set(PadParam::Pan.name(), ((self.pan + 1.0) * 0.5).clamp(0.0, 1.0));
        }
        if let Some(v) = self.vel_to_vol {
            pad.set(PadParam::V2A.name(), v.clamp(0.0, 1.0));
        }
        if self.mute {
            pad.set("Mute", "true");
        }
        // A zone's range 1..127 is every velocity (0 is a note off).
        let split = self.zones.iter().any(|z| z.velocity.0 > 1 || z.velocity.1 < 127);
        pad.set("PadMode", if split { "Velocity split" } else { "All layers" });
        let name = if self.name.is_empty() { self.zones.first().map(|z| stem(&z.file)).unwrap_or_default() } else { self.name.clone() };
        pad.children.push({
            let mut s = Node::new("FixedString", Some("PadName"));
            s.set("string", name);
            s
        });
        for (l, z) in self.zones.iter().enumerate() {
            pad.children.push(z.layer(l, self.choke, split));
        }
        pad
    }
}

impl Zone {
    fn layer(&self, index: usize, choke: u32, split: bool) -> Node {
        let mut n = crate::v2::default_layer(index);
        n.set("AudioFileName", self.file.to_string_lossy());
        n.children.push({
            let mut s = Node::new("FixedString", Some("LayerName"));
            s.set("string", stem(&self.file));
            s
        });
        if self.db.abs() > 0.01 {
            n.set(LayerParam::Gain.name(), db_to_value01(self.db));
        }
        if self.pan.abs() > 1e-4 {
            n.set(LayerParam::Pan.name(), ((self.pan + 1.0) * 0.5).clamp(0.0, 1.0));
        }
        let (octave, coarse, fine) = tuning(self.semitones);
        if octave != 0 {
            n.set(LayerParam::Octave.name(), LayerParam::Octave.curve().from_units(octave as f32));
        }
        if coarse != 0 {
            n.set(LayerParam::CoarseTune.name(), LayerParam::CoarseTune.curve().from_units(coarse as f32));
        }
        if fine.abs() >= 0.5 {
            n.set(LayerParam::FineTune.name(), LayerParam::FineTune.curve().from_units(fine));
        }
        if self.reverse {
            n.set("IsReversed", "true");
        }
        if self.attack_secs >= 0.001 {
            n.set(LayerParam::AmpEnvAttack.name(), LayerParam::AmpEnvAttack.curve().from_units(self.attack_secs).clamp(0.0, 1.0));
        }
        if (1..=8).contains(&choke) {
            n.set("CutMode", format!("Global group {choke}"));
        }
        if let Some((start, end, length)) = self.markers {
            let mut m = Node::new("AudioFileMarkers", Some("StartEndMarkers"));
            m.version = Some(2);
            m.set("StartSample", start).set("EndSample", end).set("SampleLength", length);
            n.children.push(m);
        }
        if split {
            let (lo, hi) = self.velocity;
            let mut s = Node::new("LayerVelocitySplit", Some("VelocitySplit"));
            s.set("LowPoint", lo).set("LowTurn", lo).set("HighTurn", hi).set("Highest", hi);
            n.children.push(s);
        }
        n
    }
}

/// Semitones as whole octaves (±4), semitones (±12) and cents (±100).
fn tuning(semitones: f32) -> (i32, i32, f32) {
    let whole = semitones.round();
    let fine = ((semitones - whole) * 100.0).clamp(-100.0, 100.0);
    let whole = whole as i32;
    let octave = if whole.abs() > 12 { (whole / 12).clamp(-4, 4) } else { 0 };
    (octave, (whole - 12 * octave).clamp(-12, 12), fine)
}

fn stem(p: &Path) -> String {
    p.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default()
}

// --- the preset's tree ---

/// The child of `n` along `path` (element classes).
fn at<'a>(n: &'a Node, path: &[&str]) -> Option<&'a Node> {
    path.iter().try_fold(n, |n, class| n.children.iter().find(|c| c.class == *class))
}

/// The value of the element at `path`: its `Value`, or its `Manual` child's
/// (automatable parameters).
fn value<'a>(n: &'a Node, path: &[&str]) -> Option<&'a str> {
    let e = at(n, path)?;
    e.attr("Value").or_else(|| at(e, &["Manual"]).and_then(|m| m.attr("Value")))
}

fn num(n: &Node, path: &[&str]) -> Option<f32> {
    value(n, path)?.trim().parse().ok().filter(|v: &f32| v.is_finite())
}

fn flag(n: &Node, path: &[&str]) -> Option<bool> {
    value(n, path).map(|v| v == "true")
}

/// The first element (depth first) with a child of `class`.
fn parent_of<'a>(n: &'a Node, class: &str) -> Option<&'a Node> {
    if n.children.iter().any(|c| c.class == class) {
        return Some(n);
    }
    n.children.iter().find_map(|c| parent_of(c, class))
}

/// The samplers under `n`, in order, not looking into nested drum racks.
fn samplers<'a>(n: &'a Node, out: &mut Vec<&'a Node>) {
    for c in &n.children {
        match c.class.as_str() {
            "OriginalSimpler" | "MultiSampler" | "DrumCell" => out.push(c),
            "DrumGroupDevice" | "DrumBranchPreset" => {}
            _ => samplers(c, out),
        }
    }
}

fn drum_pads(doc: &Node, roots: &[PathBuf]) -> Vec<Option<Pad>> {
    let mut pads: Vec<Option<Pad>> = vec![None; PADS];
    if let Some(rack) = parent_of(doc, "DrumBranchPreset") {
        let mut extra = Vec::new();
        for b in rack.children_of("DrumBranchPreset") {
            let Some(received) = num(b, &["ZoneSettings", "ReceivingNote"]) else { continue };
            let note = 128 - received as i32;
            if !(0..128).contains(&note) {
                continue;
            }
            let Some(pad) = branch(b, roots) else { continue };
            match usize::try_from(note - FIRST_NOTE).ok().filter(|&i| i < PADS) {
                Some(i) if pads[i].is_none() => pads[i] = Some(pad),
                _ => extra.push((note, pad)),
            }
        }
        extra.sort_by_key(|(note, _)| *note);
        for (_, pad) in extra {
            if let Some(slot) = pads.iter_mut().find(|p| p.is_none()) {
                *slot = Some(pad);
            }
        }
    } else if let Some(imp) = find(doc, "InstrumentImpulse") {
        let global = num(imp, &["GlobalVolume"]).unwrap_or(0.0);
        for (i, slot) in pads.iter_mut().take(8).enumerate() {
            let k = |s: &str| format!("{s}.{i}");
            let Some(file) = at(imp, &[&k("TabSample"), "Value", "SampleRef"]).and_then(|r| sample_file(r, roots)) else { continue };
            let zone = Zone {
                db: num(imp, &[&k("VolumeVolume")]).unwrap_or(0.0) + global,
                pan: num(imp, &[&k("VolumePan")]).unwrap_or(0.0),
                semitones: num(imp, &[&k("StartTune")]).unwrap_or(0.0),
                velocity: (0, 127),
                file,
                ..Zone::default()
            };
            *slot = Some(Pad { mute: flag(imp, &[&k("VolumeMute")]).unwrap_or(false), zones: vec![zone], ..Pad::default() });
        }
    }
    pads
}

fn find<'a>(n: &'a Node, class: &str) -> Option<&'a Node> {
    if n.class == class {
        return Some(n);
    }
    n.children.iter().find_map(|c| find(c, class))
}

/// A drum rack pad with at least one sample.
fn branch(b: &Node, roots: &[PathBuf]) -> Option<Pad> {
    let key = num(b, &["ZoneSettings", "SendingNote"]).unwrap_or(60.0) as i32;
    let mut devices = Vec::new();
    samplers(b, &mut devices);
    let mut pad = Pad { name: value(b, &["Name"]).unwrap_or_default().trim().to_owned(), choke: num(b, &["ZoneSettings", "ChokeGroup"]).unwrap_or(0.0).max(0.0) as u32, ..Pad::default() };
    for d in devices {
        let (zones, vel) = match d.class.as_str() {
            "DrumCell" => (drum_sampler(d, roots), num(d, &["Voice_VelocityToVolume"])),
            _ => (sampler(d, key, roots), num(d, &["VolumeAndPan", "VolumeVelScale"])),
        };
        if pad.zones.is_empty() && !zones.is_empty() {
            pad.vel_to_vol = vel;
        }
        pad.zones.extend(zones);
    }
    pad.zones.truncate(LAYERS);
    if pad.zones.is_empty() {
        return None;
    }
    if let Some(m) = find(b, "AudioBranchMixerDevice") {
        let amp = num(m, &["Volume"]).unwrap_or(1.0);
        pad.gain_db = if amp > 0.0 { 20.0 * amp.log10() } else { -1000.0 };
        pad.pan = num(m, &["Panorama"]).unwrap_or(0.0);
        pad.mute = flag(m, &["Speaker"]) == Some(false);
    }
    Some(pad)
}

/// The zones of a classic or multi-zone sampler that play at `key`.
fn sampler(d: &Node, key: i32, roots: &[PathBuf]) -> Vec<Zone> {
    let db = num(d, &["VolumeAndPan", "Volume"]).unwrap_or(0.0);
    let pan = num(d, &["VolumeAndPan", "Panorama"]).unwrap_or(0.0);
    let semitones = num(d, &["Pitch", "TransposeKey"]).unwrap_or(0.0) + num(d, &["Pitch", "TransposeFine"]).unwrap_or(0.0) / 100.0;
    let reverse = flag(d, &["Player", "Reverse"]).unwrap_or(false);
    let attack_secs = num(d, &["VolumeAndPan", "Envelope", "AttackTime"]).unwrap_or(0.0) / 1000.0;
    let Some(parts) = at(d, &["Player", "MultiSampleMap", "SampleParts"]) else { return Vec::new() };
    parts
        .children_of("MultiSamplePart")
        .filter(|p| {
            let (lo, hi) = (num(p, &["KeyRange", "Min"]).unwrap_or(0.0) as i32, num(p, &["KeyRange", "Max"]).unwrap_or(127.0) as i32);
            lo <= key && key <= hi
        })
        .filter_map(|p| {
            let r = at(p, &["SampleRef"])?;
            let file = sample_file(r, roots)?;
            let amp = num(p, &["Volume"]).unwrap_or(1.0);
            let length = num(r, &["DefaultDuration"]).unwrap_or(0.0) as u32;
            let (start, end) = (num(p, &["SampleStart"]).unwrap_or(0.0) as u32, num(p, &["SampleEnd"]).map(|v| v as u32));
            let vel = (num(p, &["VelocityRange", "Min"]).unwrap_or(0.0) as u32, num(p, &["VelocityRange", "Max"]).unwrap_or(127.0) as u32);
            Some(Zone {
                file,
                db: db + if amp > 0.0 { 20.0 * amp.log10() } else { -1000.0 },
                pan: (pan + num(p, &["Panorama"]).unwrap_or(0.0)).clamp(-1.0, 1.0),
                semitones,
                reverse,
                attack_secs,
                markers: markers(start, end, length),
                velocity: (vel.0.min(127), vel.1.min(127)),
            })
        })
        .collect()
}

/// The drum sampler's one sample.
fn drum_sampler(d: &Node, roots: &[PathBuf]) -> Vec<Zone> {
    let Some(r) = at(d, &["UserSample", "Value", "SampleRef"]) else { return Vec::new() };
    let Some(file) = sample_file(r, roots) else { return Vec::new() };
    let amp = num(d, &["Voice_Gain"]).unwrap_or(1.0);
    let length = num(r, &["DefaultDuration"]).unwrap_or(0.0);
    let start = num(d, &["Voice_PlaybackStart"]).unwrap_or(0.0).clamp(0.0, 1.0);
    let len = num(d, &["Voice_PlaybackLength"]).unwrap_or(1.0).clamp(0.0, 1.0);
    let end = (start + len).min(1.0);
    vec![Zone {
        file,
        db: num(d, &["Volume"]).unwrap_or(0.0) + if amp > 0.0 { 20.0 * amp.log10() } else { -1000.0 },
        pan: num(d, &["Pan"]).unwrap_or(0.0),
        semitones: num(d, &["Voice_Transpose"]).unwrap_or(0.0) + num(d, &["Voice_Detune"]).unwrap_or(0.0) / 100.0,
        attack_secs: num(d, &["Voice_Envelope_Attack"]).unwrap_or(0.0),
        markers: markers((start * length) as u32, Some((end * length) as u32), length as u32),
        velocity: (0, 127),
        ..Zone::default()
    }]
}

/// Start and end markers when they cut the sample (`length` frames).
fn markers(start: u32, end: Option<u32>, length: u32) -> Option<(u32, u32, u32)> {
    if length == 0 {
        return None;
    }
    let end = end.unwrap_or(length).min(length);
    (start > 0 || end + 1 < length).then_some((start.min(end), end, length))
}

// --- sample paths ---

/// The sample a `SampleRef` names: its relative path in one of `roots`, its
/// stored path, or (when neither exists) the stored or first relative path.
fn sample_file(sample_ref: &Node, roots: &[PathBuf]) -> Option<PathBuf> {
    let r = at(sample_ref, &["FileRef"])?;
    let relative = relative_path(r);
    let stored = value(r, &["Path"]).filter(|p| !p.is_empty()).map(PathBuf::from);
    if let Some(rel) = &relative {
        if let Some(f) = roots.iter().map(|root| root.join(rel)).find(|f| f.is_file()) {
            return Some(f);
        }
    }
    if let Some(s) = stored.as_ref().filter(|s| s.is_file()) {
        return Some(s.clone());
    }
    stored.or_else(|| relative.and_then(|rel| roots.first().map(|r| r.join(rel))))
}

/// The relative path of a `FileRef`: a `RelativePath` value, or (older
/// files) its `RelativePathElement` folders and the `Name`.
fn relative_path(r: &Node) -> Option<PathBuf> {
    let rp = at(r, &["RelativePath"])?;
    if let Some(v) = rp.attr("Value") {
        return (!v.is_empty()).then(|| v.split(['/', '\\']).filter(|s| !s.is_empty()).collect());
    }
    let mut p: PathBuf = rp.children_of("RelativePathElement").filter_map(|e| e.attr("Dir")).filter(|d| !d.is_empty()).collect();
    p.push(value(r, &["Name"])?);
    Some(p)
}

/// The folders a preset's relative sample paths may start from: the
/// preset's folder and its parents (a library or pack holding it), then the
/// default library locations.
fn library_roots(preset: &Path) -> Vec<PathBuf> {
    let mut roots: Vec<PathBuf> = preset.parent().map(|d| d.ancestors().map(Path::to_path_buf).collect()).unwrap_or_default();
    roots.extend(default_libraries());
    roots
}

fn default_libraries() -> Vec<PathBuf> {
    /// `<dir>/*/<inner>`, newest name first.
    fn installs(out: &mut Vec<PathBuf>, dir: PathBuf, inner: &str) {
        if let Ok(rd) = std::fs::read_dir(&dir) {
            let mut v: Vec<PathBuf> = rd.flatten().map(|e| e.path().join(inner)).filter(|p| p.is_dir()).collect();
            v.sort();
            v.reverse();
            out.extend(v);
        }
    }
    let mut out = Vec::new();
    if cfg!(windows) {
        if let Some(pd) = std::env::var_os("ProgramData") {
            installs(&mut out, PathBuf::from(pd).join("Ableton"), "Resources/Core Library");
        }
    } else if cfg!(target_os = "macos") {
        installs(&mut out, PathBuf::from("/Applications"), "Contents/App-Resources/Core Library");
    }
    if let Some(home) = std::env::var_os(if cfg!(windows) { "USERPROFILE" } else { "HOME" }) {
        let docs = PathBuf::from(home).join(if cfg!(target_os = "macos") { "Music/Ableton" } else { "Documents/Ableton" });
        out.push(docs.join("User Library"));
        installs(&mut out, docs.join("Factory Packs"), "");
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rack(pads: &str) -> String {
        format!(r#"<?xml version="1.0" encoding="UTF-8"?><Ableton><GroupDevicePreset><Device><DrumGroupDevice Id="0"/></Device><BranchPresets>{pads}</BranchPresets></GroupDevicePreset></Ableton>"#)
    }

    fn simpler_pad(note: u32, rel: &str, extra: &str) -> String {
        format!(
            r#"<DrumBranchPreset Id="0"><Name Value="{rel}" /><DevicePresets><AbletonDevicePreset><Device><OriginalSimpler Id="0">
            <Player><MultiSampleMap><SampleParts>
              <MultiSamplePart Id="0"><KeyRange><Min Value="0" /><Max Value="127" /></KeyRange><VelocityRange><Min Value="1" /><Max Value="127" /></VelocityRange>
                <Volume Value="1" /><Panorama Value="0" /><SampleStart Value="0" /><SampleEnd Value="99" />
                <SampleRef><FileRef><RelativePath Value="Samples/{rel}.wav" /><Path Value="/nowhere/{rel}.wav" /></FileRef><DefaultDuration Value="100" /></SampleRef>
              </MultiSamplePart></SampleParts></MultiSampleMap><Reverse><Manual Value="true" /></Reverse></Player>
            <Pitch><TransposeKey><Manual Value="-2" /></TransposeKey><TransposeFine><Manual Value="25" /></TransposeFine></Pitch>
            <VolumeAndPan><Volume><Manual Value="-6" /></Volume><Panorama><Manual Value="0.5" /></Panorama><VolumeVelScale><Manual Value="0.35" /></VolumeVelScale></VolumeAndPan>
            {extra}</OriginalSimpler></Device></AbletonDevicePreset></DevicePresets>
            <MixerPreset><AudioBranchMixerDevice><Speaker><Manual Value="true" /></Speaker><Volume><Manual Value="0.5011872" /></Volume><Panorama><Manual Value="0" /></Panorama></AudioBranchMixerDevice></MixerPreset>
            <ZoneSettings><ReceivingNote Value="{}" /><SendingNote Value="60" /><ChokeGroup Value="2" /></ZoneSettings></DrumBranchPreset>"#,
            128 - note
        )
    }

    fn scratch(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("einst-adg-{}-{name}", std::process::id()));
        std::fs::create_dir_all(dir.join("Samples")).unwrap();
        dir
    }

    fn write_gz(path: &Path, text: &str) {
        use std::io::Write;
        let mut e = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::fast());
        e.write_all(text.as_bytes()).unwrap();
        std::fs::write(path, e.finish().unwrap()).unwrap();
    }

    #[test]
    fn pads_map_by_note_with_their_settings() {
        let dir = scratch("map");
        for s in ["Kick", "Snare", "Far"] {
            std::fs::write(dir.join("Samples").join(format!("{s}.wav")), b"RIFF").unwrap();
        }
        let f = dir.join("Kit.adg");
        write_gz(&f, &rack(&[simpler_pad(36, "Kick", ""), simpler_pad(38, "Snare", ""), simpler_pad(70, "Far", "")].concat()));
        let kit = convert(&f).unwrap();
        let _ = std::fs::remove_dir_all(&dir);
        let names: Vec<_> = kit.children.iter().map(|p| p.name.clone().unwrap()).collect();
        assert_eq!(names, ["Pad0", "Pad1", "Pad2"], "note 36 → 1, note 70 fills the first free pad, 38 → 3");
        let kick = &kit.children[0];
        let snare = kit.children.iter().find(|p| p.name.as_deref() == Some("Pad2")).unwrap();
        assert!(snare.children[1].attr("AudioFileName").unwrap().ends_with("Snare.wav"));
        assert_eq!(kick.child("FixedString", "PadName").and_then(|n| n.attr("string")), Some("Kick"));
        assert!((kick.f32("Gain").unwrap() - db_to_value01(-6.0)).abs() < 1e-4);
        assert!((kick.f32("V2A").unwrap() - 0.35).abs() < 1e-6);
        let l = &kick.children[1];
        let file = PathBuf::from(l.attr("AudioFileName").unwrap());
        assert!(file.ends_with(Path::new("Samples").join("Kick.wav")) && file.is_absolute(), "{file:?}");
        assert!((l.f32("LayerGain").unwrap() - db_to_value01(-6.0)).abs() < 1e-4);
        assert!((l.f32("LayerPan").unwrap() - 0.75).abs() < 1e-6);
        assert!((LayerParam::CoarseTune.curve().to_units(l.f32("CoarseTune").unwrap()) + 2.0).abs() < 1e-4);
        assert!((LayerParam::FineTune.curve().to_units(l.f32("FineTune").unwrap()) - 25.0).abs() < 1e-3);
        assert_eq!(l.attr("IsReversed"), Some("true"));
        assert_eq!(l.attr("CutMode"), Some("Global group 2"));
        assert!(l.child("AudioFileMarkers", "StartEndMarkers").is_none(), "the whole sample");
        assert_eq!(kick.attr("PadMode"), Some("All layers"), "velocity 1..127 is every velocity");
    }

    #[test]
    fn eight_slot_instrument_fills_eight_pads() {
        let dir = scratch("slots");
        std::fs::write(dir.join("Samples").join("Kick.wav"), b"RIFF").unwrap();
        let slot = |i: u32, f: &str| {
            format!(
                r#"<TabSample.{i}><Value><SampleRef><FileRef><RelativePath Value="Samples/{f}" /></FileRef></SampleRef></Value></TabSample.{i}><VolumeVolume.{i}><Manual Value="-3" /></VolumeVolume.{i}><StartTune.{i}><Manual Value="14" /></StartTune.{i}>"#
            )
        };
        let f = dir.join("Imp.adg");
        write_gz(&f, &format!(r#"<Ableton><InstrumentImpulse Id="0"><GlobalVolume><Manual Value="0" /></GlobalVolume>{}{}</InstrumentImpulse></Ableton>"#, slot(0, "Kick.wav"), slot(3, "Kick.wav")));
        let kit = convert(&f).unwrap();
        let _ = std::fs::remove_dir_all(&dir);
        let names: Vec<_> = kit.children.iter().map(|p| p.name.clone().unwrap()).collect();
        assert_eq!(names, ["Pad0", "Pad3"]);
        let l = &kit.children[1].children[1];
        assert!((LayerParam::Octave.curve().to_units(l.f32("Octave").unwrap()) - 1.0).abs() < 1e-4);
        assert!((LayerParam::CoarseTune.curve().to_units(l.f32("CoarseTune").unwrap()) - 2.0).abs() < 1e-4);
    }

    #[test]
    fn tuning_splits_octaves_from_large_transposes() {
        assert_eq!(tuning(-2.0), (0, -2, 0.0));
        assert_eq!(tuning(12.0), (0, 12, 0.0));
        assert_eq!(tuning(-24.0), (-2, 0, 0.0));
        let (o, c, f) = tuning(0.3);
        assert_eq!((o, c), (0, 0));
        assert!((f - 30.0).abs() < 1e-3);
    }

    #[test]
    fn no_samples_is_an_error() {
        let dir = scratch("empty");
        let f = dir.join("Synth.adg");
        write_gz(&f, &rack(""));
        assert!(convert(&f).is_err());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
