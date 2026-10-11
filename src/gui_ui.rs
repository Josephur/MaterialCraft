// ======================================================================= UI (included into gui.rs)

struct NodeSnap {
    id: u32,
    kind: String,
    slots: Vec<(&'static str, bool)>,
    ins: Vec<(String, u32)>,
    pos: [f32; 2],
    graph_name: Option<String>,
}

struct PRow {
    key: String,
    label: String,
    def: f32,
    lo: f32,
    hi: f32,
    int: bool,
    exposable: bool,
}

impl App {
    fn screen_to_world(&self, p: Pos2) -> Pos2 {
        let w = ((p - self.canvas_rect.min) - self.pan) / self.zoom;
        pos2(w.x, w.y)
    }

    fn open_search(&mut self, screen: Pos2, link_from: Option<u32>) {
        let w = self.screen_to_world(screen);
        self.search = Some(SearchState { screen, world: pos2(w.x - CARD * 0.5, w.y - CARD_H * 0.5), text: String::new(), idx: 0, link_from, fresh: true });
    }

    // ------------------------------------------------------------ graph canvas
    fn canvas(&mut self, ui: &mut egui::Ui) {
        let (resp, painter) = ui.allocate_painter(ui.available_size(), Sense::click_and_drag());
        let rect = resp.rect;
        self.canvas_rect = rect;
        painter.rect_filled(rect, 0.0, Color32::from_rgb(58, 58, 58));
        let mods = ui.input(|i| i.modifiers);
        let pointer = ui.input(|i| i.pointer.hover_pos());
        let released = ui.input(|i| i.pointer.any_released());
        let mut acts: Vec<Act> = Vec::new();

        if resp.hovered() {
            let sc = ui.input(|i| i.raw_scroll_delta.y);
            if sc != 0.0 {
                let old = self.zoom;
                let nz = (old * (1.0 + sc * 0.0012)).clamp(0.25, 2.5);
                if let Some(p) = pointer {
                    let wp = ((p - rect.min) - self.pan) / old;
                    self.pan = (p - rect.min) - wp * nz;
                }
                self.zoom = nz;
            }
        }
        let pan_mode = mods.alt || ui.input(|i| i.key_down(Key::Space));
        if resp.dragged_by(PointerButton::Middle) || (resp.dragged_by(PointerButton::Primary) && pan_mode && self.connecting.is_none()) {
            self.pan += resp.drag_delta();
        }
        if resp.drag_started_by(PointerButton::Primary) && !pan_mode && self.connecting.is_none() {
            self.rubber = pointer;
            if !mods.shift {
                self.sel.clear();
                self.sel_comment = None;
            }
        }
        if resp.clicked() && !mods.shift {
            self.sel.clear();
            self.sel_comment = None;
        }

        let z = self.zoom;
        let org = rect.min + self.pan;
        let ts = move |p: [f32; 2]| pos2(org.x + p[0] * z, org.y + p[1] * z);

        // grid
        let minor = Stroke::new(1.0, Color32::from_rgb(63, 63, 63));
        let major = Stroke::new(1.0, Color32::from_rgb(50, 50, 50));
        let step = 32.0 * z;
        if step > 6.0 {
            let (ox, oy) = (self.pan.x.rem_euclid(step), self.pan.y.rem_euclid(step));
            let mut x = rect.min.x + ox;
            let mut k = ((rect.min.x + ox - org.x) / step).round() as i64;
            while x < rect.max.x {
                painter.line_segment([pos2(x, rect.min.y), pos2(x, rect.max.y)], if k.rem_euclid(4) == 0 { major } else { minor });
                x += step;
                k += 1;
            }
            let mut y = rect.min.y + oy;
            let mut k = ((rect.min.y + oy - org.y) / step).round() as i64;
            while y < rect.max.y {
                painter.line_segment([pos2(rect.min.x, y), pos2(rect.max.x, y)], if k.rem_euclid(4) == 0 { major } else { minor });
                y += step;
                k += 1;
            }
        }

        // comment frames (drawn under nodes)
        for ci in 0..self.g.comments.len() {
            let c = self.g.comments[ci].clone();
            let r = Rect::from_min_size(ts(c.pos), vec2(c.size[0], c.size[1]) * z);
            let col = COMMENT_COLORS[c.color as usize % COMMENT_COLORS.len()];
            let base = Color32::from_rgb(col[0], col[1], col[2]);
            let selected = self.sel_comment == Some(ci);
            painter.rect_filled(r, Rounding::same(4.0 * z), base.gamma_multiply(0.16));
            let title = Rect::from_min_size(r.min, vec2(r.width(), 22.0 * z));
            painter.rect_filled(title, Rounding { nw: 4.0 * z, ne: 4.0 * z, sw: 0.0, se: 0.0 }, base.gamma_multiply(0.5));
            painter.rect_stroke(r, Rounding::same(4.0 * z), Stroke::new(if selected { 2.0 } else { 1.0 }, if selected { Color32::from_rgb(255, 160, 40) } else { base.gamma_multiply(0.8) }));
            painter.text(title.left_center() + vec2(8.0 * z, 0.0), Align2::LEFT_CENTER, &c.text, FontId::proportional(13.0 * z), Color32::WHITE);
            let tr = ui.interact(title, Id::new(("cmt", ci)), Sense::click_and_drag());
            if tr.drag_started() {
                self.snap();
                let cr = Rect::from_min_size(pos2(c.pos[0], c.pos[1]), vec2(c.size[0], c.size[1]));
                self.comment_members = self.g.nodes.iter().filter(|(_, n)| cr.contains(pos2(n.pos[0] + CARD * 0.5, n.pos[1] + CARD_H * 0.5))).map(|(i, _)| *i).collect();
                self.sel_comment = Some(ci);
                self.sel.clear();
            }
            if tr.clicked() {
                self.sel_comment = Some(ci);
                self.sel.clear();
            }
            if tr.dragged() {
                let d = tr.drag_delta() / z;
                self.g.comments[ci].pos[0] += d.x;
                self.g.comments[ci].pos[1] += d.y;
                for id in self.comment_members.clone() {
                    if let Some(n) = self.g.nodes.get_mut(&id) {
                        n.pos[0] += d.x;
                        n.pos[1] += d.y;
                    }
                }
            }
            let hs = vec2(14.0, 14.0) * z;
            let handle = Rect::from_min_size(r.max - hs, hs);
            painter.rect_filled(handle, Rounding::same(2.0), base.gamma_multiply(0.7));
            let hr = ui.interact(handle, Id::new(("cmtr", ci)), Sense::drag());
            if hr.drag_started() {
                self.snap();
            }
            if hr.dragged() {
                let d = hr.drag_delta() / z;
                self.g.comments[ci].size[0] = (self.g.comments[ci].size[0] + d.x).max(80.0);
                self.g.comments[ci].size[1] = (self.g.comments[ci].size[1] + d.y).max(50.0);
            }
        }

        // snapshot of nodes for this frame
        let snaps: Vec<NodeSnap> = self
            .g
            .nodes
            .iter()
            .map(|(id, n)| NodeSnap {
                id: *id,
                kind: n.kind.clone(),
                slots: slots(&n.kind),
                ins: n.ins.iter().map(|(k, v)| (k.clone(), *v)).collect(),
                pos: n.pos,
                graph_name: n.strs.get("graph").cloned(),
            })
            .collect();
        let posmap: HashMap<u32, [f32; 2]> = snaps.iter().map(|s| (s.id, s.pos)).collect();
        let out_port = move |p: [f32; 2]| ts(p) + vec2(CARD * z, (BAR + CARD * 0.5) * z);
        let in_port = move |p: [f32; 2], i: usize, n: usize| ts(p) + vec2(0.0, (BAR + CARD * (i as f32 + 1.0) / (n as f32 + 1.0)) * z);
        let link = |painter: &egui::Painter, a: Pos2, b: Pos2, col: Color32, w: f32| {
            let dx = ((b.x - a.x).abs() * 0.5).max(40.0 * z);
            painter.add(CubicBezierShape::from_points_stroke([a, a + vec2(dx, 0.0), b - vec2(dx, 0.0), b], false, Color32::TRANSPARENT, Stroke::new(w * z.max(0.7), col)));
        };

        // links
        for s in &snaps {
            let n = s.slots.len();
            for (slot, src) in &s.ins {
                if let (Some(i), Some(sp)) = (s.slots.iter().position(|x| x.0 == slot), posmap.get(src)) {
                    let hot = self.sel.contains(&s.id) || self.sel.contains(src);
                    link(&painter, out_port(*sp), in_port(s.pos, i, n), if hot { Color32::from_rgb(255, 170, 60) } else { Color32::from_rgb(185, 185, 185) }, 1.8);
                }
            }
        }

        let in_sub = !self.parents.is_empty();
        let out_names: Vec<&'static str> = if in_sub { vec!["out"] } else { OUTPUT_NAMES.to_vec() };
        let mut ports: Vec<(u32, &'static str, Pos2)> = Vec::new();
        let mut cards: Vec<Rect> = Vec::new();

        for s in &snaps {
            let id = s.id;
            let card = Rect::from_min_size(ts(s.pos), vec2(CARD, CARD_H) * z);
            cards.push(card);
            let selected = self.sel.contains(&id);
            let err = self.node_err.get(&id).cloned();
            painter.rect_filled(card.translate(vec2(2.0, 3.0) * z), Rounding::same(3.0 * z), Color32::from_black_alpha(70));
            painter.rect_filled(card, Rounding::same(3.0 * z), Color32::from_rgb(32, 32, 32));
            let bar = Rect::from_min_size(card.min, vec2(card.width(), BAR * z));
            painter.rect_filled(bar, Rounding { nw: 3.0 * z, ne: 3.0 * z, sw: 0.0, se: 0.0 }, kind_color(&s.kind));
            let label = match (&s.graph_name, s.kind.as_str()) {
                (Some(g), "subgraph") => g.clone(),
                _ => s.kind.replace('_', " "),
            };
            let fs = if label.len() > 15 { 9.0 } else { 10.5 };
            painter.text(bar.center(), Align2::CENTER_CENTER, label, FontId::proportional(fs * z), Color32::WHITE);
            let th = Rect::from_min_size(card.min + vec2(3.0, BAR + 3.0) * z, vec2(CARD - 6.0, CARD - 6.0) * z);
            match self.thumbs.get(&id) {
                Some(t) => {
                    painter.image(t.id(), th, Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)), Color32::WHITE);
                }
                None => {
                    painter.rect_filled(th, 0.0, Color32::from_rgb(22, 22, 22));
                }
            }
            if err.is_some() {
                painter.rect_filled(th, 0.0, Color32::from_rgba_unmultiplied(170, 30, 30, 150));
                painter.text(th.center(), Align2::CENTER_CENTER, "!", FontId::proportional(30.0 * z), Color32::WHITE);
            }
            painter.rect_stroke(card, Rounding::same(3.0 * z), Stroke::new(if selected { 2.4 } else { 1.0 }, if selected { Color32::from_rgb(255, 160, 40) } else { Color32::from_rgb(14, 14, 14) }));
            if self.active == Some(id) && !selected {
                painter.rect_stroke(card, Rounding::same(3.0 * z), Stroke::new(1.5, Color32::from_rgb(150, 150, 150)));
            }

            let outs: Vec<&str> = self.g.outputs.iter().filter(|(_, o)| *o == id).map(|(n, _)| n.as_str()).collect();
            if !outs.is_empty() {
                let txt = outs.join(", ");
                let r = painter.text(card.left_bottom() + vec2(2.0, 3.0) * z, Align2::LEFT_TOP, &txt, FontId::proportional(10.0 * z), Color32::BLACK);
                painter.rect_filled(r.expand(2.0 * z), Rounding::same(2.0 * z), Color32::from_rgb(240, 170, 50));
                painter.text(r.left_top(), Align2::LEFT_TOP, &txt, FontId::proportional(10.0 * z), Color32::BLACK);
            }

            // interaction: card
            let nr = ui.interact(card, Id::new(("node", id)), Sense::click_and_drag());
            if nr.drag_started() {
                self.snap();
                if !self.sel.contains(&id) {
                    if !(mods.shift || mods.command) {
                        self.sel.clear();
                    }
                    self.sel.insert(id);
                }
                self.sel_comment = None;
                if self.active != Some(id) {
                    self.active = Some(id);
                    self.dirty = true;
                }
            }
            if nr.dragged() && self.connecting.is_none() {
                let d = nr.drag_delta() / z;
                for sid in self.sel.clone() {
                    if let Some(n) = self.g.nodes.get_mut(&sid) {
                        n.pos[0] += d.x;
                        n.pos[1] += d.y;
                    }
                }
            }
            if nr.clicked() {
                if mods.shift || mods.command {
                    if !self.sel.remove(&id) {
                        self.sel.insert(id);
                    }
                } else {
                    self.sel = [id].into_iter().collect();
                }
                self.sel_comment = None;
                if self.active != Some(id) {
                    self.active = Some(id);
                    self.dirty = true;
                }
            }
            if nr.double_clicked() {
                if let (Some(g), "subgraph") = (&s.graph_name, s.kind.as_str()) {
                    acts.push(Act::EnterSub(g.clone()));
                }
            }
            if nr.secondary_clicked() && !self.sel.contains(&id) {
                self.sel = [id].into_iter().collect();
                self.active = Some(id);
                self.dirty = true;
            }
            if let Some(e) = &err {
                nr.clone().on_hover_text(e);
            }
            let (kind, gname) = (s.kind.clone(), s.graph_name.clone());
            nr.context_menu(|ui| {
                if ui.button("Copy").clicked() {
                    acts.push(Act::Copy);
                    ui.close_menu();
                }
                if ui.button("Duplicate").clicked() {
                    acts.push(Act::Duplicate);
                    ui.close_menu();
                }
                if ui.button("Delete").clicked() {
                    acts.push(Act::Delete);
                    ui.close_menu();
                }
                ui.separator();
                if kind == "subgraph" {
                    if let Some(g) = &gname {
                        if ui.button("Open subgraph").clicked() {
                            acts.push(Act::EnterSub(g.clone()));
                            ui.close_menu();
                        }
                    }
                }
                if ui.button("Create subgraph from selection").clicked() {
                    acts.push(Act::Subgraph);
                    ui.close_menu();
                }
                ui.menu_button("Set as output", |ui| {
                    for name in &out_names {
                        if ui.button(*name).clicked() {
                            acts.push(Act::SetOutput(name.to_string(), id));
                            ui.close_menu();
                        }
                    }
                });
            });

            // ports
            let op = out_port(s.pos);
            painter.circle(op, 4.8 * z, Color32::from_rgb(235, 235, 235), Stroke::new(1.0, Color32::BLACK));
            let pr = ui.interact(Rect::from_center_size(op, Vec2::splat(14.0 * z)), Id::new(("out", id)), Sense::drag());
            if pr.drag_started() {
                self.connecting = Some(id);
            }
            let n = s.slots.len();
            for (i, (slot, required)) in s.slots.iter().enumerate() {
                let ip = in_port(s.pos, i, n);
                ports.push((id, *slot, ip));
                painter.circle(ip, 4.5 * z, if *required { Color32::from_rgb(210, 210, 210) } else { Color32::from_rgb(60, 60, 60) }, Stroke::new(1.2, Color32::from_rgb(190, 190, 190)));
                let ir = ui.interact(Rect::from_center_size(ip, Vec2::splat(14.0 * z)), Id::new(("in", id, i)), Sense::drag());
                if ir.drag_started() {
                    let had = self.g.nodes.get(&id).and_then(|nd| nd.ins.get(*slot).copied());
                    if let Some(old) = had {
                        self.snap();
                        if let Some(nd) = self.g.nodes.get_mut(&id) {
                            nd.ins.remove(*slot);
                        }
                        self.connecting = Some(old);
                        self.dirty = true;
                    }
                }
                if ir.hovered() {
                    painter.text(ip + vec2(-8.0 * z, 0.0), Align2::RIGHT_CENTER, *slot, FontId::proportional(10.5 * z.max(0.8)), Color32::WHITE);
                }
            }
        }

