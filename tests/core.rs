use materialcraft::graph::{Error, Graph, EXAMPLE};
use materialcraft::{expr, noise, png};

#[test]
fn example_graph_renders() {
    let g = Graph::parse(EXAMPLE).unwrap();
    let out = g.evaluate(Some(64)).unwrap();
    assert_eq!(out.len(), 6);
    assert!(out["height"].px.iter().all(|p| p[0].is_finite()));
}

#[test]
fn deterministic_by_seed() {
    let g = Graph::parse("size 32\nnode 1 fbm scale=4 seed=5\noutput a 1\n").unwrap();
    assert_eq!(g.evaluate(None).unwrap()["a"].px, g.evaluate(None).unwrap()["a"].px);
}

#[test]
fn noise_is_tileable() {
    for i in 0..20 {
        let v = i as f32 / 20.0;
        let a = noise::fbm(0.0, v, 4, 4, 4, 0.5, 1, 0);
        let b = noise::fbm(1.0, v, 4, 4, 4, 0.5, 1, 0);
        assert!((a - b).abs() < 1e-4);
    }
}

#[test]
fn detects_cycle() {
    let g = "size 8\nnode 1 invert in=@2\nnode 2 invert in=@1\noutput a 1\n";
    assert!(matches!(Graph::parse(g), Err(Error::Cycle(_))));
}

#[test]
fn detects_missing_input_and_node() {
    assert!(matches!(Graph::parse("node 1 blur\noutput a 1\n"), Err(Error::MissingInput(..))));
    assert!(matches!(Graph::parse("node 1 blur in=@9\noutput a 1\n"), Err(Error::MissingNode(..))));
    assert!(matches!(Graph::parse("node 1 nope\noutput a 1\n"), Err(Error::UnknownNode(..))));
}

#[test]
fn expressions() {
    let e = expr::parse("clamp(u*2+1, 0, 1) + 2^3").unwrap();
    let env = expr::Env { vars: [0.1; 10], tex: None };
    assert!((expr::eval(&e, &env) - 9.0).abs() < 1e-4);
    assert!(expr::parse("sin(").is_err());
    assert!(expr::parse("foo(1)").is_err());
}

#[test]
fn png_signature_and_roundtrip_size() {
    let px = vec![[0.5, 0.25, 1.0, 1.0]; 16];
    let b = png::encode(4, 4, &px, 16);
    assert_eq!(&b[1..4], b"PNG");
}

#[test]
fn pixel_processor_and_save_roundtrip() {
    let t = "size 16\nnode 1 pixel_processor expr=\"u*v\"\noutput a 1\n";
    let g = Graph::parse(t).unwrap();
    let g2 = Graph::parse(&g.to_text()).unwrap();
    assert_eq!(g.evaluate(None).unwrap()["a"].px, g2.evaluate(None).unwrap()["a"].px);
}

use materialcraft::bake;
use materialcraft::graph::Graph as G2;
use std::collections::BTreeSet;

#[test]
fn png_roundtrip_decode() {
    let mut px = Vec::new();
    for i in 0..64 {
        px.push([i as f32 / 63.0, 0.25, 1.0 - i as f32 / 63.0, 1.0]);
    }
    for depth in [8u8, 16] {
        let bytes = png::encode(8, 8, &px, depth);
        let t = png::decode(&bytes).unwrap();
        assert_eq!((t.w, t.h), (8, 8));
        for (a, b) in px.iter().zip(&t.px) {
            for c in 0..3 {
                assert!((a[c] - b[c]).abs() < 0.01, "{a:?} vs {b:?}");
            }
        }
    }
}

#[test]
fn subgraph_extract_preserves_result() {
    let mut g = G2::parse(EXAMPLE).unwrap();
    let before = g.evaluate(Some(48)).unwrap();
    let sel: BTreeSet<u32> = [2, 4].into_iter().collect();
    let id = g.extract_subgraph(&sel, "sg").unwrap();
    assert!(g.nodes.contains_key(&id));
    let g2 = G2::parse(&g.to_text()).unwrap();
    let after = g2.evaluate(Some(48)).unwrap();
    assert_eq!(before["basecolor"].px, after["basecolor"].px);
}

