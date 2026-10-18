// SPDX-License-Identifier: MIT OR Apache-2.0
// ======================================================================= Spline editor (included into gui.rs)
// The spline lives in the package as an embedded text resource (`*.spl`). While it is being edited the 2D view shows
// its points and handles over the picture of the selected path node; every change is written back to the resource, so
// the picture updates at once and undo / redo and saving work like for any other edit.

use materialcraft::spline::{SPath, SPoint, Spline};

struct SplEdit {
    node: u32,
    file: String,
    sp: Spline,
    /// the resource text last written or read, to notice undo / redo and other outside changes
    last: String,
    path: usize,
    sel: Option<usize>,
    /// what is being dragged: (0 point / 1 handle towards the previous point / 2 handle towards the next, point, moved yet)
    drag: Option<(u8, usize, bool)>,
    /// point under the pointer when the context menu was opened
    menu_pt: Option<(usize, usize)>,
    snap: bool,
}

const SPL_HIT: f32 = 9.0;

fn spl_default_preset(kind: &str) -> i32 {
    if kind == "path_fill" || kind == "path_border" {
        3
    } else {
        2
    }
}

fn is_path_node(kind: &str) -> bool {
    kind.starts_with("path_")
}

/// Switch the tangent mode of point `si`: Auto drops the handles, Mirrored and Free freeze the current ones,
/// Corner removes them.
fn spl_set_tangent(path: &mut materialcraft::spline::SPath, si: usize, mode: u8) {
    if si >= path.pts.len() {
        return;
    }
    let (hi, ho) = path.handles(si);
    let pt = &mut path.pts[si];
    pt.tan = mode;
    match mode {
        0 => {
            pt.hin = None;
            pt.hout = None;
        }
        1 => {
            pt.hin = Some(hi);
            pt.hout = Some([-hi[0], -hi[1]]);
        }
        2 => {
            pt.hin = Some(hi);
            pt.hout = Some(ho);
        }
        _ => {
            pt.hin = Some([0.0, 0.0]);
            pt.hout = Some([0.0, 0.0]);
        }
    }
}

impl App {
    /// Outline the light areas of node `node` as a new spline resource and show it with a new Path Stroke node.
    fn spl_trace(&mut self, node: u32) {
        let t = match self.g.evaluate_node(node, 256) {
            Ok(t) => t,
            Err(e) => return self.status(format!("Cannot trace: {e}"), false),
        };
        match Spline::from_mask(&t, 0.5, 0.002) {
            Ok(sp) => {
                let file = self.spl_unique_name("traced");
                let at = self.g.nodes.get(&node).map(|n| pos2(n.pos[0] + CARD + 90.0, n.pos[1])).unwrap_or(pos2(0.0, 0.0));
                self.snap();
                self.g.resources.insert(file.clone(), std::sync::Arc::new(sp.to_text().into_bytes()));
                let id = self.add_node_at("path_stroke", None, at);
                if let Some(n) = self.g.nodes.get_mut(&id) {
                    n.strs.insert("file".into(), file.clone());
                    n.nums.insert("width".into(), 4.0);
                }
                self.status(format!("Traced {} outlines ({} points) into {file} and drew them with a new Path Stroke node", sp.paths.len(), sp.point_count()), true);
            }
            Err(e) => self.status(format!("Cannot trace: {e}"), false),
        }
    }

    fn spl_unique_name(&self, base: &str) -> String {
        let mut name = format!("{base}.spl");
        let mut k = 2;
        while self.g.resources.contains_key(&name) {
            name = format!("{base}_{k}.spl");
            k += 1;
        }
        name
    }

    /// Start editing the spline of path node `node`. A node that uses a built-in shape gets a copy of it as a new resource.
    fn spl_start(&mut self, node: u32) {
        let Some(n) = self.g.nodes.get(&node) else { return };
        if !is_path_node(&n.kind) {
            return;
        }
        let existing = n.strs.get("file").cloned().filter(|f| !f.is_empty());
        let (file, sp, created) = match existing {
            Some(f) => match self.g.resources.get(&f).map(|b| Spline::from_bytes(b)) {
                Some(Ok(sp)) => (f, sp, false),
                Some(Err(e)) => return self.status(format!("{f}: {e}"), false),
                None => return self.status(format!("'{f}' is not embedded in this package. Use Import SVG paths or pick an embedded spline."), false),
            },
            None => {
                let k = &n.kind;
                let preset = n.nums.get("preset").copied().unwrap_or(spl_default_preset(k) as f32).round().clamp(0.0, 6.0) as u32 + 1;
                let sp = Spline::preset(preset, n.nums.get("amplitude").copied().unwrap_or(0.12), n.nums.get("frequency").copied().unwrap_or(2.0));
                (self.spl_unique_name("spline"), sp, true)
            }
        };
        if created {
            self.snap();
            self.g.resources.insert(file.clone(), std::sync::Arc::new(sp.to_text().into_bytes()));
            if let Some(n) = self.g.nodes.get_mut(&node) {
                n.strs.insert("file".into(), file.clone());
            }
            self.dirty = true;
        }
        let last = sp.to_text();
        self.active = Some(node);
        self.sel.clear();
        self.sel.insert(node);
        self.spl = Some(SplEdit { node, file, sp, last, path: 0, sel: None, drag: None, menu_pt: None, snap: false });
        self.status("Editing a spline in the 2D view: click to add points, drag to move, right-click for more. Alt or middle button pans.", true);
    }