        // connection drag
        if let Some(src) = self.connecting {
            if let (Some(pt), Some(sp)) = (pointer, posmap.get(&src)) {
                link(&painter, out_port(*sp), pt, Color32::from_rgb(255, 190, 70), 2.0);
            }
            if released {
                if let Some(pt) = pointer {
                    let near = ports.iter().filter(|(_, _, p)| (*p - pt).length() < 14.0 * z).min_by(|a, b| (a.2 - pt).length().partial_cmp(&(b.2 - pt).length()).unwrap_or(std::cmp::Ordering::Equal));
                    if let Some((dst, slot, _)) = near {
                        let (dst, slot) = (*dst, slot.to_string());
                        self.try_connect(src, dst, &slot);
                    } else if rect.contains(pt) && !cards.iter().any(|c| c.contains(pt)) {
                        self.open_search(pt, Some(src));
                    }
                }
                self.connecting = None;
            }
        }

        // rubber-band selection
        if let Some(start) = self.rubber {
            if let Some(pt) = pointer {
                let r = Rect::from_two_pos(start, pt);
                painter.rect(r, 0.0, Color32::from_rgba_unmultiplied(255, 160, 40, 28), Stroke::new(1.0, Color32::from_rgb(255, 160, 40)));
                if released {
                    for (s, c) in snaps.iter().zip(&cards) {
                        if r.intersects(*c) {
                            self.sel.insert(s.id);
                        }
                    }
                    if self.active.is_none() {
                        self.active = self.sel.iter().next().copied();
                        self.dirty = true;
                    }
                    self.rubber = None;
                }
            } else if released {
                self.rubber = None;
            }
        }

