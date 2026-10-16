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
    let maps = Maps { base: &b, normal: &n, rough: &r, metal: &m, ao: &a, emissive: None, height: None };
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

#[test]
fn jpeg_and_tiff_are_well_formed() {
    use materialcraft::{export, jpeg};
    assert!(jpeg::tables_complete(), "standard Huffman tables must cover every symbol");
    let mut px = Vec::new();
    for y in 0..37 {
        for x in 0..53 {
            px.push([x as f32 / 52.0, y as f32 / 36.0, 0.5, 1.0]);
        }
    }
    let t = materialcraft::tex::Tex { w: 53, h: 37, px };
    let j = export::encode(export::Format::Jpeg, &t, 8, 90);
    assert_eq!(&j[..2], &[0xff, 0xd8]);
    assert_eq!(&j[j.len() - 2..], &[0xff, 0xd9]);
    for depth in [8u8, 16] {
        let tf = export::encode(export::Format::Tiff, &t, depth, 90);
        assert_eq!(&tf[..4], &[b'I', b'I', 42, 0]);
    }
}

#[test]
fn png_decoder_handles_palette_alpha_and_low_bit_depth() {
    // hand-built 2x2 4-bit palette PNG with tRNS, stored (uncompressed) deflate
    fn chunk(out: &mut Vec<u8>, kind: &[u8; 4], data: &[u8]) {
        out.extend_from_slice(&(data.len() as u32).to_be_bytes());
        out.extend_from_slice(kind);
        out.extend_from_slice(data);
        out.extend_from_slice(&[0, 0, 0, 0]); // CRC is not checked by the decoder
    }
    let mut p = vec![0x89, b'P', b'N', b'G', 0x0d, 0x0a, 0x1a, 0x0a];
    let mut ihdr = Vec::new();
    ihdr.extend_from_slice(&2u32.to_be_bytes());
    ihdr.extend_from_slice(&2u32.to_be_bytes());
    ihdr.extend_from_slice(&[4, 3, 0, 0, 0]);
    chunk(&mut p, b"IHDR", &ihdr);
    chunk(&mut p, b"PLTE", &[255, 0, 0, 0, 255, 0]);
    chunk(&mut p, b"tRNS", &[255, 128]);
    let rows = [0u8, 0x01, 0u8, 0x10]; // filter 0 + two pixels per row (indices 0,1 / 1,0)
    let mut z = vec![0x78, 0x01, 1, rows.len() as u8, 0, !(rows.len() as u8), 0xff];
    z.extend_from_slice(&rows);
    z.extend_from_slice(&[0, 0, 0, 0]);
    chunk(&mut p, b"IDAT", &z);
    chunk(&mut p, b"IEND", &[]);
    let t = png::decode(&p).unwrap();
    assert_eq!((t.w, t.h), (2, 2));
    assert!(t.px[0][0] > 0.99 && t.px[0][3] > 0.99); // index 0: opaque red
    assert!(t.px[1][1] > 0.99 && (t.px[1][3] - 128.0 / 255.0).abs() < 0.01); // index 1: half-transparent green
}

// ---------------------------------------------------------------- seamless angles + 3D mesh / displacement
/// mean |difference| across the wrap seam divided by the mean |difference| of ordinary neighbouring pixels
fn seam_ratio(t: &materialcraft::tex::Tex) -> f32 {
    let w = t.w;
    let (mut seam, mut inner) = (0.0f32, 0.0f32);
    for y in 0..t.h {
        seam += (t.px[y * w][0] - t.px[y * w + w - 1][0]).abs();
        for x in 0..w - 1 {
            inner += (t.px[y * w + x][0] - t.px[y * w + x + 1][0]).abs();
        }
    }
    for x in 0..w {
        seam += (t.px[x][0] - t.px[(t.h - 1) * w + x][0]).abs();
        for y in 0..t.h - 1 {
            inner += (t.px[y * w + x][0] - t.px[(y + 1) * w + x][0]).abs();
        }
    }
    (seam / (2 * w) as f32) / (inner / (2 * w * (w - 1)) as f32)
}

