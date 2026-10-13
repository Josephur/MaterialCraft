// ======================================================================= Node properties (included into gui.rs)

#[derive(Clone, Copy)]
enum PK {
    F { def: f32, lo: f32, hi: f32 },
    I { def: i32, lo: i32, hi: i32 },
    E { def: i32, opts: &'static [&'static str] },
    B { def: bool },
}

#[derive(Clone, Copy)]
struct PD {
    name: &'static str,
    label: &'static str,
    k: PK,
}

fn pf(name: &'static str, label: &'static str, def: f32, lo: f32, hi: f32) -> PD {
    PD { name, label, k: PK::F { def, lo, hi } }
}
fn pi(name: &'static str, label: &'static str, def: i32, lo: i32, hi: i32) -> PD {
    PD { name, label, k: PK::I { def, lo, hi } }
}
fn pe(name: &'static str, label: &'static str, def: i32, opts: &'static [&'static str]) -> PD {
    PD { name, label, k: PK::E { def, opts } }
}
fn pb(name: &'static str, label: &'static str, def: bool) -> PD {
    PD { name, label, k: PK::B { def } }
}

const BLEND_MODES: &[&str] = &["Copy", "Add", "Subtract", "Multiply", "Screen", "Overlay", "Max", "Min", "Difference", "Soft light"];
const SHAPES: &[&str] = &["Square", "Disc", "Paraboloid", "Gaussian", "Diamond", "Pyramid", "Half sphere", "Cone"];

fn p_seed() -> PD {
    pi("seed", "Random seed", 0, 0, 9999)
}
fn p_con() -> PD {
    pf("contrast", "Contrast", 1.0, 0.0, 4.0)
}
fn p_inv() -> PD {
    pb("invert", "Invert", false)
}
fn p_dis() -> PD {
    pf("disorder", "Disorder", 0.0, 0.0, 1.0)
}