        // background context menu
        let click_pos = pointer;
        resp.context_menu(|ui| {
            if ui.button("Add node...   Tab").clicked() {
                acts.push(Act::Search(click_pos));
                ui.close_menu();
            }
            if ui.add_enabled(!self.clip.is_empty(), egui::Button::new("Paste")).clicked() {
                acts.push(Act::Paste(click_pos.map(|p| self.screen_to_world(p))));
                ui.close_menu();
            }
            if ui.button("Add comment frame   C").clicked() {
                acts.push(Act::Comment);
                ui.close_menu();
            }
            ui.separator();
            if ui.button("Select all").clicked() {
                acts.push(Act::SelectAll);
                ui.close_menu();
            }
            if ui.button("Frame all   F").clicked() {
                acts.push(Act::FrameAll);
                ui.close_menu();
            }
            if ui.button("Auto-layout").clicked() {
                acts.push(Act::AutoLayout);
                ui.close_menu();
            }
        });

        if self.g.nodes.is_empty() {
            painter.text(rect.center(), Align2::CENTER_CENTER, "Press Tab to add a node", FontId::proportional(16.0), Color32::from_gray(130));
        }
        self.acts.extend(acts);
    }

    // ------------------------------------------------------------ inspector
    fn param_rows(&self, id: u32) -> Vec<PRow> {
        let Some(n) = self.g.nodes.get(&id) else { return vec![] };
        let mut rows = Vec::new();
        if n.kind == "subgraph" {
            if let Some(sub) = n.strs.get("graph").and_then(|g| self.g.subgraphs.get(g)) {
                for (nid, inner) in &sub.nodes {
                    for (name, def, lo, hi) in params(&inner.kind) {
                        rows.push(PRow {
                            key: format!("{nid}.{name}"),
                            label: format!("#{nid} {} . {name}", inner.kind.replace('_', " ")),
                            def: inner.nums.get(*name).copied().unwrap_or(*def),
                            lo: *lo,
                            hi: *hi,
                            int: is_int(&inner.kind, name),
                            exposable: false,
                        });
                    }
                }
            }
        } else {
            for (name, def, lo, hi) in params(&n.kind) {
                rows.push(PRow { key: name.to_string(), label: name.to_string(), def: *def, lo: *lo, hi: *hi, int: is_int(&n.kind, name), exposable: true });
            }
        }
        rows
    }