    fn spl_import_svg(&mut self, node: u32) {
        let Some(p) = rfd::FileDialog::new().add_filter("SVG image", &["svg"]).pick_file() else { return };
        let text = match std::fs::read_to_string(&p) {
            Ok(t) => t,
            Err(e) => return self.status(format!("{}: {e}", p.display()), false),
        };
        match Spline::from_svg(&text) {
            Ok(sp) => {
                let stem = p.file_stem().map(|s| s.to_string_lossy().to_string()).unwrap_or_else(|| "spline".into());
                let stem: String = stem.chars().map(|c| if c.is_alphanumeric() || c == '_' { c } else { '_' }).collect();
                let file = self.spl_unique_name(&stem);
                self.snap();
                self.g.resources.insert(file.clone(), std::sync::Arc::new(sp.to_text().into_bytes()));
                if let Some(n) = self.g.nodes.get_mut(&node) {
                    n.strs.insert("file".into(), file.clone());
                }
                self.dirty = true;
                self.status(format!("Imported {} paths ({} points) as {file}", sp.paths.len(), sp.point_count()), true);
            }
            Err(e) => self.status(format!("{}: {e}", p.display()), false),
        }
    }

    fn spl_write(&mut self, e: &mut SplEdit) {
        let t = e.sp.to_text();
        self.g.resources.insert(e.file.clone(), std::sync::Arc::new(t.clone().into_bytes()));
        e.last = t;
        self.dirty = true;
        self.modified = true;
    }

    /// Re-read the resource when something else (undo, redo) changed it; false when the editor must close.
    fn spl_sync(&mut self, e: &mut SplEdit) -> bool {
        if !self.g.nodes.get(&e.node).map(|n| is_path_node(&n.kind) && n.strs.get("file") == Some(&e.file)).unwrap_or(false) {
            return false;
        }
        match self.g.resources.get(&e.file) {
            None => false,
            Some(b) => {
                if b.as_slice() != e.last.as_bytes() {
                    if let Ok(sp) = Spline::from_bytes(b) {
                        e.last = sp.to_text();
                        e.sp = sp;
                        e.path = e.path.min(e.sp.paths.len().saturating_sub(1));
                        e.sel = None;
                        e.drag = None;
                    }
                }
                true
            }
        }
    }