/// Editable parameters of each node type (defaults match the engine).
fn pdefs(kind: &str) -> Vec<PD> {
    match kind {
        "white_noise" => vec![p_seed()],
        "gaussian_noise" => vec![pi("scale", "Scale (grain)", 1, 1, 16), p_seed(), p_con(), p_inv()],
        "perlin" => vec![pi("scale", "Scale", 4, 1, 64), p_dis(), p_seed(), p_con(), p_inv()],
        "fbm" => vec![pi("scale", "Scale", 4, 1, 64), pi("octaves", "Octaves", 5, 1, 10), pf("gain", "Gain", 0.5, 0.0, 1.0), p_dis(), p_seed(), pe("mode", "Mode", 0, &["fBm", "Ridged", "Turbulence"]), p_con(), p_inv()],
        "clouds" => vec![pi("scale", "Scale", 3, 1, 64), pi("octaves", "Octaves", 6, 1, 10), pf("gain", "Gain", 0.6, 0.0, 1.0), p_dis(), p_seed(), p_con(), p_inv()],
        "voronoi" => vec![pi("scale", "Scale", 6, 1, 64), pf("jitter", "Jitter", 1.0, 0.0, 1.0), p_seed(), pe("mode", "Output", 0, &["Distance", "Cell ID", "Edges", "Second distance"])],
        "bnw_spots" => vec![pe("variant", "Type", 0, &["Spots 1", "Spots 2", "Spots 3"]), pi("scale", "Scale", 8, 1, 64), p_dis(), p_seed(), p_con(), p_inv()],
        "crystal" => vec![pe("variant", "Type", 0, &["Crystal 1", "Crystal 2"]), pi("scale", "Scale", 6, 1, 64), p_dis(), p_seed(), p_con(), p_inv()],
        "directional_noise" => vec![pf("scale", "Scale", 6.0, 1.0, 32.0), pf("stretch", "Stretch", 8.0, 1.0, 32.0), pf("angle", "Angle", 0.0, 0.0, 1.0), p_seed(), p_con(), p_inv()],
        "grunge" => vec![pe("variant", "Type", 0, &["Concrete", "Scratches", "Spots", "Smudges"]), p_dis(), p_seed(), p_con(), p_inv()],
        "plasma" => vec![pi("scale", "Scale", 3, 1, 8), p_dis(), p_seed(), p_con(), p_inv()],
        "gradient" => vec![pe("mode", "Type", 0, &["Linear", "Radial"]), pf("angle", "Angle", 0.0, 0.0, 1.0)],
        "gradient_axial" => vec![pf("angle", "Angle", 0.0, 0.0, 1.0)],
        "gradient_circular" => vec![pf("offset", "Offset", 0.0, 0.0, 1.0)],
        "circle" => vec![pf("radius", "Radius", 0.4, 0.0, 0.7), pf("softness", "Edge softness", 0.01, 0.0, 0.5)],
        "rect" => vec![pf("width", "Width", 0.6, 0.0, 1.0), pf("height", "Height", 0.4, 0.0, 1.0), pf("softness", "Edge softness", 0.01, 0.0, 0.5)],
        "polygon" => vec![pi("sides", "Sides", 5, 3, 12), pf("radius", "Radius", 0.4, 0.0, 0.7), pf("softness", "Edge softness", 0.01, 0.0, 0.5), pf("rotation", "Rotation", 0.0, 0.0, 1.0)],
        "shape" => vec![
            pe("shape", "Shape", 1, SHAPES),
            pf("size_x", "Width", 0.6, 0.0, 1.5),
            pf("size_y", "Height", 0.6, 0.0, 1.5),
            pf("rotation", "Rotation", 0.0, 0.0, 1.0),
            pf("pos_x", "Position X", 0.0, -0.5, 0.5),
            pf("pos_y", "Position Y", 0.0, -0.5, 0.5),
            pi("tiling", "Tiling", 1, 1, 16),
            pf("softness", "Edge softness", 0.02, 0.0, 1.0),
        ],
        "star" => vec![pi("points", "Points", 5, 3, 16), pf("inner", "Inner radius", 0.45, 0.0, 1.0), pf("size", "Size", 0.4, 0.0, 0.7), pf("softness", "Edge softness", 0.01, 0.0, 0.5), pf("rotation", "Rotation", 0.0, 0.0, 1.0)],
        "checker" => vec![pi("tiles", "Tiles", 8, 1, 64)],
        "bricks" => vec![
            pi("rows", "Rows", 8, 1, 32),
            pi("cols", "Columns", 4, 1, 32),
            pf("mortar", "Mortar", 0.03, 0.0, 0.2),
            pf("bevel", "Bevel", 0.05, 0.001, 0.5),
            pf("offset", "Row offset", 0.5, 0.0, 1.0),
            pf("lum_var", "Brick variation", 0.3, 0.0, 1.0),
            p_seed(),
        ],
        "stripes" => vec![pi("count", "Count", 8, 1, 64), pf("angle", "Angle", 0.0, 0.0, 1.0), pf("softness", "Edge softness", 0.1, 0.001, 0.5), pf("width", "Stripe width", 0.5, 0.0, 1.0)],
        "weave" => vec![pi("count", "Count", 6, 1, 32), pf("width", "Thread width", 0.8, 0.1, 1.0)],
        "tile_sampler" => vec![
            pi("x_amount", "X amount", 8, 1, 64),
            pi("y_amount", "Y amount", 8, 1, 64),
            pf("size", "Size", 0.8, 0.05, 3.0),
            pf("size_random", "Size random", 0.0, 0.0, 1.0),
            pf("rotation", "Rotation", 0.0, 0.0, 1.0),
            pf("rotation_random", "Rotation random", 0.0, 0.0, 1.0),
            pf("position_random", "Position random", 0.0, 0.0, 1.0),
            pf("color", "Color", 1.0, 0.0, 1.0),
            pf("color_random", "Color random", 0.0, 0.0, 1.0),
            pf("mask_threshold", "Mask map threshold", 0.5, 0.0, 1.0),
            p_seed(),
        ],
        "tile_generator" => vec![
            pi("x_amount", "X amount", 6, 1, 64),
            pi("y_amount", "Y amount", 6, 1, 64),
            pf("size", "Size", 0.92, 0.05, 2.0),
            pf("offset", "Row offset", 0.5, 0.0, 1.0),
            pf("rotation", "Rotation", 0.0, 0.0, 1.0),
            pf("rotation_random", "Rotation random", 0.0, 0.0, 1.0),
            pf("size_random", "Size random", 0.0, 0.0, 1.0),
            pf("position_random", "Position random", 0.0, 0.0, 1.0),
            pf("color", "Color", 1.0, 0.0, 1.0),
            pf("color_random", "Color random", 0.0, 0.0, 1.0),
            pf("mask_threshold", "Mask map threshold", 0.5, 0.0, 1.0),
            p_seed(),
        ],
        "splatter" => vec![
            pi("count", "Count", 40, 0, 500),
            pf("scale", "Size", 0.1, 0.01, 0.5),
            pf("scale_var", "Size random", 0.3, 0.0, 1.0),
            pf("rot_var", "Rotation random", 1.0, 0.0, 1.0),
            pf("lum_var", "Color random", 0.0, 0.0, 1.0),
            p_seed(),
        ],
        "splatter_circular" => vec![
            pi("number", "Number", 12, 1, 256),
            pf("radius", "Radius", 0.3, 0.0, 0.5),
            pf("size", "Size", 0.1, 0.005, 0.5),
            pf("size_random", "Size random", 0.0, 0.0, 1.0),
            pf("rotation", "Rotation", 0.0, 0.0, 1.0),
            pf("rotation_random", "Rotation random", 0.0, 0.0, 1.0),
            pf("position_random", "Position random", 0.0, 0.0, 1.0),
            p_seed(),
        ],
        "sdf_shape" => vec![
            pe("shape", "Shape", 0, &["Circle", "Box", "Polygon", "Ring"]),
            pf("size", "Size", 0.25, 0.0, 0.5),
            pf("aspect", "Aspect", 1.0, 0.1, 3.0),
            pi("sides", "Sides", 5, 3, 12),
            pf("rotation", "Rotation", 0.0, 0.0, 1.0),
            pf("round", "Corner rounding", 0.0, 0.0, 0.3),
            pf("x", "Position X", 0.0, -0.5, 0.5),
            pf("y", "Position Y", 0.0, -0.5, 0.5),
            pf("range", "Distance range", 0.25, 0.02, 1.0),
        ],
        "sdf_op" => vec![pe("op", "Operation", 0, &["Union", "Subtract", "Intersect"]), pf("smooth", "Smoothing", 0.0, 0.0, 0.3), pf("range", "Distance range", 0.25, 0.02, 1.0)],
        "sdf_mask" => vec![pf("offset", "Offset", 0.0, -0.5, 0.5), pf("softness", "Edge softness", 0.02, 0.0, 0.5)],
        "sdf_form" => vec![
            pe("shape", "Shape", 0, &["Sphere", "Box", "Torus", "Cylinder"]),
            pf("size", "Size", 0.3, 0.05, 0.5),
            pi("repeat", "Repeat", 3, 1, 16),
            pf("round", "Rounding", 0.05, 0.0, 0.3),
            pf("scatter", "Size variation", 0.3, 0.0, 1.0),
            pf("height", "Height", 1.5, 0.1, 4.0),
            p_seed(),
        ],
        "input" => vec![pi("idx", "Input index", 0, 0, 3), pb("gray", "Grayscale", false)],
        "switch" => vec![pi("index", "Index", 0, 0, 3)],
        "blur" => vec![pf("radius", "Intensity", 4.0, 0.0, 64.0)],
        "blur_hq" => vec![pf("intensity", "Intensity", 6.0, 0.0, 64.0), pi("quality", "Quality", 1, 1, 4)],
        "directional_blur" => vec![pf("angle", "Angle", 0.0, 0.0, 1.0), pf("length", "Length", 0.05, 0.0, 0.3)],
        "sharpen" => vec![pf("radius", "Radius", 2.0, 0.0, 32.0), pf("amount", "Amount", 1.0, 0.0, 5.0)],
        "levels" => vec![
            pb("auto", "Auto levels", false),
            pf("low", "Input low", 0.0, 0.0, 1.0),
            pf("high", "Input high", 1.0, 0.0, 1.0),
            pf("gamma", "Gamma", 1.0, 0.1, 4.0),
            pf("out_low", "Output low", 0.0, 0.0, 1.0),
            pf("out_high", "Output high", 1.0, 0.0, 1.0),
        ],
        "adjust" => vec![pf("brightness", "Brightness", 0.0, -1.0, 1.0), pf("contrast", "Contrast", 1.0, 0.0, 3.0)],
        "threshold" => vec![pf("threshold", "Threshold", 0.5, 0.0, 1.0), pf("softness", "Edge softness", 0.0, 0.0, 1.0)],
        "histogram_scan" => vec![pf("position", "Position", 0.5, 0.0, 1.0), pf("contrast", "Contrast", 0.5, 0.0, 1.0), pb("invert", "Invert", false)],
        "histogram_range" => vec![pf("position", "Position", 0.5, 0.0, 1.0), pf("range", "Range", 0.2, 0.0, 1.0), pf("contrast", "Contrast", 0.5, 0.0, 1.0), pb("invert", "Invert", false)],
        "quantize" => vec![pi("steps", "Steps", 4, 2, 32)],
        "channel" => vec![pe("ch", "Channel", 0, &["Red", "Green", "Blue", "Alpha"])],
        "tint" => vec![pf("r", "Red", 1.0, 0.0, 1.0), pf("g", "Green", 1.0, 0.0, 1.0), pf("b", "Blue", 1.0, 0.0, 1.0)],
        "colorize" => vec![pe("interp", "Interpolation", 0, &["Linear", "Smooth", "Step"])],
        "hsl" => vec![pf("hue", "Hue", 0.0, -1.0, 1.0), pf("sat", "Saturation", 1.0, 0.0, 2.0), pf("val", "Lightness", 1.0, 0.0, 2.0)],
        "transform" => vec![
            pf("offset_x", "Offset X", 0.0, -1.0, 1.0),
            pf("offset_y", "Offset Y", 0.0, -1.0, 1.0),
            pf("scale_x", "Scale X", 1.0, 0.1, 4.0),
            pf("scale_y", "Scale Y", 1.0, 0.1, 4.0),
            pf("rotation", "Rotation", 0.0, 0.0, 1.0),
            pe("tiling", "Tiling mode", 0, &["H and V tiling", "No tiling"]),
        ],
        "mirror" => vec![pe("axis", "Axis", 0, &["Horizontal", "Vertical", "Both"])],
        "warp" => vec![pf("intensity", "Intensity", 0.05, 0.0, 0.5)],
        "directional_warp" => vec![pf("angle", "Angle", 0.0, 0.0, 1.0), pf("intensity", "Intensity", 0.1, 0.0, 0.5)],
        "vector_warp" => vec![pf("intensity", "Intensity", 0.05, 0.0, 0.5)],
        "slope_blur" => vec![pi("samples", "Samples", 16, 1, 64), pf("intensity", "Intensity", 0.1, 0.0, 1.0), pe("mode", "Mode", 0, &["Blur", "Min", "Max"])],
        "swirl" => vec![pf("amount", "Amount", 0.3, -2.0, 2.0), pf("radius", "Radius", 0.5, 0.0, 1.0)],
        "highpass" => vec![pf("radius", "Radius", 6.0, 0.0, 64.0)],
        "clamp" => vec![pf("low", "Low", 0.0, 0.0, 1.0), pf("high", "High", 1.0, 0.0, 1.0)],
        "blend" => vec![pe("mode", "Blending mode", 0, BLEND_MODES), pf("opacity", "Opacity", 1.0, 0.0, 1.0)],
        "normal" => vec![pf("strength", "Intensity", 2.0, 0.0, 10.0), pb("directx", "DirectX format", false)],
        "ao" => vec![pf("radius", "Spread", 6.0, 0.0, 64.0), pf("intensity", "Height depth", 2.0, 0.0, 10.0)],
        "curvature" => vec![pf("radius", "Radius", 3.0, 0.0, 64.0), pf("intensity", "Intensity", 4.0, 0.0, 10.0)],
        "curvature_smooth" => vec![pf("smoothing", "Smoothing", 2.0, 0.0, 32.0), pf("intensity", "Intensity", 6.0, 0.0, 20.0)],
        "edge_detect" => vec![pf("intensity", "Intensity", 2.0, 0.0, 10.0)],
        "emboss" => vec![pf("angle", "Angle", 0.125, 0.0, 1.0), pf("intensity", "Intensity", 4.0, 0.0, 10.0)],
        "flood_fill" => vec![pf("threshold", "Threshold", 0.05, 0.0, 1.0)],
        "flood_fill_random" => vec![pf("min", "Minimum", 0.0, 0.0, 1.0), pf("max", "Maximum", 1.0, 0.0, 1.0), p_seed()],
        "flood_fill_color" => vec![p_seed()],
        "flood_fill_gradient" => vec![pf("angle", "Angle", 0.0, 0.0, 1.0), pf("variation", "Angle variation", 0.0, 0.0, 1.0), p_seed()],
        "bevel" => vec![
            pf("distance", "Distance", 0.05, 0.0, 0.5),
            pf("smoothing", "Smoothing", 0.5, 0.0, 1.0),
            pf("threshold", "Shape threshold", 0.5, 0.0, 1.0),
            pb("use_as_normal", "Output as normal", false),
            pf("normal_intensity", "Normal intensity", 4.0, 0.0, 16.0),
        ],
        "distance" => vec![pf("distance", "Distance range", 0.1, 0.0, 1.0), pf("threshold", "Shape threshold", 0.5, 0.0, 1.0), pe("mode", "Mode", 0, &["Inside", "Outside", "Both"])],
        "color_match" => vec![pf("strength", "Strength", 1.0, 0.0, 1.0)],
        "height_blend" => vec![pf("opacity", "Opacity", 1.0, 0.0, 1.0), pf("contrast", "Height transition", 0.5, 0.0, 2.0), pe("output", "Output", 0, &["Height", "Blend mask"])],
        "make_it_tile_photo" => vec![pf("width", "Blend width", 0.6, 0.1, 1.0)],
        "make_it_tile_patch" => vec![pi("patches", "Patches", 6, 2, 32), pf("softness", "Softness", 0.5, 0.0, 1.0), p_seed()],
        "edge_wear" => vec![pf("amount", "Amount", 0.5, 0.0, 1.0), pf("width", "Edge width", 2.0, 0.0, 16.0), pf("contrast", "Contrast", 1.0, 0.1, 4.0)],
        "dirt" | "dust" => vec![pf("amount", "Amount", 0.5, 0.0, 1.0), pf("contrast", "Contrast", 1.0, 0.1, 4.0)],
        _ => vec![],
    }
}