    fn inspector(&mut self, ui: &mut egui::Ui) {
        ui.spacing_mut().slider_width = 130.0;
        if let Some(ci) = self.sel_comment.filter(|c| *c < self.g.comments.len()) {
            ui.heading("Comment frame");
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
            ui.separator();
        }

        if let Some(id) = self.active.filter(|a| self.g.nodes.contains_key(a)) {
            let node = self.g.nodes[&id].clone();
            let kind = node.kind.clone();
            ui.horizontal(|ui| {
                let (r, _) = ui.allocate_exact_size(vec2(6.0, 18.0), Sense::hover());
                ui.painter().rect_filled(r, 1.0, kind_color(&kind));
                ui.heading(format!("{}  #{id}", kind.replace('_', " ")));
            });
            if let Some(sp) = spec(&kind) {
                ui.small(sp.doc);
            }
            if let Some(e) = self.node_err.get(&id) {
                ui.colored_label(Color32::from_rgb(255, 120, 110), e);
            }
            ui.add_space(4.0);
            let rows = self.param_rows(id);
            let mut edits: Vec<(String, f32)> = Vec::new();
            let mut expose_toggle: Option<(String, f32, f32)> = None;
            for r in &rows {
                let mut v = node.nums.get(&r.key).copied().unwrap_or(r.def);
                let exposed = self.g.exposed.iter().any(|e| e.node == id && e.param == r.key);
                ui.horizontal(|ui| {
                    let mut sl = egui::Slider::new(&mut v, r.lo..=r.hi).text(&r.label);
                    if r.int {
                        sl = sl.step_by(1.0).fixed_decimals(0);
                    }
                    if ui.add(sl).changed() {
                        edits.push((r.key.clone(), v));
                    }
                    if r.exposable && ui.selectable_label(exposed, "P").on_hover_text("Publish this parameter (exposed in .mfa packages)").clicked() {
                        expose_toggle = Some((r.key.clone(), r.lo, r.hi));
                    }
                });
            }
            let mut str_edits: Vec<(String, Option<String>)> = Vec::new();
            match kind.as_str() {
                "pixel_processor" => {
                    ui.small("vars: u v x y w h r g b l   funcs: sin cos abs sqrt min max pow floor fract clamp mix step smoothstep noise rand tex atan2 exp log");
                    for key in ["expr", "expr_g", "expr_b", "expr_a"] {
                        let mut s = node.strs.get(key).cloned().unwrap_or_default();
                        if ui.add(egui::TextEdit::singleline(&mut s).hint_text(key).desired_width(f32::INFINITY)).changed() {
                            str_edits.push((key.to_string(), if s.trim().is_empty() { None } else { Some(s.replace('"', "")) }));
                        }
                    }
                }
                "bitmap" => {
                    let file = node.strs.get("file").cloned().unwrap_or_default();
                    ui.label(if file.is_empty() { "(no file)".to_string() } else { file.clone() });
                    if ui.button("Browse PNG...").clicked() {
                        if let Some(p) = rfd::FileDialog::new().add_filter("PNG image", &["png"]).pick_file() {
                            str_edits.push(("file".into(), Some(p.to_string_lossy().to_string())));
                        }
                    }
                }
                "subgraph" => {
                    if let Some(g) = node.strs.get("graph") {
                        ui.label(format!("Instance of '{g}'"));
                        if ui.button("Open subgraph").clicked() {
                            self.acts.push(Act::EnterSub(g.clone()));
                        }
                        if rows.is_empty() {
                            ui.small("This subgraph has no adjustable parameters.");
                        }
                    }
                }
                _ => {}
            }
            if !edits.is_empty() || !str_edits.is_empty() {
                if !self.edit_open {
                    self.snap();
                    self.edit_open = true;
                }
                if let Some(n) = self.g.nodes.get_mut(&id) {
                    for (k, v) in edits {
                        n.nums.insert(k, v);
                    }
                    for (k, v) in str_edits {
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
            if let Some((key, lo, hi)) = expose_toggle {
                self.snap();
                if let Some(p) = self.g.exposed.iter().position(|e| e.node == id && e.param == key) {
                    self.g.exposed.remove(p);
                } else {
                    let label = format!("{} {} {}", kind.replace('_', " "), id, key);
                    self.g.exposed.push(Exposed { label, node: id, param: key, min: lo, max: hi });
                }
            }
            ui.separator();
        } else if self.sel_comment.is_none() {
            ui.label("Select a node to edit its parameters.");
            ui.separator();
        }

        egui::CollapsingHeader::new("Graph").default_open(true).show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.label("Output size");
                egui::ComboBox::from_id_salt("gsize").selected_text(format!("{0} x {0}", self.g.size)).show_ui(ui, |ui| {
                    for s in [128usize, 256, 512, 1024, 2048, 4096] {
                        ui.selectable_value(&mut self.g.size, s, format!("{s} x {s}"));
                    }
                });
            });
            ui.horizontal(|ui| {
                ui.label("Random seed");
                let mut so = self.g.seed_offset;
                if ui.add(egui::DragValue::new(&mut so).speed(1)).changed() {
                    self.snap();
                    self.g.seed_offset = so;
                    self.dirty = true;
                }
            });
        });
        egui::CollapsingHeader::new("Outputs").default_open(true).show(ui, |ui| {
            let mut remove = None;
            for (i, (name, id)) in self.g.outputs.iter().enumerate() {
                ui.horizontal(|ui| {
                    ui.label(format!("{name}  <-  #{id}"));
                    if ui.small_button("x").clicked() {
                        remove = Some(i);
                    }
                });
            }
            if let Some(i) = remove {
                self.snap();
                self.g.outputs.remove(i);
                self.dirty = true;
            }
            if let Some(id) = self.active {
                ui.small("Assign the active node to:");
                let names: Vec<&str> = if self.parents.is_empty() { OUTPUT_NAMES.to_vec() } else { vec!["out"] };
                ui.horizontal_wrapped(|ui| {
                    for name in names {
                        if ui.small_button(name).clicked() {
                            self.acts.push(Act::SetOutput(name.to_string(), id));
                        }
                    }
                });
            }
        });
        egui::CollapsingHeader::new("Published parameters").default_open(false).show(ui, |ui| {
            if self.g.exposed.is_empty() {
                ui.small("Click 'P' next to a parameter to publish it. Published parameters can be set when rendering a .mfa package.");
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

    // ------------------------------------------------------------ 2D / 3D views
    fn view2d_ui(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            ui.strong("2D view");
            if let Some(id) = self.active.filter(|a| self.g.nodes.contains_key(a)) {
                ui.label(format!("{} #{id}  {}x{}", self.g.nodes[&id].kind.replace('_', " "), self.tex2d_size.0, self.tex2d_size.1));
            }
            ui.checkbox(&mut self.tile_preview, "tile 2x2");
            let before = self.prev_size;
            egui::ComboBox::from_id_salt("psize").width(70.0).selected_text(format!("{}px", self.prev_size)).show_ui(ui, |ui| {
                for s in [128usize, 256, 512, 1024] {
                    ui.selectable_value(&mut self.prev_size, s, format!("{s}px"));
                }
            });
            if before != self.prev_size {
                self.dirty = true;
            }
        });
        let avail = ui.available_size();
        let side = avail.x.min(avail.y).max(32.0);
        let (rect, _) = ui.allocate_exact_size(avail, Sense::hover());
        let img_rect = Rect::from_center_size(rect.center(), vec2(side, side));
        ui.painter().rect_filled(img_rect, 0.0, Color32::from_rgb(20, 20, 20));
        if let Some(t) = &self.tex2d {
            let uv = if self.tile_preview { Rect::from_min_max(pos2(0.0, 0.0), pos2(2.0, 2.0)) } else { Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)) };
            ui.painter().image(t.id(), img_rect, uv, Color32::WHITE);
        } else {
            ui.painter().text(img_rect.center(), Align2::CENTER_CENTER, "select a node", FontId::proportional(13.0), Color32::from_gray(120));
        }
    }

    fn view3d_ui(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            ui.strong("3D view");
            for (i, n) in ["Sphere", "Cube", "Plane"].iter().enumerate() {
                if ui.selectable_label(self.view3d.shape == i as u32, *n).clicked() {
                    self.view3d.shape = i as u32;
                    self.dirty3d = true;
                }
            }
            let mut ch = false;
            ch |= ui.add(egui::DragValue::new(&mut self.view3d.tiling).speed(0.05).range(0.25..=16.0).prefix("tile ")).changed();
            ch |= ui.add(egui::DragValue::new(&mut self.view3d.exposure).speed(0.02).range(0.1..=4.0).prefix("exp ")).changed();
            ch |= ui.add(egui::DragValue::new(&mut self.view3d.light_intensity).speed(0.05).range(0.0..=10.0).prefix("light ")).changed();
            if ch {
                self.dirty3d = true;
            }
        });
        let avail = ui.available_size();
        let side = avail.x.min(avail.y).max(32.0);
        let (rect, resp) = ui.allocate_exact_size(avail, Sense::click_and_drag());
        let img_rect = Rect::from_center_size(rect.center(), vec2(side, side));
        if resp.dragged() {
            let d = resp.drag_delta();
            if ui.input(|i| i.modifiers.shift) {
                self.view3d.light_yaw -= d.x * 0.01;
                self.view3d.light_pitch = (self.view3d.light_pitch + d.y * 0.01).clamp(-1.4, 1.4);
            } else {
                self.view3d.yaw -= d.x * 0.01;
                self.view3d.pitch = (self.view3d.pitch + d.y * 0.01).clamp(-1.4, 1.4);
            }
            self.dirty3d = true;
        }
        if resp.hovered() {
            let sc = ui.input(|i| i.raw_scroll_delta.y);
            if sc != 0.0 {
                self.view3d.dist = (self.view3d.dist * (1.0 - sc * 0.001)).clamp(1.6, 9.0);
                self.dirty3d = true;
            }
        }
        if let Some(t) = &self.tex3d {
            ui.painter().image(t.id(), img_rect, Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)), Color32::WHITE);
        }
        ui.painter().text(rect.left_bottom() + vec2(4.0, -4.0), Align2::LEFT_BOTTOM, "drag: orbit   shift+drag: light   wheel: zoom", FontId::proportional(10.0), Color32::from_gray(120));
    }

    // ------------------------------------------------------------ actions
    fn do_act(&mut self, a: Act) {
        match a {
            Act::New => {
                self.parents.clear();
                self.g = Graph::default();
                self.file = None;
                self.clear_history();
                self.thumbs.clear();
                self.fit_view();
            }
            Act::Open => self.open_file(),
            Act::Save => self.save(),
            Act::SaveAs => self.save_as(),
            Act::Publish => self.publish(),
            Act::ImportBitmap => self.import_bitmap(),
            Act::Export => self.export.open = true,
            Act::Bake => self.bake.open = true,
            Act::Undo => self.undo(),
            Act::Redo => self.redo(),
            Act::Copy => self.copy(),
            Act::Cut => {
                self.copy();
                self.delete_selected();
            }
            Act::Paste(at) => self.paste(at),
            Act::Duplicate => {
                self.copy();
                self.paste(None);
            }
            Act::Delete => self.delete_selected(),
            Act::SelectAll => {
                self.sel = self.g.nodes.keys().copied().collect();
                self.active = self.sel.iter().next().copied();
                self.dirty = true;
            }
            Act::Comment => self.add_comment(),
            Act::Subgraph => self.make_subgraph(),
            Act::EnterSub(n) => {
                if self.parents.iter().any(|(_, name)| *name == n) {
                    return;
                }
                if !self.g.subgraphs.contains_key(&n) {
                    self.goto_root();
                }
                self.enter_sub(&n);
            }
            Act::ExitSub => self.exit_sub(),
            Act::AutoLayout => {
                self.snap();
                auto_layout(&mut self.g);
                self.fit_view();
            }
            Act::FrameAll => self.fit_view(),
            Act::Search(p) => {
                let p = p.unwrap_or_else(|| self.canvas_rect.center());
                self.open_search(p, None);
            }
            Act::SetOutput(name, id) => {
                self.snap();
                self.g.outputs.retain(|(n, _)| *n != name);
                self.g.outputs.push((name, id));
                self.dirty = true;
            }
        }
    }

    fn search_ui(&mut self, ctx: &egui::Context) {
        let Some(mut st) = self.search.take() else { return };
        let q = st.text.to_lowercase();
        let mut list: Vec<(u8, Entry)> = self
            .entries()
            .into_iter()
            .filter_map(|e| {
                let l = e.label.to_lowercase();
                let s = if q.is_empty() {
                    1
                } else if l.starts_with(&q) {
                    0
                } else if l.contains(&q) {
                    1
                } else if e.doc.to_lowercase().contains(&q) {
                    2
                } else {
                    return None;
                };
                Some((s, e))
            })
            .collect();
        list.sort_by(|a, b| (a.0, a.1.cat, &a.1.label).cmp(&(b.0, b.1.cat, &b.1.label)));
        list.truncate(14);
        let (down, up, enter, esc) = ctx.input_mut(|i| {
            (i.consume_key(Modifiers::NONE, Key::ArrowDown), i.consume_key(Modifiers::NONE, Key::ArrowUp), i.consume_key(Modifiers::NONE, Key::Enter), i.consume_key(Modifiers::NONE, Key::Escape))
        });
        if down {
            st.idx += 1;
        }
        if up {
            st.idx = st.idx.saturating_sub(1);
        }
        st.idx = st.idx.min(list.len().saturating_sub(1));
        let mut chosen: Option<Entry> = None;
        let mut close = esc;
        let area = egui::Area::new(Id::new("node_search")).order(egui::Order::Foreground).fixed_pos(st.screen).show(ctx, |ui| {
            egui::Frame::popup(ui.style()).show(ui, |ui| {
                ui.set_width(280.0);
                let r = ui.add(egui::TextEdit::singleline(&mut st.text).hint_text("Search nodes - Enter to add").desired_width(f32::INFINITY));
                r.request_focus();
                if r.changed() {
                    st.idx = 0;
                }
                ui.separator();
                if list.is_empty() {
                    ui.small("no matching nodes");
                }
                for (i, (_, e)) in list.iter().enumerate() {
                    let resp = ui.selectable_label(i == st.idx, format!("{}    ({})", e.label, e.cat));
                    if resp.clicked() {
                        chosen = Some(e.clone());
                    }
                    if i == st.idx {
                        ui.small(&e.doc);
                    }
                }
            });
        });
        if enter && chosen.is_none() {
            chosen = list.get(st.idx).map(|x| x.1.clone());
        }
        if !st.fresh {
            let clicked_outside = ctx.input(|i| i.pointer.any_click()) && ctx.input(|i| i.pointer.interact_pos()).map(|p| !area.response.rect.contains(p)).unwrap_or(false);
            if clicked_outside {
                close = true;
            }
        }
        st.fresh = false;
        if let Some(e) = chosen {
            let (w, l) = (st.world, st.link_from);
            self.spawn(&e, w, l);
            close = true;
        }
        if !close {
            self.search = Some(st);
        }
    }

    fn windows(&mut self, ctx: &egui::Context) {
        if self.export.open {
            let mut open = true;
            egui::Window::new("Export textures").open(&mut open).collapsible(false).resizable(false).show(ctx, |ui| {
                ui.horizontal(|ui| {
                    if ui.button("Choose folder...").clicked() {
                        if let Some(d) = rfd::FileDialog::new().pick_folder() {
                            self.export.dir = Some(d);
                        }
                    }
                    ui.label(self.export.dir.as_ref().map(|d| d.display().to_string()).unwrap_or_else(|| "(none)".into()));
                });
                ui.horizontal(|ui| {
                    ui.label("File prefix");
                    ui.text_edit_singleline(&mut self.export.prefix);
                });
                ui.label("Resolutions (one sub-folder each when several are selected):");
                ui.horizontal(|ui| {
                    for (i, s) in [256, 512, 1024, 2048, 4096].iter().enumerate() {
                        ui.checkbox(&mut self.export.sizes[i], s.to_string());
                    }
                });
                ui.checkbox(&mut self.export.depth16, "16-bit PNG");
                ui.small(format!("Outputs: {}", self.g.outputs.iter().map(|o| o.0.as_str()).collect::<Vec<_>>().join(", ")));
                if ui.button("Export").clicked() {
                    self.run_export();
                }
            });
            self.export.open = open;
        }
        if self.bake.open {
            let mut open = true;
            egui::Window::new("Mesh map baker").open(&mut open).collapsible(false).resizable(false).show(ctx, |ui| {
                let busy = self.bake.job.is_some();
                let pick = |label: &str, slot: &mut Option<PathBuf>, ui: &mut egui::Ui| {
                    ui.horizontal(|ui| {
                        if ui.button(label).clicked() {
                            if let Some(p) = rfd::FileDialog::new().add_filter("Wavefront OBJ", &["obj"]).pick_file() {
                                *slot = Some(p);
                            }
                        }
                        ui.label(slot.as_ref().map(|p| p.display().to_string()).unwrap_or_else(|| "(none)".into()));
                    });
                };
                pick("Low-poly mesh (.obj, needs UVs)", &mut self.bake.low, ui);
                pick("High-poly mesh (.obj)", &mut self.bake.high, ui);
                ui.horizontal(|ui| {
                    if ui.button("Output folder...").clicked() {
                        if let Some(d) = rfd::FileDialog::new().pick_folder() {
                            self.bake.out = Some(d);
                        }
                    }
                    ui.label(self.bake.out.as_ref().map(|p| p.display().to_string()).unwrap_or_else(|| "(none)".into()));
                });
                if ui.button("Use demo sphere meshes").clicked() {
                    let d = std::env::temp_dir().join("matforge_demo");
                    let _ = std::fs::create_dir_all(&d);
                    let (lo, hi) = (d.join("low.obj"), d.join("high.obj"));
                    if std::fs::write(&lo, bake::uv_sphere_obj(24, 16, 1.0, 0.0)).is_ok() && std::fs::write(&hi, bake::uv_sphere_obj(160, 100, 1.0, 0.04)).is_ok() {
                        self.bake.low = Some(lo);
                        self.bake.high = Some(hi);
                        if self.bake.out.is_none() {
                            self.bake.out = Some(d.join("baked"));
                        }
                    }
                }
                ui.separator();
                egui::ComboBox::from_label("Resolution").selected_text(self.bake.s.size.to_string()).show_ui(ui, |ui| {
                    for s in [256usize, 512, 1024, 2048] {
                        ui.selectable_value(&mut self.bake.s.size, s, s.to_string());
                    }
                });
                ui.add(egui::Slider::new(&mut self.bake.s.max_dist, 0.005..=0.5).text("Max ray distance (x model size)"));
                ui.add(egui::Slider::new(&mut self.bake.s.ao_rays, 4..=256).text("AO / thickness rays"));
                ui.add(egui::Slider::new(&mut self.bake.s.ao_dist, 0.02..=1.0).text("AO / thickness distance"));
                ui.add(egui::Slider::new(&mut self.bake.s.padding, 0..=32).text("Edge padding (px)"));
                ui.small("Bakes: normal, world normal, position, ambient occlusion, thickness, curvature, ID.");
                if ui.add_enabled(!busy, egui::Button::new(if busy { "Baking..." } else { "Bake" })).clicked() {
                    self.start_bake();
                }
                if !self.bake.msg.is_empty() {
                    ui.label(&self.bake.msg);
                }
            });
            self.bake.open = open;
        }
        if self.show_about {
            let mut open = true;
            egui::Window::new("Keyboard shortcuts").open(&mut open).collapsible(false).show(ctx, |ui| {
                for (k, d) in [
                    ("Tab", "add a node at the cursor (search)"),
                    ("Drag link to empty space", "add a node and auto-connect"),
                    ("Ctrl+Z / Ctrl+Y", "undo / redo"),
                    ("Ctrl+C / X / V / D", "copy / cut / paste / duplicate"),
                    ("Ctrl+A", "select all"),
                    ("Shift/Ctrl + click", "toggle selection"),
                    ("Drag background", "rubber-band select"),
                    ("Middle-drag, Alt+drag, Space+drag", "pan"),
                    ("Wheel", "zoom"),
                    ("F", "frame all"),
                    ("C", "comment frame around selection"),
                    ("Delete", "delete selection"),
                    ("Double-click subgraph", "open it"),
                    ("Ctrl+S / Ctrl+O", "save / open"),
                ] {
                    ui.horizontal(|ui| {
                        ui.strong(k);
                        ui.label(d);
                    });
                }
            });
            self.show_about = open;
        }
    }
}

