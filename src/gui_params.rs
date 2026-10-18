// SPDX-License-Identifier: MIT OR Apache-2.0
// ======================================================================= Node properties (included into gui.rs)


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
    painter.circle(c + vec2(ha.cos(), ha.sin()) * ((r_in + r_out) * 0.5), 5.5, hsv_color32(h, 1.0, 1.0), Stroke::new(2.0_f32, Color32::WHITE));
    painter.circle(pos2(sq.min.x + sv * sq.width(), sq.min.y + (1.0 - vv) * sq.height()), 5.0, Color32::from_rgb((rgb[0] * 255.0) as u8, (rgb[1] * 255.0) as u8, (rgb[2] * 255.0) as u8), Stroke::new(2.0_f32, if vv > 0.55 { Color32::BLACK } else { Color32::WHITE }));
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
    painter.rect_stroke(bar, 0.0, Stroke::new(1.0_f32, Color32::from_gray(20)), egui::StrokeKind::Inside);
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
        let edge = if sel == i { Color32::from_rgb(90, 215, 190) } else { Color32::from_gray(200) };
        painter.add(egui::Shape::convex_polygon(vec![pos2(x, bar.max.y + 1.0), pos2(x - 5.0, bar.max.y + 8.0), pos2(x + 5.0, bar.max.y + 8.0)], edge, Stroke::NONE));
        let sq = Rect::from_center_size(pos2(x, bar.max.y + 15.0), vec2(11.0, 9.0));
        painter.rect_filled(sq, 1.0, c32(stops[i].1));
        painter.rect_stroke(sq, 1.0, Stroke::new(1.5_f32, edge), egui::StrokeKind::Inside);
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
/// A single-line text property.
fn str_row(ui: &mut egui::Ui, node: &Node, key: &str, lbl: &str, hint: &str, strs: &mut Vec<(String, Option<String>)>) {
    let mut s = node.strs.get(key).cloned().unwrap_or_default();
    ui.horizontal(|ui| {
        row_label(ui, lbl);
        if ui.add(egui::TextEdit::singleline(&mut s).hint_text(hint).desired_width(f32::INFINITY)).changed() {
            let clean: String = s.chars().filter(|c| *c != '"').collect();
            strs.push((key.to_string(), if clean.trim().is_empty() { None } else { Some(clean) }));
        }
    });
}

/// Pick one of `options` for a text property (empty choice removes it).
fn name_combo(ui: &mut egui::Ui, id: u32, node: &Node, key: &str, lbl: &str, options: &[String], strs: &mut Vec<(String, Option<String>)>) {
    let cur = node.strs.get(key).cloned().unwrap_or_default();
    ui.horizontal(|ui| {
        row_label(ui, lbl);
        egui::ComboBox::from_id_salt((id, key)).selected_text(if cur.is_empty() { "(none)".to_string() } else { cur.clone() }).show_ui(ui, |ui| {
            if ui.selectable_label(cur.is_empty(), "(none)").clicked() {
                strs.push((key.to_string(), None));
            }
            for o in options {
                if ui.selectable_label(*o == cur, o).clicked() {
                    strs.push((key.to_string(), Some(o.clone())));
                }
            }
        });
    });
    if !cur.is_empty() && !options.contains(&cur) {
        ui.colored_label(Color32::from_rgb(255, 120, 110), format!("'{cur}' does not exist in this package"));
    }
}