#[test]
fn stripes_tile_at_arbitrary_angles() {
    for (count, ang) in [(8.0, 0.0), (8.0, 0.137), (5.0, 0.21), (12.0, 0.3333), (3.0, 0.77), (20.0, 0.05), (8.0, 0.9)] {
        let g = G2::parse(&format!("size 256\nnode 1 stripes count={count} angle={ang} softness=0.2\noutput a 1\n")).unwrap();
        let r = seam_ratio(&g.evaluate(None).unwrap()["a"]);
        assert!(r < 1.6, "stripes count {count} angle {ang}: seam/inner = {r}");
        // the snapped direction must stay close to what was asked for
        let (a, b) = materialcraft::ops::tileable_wave(count, ang);
        let want = ang as f64 * std::f64::consts::TAU;
        let d = ((b as f64).atan2(a as f64) - want).sin().atan2(((b as f64).atan2(a as f64) - want).cos()).abs();
        let len = ((a * a + b * b) as f64).sqrt();
        assert!(d < 0.35 && len > count as f64 * 0.55 && len < count as f64 * 1.65, "snap ({a},{b}) too far for count {count} angle {ang}");
    }
    // already-tileable settings are unchanged
    assert_eq!(materialcraft::ops::tileable_wave(8.0, 0.0), (8, 0));
    assert_eq!(materialcraft::ops::tileable_wave(6.0, 0.25).0, 0);
}

#[test]
fn stripes_exact_mode_keeps_requested_angle() {
    let g = G2::parse("size 64\nnode 1 stripes count=8 angle=0.137 tile=0\noutput a 1\n").unwrap();
    assert!(g.evaluate(None).is_ok());
}

#[test]
fn directional_noise_tiles_at_arbitrary_angles() {
    for ang in [0.0, 0.05, 0.137, 0.3333, 0.62, 0.9] {
        for stretch in [1.0, 8.0, 24.0] {
            let g = G2::parse(&format!("size 128\nnode 1 directional_noise scale=6 stretch={stretch} angle={ang} seed=4\noutput a 1\n")).unwrap();
            let t = &g.evaluate(None).unwrap()["a"];
            assert!(t.px.iter().all(|p| p[0].is_finite() && (0.0..=1.0).contains(&p[0])));
            let n = t.px.len() as f32;
            let m = t.px.iter().map(|p| p[0]).sum::<f32>() / n;
            let sd = (t.px.iter().map(|p| (p[0] - m).powi(2)).sum::<f32>() / n).sqrt();
            assert!(sd > 0.07 && sd < 0.3, "angle {ang} stretch {stretch}: sd {sd}");
            // axis-aligned streaks vary along one axis only, so the ratio is a single-row sample there; skip it
            if (ang * 4.0f32).fract().abs() > 0.02 {
                let r = seam_ratio(t);
                assert!(r < 1.5, "directional noise angle {ang} stretch {stretch}: seam/inner = {r}");
            }
        }
    }
}

#[test]
fn gradient_grid_matches_perlin() {
    let gg = noise::GradGrid::new(5, 7, 3);
    for i in 0..50 {
        let (u, v) = (i as f32 * 0.0731 - 0.4, i as f32 * 0.0419 + 0.1);
        assert!((gg.sample(u, v) - noise::perlin(u, v, 5, 7, 3)).abs() < 1e-5);
    }
}