impl eframe::App for App {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        if !ctx.input(|i| i.pointer.any_down()) {
            self.edit_open = false;
        }
        self.poll_eval(ctx);
        if let Some(rx) = &self.bake.job {
            match rx.try_recv() {
                Ok(Ok(files)) => {
                    self.bake.job = None;
                    self.bake.msg = format!("Done: {} maps written. Added as bitmap nodes.", files.len());
                    let base = self.view_center_world();
                    for (i, f) in files.iter().enumerate() {
                        let id = self.add_node_at("bitmap", None, pos2(base.x + (i as f32 % 4.0) * (CARD + 20.0), base.y + (i / 4) as f32 * (CARD_H + 30.0)));
                        if let Some(n) = self.g.nodes.get_mut(&id) {
                            n.strs.insert("file".into(), f.to_string_lossy().to_string());
                        }
                    }
                }
                Ok(Err(e)) => {
                    self.bake.job = None;
                    self.bake.msg = format!("Bake failed: {e}");
                }
                Err(TryRecvError::Empty) => ctx.request_repaint_after(std::time::Duration::from_millis(100)),
                Err(TryRecvError::Disconnected) => {
                    self.bake.job = None;
                    self.bake.msg = "Bake failed unexpectedly.".into();
                }
            }
        }
        if self.dirty3d {
            self.render_3d(ctx);
        }

