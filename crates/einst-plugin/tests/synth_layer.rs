//! A synth layer plays its render as rendered: a pad hit with no effects
//! gives back the render's samples, on a bare layer and on a kit's layer
//! (whose envelope routings stay neutral on a hit without a step), at the
//! render's rate and at another.

use dreist_formats::Node;
use einst_dsp::synth::{self, P, Patch};

/// The render of `patch` and what a hit on pad 1 (its layer 1 `layer`,
/// else bare) plays at `sr`.
fn hit(patch: &Patch, tag: &str, sr: f32, layer: Option<Node>) -> (Vec<f32>, Vec<f32>) {
    einst::install();
    let dir = std::env::temp_dir().join(format!("einst-synth-layer-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let file = einst::synth::render_to(&dir, patch).unwrap();
    let render = synth::render(patch);
    let mut model = einst_editor::Model::new(Node::parse("<root version=\"3000\" CurrentEngine=\"0\" />").unwrap());
    if let Some(l) = layer {
        *model.layer_mut(0, 0, 0) = l;
    }
    einst_editor::synth::assign(&mut model, 0, 0, 0, patch, &file);
    let mut cache = Default::default();
    let mut g = dreist_engine::Dreist::from_document(&model.doc, &dir, sr, Default::default(), &mut cache).expect("builds");
    g.push_midi(0, [0x90, 36, 127]);
    let n = (render.len() as f32 * sr / synth::RATE as f32) as usize + 4800;
    let (mut l, mut r) = (vec![0.0; n], vec![0.0; n]);
    for (cl, cr) in l.chunks_mut(256).zip(r.chunks_mut(256)) {
        g.process(cl, cr);
    }
    let _ = std::fs::remove_dir_all(&dir);
    (render, l)
}

/// RMS per 10 ms block.
fn blocks(x: &[f32], sr: f32) -> Vec<f32> {
    x.chunks((sr / 100.0) as usize).map(|c| (c.iter().map(|v| v * v).sum::<f32>() / c.len() as f32).sqrt()).collect()
}

fn hz(x: &[f32], sr: f32) -> f32 {
    x.windows(2).filter(|w| w[0] < 0.0 && w[1] >= 0.0).count() as f32 * sr / x.len() as f32
}

fn kick() -> Patch {
    let mut p = Patch::default();
    for (k, v) in [(P::Osc1Wave, 0.0), (P::Osc1Coarse, 0.0), (P::Osc1PitchAmount, 0.93), (P::Osc1PitchDecay, 0.49), (P::AmpDecay, 0.75)] {
        p.set(k, v);
    }
    p
}

#[test]
fn a_pad_plays_the_render() {
    for (tag, sr, layer) in [("bare", 48_000.0, None), ("kit", 48_000.0, Some(einst_formats::v2::default_layer(0))), ("44k", 44_100.0, Some(einst_formats::v2::default_layer(0)))] {
        let (render, out) = hit(&kick(), tag, sr, layer);
        let (a, b) = (blocks(&render, synth::RATE as f32), blocks(&out, sr));
        let worst = a.iter().zip(&b).take(a.len() - 2).map(|(x, y)| (20.0 * (y.max(1e-6) / x.max(1e-6)).log10()).abs()).fold(0f32, f32::max);
        assert!(worst < 0.5, "{tag}: the pad's level differs from the render's by up to {worst:.1} dB");
    }
}

/// A sine plays at its rendered pitch on a kit's layer (its pitch routing
/// at depth 0.5 is none).
#[test]
fn a_kit_layer_keeps_the_pitch() {
    let mut sine = Patch::default();
    for (k, v) in [(P::Osc1Wave, 0.0), (P::AmpDecay, 1.0), (P::Cutoff, 1.0)] {
        sine.set(k, v);
    }
    let (render, out) = hit(&sine, "pitch", 48_000.0, Some(einst_formats::v2::default_layer(0)));
    let (a, b) = (hz(&render[4800..24000], 48_000.0), hz(&out[4800..24000], 48_000.0));
    assert!((a - b).abs() < 2.0, "rendered {a} Hz, played {b} Hz");
}