#[test]
fn publish_embeds_bitmap_and_loads() {
    let dir = std::env::temp_dir().join("mf_test_bitmap");
    std::fs::create_dir_all(&dir).unwrap();
    let f = dir.join("t.png");
    let px = vec![[1.0, 0.0, 0.0, 1.0]; 16];
    std::fs::write(&f, png::encode(4, 4, &px, 8)).unwrap();
    let txt = format!("size 16\nnode 1 bitmap file=\"{}\"\noutput basecolor 1\nexpose \"X\" 1 seed 0 1\n", f.display());
    let g = G2::parse(&txt).unwrap();
    let pkg = g.publish().unwrap();
    std::fs::remove_file(&f).unwrap();
    let g2 = G2::parse(&pkg).unwrap();
    let out = g2.evaluate(None).unwrap();
    assert!(out["basecolor"].px.iter().all(|p| p[0] > 0.99 && p[1] < 0.01));
}

#[test]
fn exposed_parameters_apply() {
    let mut g = G2::parse(EXAMPLE).unwrap();
    let mut p = std::collections::BTreeMap::new();
    p.insert("Brick count".to_string(), 6.0);
    g.apply_params(&p).unwrap();
    assert_eq!(g.nodes[&1].nums["cols"], 6.0);
    p.insert("Nope".to_string(), 1.0);
    assert!(g.apply_params(&p).is_err());
}

#[test]
fn sdf_nodes_and_forms() {
    let t = "size 32\nnode 1 sdf_shape shape=0 size=0.3\nnode 2 sdf_shape shape=1 size=0.2 x=0.2\nnode 3 sdf_op a=@1 b=@2 op=0 smooth=0.05\nnode 4 sdf_mask in=@3\nnode 5 sdf_form shape=2 repeat=2\noutput a 4\noutput b 5\n";
    let out = G2::parse(t).unwrap().evaluate(None).unwrap();
    assert!(out["a"].px.iter().any(|p| p[0] > 0.9) && out["a"].px.iter().any(|p| p[0] < 0.1));
    assert!(out["b"].px.iter().any(|p| p[0] > 0.05));
}

#[test]
fn baker_produces_maps() {
    let low = bake::parse_obj(&bake::uv_sphere_obj(12, 8, 1.0, 0.0)).unwrap();
    let high = bake::parse_obj(&bake::uv_sphere_obj(48, 32, 1.0, 0.05)).unwrap();
    let s = bake::BakeSettings { size: 64, max_dist: 0.2, ao_rays: 8, ao_dist: 0.5, padding: 2 };
    let m = bake::bake(&low, &high, &s).unwrap();
    assert_eq!(m.len(), 7);
    let n = &m["normal"];
    assert!(n.px.iter().all(|p| p[2] > 0.3));
    assert!(n.px.iter().any(|p| (p[0] - 0.5).abs() > 0.02));
    assert!(bake::parse_obj("v 0 0 0\n").is_err());
}

#[test]
fn renderer_draws_shapes() {
    use materialcraft::render3d::{render, Maps, View};
    use materialcraft::tex::Tex;
    let (b, n, r, m, a) = (Tex::solid(8, 8, [0.8, 0.2, 0.2, 1.0]), Tex::solid(8, 8, [0.5, 0.5, 1.0, 1.0]), Tex::solid(8, 8, [0.4; 4]), Tex::solid(8, 8, [0.0, 0.0, 0.0, 1.0]), Tex::solid(8, 8, [1.0; 4]));
    let maps = Maps { base: &b, normal: &n, rough: &r, metal: &m, ao: &a, emissive: None };
    for shape in 0..3 {
        let img = render(&maps, &View { shape, ..View::default() }, 64, 64);
        let c = img.px[32 * 64 + 32];
        assert!(c[0] > c[2], "shape {shape} centre should be reddish, got {c:?}");
    }
}