        // keyboard shortcuts
        let pointer = ctx.input(|i| i.pointer.hover_pos()).filter(|p| self.canvas_rect.contains(*p));
        if self.search.is_some() {
            ctx.input_mut(|i| i.consume_key(Modifiers::NONE, Key::Tab));
        } else if !self.text_focus {
            let tab = ctx.input_mut(|i| i.consume_key(Modifiers::NONE, Key::Tab));
            if tab {
                ctx.memory_mut(|m| {
                    if let Some(id) = m.focused() {
                        m.surrender_focus(id);
                    }
                });
                self.open_search(pointer.unwrap_or_else(|| self.canvas_rect.center()), None);
            }
        }
        if self.search.is_none() && !self.text_focus {
            let k = ctx.input_mut(|i| {
                let redo = i.consume_key(Modifiers::COMMAND | Modifiers::SHIFT, Key::Z) || i.consume_key(Modifiers::COMMAND, Key::Y);
                let undo = i.consume_key(Modifiers::COMMAND, Key::Z);
                let copy = i.events.iter().any(|e| matches!(e, egui::Event::Copy)) || i.consume_key(Modifiers::COMMAND, Key::C);
                let cut = i.events.iter().any(|e| matches!(e, egui::Event::Cut)) || i.consume_key(Modifiers::COMMAND, Key::X);
                let paste = i.events.iter().any(|e| matches!(e, egui::Event::Paste(_))) || i.consume_key(Modifiers::COMMAND, Key::V);
                let dup = i.consume_key(Modifiers::COMMAND, Key::D);
                let all = i.consume_key(Modifiers::COMMAND, Key::A);
                let save = i.consume_key(Modifiers::COMMAND, Key::S);
                let open = i.consume_key(Modifiers::COMMAND, Key::O);
                let del = i.consume_key(Modifiers::NONE, Key::Delete) || i.consume_key(Modifiers::NONE, Key::Backspace);
                let frame = i.consume_key(Modifiers::NONE, Key::F);
                let com = i.consume_key(Modifiers::NONE, Key::C);
                (redo, undo, copy, cut, paste, dup, all, save, open, del, frame, com)
            });
            let w = pointer.map(|p| self.screen_to_world(p));
            for (on, a) in [
                (k.0, Act::Redo),
                (k.1, Act::Undo),
                (k.2, Act::Copy),
                (k.3, Act::Cut),
                (k.4, Act::Paste(w)),
                (k.5, Act::Duplicate),
                (k.6, Act::SelectAll),
                (k.7, Act::Save),
                (k.8, Act::Open),
                (k.9, Act::Delete),
                (k.10, Act::FrameAll),
                (k.11, Act::Comment),
            ] {
                if on {
                    self.acts.push(a);
                }
            }
        }