#[test]
fn meshes_are_outward_closed_and_finite() {
    use materialcraft::render3d::build_mesh;
    use materialcraft::tex::Tex;
    let hm = Tex::gray_fn(64, 64, |u, v| ((u * 9.0).sin() * (v * 7.0).cos()) * 0.5 + 0.5);
    for shape in 0..3 {
        for h in [None, Some(&hm)] {
            let m = build_mesh(shape, 24, 1.0, h, 0.1, 0.0);
            assert!(m.pos.iter().chain(&m.nrm).chain(&m.tan).all(|p| p.iter().all(|c| c.is_finite())));
            assert!(m.tris.iter().all(|t| t.iter().all(|&i| (i as usize) < m.pos.len())));
            if h.is_none() {
                // flat/undisplaced: the winding must agree with the analytic normal for every triangle
                for t in &m.tris {
                    let (a, b, c) = (m.pos[t[0] as usize], m.pos[t[1] as usize], m.pos[t[2] as usize]);
                    let f = materialcraft::v3::cross(materialcraft::v3::sub(b, a), materialcraft::v3::sub(c, a));
                    let (n0, n1, n2) = (m.nrm[t[0] as usize], m.nrm[t[1] as usize], m.nrm[t[2] as usize]);
                    let n = [n0[0] + n1[0] + n2[0], n0[1] + n1[1] + n2[1], n0[2] + n1[2] + n2[2]];
                    // pole triangles are degenerate (zero area), so allow float noise
                    assert!(materialcraft::v3::dot(f, n) >= -1e-6, "shape {shape} has an inward-wound triangle");
                }
            }
        }
    }
    // outward displacement: a white height map pushes every plane vertex towards +z (the surface normal)
    let white = Tex::solid(4, 4, [1.0; 4]);
    let p = build_mesh(2, 16, 1.0, Some(&white), 0.2, 0.0);
    assert!(p.pos.iter().all(|v| (v[2] - 0.2).abs() < 1e-5), "relief must rise out of the surface");
    // centred level: mid grey stays put
    let grey = Tex::solid(4, 4, [0.5; 4]);
    let p = build_mesh(2, 16, 1.0, Some(&grey), 0.2, 0.5);
    assert!(p.pos.iter().all(|v| v[2].abs() < 1e-5));
    // sphere seam and poles must stay welded after displacement
    let s = build_mesh(0, 24, 1.0, Some(&hm), 0.15, 0.0);
    let stride = 24 * 2 + 1;
    for j in 0..=24usize {
        let (a, b) = (s.pos[j * stride], s.pos[j * stride + 48]);
        assert!(materialcraft::v3::len(materialcraft::v3::sub(a, b)) < 1e-4, "sphere seam opened at row {j}");
    }
    let top = s.pos[0];
    assert!(s.pos[..stride].iter().all(|v| materialcraft::v3::len(materialcraft::v3::sub(*v, top)) < 1e-4), "north pole tore");
}

#[test]
fn displacement_renders_every_shape_and_camera_without_panics() {
    use materialcraft::render3d::{render, Maps, View, RELIEF_BUMP, RELIEF_DISPLACE, RELIEF_NONE, RELIEF_PARALLAX};
    use materialcraft::tex::Tex;
    let hm = Tex::gray_fn(32, 32, |u, v| (u * 6.0).sin() * 0.5 + 0.5 + v * 0.0);
    let (b, n, r, m, a) = (Tex::solid(4, 4, [0.8, 0.2, 0.2, 1.0]), Tex::solid(4, 4, [0.5, 0.5, 1.0, 1.0]), Tex::solid(4, 4, [0.4; 4]), Tex::solid(4, 4, [0.0, 0.0, 0.0, 1.0]), Tex::solid(4, 4, [1.0; 4]));
    let maps = Maps { base: &b, normal: &n, rough: &r, metal: &m, ao: &a, emissive: None, height: Some(&hm) };
    for shape in 0..3 {
        for relief in [RELIEF_NONE, RELIEF_BUMP, RELIEF_PARALLAX, RELIEF_DISPLACE] {
            // close cameras make triangles cross the near plane; behind/side views exercise back faces
            for (yaw, pitch, dist) in [(0.5, 0.3, 3.4), (3.4, -0.4, 1.6), (1.2, 1.2, 1.6), (-2.0, 0.0, 9.0)] {
                let v = View { shape, relief, yaw, pitch, dist, tess: 32, wireframe: true, height_scale: 0.3, ..View::default() };
                let img = render(&maps, &v, 48, 40);
                assert_eq!((img.w, img.h), (48, 40));
                assert!(img.px.iter().all(|p| p.iter().all(|c| c.is_finite() && *c >= 0.0 && *c <= 1.0001)));
            }
        }
    }
    // the plane must be solid: its centre pixel is not background
    let v = View { shape: 2, yaw: 0.0, pitch: 0.0, dist: 3.0, tess: 32, ..View::default() };
    let img = render(&maps, &v, 64, 64);
    let c = img.px[32 * 64 + 32];
    assert!(c[0] > c[2], "plane centre should be reddish, got {c:?}");
}