// ---------------------------------------------------------------- colour wheel
fn hsv_color32(h: f32, s: f32, v: f32) -> Color32 {
    let (r, g, b) = ops::hsv_to_rgb(h, s, v);
    Color32::from_rgb((r * 255.0) as u8, (g * 255.0) as u8, (b * 255.0) as u8)
}

/// Hue ring + saturation/value square. Returns true when the colour changed.
fn color_wheel(ui: &mut egui::Ui, id: Id, rgb: &mut [f32; 3]) -> bool {
    let size = 164.0;
    let (rect, resp) = ui.allocate_exact_size(vec2(size, size), Sense::click_and_drag());
    let c = rect.center();
    let (r_out, r_in) = (size * 0.5 - 2.0, size * 0.5 - 18.0);
    let (mut h, mut sv, mut vv) = ops::rgb_to_hsv(rgb[0], rgb[1], rgb[2]);
    let hue_id = id.with("hue");
    if sv < 0.01 || vv < 0.01 {
        if let Some(hs) = ui.data(|d| d.get_temp::<f32>(hue_id)) {
            h = hs;
        }
    }
    let half = r_in * 0.68;
    let sq = Rect::from_center_size(c, vec2(half * 2.0, half * 2.0));
    let mode_id = id.with("mode");
    let mut changed = false;
    if resp.drag_started() || resp.clicked() {
        if let Some(p) = resp.interact_pointer_pos() {
            let d = (p - c).length();
            let mode = if d > r_in - 4.0 && d < r_out + 8.0 { 1u8 } else if sq.expand(4.0).contains(p) { 2 } else { 0 };
            ui.data_mut(|dd| dd.insert_temp(mode_id, mode));
        }
    }
    if resp.dragged() || resp.clicked() {
        if let Some(p) = resp.interact_pointer_pos() {
            match ui.data(|d| d.get_temp::<u8>(mode_id)).unwrap_or(0) {
                1 => {
                    h = ((p - c).y.atan2((p - c).x) / std::f32::consts::TAU).rem_euclid(1.0);
                    changed = true;
                }
                2 => {
                    sv = ((p.x - sq.min.x) / sq.width()).clamp(0.0, 1.0);
                    vv = 1.0 - ((p.y - sq.min.y) / sq.height()).clamp(0.0, 1.0);
                    changed = true;
                }
                _ => {}
            }
        }
    }
    if changed {
        ui.data_mut(|d| d.insert_temp(hue_id, h));
        let (r, g, b) = ops::hsv_to_rgb(h, sv, vv);
        *rgb = [r, g, b];
    }
    let painter = ui.painter_at(rect.expand(2.0));
    // ring
    let mut mesh = egui::Mesh::default();
    let segs = 120;
    for i in 0..segs {
        let (a0, a1) = (i as f32 / segs as f32 * std::f32::consts::TAU, (i + 1) as f32 / segs as f32 * std::f32::consts::TAU);
        let col = hsv_color32((i as f32 + 0.5) / segs as f32, 1.0, 1.0);
        let base = mesh.vertices.len() as u32;
        for (a, r) in [(a0, r_in), (a0, r_out), (a1, r_out), (a1, r_in)] {
            mesh.colored_vertex(c + vec2(a.cos(), a.sin()) * r, col);
        }
        mesh.add_triangle(base, base + 1, base + 2);
        mesh.add_triangle(base, base + 2, base + 3);
    }
    painter.add(egui::Shape::mesh(mesh));
    // saturation / value square
    let mut mesh = egui::Mesh::default();
    let n = 12;
    for yi in 0..=n {
        for xi in 0..=n {
            let (s, v) = (xi as f32 / n as f32, 1.0 - yi as f32 / n as f32);
            mesh.colored_vertex(pos2(sq.min.x + s * sq.width(), sq.min.y + (1.0 - v) * sq.height()), hsv_color32(h, s, v));
        }
    }
    for yi in 0..n {
        for xi in 0..n {
            let a = (yi * (n + 1) + xi) as u32;
            let b = a + 1;
            let c2 = a + (n + 1) as u32;
            mesh.add_triangle(a, b, c2);
            mesh.add_triangle(b, b + (n + 1) as u32, c2);
        }
    }
    painter.add(egui::Shape::mesh(mesh));
    // markers
    let ha = h * std::f32::consts::TAU;
    painter.circle(c + vec2(ha.cos(), ha.sin()) * ((r_in + r_out) * 0.5), 5.5, hsv_color32(h, 1.0, 1.0), Stroke::new(2.0, Color32::WHITE));
    painter.circle(pos2(sq.min.x + sv * sq.width(), sq.min.y + (1.0 - vv) * sq.height()), 5.0, Color32::from_rgb((rgb[0] * 255.0) as u8, (rgb[1] * 255.0) as u8, (rgb[2] * 255.0) as u8), Stroke::new(2.0, if vv > 0.55 { Color32::BLACK } else { Color32::WHITE }));
    changed
}