        let mut acts: Vec<Act> = Vec::new();
        let in_sub = !self.parents.is_empty();
        let (can_undo, can_redo, has_sel, has_clip) = (!self.undo.is_empty(), !self.redo.is_empty(), !self.sel.is_empty(), !self.clip.is_empty());

        egui::TopBottomPanel::top("menu").show(ctx, |ui| {
            egui::menu::bar(ui, |ui| {
                ui.menu_button("File", |ui| {
                    for (label, a) in [("New", Act::New), ("Open...   Ctrl+O", Act::Open), ("Save   Ctrl+S", Act::Save), ("Save as...", Act::SaveAs)] {
                        if ui.button(label).clicked() {
                            acts.push(a);
                            ui.close_menu();
                        }
                    }
                    ui.separator();
                    if ui.button("Import bitmap...").clicked() {
                        acts.push(Act::ImportBitmap);
                        ui.close_menu();
                    }
                    if ui.button("Export textures...").clicked() {
                        acts.push(Act::Export);
                        ui.close_menu();
                    }
                    if ui.button("Publish package (.mfa)...").clicked() {
                        acts.push(Act::Publish);
                        ui.close_menu();
                    }
                });
                ui.menu_button("Edit", |ui| {
                    if ui.add_enabled(can_undo, egui::Button::new("Undo   Ctrl+Z")).clicked() {
                        acts.push(Act::Undo);
                        ui.close_menu();
                    }
                    if ui.add_enabled(can_redo, egui::Button::new("Redo   Ctrl+Y")).clicked() {
                        acts.push(Act::Redo);
                        ui.close_menu();
                    }
                    ui.separator();
                    for (label, a, en) in [
                        ("Cut   Ctrl+X", Act::Cut, has_sel),
                        ("Copy   Ctrl+C", Act::Copy, has_sel),
                        ("Paste   Ctrl+V", Act::Paste(None), has_clip),
                        ("Duplicate   Ctrl+D", Act::Duplicate, has_sel),
                        ("Delete   Del", Act::Delete, true),
                        ("Select all   Ctrl+A", Act::SelectAll, true),
                    ] {
                        if ui.add_enabled(en, egui::Button::new(label)).clicked() {
                            acts.push(a);
                            ui.close_menu();
                        }
                    }
                    ui.separator();
                    if ui.button("Add node...   Tab").clicked() {
                        acts.push(Act::Search(None));
                        ui.close_menu();
                    }
                    if ui.button("Comment frame   C").clicked() {
                        acts.push(Act::Comment);
                        ui.close_menu();
                    }
                    if ui.add_enabled(has_sel, egui::Button::new("Create subgraph from selection")).clicked() {
                        acts.push(Act::Subgraph);
                        ui.close_menu();
                    }
                });
                ui.menu_button("View", |ui| {
                    if ui.button("Frame all   F").clicked() {
                        acts.push(Act::FrameAll);
                        ui.close_menu();
                    }
                    if ui.button("Auto-layout").clicked() {
                        acts.push(Act::AutoLayout);
                        ui.close_menu();
                    }
                    if ui.add_enabled(in_sub, egui::Button::new("Leave subgraph")).clicked() {
                        acts.push(Act::ExitSub);
                        ui.close_menu();
                    }
                });
                ui.menu_button("Tools", |ui| {
                    if ui.button("Mesh map baker...").clicked() {
                        acts.push(Act::Bake);
                        ui.close_menu();
                    }
                });
                ui.menu_button("Help", |ui| {
                    if ui.button("Keyboard shortcuts").clicked() {
                        self.show_about = true;
                        ui.close_menu();
                    }
                });
            });
        });

