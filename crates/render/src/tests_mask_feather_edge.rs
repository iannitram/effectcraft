//! A mask's feather behaves as if the mask lived on an endless plane (After Effects): where the
//! path covers the layer's edge and beyond, the feather must not fade toward that edge.
//!
//! The coverage used to be rasterised into a buffer the size of the layer and then blurred, so
//! everything past the layer counted as "no mask": a feathered mask larger than its layer still
//! faded to about half at the layer's edges, and Mask Expansion stopped having any effect once
//! the path had left the layer.

use effectcraft_color::Label;
use effectcraft_keyframe::{ShapePath, Value};
use effectcraft_project::build::{self, Ids};
use effectcraft_project::{Comp, FeatherFalloff, GroupKind, ItemId, ItemKind, LayerSource, MaskMode, Node, Project, Solid};
use effectcraft_time::{FrameRate, Tick};

use crate::{NoFootage, RenderOpts, Renderer};

const W: u32 = 200;
const H: u32 = 100;

/// A white 200×100 solid in a 200×100 comp with one rectangular mask.
fn project(rect: ([f64; 2], f64, f64), mode: MaskMode, feather: f64, expansion: f64, linear: bool) -> (Project, ItemId) {
    let mut p = Project::default();
    let comp = Comp::new(W, H, FrameRate::FPS_30, Tick::from_seconds_f64(1.0));
    let sid = p.add_item("S", Label::Red, None, ItemKind::Solid(Solid { color: [1.0; 3], width: W, height: H, pixel_aspect: 1.0 }));
    let mut l = build::layer(&mut p, &comp, "S", LayerSource::Solid { item: sid }, (W, H), None);
    let mut next = p.next_id;
    let mut m = build::mask(&mut Ids(&mut next), "Mask 1", ShapePath::rect(rect.0, rect.1, rect.2), mode, [255, 0, 0]);
    p.next_id = next;
    if let GroupKind::Mask { feather_falloff, .. } = &mut m.kind
        && linear
    {
        *feather_falloff = FeatherFalloff::Linear;
    }
    m.get_mut("feather").unwrap().value = Value::Vec2([feather, feather]);
    m.get_mut("expansion").unwrap().value = Value::Scalar(expansion);
    l.props.sub_mut("masks").unwrap().children.push(Node::Group(m));
    let cid = p.add_item("C", Label::Sandstone, None, ItemKind::Comp(comp.into()));
    p.comp_mut(cid).unwrap().layers.push(l);
    (p, cid)
}

fn alpha(p: &Project, cid: ItemId, x: i64, y: i64) -> f32 {
    Renderer::new(p, &NoFootage, RenderOpts::default()).comp_frame(cid, Tick::ZERO).get(x, y)[3]
}

const WHOLE: ([f64; 2], f64, f64) = ([100.0, 50.0], 200.0, 100.0);

#[test]
fn a_feathered_mask_larger_than_the_layer_does_not_fade_at_the_layers_edges() {
    for linear in [false, true] {
        // Add: the path, expanded 150 px past every edge, covers the layer and far beyond.
        let (p, cid) = project(WHOLE, MaskMode::Add, 40.0, 150.0, linear);
        for (x, y) in [(0, 50), (199, 50), (100, 0), (100, 99), (0, 0), (199, 99), (100, 50)] {
            let a = alpha(&p, cid, x, y);
            assert!(a > 0.98, "add, linear {linear}: alpha {a} at ({x}, {y}), expected the layer whole");
        }
        // Subtract: the same hole leaves nothing of the layer.
        let (p, cid) = project(WHOLE, MaskMode::Subtract, 40.0, 150.0, linear);
        for (x, y) in [(0, 50), (199, 50), (100, 0), (100, 99), (0, 0), (100, 50)] {
            let a = alpha(&p, cid, x, y);
            assert!(a < 0.02, "subtract, linear {linear}: alpha {a} at ({x}, {y}), expected nothing left");
        }
    }
}

#[test]
fn expansion_keeps_working_after_the_path_has_left_the_layer() {
    // The path ends exactly at the layer's edges: its feathered edge sits on them, half covered.
    let (p, cid) = project(WHOLE, MaskMode::Subtract, 40.0, 0.0, false);
    let at_edge = alpha(&p, cid, 0, 50);
    assert!((0.3..0.7).contains(&at_edge), "no expansion: alpha {at_edge} at the edge, expected about half");
    // Pushed 30 px out, the feather's soft edge is still partly inside; pushed 150 px out, gone.
    let (p, cid) = project(WHOLE, MaskMode::Subtract, 40.0, 30.0, false);
    let near = alpha(&p, cid, 0, 50);
    let (p, cid) = project(WHOLE, MaskMode::Subtract, 40.0, 150.0, false);
    let far = alpha(&p, cid, 0, 50);
    assert!(near < at_edge - 0.1 && far < near - 0.01, "expansion 0 / 30 / 150 at the edge: {at_edge} / {near} / {far}");
}

#[test]
fn a_feathered_edge_inside_the_layer_is_unchanged() {
    // The left half: the soft edge is centred on x = 100 and spans about the feather's width.
    let (p, cid) = project(([50.0, 50.0], 100.0, 100.0), MaskMode::Add, 40.0, 0.0, false);
    let (inside, edge, outside) = (alpha(&p, cid, 40, 50), alpha(&p, cid, 100, 50), alpha(&p, cid, 160, 50));
    assert!(inside > 0.97 && outside < 0.03, "inside {inside}, outside {outside}");
    assert!((0.4..0.6).contains(&edge), "at the edge: {edge}");
    // And the layer's own left edge, which the path also ends on, still fades (nothing is
    // covered beyond it).
    let left = alpha(&p, cid, 0, 50);
    assert!((0.3..0.7).contains(&left), "at the layer's left edge: {left}");
}