    fn spl_toolbar(&mut self, ui: &mut egui::Ui) {
        let Some(mut e) = self.spl.take() else { return };
        if !self.spl_sync(&mut e) {
            return;
        }
        let mut done = false;
        let mut changed = false;
        let mut snap_first = false;
        ui.horizontal_wrapped(|ui| {
            ui.add_space(4.0);
            ui.strong(format!("Spline {}", e.file));
            if ui.button("Done").clicked() {
                done = true;
            }
            ui.separator();
            if ui.button("New path").on_hover_text("Start another path; the next click adds its first point").clicked() {
                snap_first = true;
                e.sp.paths.push(SPath::default());
                e.path = e.sp.paths.len() - 1;
                e.sel = None;
                changed = true;
            }
            if !e.sp.paths.is_empty() {
                if ui.small_button("<").clicked() {
                    e.path = (e.path + e.sp.paths.len() - 1) % e.sp.paths.len();
                    e.sel = None;
                }
                ui.label(format!("path {} of {}", e.path + 1, e.sp.paths.len()));
                if ui.small_button(">").clicked() {
                    e.path = (e.path + 1) % e.sp.paths.len();
                    e.sel = None;
                }
                let pi = e.path.min(e.sp.paths.len() - 1);
                let (mut closed, mut smooth) = (e.sp.paths[pi].closed, e.sp.paths[pi].smooth);
                if ui.checkbox(&mut closed, "Closed").changed() {
                    snap_first = true;
                    e.sp.paths[pi].closed = closed;
                    changed = true;
                }
                if ui.checkbox(&mut smooth, "Smooth").on_hover_text("Points without handles get automatic smooth handles").changed() {
                    snap_first = true;
                    e.sp.paths[pi].smooth = smooth;
                    changed = true;
                }
                if ui.button("Delete path").clicked() {
                    snap_first = true;
                    e.sp.paths.remove(pi);
                    e.path = e.path.min(e.sp.paths.len().saturating_sub(1));
                    e.sel = None;
                    changed = true;
                }
            }
            ui.checkbox(&mut e.snap, "Snap").on_hover_text("Snap points to a 1/32 grid (hold Shift to toggle while dragging)");
            if let Some(si) = e.sel {
                if let Some(pt) = e.sp.paths.get_mut(e.path).and_then(|p| p.pts.get_mut(si)) {
                    ui.separator();
                    let mut w = pt.w;
                    ui.label("Width");
                    if ui.add(egui::DragValue::new(&mut w).speed(0.02).range(0.0..=16.0)).changed() {
                        pt.w = w;
                        changed = true;
                    }
                    let mut z = pt.z;
                    ui.label("Height");
                    if ui.add(egui::DragValue::new(&mut z).speed(0.01).range(0.0..=1.0)).on_hover_text("Height value of the path at this point (read by the Path Sample node)").changed() {
                        pt.z = z;
                        changed = true;
                    }
                    let modes = ["Auto", "Mirrored", "Free", "Corner"];
                    let mut tan = pt.tan.min(3);
                    egui::ComboBox::from_id_salt("spl_tangent").selected_text(format!("Tangent: {}", modes[tan as usize])).show_ui(ui, |ui| {
                        for (i, m) in modes.iter().enumerate() {
                            ui.selectable_value(&mut tan, i as u8, *m);
                        }
                    }).response.on_hover_text("Auto: smooth handles from the neighbours. Mirrored: both handles move together. Free: each handle moves alone. Corner: sharp point.");
                    if tan != pt.tan.min(3) {
                        snap_first = true;
                        spl_set_tangent(&mut e.sp.paths[e.path], si, tan);
                        changed = true;
                    }
                    if ui.button("Delete point").clicked() {
                        snap_first = true;
                        e.sp.paths[e.path].pts.remove(si);
                        e.sel = None;
                        changed = true;
                    }
                }
            }
        });
        if snap_first {
            self.snap();
        }
        if changed {
            self.spl_write(&mut e);
        }
        if !done {
            self.spl = Some(e);
        }
    }

    /// Delete the selected point (Delete key in the 2D view while editing).
    fn spl_delete_point(&mut self) {
        let Some(mut e) = self.spl.take() else { return };
        if let Some(si) = e.sel {
            if e.sp.paths.get(e.path).map(|p| si < p.pts.len()).unwrap_or(false) {
                self.snap();
                e.sp.paths[e.path].pts.remove(si);
                e.sel = None;
                self.spl_write(&mut e);
            }
        }
        self.spl = Some(e);
    }