        egui::TopBottomPanel::bottom("status").show(ctx, |ui| {
            ui.horizontal(|ui| {
                let (msg, ok) = &self.status;
                ui.colored_label(if *ok { Color32::from_gray(170) } else { Color32::from_rgb(255, 120, 110) }, msg);
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.label(format!("{}   |   {} nodes   |   {}%{}", self.title_name(), self.g.nodes.len(), (self.zoom * 100.0) as i32, if self.job.is_some() { "   |   computing..." } else { "" }));
                });
            });
        });

        let mut lib_pick: Option<Entry> = None;
        egui::SidePanel::left("left").default_width(215.0).show(ctx, |ui| {
            ui.heading("Explorer");
            ui.small(format!("Package: {}", self.title_name()));
            let root_subs: Vec<String> = match self.parents.first() {
                Some((p, _)) => p.subgraphs.keys().cloned().collect(),
                None => self.g.subgraphs.keys().cloned().collect(),
            };
            let current = self.parents.last().map(|(_, n)| n.clone());
            if ui.selectable_label(current.is_none(), "Graph (root)").clicked() {
                for _ in 0..self.parents.len() {
                    acts.push(Act::ExitSub);
                }
            }
            for name in root_subs {
                if ui.selectable_label(current.as_deref() == Some(name.as_str()), format!("   subgraph: {name}")).clicked() {
                    for _ in 0..self.parents.len() {
                        acts.push(Act::ExitSub);
                    }
                    acts.push(Act::EnterSub(name));
                }
            }
            ui.separator();
            ui.heading("Library");
            ui.add(egui::TextEdit::singleline(&mut self.lib_filter).hint_text("filter...").desired_width(f32::INFINITY));
            let f = self.lib_filter.to_lowercase();
            let all = self.entries();
            egui::ScrollArea::vertical().show(ui, |ui| {
                for cat in ["Generators", "SDF", "Filters", "Utility", "Subgraphs"] {
                    let items: Vec<&Entry> = all.iter().filter(|e| e.cat == cat && (f.is_empty() || e.label.to_lowercase().contains(&f))).collect();
                    if items.is_empty() {
                        continue;
                    }
                    egui::CollapsingHeader::new(cat).default_open(true).open(if f.is_empty() { None } else { Some(true) }).show(ui, |ui| {
                        for e in items {
                            let r = ui.add_sized([ui.available_width(), 18.0], egui::Button::new(&e.label).fill(kind_color(&e.kind).gamma_multiply(0.35)));
                            if r.on_hover_text(&e.doc).clicked() {
                                lib_pick = Some(e.clone());
                            }
                        }
                    });
                }
            });
        });

        egui::SidePanel::right("params").default_width(360.0).show(ctx, |ui| {
            ui.heading("Parameters");
            egui::ScrollArea::vertical().show(ui, |ui| self.inspector(ui));
        });

        egui::TopBottomPanel::bottom("views").resizable(true).default_height(300.0).min_height(120.0).show(ctx, |ui| {
            ui.columns(2, |cols| {
                self.view2d_ui(&mut cols[0]);
                self.view3d_ui(&mut cols[1]);
            });
        });

        egui::CentralPanel::default().frame(egui::Frame::none().fill(Color32::from_rgb(46, 46, 46))).show(ctx, |ui| {
            ui.horizontal(|ui| {
                ui.add_space(4.0);
                if in_sub && ui.button("< Back").clicked() {
                    acts.push(Act::ExitSub);
                }
                let mut path = String::from("Graph");
                for (_, n) in &self.parents {
                    path.push_str(&format!("  >  {n}"));
                }
                ui.strong(path);
                if has_sel {
                    ui.small(format!("   {} selected", self.sel.len()));
                }
            });
            self.canvas(ui);
        });

        if let Some(e) = lib_pick {
            let p = self.view_center_world();
            self.spawn(&e, p, None);
        }
        self.windows(ctx);
        self.search_ui(ctx);

        self.acts.extend(acts);
        let todo = std::mem::take(&mut self.acts);
        for a in todo {
            self.do_act(a);
        }

        if self.dirty && self.job.is_none() {
            self.dirty = false;
            self.spawn_eval();
        }
        if self.job.is_some() {
            ctx.request_repaint_after(std::time::Duration::from_millis(30));
        }
        self.text_focus = ctx.wants_keyboard_input();
    }
}

fn main() -> eframe::Result<()> {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default().with_inner_size([1500.0, 920.0]).with_title("MatForge"),
        ..Default::default()
    };
    eframe::run_native("MatForge", options, Box::new(|cc| Ok(Box::new(App::new(cc)))))
}