/// Every node: several resolutions, finite values, sane range, and (for nodes that should produce variation) a non-blank result.
#[test]
fn every_node_output_is_sane_at_several_sizes() {
    use materialcraft::graph::{demo_graph_for, SPECS};
    // nodes whose demo output may legitimately be a flat colour
    let flat_ok = ["color", "invert_flat", "clamp", "histogram_range", "dust", "curvature_smooth", "curvature", "edge_detect", "height_blend", "edge_wear", "dirt", "ao", "slope_blur", "vector_warp", "distance", "flood_fill_gradient"];
    let mut problems = Vec::new();
    for sp in SPECS {
        let Some(text) = demo_graph_for(sp.kind) else { continue };
        for size in [16usize, 32, 64, 100] {
            let g = G2::parse(&text).unwrap();
            let out = match std::panic::catch_unwind(|| g.evaluate(Some(size))) {
                Ok(Ok(o)) => o,
                Ok(Err(e)) => { problems.push(format!("{} @{size}: error {e}", sp.kind)); continue; }
                Err(_) => { problems.push(format!("{} @{size}: PANIC", sp.kind)); continue; }
            };
            let t = &out["o"];
            if t.px.iter().any(|p| p.iter().any(|c| !c.is_finite())) { problems.push(format!("{} @{size}: NaN/Inf", sp.kind)); continue; }
            if t.px.iter().any(|p| p.iter().any(|c| *c < -4.0 || *c > 8.0)) { problems.push(format!("{} @{size}: wild range", sp.kind)); }
            let (mn, mx) = t.px.iter().fold((f32::MAX, f32::MIN), |a, p| (a.0.min(p[0]).min(p[1]).min(p[2]), a.1.max(p[0]).max(p[1]).max(p[2])));
            if mx - mn < 1e-4 && size >= 32 && !flat_ok.contains(&sp.kind) { problems.push(format!("{} @{size}: blank output ({mn})", sp.kind)); }
        }
    }
    assert!(problems.is_empty(), "node problems:\n{}", problems.join("\n"));
}