fn rgb_hex(c: [f32; 3]) -> String {
    format!("#{:02x}{:02x}{:02x}", (c[0].clamp(0.0, 1.0) * 255.0 + 0.5) as u8, (c[1].clamp(0.0, 1.0) * 255.0 + 0.5) as u8, (c[2].clamp(0.0, 1.0) * 255.0 + 0.5) as u8)
}

fn parse_hex(s: &str) -> Option<[f32; 3]> {
    let t = s.trim().trim_start_matches('#');
    if t.len() != 6 || !t.is_ascii() {
        return None;
    }
    let p = |i: usize| u8::from_str_radix(&t[i..i + 2], 16).ok().map(|v| v as f32 / 255.0);
    Some([p(0)?, p(2)?, p(4)?])
}

// ---------------------------------------------------------------- gradient editor
fn grad_at(stops: &[(f32, [f32; 3])], t: f32) -> [f32; 3] {
    if stops.is_empty() {
        return [t; 3];
    }
    if t <= stops[0].0 {
        return stops[0].1;
    }
    for i in 1..stops.len() {
        if t <= stops[i].0 {
            let (a, b) = (stops[i - 1], stops[i]);
            let f = (t - a.0) / (b.0 - a.0).max(1e-6);
            return [a.1[0] + (b.1[0] - a.1[0]) * f, a.1[1] + (b.1[1] - a.1[1]) * f, a.1[2] + (b.1[2] - a.1[2]) * f];
        }
    }
    stops[stops.len() - 1].1
}

