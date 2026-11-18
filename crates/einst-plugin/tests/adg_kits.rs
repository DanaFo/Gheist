//! Drum rack presets of another DAW's factory library open as kits and
//! play (`einst_formats::adg`). The library is `EINST_ADG_LIBRARY`, else its
//! default install; the test skips without one.

use std::path::{Path, PathBuf};

fn library() -> Option<PathBuf> {
    if let Some(p) = std::env::var_os("EINST_ADG_LIBRARY") {
        return Some(PathBuf::from(p));
    }
    let installs: Vec<PathBuf> = if cfg!(windows) {
        let pd = PathBuf::from(std::env::var_os("ProgramData")?).join("Ableton");
        std::fs::read_dir(pd).ok()?.flatten().map(|e| e.path().join("Resources/Core Library")).collect()
    } else {
        std::fs::read_dir("/Applications").ok()?.flatten().map(|e| e.path().join("Contents/App-Resources/Core Library")).collect()
    };
    installs.into_iter().find(|p| p.is_dir())
}

fn presets(dir: &Path, out: &mut Vec<PathBuf>) {
    for e in std::fs::read_dir(dir).into_iter().flatten().flatten() {
        let p = e.path();
        if p.is_dir() {
            presets(&p, out);
        } else if einst_formats::adg::is_drum_rack(&p) {
            out.push(p);
        }
    }
}

#[test]
fn factory_drum_kits_open_and_play() {
    let Some(lib) = library() else {
        eprintln!("no drum rack library: skipped");
        return;
    };
    einst::install();
    let mut kits = Vec::new();
    for sub in ["Racks/Drum Racks", "Devices/Instruments/Impulse"] {
        presets(&lib.join(sub), &mut kits);
    }
    kits.sort();
    assert!(!kits.is_empty(), "no drum racks under {}", lib.display());
    let (mut opened, mut no_samples, mut layers) = (0, Vec::new(), 0);
    let mut failures = Vec::new();
    let mut cache = Default::default();
    for kit in &kits {
        let name = kit.strip_prefix(&lib).unwrap_or(kit).display().to_string();
        let doc = match einst_formats::adg::convert(kit) {
            Ok(d) => d,
            Err(einst_formats::Error::Shape(..)) => {
                no_samples.push(name);
                continue;
            }
            Err(e) => {
                failures.push(format!("{name}: {e}"));
                continue;
            }
        };
        for l in doc.children.iter().flat_map(|p| p.children_of("Layer")) {
            layers += 1;
            let f = l.attr("AudioFileName").unwrap_or_default();
            if !Path::new(f).is_file() {
                failures.push(format!("{name}: missing sample {f}"));
            }
        }
        let first = doc.children.iter().filter_map(|p| p.name.as_deref()?.strip_prefix("Pad")?.parse::<u8>().ok()).next().unwrap();
        let mut g = match einst_plugin::load_project(kit, 44_100.0, Default::default(), &mut cache) {
            Ok(g) => g,
            Err(e) => {
                failures.push(format!("{name}: {e}"));
                continue;
            }
        };
        g.push_midi(0, [0x90, 36 + first, 127]);
        let (mut l, mut r) = (vec![0.0; 8192], vec![0.0; 8192]);
        g.process(&mut l, &mut r);
        let peak = l.iter().chain(&r).fold(0.0f32, |m, v| m.max(v.abs()));
        if !(peak > 1e-4) {
            failures.push(format!("{name}: pad {} silent (peak {peak})", first + 1));
        }
        opened += 1;
    }
    eprintln!("{opened} of {} kits open and play ({layers} layers); without sampled pads: {no_samples:?}", kits.len());
    assert!(failures.is_empty(), "{} failures:\n{}", failures.len(), failures.join("\n"));
    assert!(opened * 10 >= kits.len() * 8, "most kits are sampled");
}