/// Tone-curve editor: pick a channel, click to add a point, drag to move it, double-click to remove it.
fn curve_editor(ui: &mut egui::Ui, id: u32, node: &Node, strs: &mut Vec<(String, Option<String>)>) {
    use materialcraft::curve::Curve;
    let chans = [("Master", "points"), ("R", "points_r"), ("G", "points_g"), ("B", "points_b"), ("A", "points_a")];
    let ch_id = egui::Id::new(("curve_channel", id));
    let mut ch = ui.data_mut(|d| d.get_temp::<usize>(ch_id)).unwrap_or(0).min(4);
    ui.horizontal(|ui| {
        for (i, (n, _)) in chans.iter().enumerate() {
            if ui.selectable_label(ch == i, *n).clicked() {
                ch = i;
            }
        }
    });
    ui.data_mut(|d| d.insert_temp(ch_id, ch));
    let key = chans[ch].1;
    let text = node.strs.get(key).cloned().unwrap_or_default();
    let curve = Curve::parse(&text).unwrap_or_else(|_| Curve::identity());
    let mut pts: Vec<(f32, f32)> = curve.pts.iter().map(|p| (p.0, p.1)).collect();
    let side = ui.available_width().clamp(120.0, 240.0);
    let (rect, resp) = ui.allocate_exact_size(vec2(side, side), Sense::click_and_drag());
    let to_s = |x: f32, y: f32| pos2(rect.left() + x * side, rect.bottom() - y * side);
    let from_s = |p: Pos2| (((p.x - rect.left()) / side).clamp(0.0, 1.0), (1.0 - (p.y - rect.top()) / side).clamp(0.0, 1.0));
    let drag_id = egui::Id::new(("curve_drag", id));
    let mut changed = false;
    if let Some(pos) = resp.interact_pointer_pos() {
        let near = |pts: &Vec<(f32, f32)>| pts.iter().position(|p| to_s(p.0, p.1).distance(pos) < 10.0);
        if resp.drag_started() {
            let hit = near(&pts).or_else(|| {
                // grab the nearest point, or create one under the pointer
                let (x, y) = from_s(pos);
                pts.push((x, y));
                pts.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap_or(std::cmp::Ordering::Equal));
                changed = true;
                pts.iter().position(|p| (p.0 - x).abs() < 1e-6 && (p.1 - y).abs() < 1e-6)
            });
            ui.data_mut(|d| d.insert_temp(drag_id, hit.unwrap_or(usize::MAX)));
        } else if resp.dragged() {
            if let Some(i) = ui.data_mut(|d| d.get_temp::<usize>(drag_id)).filter(|i| *i < pts.len()) {
                let (mut x, y) = from_s(pos);
                // keep the order of the points
                let lo = if i > 0 { pts[i - 1].0 + 0.002 } else { 0.0 };
                let hi = if i + 1 < pts.len() { pts[i + 1].0 - 0.002 } else { 1.0 };
                x = x.clamp(lo.min(hi), hi.max(lo));
                pts[i] = (x, y);
                changed = true;
            }
        } else if resp.double_clicked() && pts.len() > 2 {
            if let Some(i) = near(&pts) {
                pts.remove(i);
                changed = true;
            }
        }
    }
    let shown = if changed { Curve::new(pts.iter().map(|p| (p.0, p.1, None, None)).collect()) } else { curve };
    let painter = ui.painter_at(rect);
    painter.rect_filled(rect, egui::CornerRadius::same((3.0) as u8), Color32::from_gray(28));
    for k in 1..4 {
        let t = k as f32 / 4.0;
        painter.line_segment([to_s(t, 0.0), to_s(t, 1.0)], Stroke::new(1.0_f32, Color32::from_gray(48)));
        painter.line_segment([to_s(0.0, t), to_s(1.0, t)], Stroke::new(1.0_f32, Color32::from_gray(48)));
    }
    painter.line_segment([to_s(0.0, 0.0), to_s(1.0, 1.0)], Stroke::new(1.0_f32, Color32::from_gray(70)));
    let col = match ch {
        1 => Color32::from_rgb(235, 90, 90),
        2 => Color32::from_rgb(90, 220, 110),
        3 => Color32::from_rgb(100, 140, 255),
        _ => Color32::from_gray(225),
    };
    let line: Vec<Pos2> = (0..=64).map(|i| {
        let x = i as f32 / 64.0;
        to_s(x, shown.eval(x))
    }).collect();
    painter.add(egui::Shape::line(line, Stroke::new(2.0_f32, col)));
    for p in &pts {
        painter.circle(to_s(p.0, p.1), 4.5, Color32::from_gray(30), Stroke::new(1.5_f32, col));
    }
    ui.small("Click and drag to add or move a point, double-click a point to remove it.");
    if ui.small_button("Reset this curve").clicked() {
        strs.push((key.to_string(), None));
    } else if changed {
        let t = pts.iter().map(|p| format!("{:.4}:{:.4}", p.0, p.1)).collect::<Vec<_>>().join(",");
        strs.push((key.to_string(), Some(t)));
    }
}

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
                    ui.painter().rect_filled(r, egui::CornerRadius::same((3.0) as u8), Color32::from_rgb(col[0], col[1], col[2]));
                    if c.color as usize == i {
                        ui.painter().rect_stroke(r, egui::CornerRadius::same((3.0) as u8), Stroke::new(2.0_f32, Color32::WHITE), egui::StrokeKind::Inside);
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
        self.functions_panel(ui);
    }

    /// Function graphs and pattern programs of this package: open or create them.
    fn functions_panel(&mut self, ui: &mut egui::Ui) {
        let list: Vec<(String, bool)> = self.g.subgraphs.iter().filter(|(_, g)| g.graph_kind() != "texture").map(|(k, g)| (k.clone(), g.graph_kind() == "function")).collect();
        let mut open = None;
        let mut make: Option<bool> = None;
        egui::CollapsingHeader::new(egui::RichText::new("FUNCTIONS AND PATTERN PROGRAMS").strong()).default_open(!list.is_empty()).show(ui, |ui| {
            if list.is_empty() {
                ui.small("A function graph computes values that can drive any parameter or a pixel processor. A pattern program is drawn by a Pattern Program Map node.");
            }
            for (name, is_fn) in &list {
                ui.horizontal(|ui| {
                    ui.label(format!("{name}  ({})", if *is_fn { "function" } else { "pattern program" }));
                    if ui.small_button("Open").clicked() {
                        open = Some(name.clone());
                    }
                });
            }
            ui.horizontal(|ui| {
                if ui.button("New function").clicked() {
                    make = Some(true);
                }
                if ui.button("New pattern program").clicked() {
                    make = Some(false);
                }
            });
        });
        if let Some(n) = open {
            self.acts.push(Act::EnterSub(n));
        }
        if let Some(is_fn) = make {
            let base = if is_fn { "function" } else { "pattern" };
            let mut n = 1;
            let name = loop {
                let c = format!("{base}_{n}");
                if !self.g.subgraphs.contains_key(&c) && !self.parents.iter().any(|(p, _)| p.subgraphs.contains_key(&c)) {
                    break c;
                }
                n += 1;
            };
            let text = if is_fn {
                "meta kind \"function\"\nnode 1 f_pos\nnode 2 f_swizzle a=@1 mask=\"x\"\nnode 3 f_out in=@2\n"
            } else {
                "meta kind \"fxmap\"\nnode 1 fxm_marker w=0.6 h=0.6\nnode 2 fxm_iterate body=@1 grid_x=4 grid_y=4\nnode 3 fxm_out in=@2\n"
            };
            match Graph::parse_unchecked(text) {
                Ok(g) => {
                    self.snap();
                    self.g.subgraphs.insert(name.clone(), g);
                    self.dirty = true;
                    self.status(format!("Created '{name}'. Open it to edit; pick it on a node with the 'fn' button or the Function / Pattern program field."), true);
                }
                Err(e) => self.status(e.to_string(), false),
            }
        }
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
        if kind.starts_with("f_") && kind != "f_out" {
            let none = BTreeMap::new();
            match materialcraft::func::probe_value(&self.g, &self.g.subgraphs, id, &none) {
                Ok(v) => {
                    ui.label(egui::RichText::new(format!("{} = {}", v.pin().name(), v.show())).monospace());
                }
                Err(e) => {
                    ui.colored_label(Color32::from_rgb(255, 120, 110), e);
                }
            }
        }
        if let Some(e) = self.node_err.get(&id) {
            ui.colored_label(Color32::from_rgb(255, 120, 110), e);
        }
        let mut nums: Vec<(String, f32)> = Vec::new();
        let mut strs: Vec<(String, Option<String>)> = Vec::new();
        let mut expose: Option<(String, f32, f32)> = None;
        let get = |k: &str, d: f32| node.nums.get(k).copied().unwrap_or(d);

        // ---- inputs overview
        let mut sl = slots(&kind);
        // a graph node only has inputs when that graph defines Input nodes
        if kind == "subgraph" && !node.strs.get("graph").and_then(|g| self.g.subgraphs.get(g)).map(|sg| sg.nodes.values().any(|x| x.kind == "input")).unwrap_or(true) {
            sl.clear();
        }
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

        // ---- output size (relative to input / relative to parent / absolute, plus a power-of-two modifier)
        if !matches!(kind.as_str(), "comment" | "frame" | "dot") {
            egui::CollapsingHeader::new(egui::RichText::new("OUTPUT SIZE").strong()).default_open(true).show(ui, |ui| {
                let mode = get("osize_mode", 0.0).round() as i32;
                let names = ["Relative to input", "Relative to parent", "Absolute"];
                ui.horizontal(|ui| {
                    row_label(ui, "Size mode");
                    egui::ComboBox::from_id_salt(("osize_mode", id)).selected_text(names[mode.clamp(0, 2) as usize]).show_ui(ui, |ui| {
                        for (i, n) in names.iter().enumerate() {
                            if ui.selectable_label(mode == i as i32, *n).clicked() {
                                nums.push(("osize_mode".into(), i as f32));
                            }
                        }
                    });
                });
                if !self.g.is_square() && matches!(kind.as_str(), "perlin" | "fbm" | "clouds" | "voronoi" | "voronoi_fractal" | "cells" | "tile_sampler" | "tile_generator") {
                    let mut sq = get("square_cells", 0.0) > 0.5;
                    if ui.checkbox(&mut sq, "Square cells").on_hover_text("Keep the cells of this pattern square in a non-square graph (the vertical cell count follows the aspect ratio)").changed() {
                        nums.push(("square_cells".into(), sq as i32 as f32));
                    }
                }
                let parent = self.g.size;
                if mode == 2 {
                    let e = get("osize_abs", 9.0).round().clamp(4.0, 13.0) as i32;
                    ui.horizontal(|ui| {
                        row_label(ui, "Size");
                        egui::ComboBox::from_id_salt(("osize_abs", id)).selected_text(format!("{0} x {0}", 1usize << e)).show_ui(ui, |ui| {
                            for k in 4..=13 {
                                if ui.selectable_label(e == k, format!("{0} x {0}", 1usize << k)).clicked() {
                                    nums.push(("osize_abs".into(), k as f32));
                                }
                            }
                        });
                    });
                } else {
                    let mut rel = get("osize_rel", 0.0).round() as i32;
                    ui.horizontal(|ui| {
                        row_label(ui, "Modifier");
                        if ui.add(egui::Slider::new(&mut rel, -12..=12)).changed() {
                            nums.push(("osize_rel".into(), rel as f32));
                        }
                    });
                    let shown = self.nominal_node_size(id);
                    ui.small(format!("Result: {0} x {0}  (graph {1} x {1})", shown, parent));
                }
                ui.small("A lower size computes faster: give heavy nodes (AO, blurs, noises) a smaller size.");
                // Output Format: precision of this node's result (the default keeps full 32-bit float)
                let fnames = ["Graph default", "8 bits integer", "16 bits integer", "16 bits float", "32 bits float"];
                let cur = (get("ofmt", 0.0).round() as i32).clamp(0, 4) as usize;
                ui.horizontal(|ui| {
                    row_label(ui, "Output format");
                    egui::ComboBox::from_id_salt(("ofmt", id)).selected_text(fnames[cur]).show_ui(ui, |ui| {
                        for (i, n) in fnames.iter().enumerate() {
                            if ui.selectable_label(cur == i, *n).clicked() {
                                nums.push(("ofmt".into(), i as f32));
                            }
                        }
                    });
                });
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
                                ui.painter().rect_stroke(r, 3.0, Stroke::new(1.0_f32, Color32::from_gray(15)), egui::StrokeKind::Inside);
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
                    let names: Vec<&str> = OUTPUT_NAMES.to_vec();
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
                    let funcs: Vec<String> = self.g.subgraphs.iter().filter(|(_, g)| g.graph_kind() == "function").map(|(k, _)| k.clone()).collect();
                    name_combo(ui, id, &node, "fn", "Function graph", &funcs, &mut strs);
                    if node.strs.get("fn").map(|f| !f.is_empty()).unwrap_or(false) {
                        ui.small("The function runs once per pixel. Its position input is the pixel position; it can sample the inputs.");
                    } else {
                        ui.small("vars: u v x y w h r g b l   funcs: sin cos abs sqrt min max pow floor fract clamp mix step smoothstep noise rand tex atan2 exp log");
                        for key in ["expr", "expr_g", "expr_b", "expr_a"] {
                            let mut s = node.strs.get(key).cloned().unwrap_or_default();
                            if ui.add(egui::TextEdit::singleline(&mut s).hint_text(key).desired_width(f32::INFINITY)).changed() {
                                strs.push((key.to_string(), if s.trim().is_empty() { None } else { Some(s.replace('"', "")) }));
                            }
                        }
                    }
                }
                "value_processor" => {
                    let funcs: Vec<String> = self.g.subgraphs.iter().filter(|(_, g)| g.graph_kind() == "function").map(|(k, _)| k.clone()).collect();
                    name_combo(ui, id, &node, "fn", "Function graph", &funcs, &mut strs);
                    ui.small("The function runs once. A float fills the image with grey, a float3 or float4 with colour. It can read image statistics.");
                }
                "fx_map" => {
                    let progs: Vec<String> = self.g.subgraphs.iter().filter(|(_, g)| g.graph_kind() == "fxmap").map(|(k, _)| k.clone()).collect();
                    name_combo(ui, id, &node, "program", "Pattern program", &progs, &mut strs);
                    if let Some(p) = node.strs.get("program").filter(|p| progs.contains(p)) {
                        if ui.button("Open program").clicked() {
                            self.acts.push(Act::EnterSub(p.clone()));
                        }
                    }
                }
                k if is_path_node(k) => {
                    let files: Vec<String> = self.g.resources.keys().filter(|f| f.ends_with(".spl")).cloned().collect();
                    name_combo(ui, id, &node, "file", "Spline", &files, &mut strs);
                    if node.strs.get("file").map(|f| f.is_empty()).unwrap_or(true) {
                        ui.small("No spline chosen: the built-in shape below is used.");
                    }
                    egui::CollapsingHeader::new("More splines to merge").id_salt(("spl_more", id)).show(ui, |ui| {
                        for key in ["file2", "file3", "file4"] {
                            name_combo(ui, id, &node, key, "Spline", &files, &mut strs);
                        }
                    });
                    ui.horizontal_wrapped(|ui| {
                        if ui.button("Edit spline").on_hover_text("Draw and change the spline in the 2D view").clicked() {
                            self.acts.push(Act::SplineEdit(id));
                        }
                        if ui.button("Import SVG paths...").clicked() {
                            self.acts.push(Act::SplineImportSvg(id));
                        }
                    });
                }
                "color_lut" => {
                    let file = node.strs.get("file").cloned().unwrap_or_default();
                    ui.label(if file.is_empty() { "(no lookup table)".to_string() } else { file });
                    if ui.button("Browse lookup table...").clicked() {
                        if let Some(p) = rfd::FileDialog::new().add_filter("Colour lookup table", &["cube", "3dl", "png", "jpg", "jpeg", "tif", "tiff", "bmp", "tga", "webp"]).pick_file() {
                            strs.push(("file".into(), Some(p.to_string_lossy().to_string())));
                        }
                    }
                }
                "svg" => {
                    let file = node.strs.get("file").cloned().unwrap_or_default();
                    ui.label(if file.is_empty() { "(no file)".to_string() } else { file });
                    if ui.button("Browse SVG...").clicked() {
                        if let Some(p) = rfd::FileDialog::new().add_filter("SVG image", &["svg"]).pick_file() {
                            strs.push(("file".into(), Some(p.to_string_lossy().to_string())));
                        }
                    }
                }
                "text" => {
                    let mut t = node.strs.get("text").cloned().unwrap_or_default();
                    if ui.add(egui::TextEdit::multiline(&mut t).hint_text("text").desired_rows(2).desired_width(f32::INFINITY)).changed() {
                        strs.push(("text".into(), Some(t.replace('"', ""))));
                    }
                    let font = node.strs.get("font").cloned().unwrap_or_default();
                    ui.label(if font.is_empty() { "Font: built-in".to_string() } else { format!("Font: {font}") });
                    ui.horizontal(|ui| {
                        if ui.button("Browse TrueType font...").clicked() {
                            if let Some(p) = rfd::FileDialog::new().add_filter("TrueType font", &["ttf"]).pick_file() {
                                strs.push(("font".into(), Some(p.to_string_lossy().to_string())));
                            }
                        }
                        if !font.is_empty() && ui.button("Built-in").clicked() {
                            strs.push(("font".into(), None));
                        }
                    });
                }
                "curve" => curve_editor(ui, id, &node, &mut strs),
                "f_string" => str_row(ui, &node, "v", "Text", "", &mut strs),
                "f_swizzle" => str_row(ui, &node, "mask", "Components", "xyzw or rgba, e.g. zyx", &mut strs),
                "f_set" | "f_get" | "f_input" | "f_graph_input" | "input_value" => {
                    str_row(ui, &node, "name", "Name", "name", &mut strs);
                    if matches!(kind.as_str(), "f_input" | "f_graph_input" | "input_value") && node.nums.get("type").copied().unwrap_or(0.0) as i32 == 9 {
                        str_row(ui, &node, "v", "Default text", "", &mut strs);
                    }
                }
                "f_sample_gray" | "f_sample_color" | "f_image_avg" | "f_image_min" | "f_image_max" => str_row(ui, &node, "slot", "Image input", "in", &mut strs),
                "f_call" => {
                    let funcs: Vec<String> = self.g.subgraphs.iter().filter(|(_, g)| g.graph_kind() == "function").map(|(k, _)| k.clone()).collect();
                    name_combo(ui, id, &node, "fn", "Function", &funcs, &mut strs);
                }
                "baked_map" => {
                    let sets: Vec<String> = materialcraft::bake::stored_sets(&self.g.resources).keys().cloned().collect();
                    name_combo(ui, id, &node, "set", "Bake set", &sets, &mut strs);
                    let maps: Vec<String> = materialcraft::bake::MapKind::ALL.iter().map(|m| m.id().to_string()).collect();
                    name_combo(ui, id, &node, "map", "Map", &maps, &mut strs);
                    if sets.is_empty() {
                        ui.small("No baked maps in this package yet: use Tools > Mesh map baker.");
                    }
                    if ui.button("Open the mesh map baker").clicked() {
                        self.acts.push(Act::Bake);
                    }
                }
                "bitmap" => {
                    let file = node.strs.get("file").cloned().unwrap_or_default();
                    ui.label(if file.is_empty() { "(no file)".to_string() } else { file });
                    if ui.button("Browse image...").clicked() {
                        if let Some(p) = rfd::FileDialog::new().add_filter("Images", materialcraft::imgio::EXTENSIONS).pick_file() {
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
                        if let Some(sub) = self.g.subgraphs.get(g) {
                            let outs = sub.output_nodes();
                            if outs.len() > 1 {
                                let cur = node.strs.get("output").cloned().unwrap_or_default();
                                let shown = if cur.is_empty() { outs.iter().find(|o| o.0 == "out").or(outs.first()).map(|o| o.1.clone()).unwrap_or_default() } else { cur };
                                ui.horizontal(|ui| {
                                    row_label(ui, "Output");
                                    egui::ComboBox::from_id_salt(("subout", id)).selected_text(shown.clone()).show_ui(ui, |ui| {
                                        for o in &outs {
                                            if ui.selectable_label(o.1 == shown, &o.1).clicked() {
                                                strs.push(("output".into(), Some(o.1.clone())));
                                            }
                                        }
                                    });
                                });
                            }
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
            if rows.is_empty() && !matches!(kind.as_str(), "color" | "colorize" | "output" | "pixel_processor" | "bitmap" | "subgraph" | "dot" | "to_gray" | "merge") {
                ui.small("No adjustable parameters.");
            }
            let funcs_all: Vec<String> = if exposable { self.g.subgraphs.iter().filter(|(_, g)| g.graph_kind() == "function").map(|(k, _)| k.clone()).collect() } else { Vec::new() };
            let mut draw = |ui: &mut egui::Ui, i: usize| {
                let (key, lbl, pk) = &rows[i];
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
                        // drive this parameter by a function graph
                        let fkey = format!("fn.{key}");
                        let bound = node.strs.get(&fkey).cloned().unwrap_or_default();
                        if !funcs_all.is_empty() || !bound.is_empty() {
                            ui.menu_button(if bound.is_empty() { "fn" } else { "fn*" }, |ui| {
                                if ui.selectable_label(bound.is_empty(), "(not driven)").clicked() {
                                    strs.push((fkey.clone(), None));
                                    ui.close();
                                }
                                for f in &funcs_all {
                                    if ui.selectable_label(*f == bound, f).clicked() {
                                        strs.push((fkey.clone(), Some(f.clone())));
                                        ui.close();
                                    }
                                }
                            })
                            .response
                            .on_hover_text("Drive this parameter by a function graph");
                        }
                    }
                });
            };
            // many parameters: sort them into categories and sub categories (a flat list is fine for short ones)
            if rows.len() >= 8 && kind != "subgraph" {
                let mut cats: Vec<(&'static str, Vec<(&'static str, Vec<usize>)>)> = Vec::new();
                for (i, (key, _, _)) in rows.iter().enumerate() {
                    let (c, sc) = param_group(&kind, key);
                    let ci = match cats.iter().position(|x| x.0 == c) {
                        Some(ci) => ci,
                        None => {
                            cats.push((c, Vec::new()));
                            cats.len() - 1
                        }
                    };
                    let subs = &mut cats[ci].1;
                    match subs.iter().position(|x| x.0 == sc) {
                        Some(si) => subs[si].1.push(i),
                        None => subs.push((sc, vec![i])),
                    }
                }
                for (ci, (cat, subs)) in cats.iter().enumerate() {
                    egui::CollapsingHeader::new(egui::RichText::new(*cat).strong()).id_salt(("pcat", id, *cat)).default_open(ci < 2).show(ui, |ui| {
                        for (sub, idx) in subs {
                            if sub.is_empty() {
                                for &i in idx {
                                    draw(ui, i);
                                }
                            } else {
                                egui::CollapsingHeader::new(*sub).id_salt(("psub", id, *cat, *sub)).default_open(true).show(ui, |ui| {
                                    for &i in idx {
                                        draw(ui, i);
                                    }
                                });
                            }
                        }
                    });
                }
            } else {
                for i in 0..rows.len() {
                    draw(ui, i);
                }
            }
            // parameters driven by functions: show the function and its inputs
            let bound: Vec<(String, String)> = node.strs.iter().filter_map(|(k, v)| k.strip_prefix("fn.").map(|p| (p.to_string(), v.clone()))).collect();
            if !bound.is_empty() {
                ui.separator();
                ui.strong("FUNCTION-DRIVEN PARAMETERS");
            }
            for (param, fname) in &bound {
                ui.label(format!("{param}  <-  {fname}()"));
                match materialcraft::func::compile_named(&self.g.subgraphs, fname) {
                    Ok(p) => {
                        for (iname, _, def) in &p.inputs {
                            let key = format!("fa.{param}.{iname}");
                            let mut v = node.nums.get(&key).copied().unwrap_or(def.as_f32());
                            ui.horizontal(|ui| {
                                row_label(ui, iname);
                                if ui.add(egui::DragValue::new(&mut v).speed(0.01)).changed() {
                                    nums.push((key.clone(), v));
                                }
                            });
                        }
                    }
                    Err(e) => {
                        ui.colored_label(Color32::from_rgb(255, 120, 110), e);
                    }
                }
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