fn c32(c: [f32; 3]) -> Color32 {
    Color32::from_rgb((c[0].clamp(0.0, 1.0) * 255.0) as u8, (c[1].clamp(0.0, 1.0) * 255.0) as u8, (c[2].clamp(0.0, 1.0) * 255.0) as u8)
}

fn stops_from_node(n: &Node) -> Vec<(f32, [f32; 3])> {
    let mut v: Vec<(f32, [f32; 3])> = (0..16)
        .filter_map(|i| {
            let p = *n.nums.get(&format!("p{i}"))?;
            let g = |c: &str| n.nums.get(&format!("{c}{i}")).copied().unwrap_or(0.0);
            Some((p, [g("r"), g("g"), g("b")]))
        })
        .collect();
    v.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap_or(std::cmp::Ordering::Equal));
    if v.is_empty() {
        v = vec![(0.0, [0.0; 3]), (1.0, [1.0; 3])];
    }
    v
}

fn gradient_editor(ui: &mut egui::Ui, id: Id, stops: &mut Vec<(f32, [f32; 3])>) -> bool {
    let mut changed = false;
    let w = ui.available_width();
    let (area, _) = ui.allocate_exact_size(vec2(w, 46.0), Sense::hover());
    let bar = Rect::from_min_size(area.min + vec2(7.0, 0.0), vec2(area.width() - 14.0, 22.0));
    let painter = ui.painter_at(area);
    let segs = 72;
    for i in 0..segs {
        let t = (i as f32 + 0.5) / segs as f32;
        let r = Rect::from_min_max(pos2(bar.min.x + bar.width() * i as f32 / segs as f32, bar.min.y), pos2(bar.min.x + bar.width() * (i + 1) as f32 / segs as f32 + 1.0, bar.max.y));
        painter.rect_filled(r, 0.0, c32(grad_at(stops, t)));
    }
    painter.rect_stroke(bar, 0.0, Stroke::new(1.0, Color32::from_gray(20)));
    let mut sel = ui.data(|d| d.get_temp::<usize>(id)).unwrap_or(0).min(stops.len() - 1);
    let bar_resp = ui.interact(bar, id.with("bar"), Sense::click());
    let mut grabbed = false;
    for i in 0..stops.len() {
        let x = bar.min.x + stops[i].0 * bar.width();
        let mr = Rect::from_center_size(pos2(x, bar.max.y + 11.0), vec2(14.0, 20.0));
        let r = ui.interact(mr, id.with(("stop", i)), Sense::click_and_drag());
        if r.clicked() || r.drag_started() {
            sel = i;
            grabbed = true;
        }
        if r.dragged() {
            if let Some(p) = r.interact_pointer_pos() {
                let lo = if i > 0 { stops[i - 1].0 + 0.002 } else { 0.0 };
                let hi = if i + 1 < stops.len() { stops[i + 1].0 - 0.002 } else { 1.0 };
                stops[i].0 = ((p.x - bar.min.x) / bar.width()).clamp(lo, hi.max(lo));
                changed = true;
                grabbed = true;
            }
        }
        let edge = if sel == i { Color32::from_rgb(255, 170, 60) } else { Color32::from_gray(200) };
        painter.add(egui::Shape::convex_polygon(vec![pos2(x, bar.max.y + 1.0), pos2(x - 5.0, bar.max.y + 8.0), pos2(x + 5.0, bar.max.y + 8.0)], edge, Stroke::NONE));
        let sq = Rect::from_center_size(pos2(x, bar.max.y + 15.0), vec2(11.0, 9.0));
        painter.rect_filled(sq, 1.0, c32(stops[i].1));
        painter.rect_stroke(sq, 1.0, Stroke::new(1.5, edge));
    }
    if bar_resp.clicked() && !grabbed && stops.len() < 16 {
        if let Some(p) = bar_resp.interact_pointer_pos() {
            let t = ((p.x - bar.min.x) / bar.width()).clamp(0.0, 1.0);
            let col = grad_at(stops, t);
            let at = stops.iter().position(|s| s.0 > t).unwrap_or(stops.len());
            stops.insert(at, (t, col));
            sel = at;
            changed = true;
        }
    }
    sel = sel.min(stops.len() - 1);
    ui.data_mut(|d| d.insert_temp(id, sel));
    ui.add_space(2.0);
    ui.horizontal(|ui| {
        ui.label(format!("Stop {}", sel + 1));
        let mut pos = stops[sel].0;
        if ui.add(egui::DragValue::new(&mut pos).speed(0.005).range(0.0..=1.0).fixed_decimals(3)).changed() {
            let lo = if sel > 0 { stops[sel - 1].0 + 0.002 } else { 0.0 };
            let hi = if sel + 1 < stops.len() { stops[sel + 1].0 - 0.002 } else { 1.0 };
            stops[sel].0 = pos.clamp(lo, hi.max(lo));
            changed = true;
        }
        if ui.add_enabled(stops.len() > 2, egui::Button::new("Remove")).clicked() {
            stops.remove(sel);
            changed = true;
        }
    });
    let mut col = stops[sel].1;
    if color_wheel(ui, id.with("wheel"), &mut col) {
        stops[sel].1 = col;
        changed = true;
    }
    changed
}