    /// Draw the spline over the picture and handle the pointer. `img` is where the texture is drawn.
    fn spl_canvas(&mut self, ui: &mut egui::Ui, painter: &egui::Painter, img: Rect, resp: &egui::Response) {
        let Some(mut e) = self.spl.take() else { return };
        let to_screen = |p: [f32; 2]| pos2(img.min.x + p[0] * img.width(), img.min.y + p[1] * img.height());
        let to_uv = |q: Pos2| [(q.x - img.min.x) / img.width().max(1.0), (q.y - img.min.y) / img.height().max(1.0)];
        let (iw, ih) = (img.width().max(1.0), img.height().max(1.0));
        let (shift, alt, space) = ui.input(|i| (i.modifiers.shift, i.modifiers.alt, i.key_down(Key::Space)));
        let hover = ui.input(|i| i.pointer.hover_pos()).filter(|p| resp.rect.contains(*p));
        let snapped = |uv: [f32; 2], on: bool| -> [f32; 2] {
            let uv = [uv[0].clamp(-0.5, 1.5), uv[1].clamp(-0.5, 1.5)];
            if on != shift {
                [(uv[0] * 32.0).round() / 32.0, (uv[1] * 32.0).round() / 32.0]
            } else {
                uv
            }
        };
        let mut changed = false;
        let mut want_snap = false;
        // what is under a screen position: (kind 0 point / 1 handle in / 2 handle out, path, point)
        let hit_at = |e: &SplEdit, q: Pos2| -> Option<(u8, usize, usize)> {
            if let Some(si) = e.sel {
                if let Some(p) = e.sp.paths.get(e.path).filter(|p| si < p.pts.len()) {
                    let (hi, ho) = p.handles(si);
                    let c = p.pts[si].p;
                    for (k, h) in [(1u8, hi), (2u8, ho)] {
                        if h[0].abs() + h[1].abs() > 1e-6 && to_screen([c[0] + h[0], c[1] + h[1]]).distance(q) <= SPL_HIT {
                            return Some((k, e.path, si));
                        }
                    }
                }
            }
            let order: Vec<usize> = std::iter::once(e.path).chain((0..e.sp.paths.len()).filter(|i| *i != e.path)).collect();
            for pi in order {
                if let Some(p) = e.sp.paths.get(pi) {
                    for (i, pt) in p.pts.iter().enumerate().rev() {
                        if to_screen(pt.p).distance(q) <= SPL_HIT {
                            return Some((0, pi, i));
                        }
                    }
                }
            }
            None
        };
        // ---- pointer
        let pan_mode = alt || space || ui.input(|i| i.pointer.middle_down());
        if let Some(q) = hover {
            if ui.input(|i| i.pointer.secondary_pressed()) {
                e.menu_pt = hit_at(&e, q).filter(|h| h.0 == 0).map(|h| (h.1, h.2));
            }
            if !pan_mode && ui.input(|i| i.pointer.primary_pressed()) {
                match hit_at(&e, q) {
                    Some((k, pi, si)) => {
                        e.path = pi;
                        e.sel = Some(si);
                        e.drag = Some((k, si, false));
                    }
                    None => {
                        let uv = snapped(to_uv(q), e.snap);
                        // on the curve: add a point there; elsewhere: append one to the current path
                        let near = e.sp.paths.get(e.path).and_then(|p| p.nearest(to_uv(q), iw, ih)).filter(|n| n.2 <= SPL_HIT * 0.8);
                        want_snap = true;
                        if e.sp.paths.is_empty() {
                            e.sp.paths.push(SPath::default());
                            e.path = 0;
                        }
                        let pi = e.path.min(e.sp.paths.len() - 1);
                        let idx = match near {
                            Some((seg, t, _)) => e.sp.paths[pi].split(seg, t),
                            None => {
                                e.sp.paths[pi].pts.push(SPoint::new(uv[0], uv[1]));
                                e.sp.paths[pi].pts.len() - 1
                            }
                        };
                        e.sel = Some(idx);
                        e.drag = Some((0, idx, near.is_none()));
                        changed = true;
                    }
                }
            }
            if resp.double_clicked() && !pan_mode {
                if let Some((0, pi, si)) = hit_at(&e, q) {
                    want_snap = true;
                    let corner = e.sp.paths[pi].pts[si].tan != 3;
                    spl_set_tangent(&mut e.sp.paths[pi], si, if corner { 3 } else { 0 });
                    changed = true;
                }
            }
        }
        if let Some((k, si, moved)) = e.drag {
            if ui.input(|i| i.pointer.primary_down()) {
                if let Some(q) = ui.input(|i| i.pointer.latest_pos()) {
                    let uv = snapped(to_uv(q), e.snap);
                    let pi = e.path.min(e.sp.paths.len().saturating_sub(1));
                    if let Some(p) = e.sp.paths.get(pi).filter(|p| si < p.pts.len()) {
                        let (c, hi, ho) = (p.pts[si].p, p.handles(si).0, p.handles(si).1);
                        let differs = match k {
                            0 => (uv[0] - c[0]).abs() > 1e-5 || (uv[1] - c[1]).abs() > 1e-5,
                            1 => ((uv[0] - c[0]) - hi[0]).abs() > 1e-5 || ((uv[1] - c[1]) - hi[1]).abs() > 1e-5,
                            _ => ((uv[0] - c[0]) - ho[0]).abs() > 1e-5 || ((uv[1] - c[1]) - ho[1]).abs() > 1e-5,
                        };
                        if differs {
                            if !moved {
                                want_snap = true;
                            }
                            let pt = &mut e.sp.paths[pi].pts[si];
                            // Auto and Mirrored points move both handles together; Free and Corner points (or Alt) one at a time
                            let together = !alt && pt.tan <= 1;
                            match k {
                                0 => pt.p = uv,
                                1 => {
                                    let h = [uv[0] - c[0], uv[1] - c[1]];
                                    pt.hin = Some(h);
                                    if together {
                                        pt.hout = Some([-h[0], -h[1]]);
                                    } else if pt.hout.is_none() {
                                        pt.hout = Some(ho);
                                    }
                                }
                                _ => {
                                    let h = [uv[0] - c[0], uv[1] - c[1]];
                                    pt.hout = Some(h);
                                    if together {
                                        pt.hin = Some([-h[0], -h[1]]);
                                    } else if pt.hin.is_none() {
                                        pt.hin = Some(hi);
                                    }
                                }
                            }
                            if k != 0 {
                                pt.tan = if together { 1 } else { 2 };
                            }
                            e.drag = Some((k, si, true));
                            changed = true;
                        }
                    }
                }
            } else {
                e.drag = None;
            }
        }
        if want_snap {
            // the resource still holds the text from before this change, so the snapshot is the state to undo to
            self.snap();
        }
        if changed {
            self.spl_write(&mut e);
        }
        // ---- context menu
        let menu_pt = e.menu_pt;
        let mut del_pt = false;
        let mut corner = false;
        let mut smooth = false;
        let mut new_path_at: Option<[f32; 2]> = None;
        resp.context_menu(|ui| {
            if let Some((pi, si)) = menu_pt {
                if ui.button("Delete point").clicked() {
                    del_pt = true;
                    ui.close();
                }
                if ui.button("Corner point").clicked() {
                    corner = true;
                    ui.close();
                }
                if ui.button("Smooth point").clicked() {
                    smooth = true;
                    ui.close();
                }
                let _ = (pi, si);
            } else {
                let at = ui.input(|i| i.pointer.interact_pos()).map(to_uv);
                if ui.button("Start a new path here").clicked() {
                    new_path_at = at;
                    ui.close();
                }
            }
        });
        if let Some((pi, si)) = menu_pt {
            if del_pt || corner || smooth {
                self.snap();
                if let Some(p) = e.sp.paths.get_mut(pi) {
                    if si < p.pts.len() {
                        if del_pt {
                            p.pts.remove(si);
                            e.sel = None;
                        } else {
                            spl_set_tangent(p, si, if corner { 3 } else { 0 });
                        }
                    }
                }
                self.spl_write(&mut e);
            }
        } else if let Some(uv) = new_path_at {
            self.snap();
            let uv = snapped(uv, e.snap);
            e.sp.paths.push(SPath { pts: vec![SPoint::new(uv[0], uv[1])], ..Default::default() });
            e.path = e.sp.paths.len() - 1;
            e.sel = Some(0);
            self.spl_write(&mut e);
        }
        // ---- drawing
        let accent = Color32::from_rgb(90, 220, 190);
        for (pi, path) in e.sp.paths.iter().enumerate() {
            if path.pts.is_empty() {
                continue;
            }
            let poly = path.flatten(iw, ih);
            let mut pts: Vec<Pos2> = poly.pts.iter().map(|p| pos2(img.min.x + p[0], img.min.y + p[1])).collect();
            if poly.closed {
                if let Some(f) = pts.first().copied() {
                    pts.push(f);
                }
            }
            let cur = pi == e.path;
            let col = if cur { accent } else { Color32::from_rgb(150, 170, 190) };
            if pts.len() >= 2 {
                painter.add(egui::Shape::line(pts.clone(), Stroke::new(3.0_f32, Color32::from_black_alpha(150))));
                painter.add(egui::Shape::line(pts, Stroke::new(1.5_f32, col)));
            }
            for (i, pt) in path.pts.iter().enumerate() {
                let c = to_screen(pt.p);
                let sel = cur && e.sel == Some(i);
                let r = Rect::from_center_size(c, vec2(8.0, 8.0));
                painter.rect_filled(r, 1.0, if sel { Color32::WHITE } else { Color32::from_black_alpha(200) });
                painter.rect_stroke(r, 1.0, Stroke::new(1.5_f32, if i == 0 { Color32::from_rgb(255, 200, 80) } else { col }), egui::StrokeKind::Inside);
                if sel {
                    let (hi, ho) = path.handles(i);
                    for h in [hi, ho] {
                        if h[0].abs() + h[1].abs() > 1e-6 {
                            let hp = to_screen([pt.p[0] + h[0], pt.p[1] + h[1]]);
                            painter.line_segment([c, hp], Stroke::new(1.0_f32, Color32::from_gray(200)));
                            painter.circle_filled(hp, 4.0, accent);
                            painter.circle_stroke(hp, 4.0, Stroke::new(1.0_f32, Color32::BLACK));
                        }
                    }
                }
            }
        }
        self.spl = Some(e);
    }
}