#[test]
fn bad_inputs_are_errors_not_panics() {
    assert!(png::decode(b"not a png").is_err());
    assert!(png::decode(&[0x89, b'P', b'N', b'G', 0x0d, 0x0a, 0x1a, 0x0a, 0, 0, 0, 0]).is_err());
    assert!(G2::parse("subgraph a\nnode 1 color\n").is_err());
    assert!(G2::parse("node 1 subgraph graph=\"missing\"\noutput a 1\n").is_err());
}

#[test]
fn every_node_type_evaluates() {
    use materialcraft::graph::{demo_graph_for, SPECS};
    let mut count = 0;
    for sp in SPECS {
        if let Some(text) = demo_graph_for(sp.kind) {
            let g = G2::parse(&text).unwrap_or_else(|e| panic!("{}: parse: {e}", sp.kind));
            let out = g.evaluate(Some(32)).unwrap_or_else(|e| panic!("{}: eval: {e}", sp.kind));
            let t = &out["o"];
            assert!(t.px.iter().all(|p| p.iter().all(|c| c.is_finite())), "{} produced NaN", sp.kind);
            count += 1;
        }
    }
    assert!(count > 60, "only {count} nodes tested");
}

#[test]
fn output_nodes_defaults_and_migration() {
    let g = G2::parse("size 16\nnode 1 output usage=normal\nnode 2 output usage=basecolor identifier=Albedo\n").unwrap();
    let out = g.evaluate(None).unwrap();
    assert!((out["normal"].px[0][2] - 1.0).abs() < 1e-4);
    assert!(out.contains_key("Albedo"));
    let mut legacy = G2::parse(EXAMPLE).unwrap();
    let before = legacy.evaluate(Some(32)).unwrap();
    legacy.migrate_outputs();
    assert!(legacy.outputs.is_empty());
    let after = legacy.evaluate(Some(32)).unwrap();
    assert_eq!(before["basecolor"].px, after["basecolor"].px);
}

#[test]
fn flood_fill_and_distance_work() {
    let t = "size 64\nnode 1 voronoi scale=4 mode=2 seed=1\nnode 2 threshold in=@1 threshold=0.2\nnode 3 flood_fill in=@2\nnode 4 flood_fill_random in=@3 seed=2\nnode 5 distance in=@2 distance=0.1\nnode 6 bevel in=@2 distance=0.05\nnode 7 make_it_tile_photo in=@1\noutput a 4\noutput b 5\noutput c 6\noutput d 7\n";
    let out = G2::parse(t).unwrap().evaluate(None).unwrap();
    let vals: std::collections::BTreeSet<u32> = out["a"].px.iter().map(|p| (p[0] * 255.0) as u32).collect();
    assert!(vals.len() >= 4, "regions should get different grays, got {}", vals.len());
    assert!(out["b"].px.iter().any(|p| p[0] > 0.9));
    assert!(out["c"].px.iter().any(|p| p[0] > 0.9));
    // photo-tile result must be seamless: the wrap seam should look like any other pixel step
    let d = &out["d"];
    let (mut seam, mut inner) = (0.0, 0.0);
    for y in 0..64 {
        seam += (d.px[y * 64][0] - d.px[y * 64 + 63][0]).abs();
        for x in 8..55 {
            inner += (d.px[y * 64 + x][0] - d.px[y * 64 + x + 1][0]).abs() / 47.0;
        }
    }
    assert!(seam <= inner * 1.6 + 0.5, "seam {seam} vs typical {inner}");
}

#[test]
fn color_node_gray_mode() {
    let g = G2::parse("size 8\nnode 1 color r=1 g=0 b=0 gray=1 v=0.25\noutput a 1\n").unwrap();
    let p = g.evaluate(None).unwrap()["a"].px[0];
    assert!((p[0] - 0.25).abs() < 1e-5 && (p[1] - 0.25).abs() < 1e-5);
}