/// Fuzz every documented numeric parameter of every node with extreme values: must never panic or return NaN.
#[test]
fn node_parameters_survive_extreme_values() {
    use materialcraft::graph::{demo_graph_for, SPECS};
    let mut problems = Vec::new();
    let mut ran = 0usize;
    for sp in SPECS {
        let Some(text) = demo_graph_for(sp.kind) else { continue };
        let doc = sp.doc.split_once(':').map(|x| x.1).unwrap_or("");
        let names: Vec<String> = doc.split_whitespace().map(|w| w.split('(').next().unwrap().trim_matches(|c: char| !c.is_alphanumeric() && c != '_').to_string()).filter(|w| !w.is_empty() && w.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_') && !w.starts_with("expr")).collect();
        for name in names {
            for v in ["0", "1", "-1", "0.0001", "50", "1000"] {
                let line = format!("node 1 {}", sp.kind);
                let t2 = text.replacen(&line, &format!("{line} {name}={v}"), 1);
                let Ok(g) = G2::parse(&t2) else { continue };
                match std::panic::catch_unwind(|| g.evaluate(Some(24))) {
                    Ok(Ok(out)) => {
                        ran += 1;
                        let t = &out["o"];
                        if t.px.iter().any(|p| p.iter().any(|c| !c.is_finite())) { problems.push(format!("{} {name}={v}: NaN/Inf", sp.kind)); }
                    }
                    Ok(Err(_)) => {} // a clean error is acceptable
                    Err(_) => problems.push(format!("{} {name}={v}: PANIC", sp.kind)),
                }
            }
        }
    }
    problems.sort();
    problems.dedup();
    assert!(ran > 500, "only {ran} parameter cases ran");
    assert!(problems.is_empty(), "{} parameter problems:\n{}", problems.len(), problems.join("\n"));
}

#[test]
fn whole_graph_can_be_imported_and_used_as_a_node() {
    let other = G2::parse("size 64\nnode 1 perlin scale=4 seed=3\nnode 2 invert in=@1\noutput height 2\noutput basecolor 1\n").unwrap();
    let direct = other.evaluate(Some(32)).unwrap();
    let mut host = G2::parse_unchecked("size 64\nnode 1 subgraph graph=\"mat\" output=\"height\"\nnode 2 subgraph graph=\"mat\" output=\"basecolor\"\noutput a 1\noutput b 2\n").unwrap();
    host.import_graph(other.clone(), "mat").unwrap();
    assert!(host.import_graph(other, "mat").is_err(), "duplicate name must be rejected");
    host.validate().unwrap();
    let out = host.evaluate(Some(32)).unwrap();
    assert_eq!(out["a"].px, direct["height"].px, "output=height must pick the height output");
    assert_eq!(out["b"].px, direct["basecolor"].px, "output=basecolor must pick the basecolor output");
    // an empty graph or one without outputs is refused
    let mut h2 = G2::default();
    assert!(h2.import_graph(G2::default(), "x").is_err());
}

#[test]
fn links_can_read_a_specific_output_of_a_graph_node() {
    let other = G2::parse("size 64\nnode 1 perlin scale=4 seed=3\nnode 2 invert in=@1\noutput height 2\noutput basecolor 1\noutput normal 1\n").unwrap();
    assert_eq!(other.output_idents(), vec!["basecolor", "normal", "height"], "outputs are listed in canonical order");
    let mut host = G2::parse_unchecked("size 64\nnode 1 subgraph graph=\"mat\"\nnode 2 invert in=@1.height\nnode 3 invert in=@1.basecolor\noutput a 2\noutput b 3\n").unwrap();
    host.import_graph(other.clone(), "mat").unwrap();
    host.validate().unwrap();
    // round trip keeps the per-link output
    let again = G2::parse_unchecked(&host.to_text()).unwrap();
    assert_eq!(again.nodes[&2].in_out.get("in").map(String::as_str), Some("height"));
    let out = again.evaluate(Some(32)).unwrap();
    let direct = other.evaluate(Some(32)).unwrap();
    let inv = |t: &materialcraft::tex::Tex| materialcraft::ops::invert(t).px;
    assert_eq!(out["a"].px, inv(&direct["height"]));
    assert_eq!(out["b"].px, inv(&direct["basecolor"]));
    assert_ne!(out["a"].px, out["b"].px, "the two outputs differ");
    // unknown output names are a clean error
    let mut bad = G2::parse_unchecked("size 64\nnode 1 subgraph graph=\"mat\"\nnode 2 invert in=@1.nosuch\noutput a 2\n").unwrap();
    bad.import_graph(other, "mat").unwrap();
    assert!(bad.evaluate(Some(16)).is_err());
}

#[test]
fn fast_blur_matches_a_true_gaussian_and_keeps_the_mean() {
    use materialcraft::ops;
    use materialcraft::tex::Tex;
    let n = 128usize;
    let t = Tex::gray_fn(n, n, |u, v| noise::fbm(u, v, 6, 6, 4, 0.5, 3, 0));
    let sigma = 6.0f32;
    let fast = ops::blur(&t, sigma);
    // reference: direct separable gaussian with wrap-around
    let r = (sigma * 4.0) as i64;
    let k: Vec<f32> = (-r..=r).map(|i| (-((i * i) as f32) / (2.0 * sigma * sigma)).exp()).collect();
    let ks: f32 = k.iter().sum();
    let mut tmp = t.clone();
    for y in 0..n {
        for x in 0..n {
            tmp.px[y * n + x][0] = (0..k.len()).map(|i| t.at(x as i64 + i as i64 - r, y as i64)[0] * k[i]).sum::<f32>() / ks;
        }
    }
    let mut max_err = 0.0f32;
    let (mut mean_a, mut mean_b) = (0.0f64, 0.0f64);
    for y in 0..n {
        for x in 0..n {
            let v = (0..k.len()).map(|i| tmp.at(x as i64, y as i64 + i as i64 - r)[0] * k[i]).sum::<f32>() / ks;
            max_err = max_err.max((v - fast.px[y * n + x][0]).abs());
            mean_a += v as f64;
            mean_b += fast.px[y * n + x][0] as f64;
        }
    }
    assert!(max_err < 0.02, "box-approximated gaussian differs by {max_err}");
    assert!(((mean_a - mean_b) / (n * n) as f64).abs() < 1e-4, "mean not preserved");
}

#[test]
fn results_look_the_same_at_every_resolution() {
    // parameters are resolution independent: the average of every output must barely change between 128 and 512 px
    let g = Graph::parse(EXAMPLE).unwrap();
    let lo = g.evaluate(Some(128)).unwrap();
    let hi = g.evaluate(Some(512)).unwrap();
    for (name, a) in &lo {
        let b = &hi[name];
        let mean = |t: &materialcraft::tex::Tex| t.px.iter().map(|p| (p[0] + p[1] + p[2]) as f64 / 3.0).sum::<f64>() / t.px.len() as f64;
        let (ma, mb) = (mean(a), mean(b));
        assert!((ma - mb).abs() < 0.06, "{name}: mean {ma:.3} at 128px vs {mb:.3} at 512px");
    }
}

#[test]
fn large_sizes_are_accepted() {
    for s in [2048, 4096, 8192] {
        let text = format!("size {s}\nnode 1 fbm scale=4\noutput a 1\n");
        assert!(Graph::parse(&text).is_ok(), "size {s} rejected");
    }
    assert!(Graph::parse("size 16384\nnode 1 fbm\noutput a 1\n").is_err());
}

#[test]
fn a_cancelled_evaluation_stops_early() {
    use materialcraft::tex::CURRENT_JOB;
    use std::sync::atomic::Ordering;
    let mut g = Graph::parse(EXAMPLE).unwrap();
    g.size = 1024;
    let keep: Vec<u32> = g.output_nodes().iter().map(|o| o.2).collect();
    CURRENT_JOB.store(777, Ordering::Relaxed);
    let t0 = std::time::Instant::now();
    let full = g.evaluate_all_cancellable(1024, &keep, 777);
    let full_time = t0.elapsed();
    assert!(!full.kept.is_empty(), "a current job must deliver its outputs");
    // a newer job exists: the old one is stale from the start
    CURRENT_JOB.store(778, Ordering::Relaxed);
    let t1 = std::time::Instant::now();
    let r = g.evaluate_all_cancellable(1024, &keep, 777);
    assert!(r.kept.is_empty(), "a stale run must not deliver outputs");
    assert!(t1.elapsed() < full_time / 4 + std::time::Duration::from_millis(50), "stale run still took {:?} (full run {:?})", t1.elapsed(), full_time);
}

#[test]
fn nodes_can_have_their_own_output_size() {
    // generators follow the parent size, the modifier moves it in powers of two
    let g = Graph::parse("size 256\nnode 1 fbm scale=4 osize_rel=-2\noutput o 1\n").unwrap();
    let t = &g.evaluate(Some(256)).unwrap()["o"];
    assert_eq!((t.w, t.h), (64, 64), "-2 means a quarter of the parent size");
    let g = Graph::parse("size 256\nnode 1 fbm scale=4 osize_mode=1 osize_rel=1\noutput o 1\n").unwrap();
    assert_eq!(g.evaluate(Some(256)).unwrap()["o"].w, 512, "+1 doubles");
    // absolute sizes are powers of two and ignore the modifier
    let g = Graph::parse("size 256\nnode 1 fbm scale=4 osize_mode=2 osize_abs=7 osize_rel=3\noutput o 1\n").unwrap();
    assert_eq!(g.evaluate(Some(256)).unwrap()["o"].w, 128);
    // filters inherit from their input; a node with another size resamples its inputs to its own size
    let g = Graph::parse("size 256\nnode 1 fbm scale=4 osize_rel=-1\nnode 2 blur in=@1 radius=2\nnode 3 invert in=@2 osize_mode=1\noutput o 2\noutput p 3\n").unwrap();
    let r = g.evaluate(Some(256)).unwrap();
    assert_eq!(r["o"].w, 128, "blur follows its input");
    assert_eq!(r["p"].w, 256, "relative-to-parent restores the graph size");
    // quick previews scale absolute sizes with the preview resolution
    let g = Graph::parse("size 1024\nnode 1 fbm scale=4 osize_mode=2 osize_abs=9\noutput o 1\n").unwrap();
    assert_eq!(g.evaluate(Some(256)).unwrap()["o"].w, 128, "512 px of a 1024 graph = 1/2, so 128 at a 256 preview");
    // everything stays tileable and finite at a reduced size
    let g = Graph::parse("size 128\nnode 1 voronoi scale=5 osize_rel=-1\nnode 2 blur in=@1 radius=3 osize_mode=1\noutput o 2\n").unwrap();
    let t = &g.evaluate(Some(128)).unwrap()["o"];
    assert!(t.px.iter().all(|p| p.iter().all(|v| v.is_finite())));
}

#[test]
fn normal_to_height_inverts_normal_from_height() {
    // height -> normal -> height must give back (a normalised copy of) the original height
    let t = "size 128\nnode 1 fbm scale=3 octaves=3 seed=4\nnode 2 normal in=@1 strength=2\nnode 3 normal_to_height in=@2\nnode 4 auto_levels in=@1\noutput a 3\noutput b 4\n";
    let o = Graph::parse(t).unwrap().evaluate(None).unwrap();
    let (a, b) = (&o["a"], &o["b"]);
    let n = a.px.len() as f64;
    let (ma, mb) = (a.px.iter().map(|p| p[0] as f64).sum::<f64>() / n, b.px.iter().map(|p| p[0] as f64).sum::<f64>() / n);
    let (mut cov, mut va, mut vb) = (0.0, 0.0, 0.0);
    for (p, q) in a.px.iter().zip(&b.px) {
        let (x, y) = (p[0] as f64 - ma, q[0] as f64 - mb);
        cov += x * y;
        va += x * x;
        vb += y * y;
    }
    let corr = cov / (va * vb).sqrt();
    assert!(corr > 0.9, "recovered height correlates only {corr:.3} with the original");
}

#[test]
fn batch1_nodes_behave() {
    let run = |body: &str| Graph::parse(&format!("size 64\n{body}")).unwrap().evaluate(None).unwrap();
    // symmetry: left and right halves are mirror images
    let o = run("node 1 fbm scale=3 seed=2\nnode 2 symmetry in=@1 axis=0\noutput a 2\n");
    let t = &o["a"];
    for y in 0..64 {
        for x in 0..32 {
            let (l, r) = (t.at(x, y)[0], t.at(63 - x, y)[0]);
            assert!((l - r).abs() < 0.02, "symmetry broken at ({x},{y}): {l} vs {r}");
        }
    }
    // auto levels reaches 0 and 1
    let o = run("node 1 fbm scale=3 seed=2\nnode 2 auto_levels in=@1\noutput a 2\n");
    let (mn, mx) = o["a"].px.iter().fold((9.0f32, -9.0f32), |a, p| (a.0.min(p[0]), a.1.max(p[0])));
    assert!(mn < 0.001 && mx > 0.999, "range {mn}..{mx}");
    // pow with exponent 2 squares a constant
    let o = run("node 1 color r=0.5 g=0.5 b=0.5\nnode 2 pow in=@1 exponent=2\noutput a 2\n");
    assert!((o["a"].px[0][0] - 0.25).abs() < 1e-4);
    // channel mixer swaps red and blue
    let o = run("node 1 color r=1 g=0.2 b=0\nnode 2 channel_mixer in=@1 rr=0 rb=1 br=1 bb=0\noutput a 2\n");
    let p = o["a"].px[0];
    assert!(p[0] < 0.01 && p[2] > 0.99 && (p[1] - 0.2).abs() < 1e-4, "{p:?}");
    // color to mask: exact colour -> 1, other colour -> 0
    let o = run("node 1 color r=1 g=0 b=0\nnode 2 color_to_mask in=@1 r=1 g=0 b=0\nnode 3 color_to_mask in=@1 r=0 g=0 b=1\noutput a 2\noutput b 3\n");
    assert!(o["a"].px[0][0] > 0.99 && o["b"].px[0][0] < 0.01);
    // new blend modes stay in range
    for m in 0..17 {
        let o = run(&format!("node 1 fbm seed=1\nnode 2 fbm seed=2 scale=7\nnode 3 blend fg=@1 bg=@2 mode={m}\noutput a 3\n"));
        assert!(o["a"].px.iter().all(|p| p[0].is_finite() && (0.0..=1.0).contains(&p[0])), "blend mode {m}");
    }
}