// ---------------------------------------------------------------- property panels
fn row_label(ui: &mut egui::Ui, text: &str) {
    ui.allocate_ui_with_layout(vec2(112.0, 18.0), egui::Layout::left_to_right(egui::Align::Center), |ui| {
        ui.label(text);
    });
}

impl App {
    fn inspector(&mut self, ui: &mut egui::Ui) {
        ui.spacing_mut().slider_width = (ui.available_width() - 190.0).clamp(70.0, 220.0);
        if let Some(ci) = self.sel_comment.filter(|c| *c < self.g.comments.len()) {
            ui.strong("Comment frame");
            let mut c = self.g.comments[ci].clone();
            let mut changed = ui.text_edit_singleline(&mut c.text).changed();
            ui.horizontal(|ui| {
                for (i, col) in COMMENT_COLORS.iter().enumerate() {
                    let (r, resp) = ui.allocate_exact_size(vec2(22.0, 18.0), Sense::click());
                    ui.painter().rect_filled(r, Rounding::same(3.0), Color32::from_rgb(col[0], col[1], col[2]));
                    if c.color as usize == i {
                        ui.painter().rect_stroke(r, Rounding::same(3.0), Stroke::new(2.0, Color32::WHITE));
                    }
                    if resp.clicked() {
                        c.color = i as u8;
                        changed = true;
                    }
                }
            });
            if changed {
                if !self.edit_open {
                    self.snap();
                    self.edit_open = true;
                }
                self.g.comments[ci] = c;
            }
            if ui.button("Delete frame").clicked() {
                self.acts.push(Act::Delete);
            }
            return;
        }
        if let Some(id) = self.active.filter(|a| self.sel.contains(a) && self.g.nodes.contains_key(a)) {
            self.node_props(ui, id);
            return;
        }
        if !self.sel.is_empty() {
            ui.label(format!("{} nodes selected", self.sel.len()));
            return;
        }
        self.graph_props(ui);
        self.published_params(ui);
    }

    fn published_params(&mut self, ui: &mut egui::Ui) {
        egui::CollapsingHeader::new(egui::RichText::new("PUBLISHED PARAMETERS").strong()).default_open(false).show(ui, |ui| {
            if self.g.exposed.is_empty() {
                ui.small("Click 'P' next to a parameter to publish it. Published parameters can be set when rendering a .mca package.");
            }
            let mut remove = None;
            let mut changed = false;
            let mut list = self.g.exposed.clone();
            for (i, e) in list.iter_mut().enumerate() {
                ui.horizontal(|ui| {
                    changed |= ui.add(egui::TextEdit::singleline(&mut e.label).desired_width(110.0)).changed();
                    ui.label(format!("#{}.{}", e.node, e.param));
                    changed |= ui.add(egui::DragValue::new(&mut e.min).speed(0.01)).changed();
                    changed |= ui.add(egui::DragValue::new(&mut e.max).speed(0.01)).changed();
                    if ui.small_button("x").clicked() {
                        remove = Some(i);
                    }
                });
            }
            if changed || remove.is_some() {
                if !self.edit_open {
                    self.snap();
                    self.edit_open = true;
                }
                if let Some(i) = remove {
                    list.remove(i);
                }
                self.g.exposed = list;
            }
        });
    }

