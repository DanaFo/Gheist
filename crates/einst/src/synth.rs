//! Synth layers: a layer whose sample is the render of a synth patch
//! (`einst_dsp::synth`). The patch is a `SynthPatch` child of the layer, one
//! attribute per parameter; the render is a WAV in the render folder named
//! by the patch's hash, so the same patch always plays the same file. A
//! document whose render is missing (another machine, a cleared folder) is
//! rendered again when it is read.

use std::path::{Path, PathBuf};

use dreist_formats::Node;
use einst_dsp::synth::{self, Patch};

/// The class (and name) of the patch node under a layer.
pub const PATCH: &str = "SynthPatch";
/// The layer name a synth layer shows.
pub const LAYER_NAME: &str = "Synth";
/// The render folder's override (tests, tools).
pub const RENDERS_ENV: &str = "EINST_SYNTH_RENDERS";
/// Renders kept in the folder; older ones go (a document still using one
/// renders it again when it is read).
const KEEP: usize = 256;

/// The folder renders go to: `Synth Renders` in the app's settings folder.
pub fn render_dir() -> Option<PathBuf> {
    if let Some(d) = std::env::var_os(RENDERS_ENV) {
        return Some(PathBuf::from(d));
    }
    Some(dreist_formats::paths::settings_base()?.join("Einst").join("Synth Renders"))
}

/// The patch of a synth layer.
pub fn patch_of(layer: &Node) -> Option<Patch> {
    let n = layer.child(PATCH, PATCH)?;
    Some(Patch::from_values(n.attrs.iter().filter_map(|(k, v)| Some((k.as_str(), v.parse::<f32>().ok()?)))))
}

/// The node storing `patch`.
pub fn patch_node(patch: &Patch) -> Node {
    let mut n = Node::new(PATCH, Some(PATCH));
    for (id, v) in patch.values() {
        n.set(id, v);
    }
    n
}

/// Stores `patch` on `layer`, replacing the one there.
pub fn set_patch(layer: &mut Node, patch: &Patch) {
    layer.children.retain(|c| c.class != PATCH);
    layer.children.push(patch_node(patch));
}

/// Drops a layer's patch (a sample replaced it).
pub fn clear_patch(layer: &mut Node) {
    layer.children.retain(|c| c.class != PATCH);
}

/// The file `patch` renders to in `dir`.
pub fn render_path(dir: &Path, patch: &Patch) -> PathBuf {
    dir.join(format!("synth-{:016x}.wav", patch.hash()))
}

/// Renders `patch` to its file in `dir` unless it is there already.
pub fn render_to(dir: &Path, patch: &Patch) -> std::io::Result<PathBuf> {
    let path = render_path(dir, patch);
    if path.is_file() {
        return Ok(path);
    }
    std::fs::create_dir_all(dir)?;
    let tmp = path.with_extension(format!("{}.tmp", std::process::id()));
    std::fs::write(&tmp, synth::wav(&synth::render(patch)))?;
    std::fs::rename(&tmp, &path).or_else(|e| if path.is_file() { std::fs::remove_file(&tmp).map(|_| ()) } else { Err(e) })?;
    prune(dir, &path);
    Ok(path)
}

/// Removes the oldest renders past [`KEEP`], never `keep`.
fn prune(dir: &Path, keep: &Path) {
    let mut files: Vec<(std::time::SystemTime, PathBuf)> = std::fs::read_dir(dir)
        .into_iter()
        .flatten()
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.file_name().and_then(|n| n.to_str()).is_some_and(|n| n.starts_with("synth-") && n.ends_with(".wav")))
        .filter_map(|p| Some((p.metadata().ok()?.modified().ok()?, p)))
        .collect();
    if files.len() <= KEEP {
        return;
    }
    files.sort();
    let extra = files.len() - KEEP;
    for (_, p) in files.into_iter().take(extra) {
        if p != keep {
            let _ = std::fs::remove_file(p);
        }
    }
}

/// Renders every synth layer under `doc` whose sample is missing (resolved
/// against `samples` when relative) and points the layer at the render.
/// Returns how many layers it rendered.
pub fn restore(doc: &mut Node, samples: Option<&Path>) -> usize {
    let Some(dir) = render_dir() else { return 0 };
    restore_into(doc, samples, &dir)
}

fn restore_into(n: &mut Node, samples: Option<&Path>, dir: &Path) -> usize {
    let mut count = 0;
    if n.class == "Layer" {
        if let Some(patch) = patch_of(n) {
            let present = n.attr("AudioFileName").filter(|f| !f.is_empty()).is_some_and(|f| {
                let p = PathBuf::from(f.replace('\\', "/"));
                p.is_file() || (p.is_relative() && samples.is_some_and(|s| s.join(&p).is_file()))
            });
            if !present {
                if let Ok(path) = render_to(dir, &patch) {
                    n.set("AudioFileName", path.to_string_lossy());
                    count += 1;
                }
            }
        }
    }
    for c in &mut n.children {
        count += restore_into(c, samples, dir);
    }
    count
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("einst-synth-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        d
    }

    #[test]
    fn patches_round_trip_through_the_layer() {
        let mut p = Patch::default();
        p.set(synth::P::Osc2Wave, 3.0);
        p.set(synth::P::Cutoff, 0.123);
        let mut layer = Node::new("Layer", Some("Layer0"));
        set_patch(&mut layer, &p);
        set_patch(&mut layer, &p);
        assert_eq!(layer.children_of(PATCH).count(), 1);
        let text = layer.to_xml();
        let back = Node::parse(&text).unwrap();
        assert_eq!(patch_of(&back), Some(p));
        clear_patch(&mut layer);
        assert_eq!(patch_of(&layer), None);
    }

    #[test]
    fn missing_renders_render_again() {
        let dir = scratch("restore");
        let p = Patch::default();
        let mut doc = Node::new("Engine", Some("Engine0"));
        let pad = doc.child_mut_or_insert("Pad", "Pad0");
        let layer = pad.child_mut_or_insert("Layer", "Layer0");
        layer.set("AudioFileName", dir.join("elsewhere/gone.wav").to_string_lossy());
        set_patch(layer, &p);
        assert_eq!(restore_into(&mut doc, None, &dir), 1);
        let f = doc.children[0].children[0].attr("AudioFileName").unwrap().to_string();
        assert_eq!(PathBuf::from(&f), render_path(&dir, &p));
        let bytes = std::fs::read(&f).unwrap();
        assert_eq!(&bytes[..4], b"RIFF");
        // Present now: nothing to do.
        assert_eq!(restore_into(&mut doc, None, &dir), 0);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn old_renders_are_pruned() {
        let dir = scratch("prune");
        std::fs::create_dir_all(&dir).unwrap();
        for i in 0..KEEP + 5 {
            std::fs::write(dir.join(format!("synth-{i:016x}.wav")), b"x").unwrap();
        }
        std::fs::write(dir.join("other.wav"), b"x").unwrap();
        let mut p = Patch::default();
        p.set(synth::P::AmpDecay, 0.0);
        let kept = render_to(&dir, &p).unwrap();
        let n = std::fs::read_dir(&dir).unwrap().count();
        assert_eq!(n, KEEP + 1, "the newest renders and the other file stay");
        assert!(kept.is_file() && dir.join("other.wav").is_file());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