    fn node_props(&mut self, ui: &mut egui::Ui, id: u32) {
        let node = self.g.nodes[&id].clone();
        let kind = node.kind.clone();
        let title = if kind == "output" { usage_label(node.strs.get("usage").map(String::as_str).unwrap_or("basecolor")) } else { label(&kind) };
        ui.horizontal(|ui| {
            let (r, _) = ui.allocate_exact_size(vec2(6.0, 20.0), Sense::hover());
            ui.painter().rect_filled(r, 1.0, kind_color(&kind));
            ui.label(egui::RichText::new(format!("{title}  #{id}")).size(15.0).strong());
        });
        if let Some(sp) = spec(&kind) {
            ui.small(sp.doc);
        }
        if let Some(e) = self.node_err.get(&id) {
            ui.colored_label(Color32::from_rgb(255, 120, 110), e);
        }
        let mut nums: Vec<(String, f32)> = Vec::new();
        let mut strs: Vec<(String, Option<String>)> = Vec::new();
        let mut expose: Option<(String, f32, f32)> = None;
        let get = |k: &str, d: f32| node.nums.get(k).copied().unwrap_or(d);

        // ---- inputs overview
        let sl = slots(&kind);
        if !sl.is_empty() {
            egui::CollapsingHeader::new(egui::RichText::new("INPUTS").strong()).default_open(true).show(ui, |ui| {
                for (name, req) in &sl {
                    match node.ins.get(*name) {
                        Some(src) => {
                            ui.label(format!("{name}  <-  #{src}"));
                        }
                        None => {
                            ui.colored_label(Color32::from_gray(140), format!("{name}  (not connected{})", if *req { ", required" } else { "" }));
                        }
                    }
                }
            });
        }

        egui::CollapsingHeader::new(egui::RichText::new("PARAMETERS").strong()).default_open(true).show(ui, |ui| {
            // ---- node-specific editors
            match kind.as_str() {
                "color" => {
                    let gray = get("gray", 0.0) > 0.5;
                    ui.horizontal(|ui| {
                        row_label(ui, "Mode");
                        if ui.selectable_label(!gray, "Color").clicked() {
                            nums.push(("gray".into(), 0.0));
                        }
                        if ui.selectable_label(gray, "Grayscale").clicked() {
                            nums.push(("gray".into(), 1.0));
                        }
                    });
                    if gray {
                        let mut v = get("v", 0.5);
                        ui.horizontal(|ui| {
                            row_label(ui, "Value");
                            if ui.add(egui::Slider::new(&mut v, 0.0..=1.0)).changed() {
                                nums.push(("v".into(), v));
                            }
                        });
                    } else {
                        let mut rgb = [get("r", 0.5), get("g", 0.5), get("b", 0.5)];
                        let mut ch = false;
                        ui.horizontal(|ui| {
                            ui.add_space(4.0);
                            ch |= color_wheel(ui, Id::new(("cwheel", id)), &mut rgb);
                            ui.vertical(|ui| {
                                let (r, _) = ui.allocate_exact_size(vec2(54.0, 36.0), Sense::hover());
                                ui.painter().rect_filled(r, 3.0, c32(rgb));
                                ui.painter().rect_stroke(r, 3.0, Stroke::new(1.0, Color32::from_gray(15)));
                                let mut hex = rgb_hex(rgb);
                                if ui.add(egui::TextEdit::singleline(&mut hex).desired_width(60.0)).changed() {
                                    if let Some(c) = parse_hex(&hex) {
                                        rgb = c;
                                        ch = true;
                                    }
                                }
                            });
                        });
                        for (i, nm) in ["Red", "Green", "Blue"].iter().enumerate() {
                            ui.horizontal(|ui| {
                                row_label(ui, nm);
                                ch |= ui.add(egui::Slider::new(&mut rgb[i], 0.0..=1.0)).changed();
                            });
                        }
                        if ch {
                            nums.push(("r".into(), rgb[0]));
                            nums.push(("g".into(), rgb[1]));
                            nums.push(("b".into(), rgb[2]));
                        }
                    }
                    let mut a = get("a", 1.0);
                    ui.horizontal(|ui| {
                        row_label(ui, "Alpha");
                        if ui.add(egui::Slider::new(&mut a, 0.0..=1.0)).changed() {
                            nums.push(("a".into(), a));
                        }
                    });
                }
                "colorize" => {
                    let mut stops = stops_from_node(&node);
                    if gradient_editor(ui, Id::new(("grad", id)), &mut stops) {
                        for i in 0..16 {
                            for k in ["p", "r", "g", "b"] {
                                nums.push((format!("{k}{i}"), f32::NAN));
                            }
                        }
                        for (i, (p, c)) in stops.iter().enumerate() {
                            nums.push((format!("p{i}"), *p));
                            nums.push((format!("r{i}"), c[0]));
                            nums.push((format!("g{i}"), c[1]));
                            nums.push((format!("b{i}"), c[2]));
                        }
                    }
                }
                "output" => {
                    let usage = node.strs.get("usage").cloned().unwrap_or_else(|| "basecolor".into());
                    let in_sub = !self.parents.is_empty();
                    let names: Vec<&str> = if in_sub { vec!["out"] } else { OUTPUT_NAMES.to_vec() };
                    ui.horizontal(|ui| {
                        row_label(ui, "Usage");
                        egui::ComboBox::from_id_salt(("usage", id)).selected_text(usage_label(&usage)).show_ui(ui, |ui| {
                            for n in &names {
                                if ui.selectable_label(usage == *n, usage_label(n)).clicked() {
                                    strs.push(("usage".into(), Some(n.to_string())));
                                }
                            }
                        });
                    });
                    let mut ident = node.strs.get("identifier").cloned().unwrap_or_default();
                    ui.horizontal(|ui| {
                        row_label(ui, "Identifier");
                        if ui.add(egui::TextEdit::singleline(&mut ident).hint_text(usage.as_str()).desired_width(f32::INFINITY)).changed() {
                            let clean: String = ident.chars().filter(|c| c.is_alphanumeric() || *c == '_' || *c == '-').collect();
                            strs.push(("identifier".into(), if clean.is_empty() { None } else { Some(clean) }));
                        }
                    });
                    ui.small(format!("Exports as {}.png at {0}x{0}. The 3D view uses this output as the '{}' channel.", self.g.size, usage));
                }
                "pixel_processor" => {
                    ui.small("vars: u v x y w h r g b l   funcs: sin cos abs sqrt min max pow floor fract clamp mix step smoothstep noise rand tex atan2 exp log");
                    for key in ["expr", "expr_g", "expr_b", "expr_a"] {
                        let mut s = node.strs.get(key).cloned().unwrap_or_default();
                        if ui.add(egui::TextEdit::singleline(&mut s).hint_text(key).desired_width(f32::INFINITY)).changed() {
                            strs.push((key.to_string(), if s.trim().is_empty() { None } else { Some(s.replace('"', "")) }));
                        }
                    }
                }
                "bitmap" => {
                    let file = node.strs.get("file").cloned().unwrap_or_default();
                    ui.label(if file.is_empty() { "(no file)".to_string() } else { file });
                    if ui.button("Browse PNG...").clicked() {
                        if let Some(p) = rfd::FileDialog::new().add_filter("PNG image", &["png"]).pick_file() {
                            strs.push(("file".into(), Some(p.to_string_lossy().to_string())));
                        }
                    }
                }
                "subgraph" => {
                    if let Some(g) = node.strs.get("graph") {
                        ui.label(format!("Instance of '{g}'"));
                        if ui.button("Open subgraph").clicked() {
                            self.acts.push(Act::EnterSub(g.clone()));
                        }
                    }
                }
                _ => {}
            }

            // ---- generic parameter rows
            let mut rows: Vec<(String, String, PK)> = Vec::new();
            let exposable = kind != "subgraph";
            if kind == "subgraph" {
                if let Some(sub) = node.strs.get("graph").and_then(|g| self.g.subgraphs.get(g)) {
                    for (nid, inner) in &sub.nodes {
                        for pd in pdefs(&inner.kind) {
                            let pk = match (pd.k, inner.nums.get(pd.name).copied()) {
                                (PK::F { lo, hi, .. }, Some(d)) => PK::F { def: d, lo, hi },
                                (PK::I { lo, hi, .. }, Some(d)) => PK::I { def: d as i32, lo, hi },
                                (PK::E { opts, .. }, Some(d)) => PK::E { def: d as i32, opts },
                                (PK::B { .. }, Some(d)) => PK::B { def: d > 0.5 },
                                (k, None) => k,
                            };
                            rows.push((format!("{nid}.{}", pd.name), format!("{} #{nid}: {}", label(&inner.kind), pd.label), pk));
                        }
                    }
                }
            } else {
                rows = pdefs(&kind).into_iter().map(|pd| (pd.name.to_string(), pd.label.to_string(), pd.k)).collect();
            }
            if rows.is_empty() && !matches!(kind.as_str(), "color" | "colorize" | "output" | "pixel_processor" | "bitmap" | "subgraph" | "dot" | "invert" | "to_gray" | "merge" | "normal_blend") {
                ui.small("No adjustable parameters.");
            }
            for (key, lbl, pk) in &rows {
                let cur = node.nums.get(key).copied();
                ui.horizontal(|ui| {
                    row_label(ui, lbl);
                    match *pk {
                        PK::F { def, lo, hi } => {
                            let mut v = cur.unwrap_or(def);
                            if ui.add(egui::Slider::new(&mut v, lo..=hi)).changed() {
                                nums.push((key.clone(), v));
                            }
                        }
                        PK::I { def, lo, hi } => {
                            let mut v = cur.unwrap_or(def as f32);
                            if ui.add(egui::Slider::new(&mut v, lo as f32..=hi as f32).step_by(1.0).fixed_decimals(0)).changed() {
                                nums.push((key.clone(), v.round()));
                            }
                        }
                        PK::E { def, opts } => {
                            let idx = (cur.unwrap_or(def as f32).round() as i32).clamp(0, opts.len() as i32 - 1) as usize;
                            egui::ComboBox::from_id_salt((id, key.as_str())).selected_text(opts[idx]).show_ui(ui, |ui| {
                                for (i, o) in opts.iter().enumerate() {
                                    if ui.selectable_label(i == idx, *o).clicked() {
                                        nums.push((key.clone(), i as f32));
                                    }
                                }
                            });
                        }
                        PK::B { def } => {
                            let mut b = cur.map(|v| v > 0.5).unwrap_or(def);
                            if ui.checkbox(&mut b, "").changed() {
                                nums.push((key.clone(), if b { 1.0 } else { 0.0 }));
                            }
                        }
                    }
                    if exposable {
                        let is_exp = self.g.exposed.iter().any(|e| e.node == id && e.param == *key);
                        let (lo, hi) = match *pk {
                            PK::F { lo, hi, .. } => (lo, hi),
                            PK::I { lo, hi, .. } => (lo as f32, hi as f32),
                            PK::E { opts, .. } => (0.0, opts.len() as f32 - 1.0),
                            PK::B { .. } => (0.0, 1.0),
                        };
                        if ui.selectable_label(is_exp, "P").on_hover_text("Publish this parameter (settable in .mca packages)").clicked() {
                            expose = Some((key.clone(), lo, hi));
                        }
                    }
                });
            }
        });

        if !nums.is_empty() || !strs.is_empty() {
            if !self.edit_open {
                self.snap();
                self.edit_open = true;
            }
            if let Some(n) = self.g.nodes.get_mut(&id) {
                for (k, v) in nums {
                    if v.is_nan() {
                        n.nums.remove(&k);
                    } else {
                        n.nums.insert(k, v);
                    }
                }
                for (k, v) in strs {
                    match v {
                        Some(s) => {
                            n.strs.insert(k, s);
                        }
                        None => {
                            n.strs.remove(&k);
                        }
                    }
                }
            }
            self.dirty = true;
        }
        if let Some((key, lo, hi)) = expose {
            self.snap();
            if let Some(p) = self.g.exposed.iter().position(|e| e.node == id && e.param == key) {
                self.g.exposed.remove(p);
            } else {
                self.g.exposed.push(Exposed { label: format!("{} {} {}", label(&kind), id, key), node: id, param: key, min: lo, max: hi });
            }
        }
    }
}
