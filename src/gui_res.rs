// SPDX-License-Identifier: MIT OR Apache-2.0
// The resource manager window (included into gui.rs): every file the graph depends on, embedded or linked, with
// thumbnails, status, the nodes that use it, and the operations on it.
use materialcraft::resources::{self, Entry as ResEntry, ResKind, Status as ResStatus};

struct ResUi {
    open: bool,
    sel: Option<String>,
    filter: String,
    rename: String,
    rename_for: Option<String>,
    /// thumbnails and one-line descriptions, keyed by (name, size, status)
    info: HashMap<(String, usize, u8), (String, Option<egui::TextureHandle>)>,
    msg: String,
    ok: bool,
    cache: Option<(std::time::Instant, Vec<ResEntry>)>,
}

impl ResUi {
    fn new() -> ResUi {
        ResUi { open: false, sel: None, filter: String::new(), rename: String::new(), rename_for: None, info: HashMap::new(), msg: String::new(), ok: true, cache: None }
    }
}

fn status_badge(ui: &mut egui::Ui, s: ResStatus) {
    let (t, c) = match s {
        ResStatus::Embedded => ("embedded", Color32::from_rgb(110, 190, 130)),
        ResStatus::Linked => ("linked", Color32::from_rgb(120, 170, 230)),
        ResStatus::Missing => ("MISSING", Color32::from_rgb(235, 110, 100)),
    };
    ui.label(egui::RichText::new(t).color(c).small());
}

impl App {
    /// Folders searched for linked files given by relative path: the project folder and its `resources` folder.
    fn sync_search_dirs(&mut self) {
        let dirs: Vec<PathBuf> = match self.file.as_ref().and_then(|f| f.parent()) {
            Some(p) if !p.as_os_str().is_empty() => vec![p.to_path_buf(), p.join("resources")],
            _ => Vec::new(),
        };
        if dirs != self.res_dirs {
            resources::set_search_dirs(dirs.clone());
            self.res_dirs = dirs;
            self.dirty = true;
        }
    }

    fn project_dir(&self) -> Option<PathBuf> {
        self.file.as_ref().and_then(|f| f.parent()).filter(|p| !p.as_os_str().is_empty()).map(|p| p.to_path_buf())
    }

    fn res_done(&mut self, msg: String, ok: bool) {
        self.res.cache = None;
        self.dirty = true;
        self.modified = true;
        self.res.msg = msg.clone();
        self.res.ok = ok;
        self.status(msg, ok);
    }

    fn res_entry_info(&mut self, ctx: &egui::Context, e: &ResEntry) -> (String, Option<egui::TextureHandle>) {
        let key = (e.name.clone(), e.size.unwrap_or(0), e.status as u8);
        if let Some(v) = self.res.info.get(&key) {
            return v.clone();
        }
        let info = self.parents.first().map(|p| &p.0).unwrap_or(&self.g).resource_info(e);
        let tex = info.thumb.as_ref().map(|t| ctx.load_texture(format!("res_{}", e.name), qimg(t), egui::TextureOptions::LINEAR));
        if self.res.info.len() > 400 {
            self.res.info.clear();
        }
        self.res.info.insert(key, (info.summary.clone(), tex.clone()));
        (info.summary, tex)
    }

    fn resources_window(&mut self, ctx: &egui::Context) {
        let mut open = true;
        // operations act on the package, not on a nested graph
        let entries = self.parents.first().map(|p| &p.0).unwrap_or(&self.g).resource_entries();
        let mut act: Option<ResAct> = None;
        egui::Window::new("Resources").open(&mut open).collapsible(false).resizable(true).default_size([880.0, 560.0]).show(ctx, |ui| {
            let missing = entries.iter().filter(|e| e.status == ResStatus::Missing).count();
            let linked = entries.iter().filter(|e| e.status == ResStatus::Linked).count();
            ui.horizontal_wrapped(|ui| {
                ui.label(format!("{} resource(s)", entries.len()));
                if missing > 0 {
                    ui.label(egui::RichText::new(format!("{missing} missing")).color(Color32::from_rgb(235, 110, 100)));
                }
                ui.separator();
                ui.add_enabled_ui(linked > 0, |ui| {
                    if ui.button("Embed all linked").on_hover_text("Copy every linked file into the package so it travels with it").clicked() {
                        act = Some(ResAct::EmbedAll);
                    }
                    if ui.button("Collect into project folder").on_hover_text("Copy linked files next to the project (resources folder) and link them by relative path").clicked() {
                        act = Some(ResAct::Collect);
                    }
                });
                if ui.button("Update from source files").on_hover_text("Re-read embedded resources whose original file changed").clicked() {
                    act = Some(ResAct::UpdateAll);
                }
                if ui.button("Remove unused").on_hover_text("Delete embedded resources no node uses").clicked() {
                    act = Some(ResAct::Prune);
                }
            });
            ui.horizontal(|ui| {
                ui.label("Filter");
                ui.add(egui::TextEdit::singleline(&mut self.res.filter).desired_width(180.0).hint_text("name"));
                if !self.res.msg.is_empty() {
                    ui.colored_label(if self.res.ok { Color32::from_gray(170) } else { Color32::from_rgb(235, 110, 100) }, self.res.msg.clone());
                }
            });
            ui.separator();
            let filter = self.res.filter.to_lowercase();
            let shown: Vec<&ResEntry> = entries.iter().filter(|e| filter.is_empty() || e.name.to_lowercase().contains(&filter)).collect();
            ui.columns(2, |cols| {
                // ---- list
                egui::ScrollArea::vertical().id_salt("res_list").show(&mut cols[0], |ui| {
                    if shown.is_empty() {
                        ui.label(if entries.is_empty() { "Nothing yet: Bitmap, SVG, Text, Color LUT and path nodes list their files here." } else { "No match." });
                    }
                    let mut last_kind: Option<ResKind> = None;
                    for e in &shown {
                        if last_kind != Some(e.kind) {
                            ui.add_space(4.0);
                            ui.small(e.kind.label());
                            last_kind = Some(e.kind);
                        }
                        let (summary, tex) = self.res_entry_info(ctx, e);
                        let sel = self.res.sel.as_deref() == Some(e.name.as_str());
                        let r = ui
                            .horizontal(|ui| {
                                match &tex {
                                    Some(t) => {
                                        let sz = t.size_vec2();
                                        let k = (36.0 / sz.x.max(sz.y)).min(4.0);
                                        ui.add(egui::Image::new((t.id(), sz * k)));
                                    }
                                    None => {
                                        ui.allocate_space(vec2(36.0, 36.0));
                                    }
                                }
                                ui.vertical(|ui| {
                                    let base = e.name.rsplit(['/', '\\']).next().unwrap_or(&e.name).to_string();
                                    let resp = ui.selectable_label(sel, base);
                                    ui.horizontal(|ui| {
                                        status_badge(ui, e.status);
                                        ui.small(format!("{}  {}", e.size.map(resources::human_size).unwrap_or_default(), summary));
                                    });
                                    resp
                                })
                                .inner
                            })
                            .inner;
                        if r.clicked() {
                            self.res.sel = Some(e.name.clone());
                        }
                        r.on_hover_text(e.name.clone());
                    }
                });
                // ---- details
                let ui = &mut cols[1];
                let Some(e) = self.res.sel.as_ref().and_then(|s| entries.iter().find(|e| &e.name == s)) else {
                    ui.label("Select a resource to see where it is used and to manage it.");
                    return;
                };
                let (summary, tex) = self.res_entry_info(ctx, e);
                ui.heading(e.name.rsplit(['/', '\\']).next().unwrap_or(&e.name));
                ui.horizontal(|ui| {
                    status_badge(ui, e.status);
                    ui.label(e.kind.label());
                    ui.label(summary);
                });
                if let Some(t) = &tex {
                    let sz = t.size_vec2();
                    let k = (200.0 / sz.x.max(sz.y)).min(3.0);
                    ui.add(egui::Image::new((t.id(), sz * k)));
                }
                match e.status {
                    ResStatus::Embedded => {
                        ui.small("Stored inside the package.");
                        if let Some(o) = &e.origin {
                            ui.small(format!("Imported from {o}"));
                            match self.parents.first().map(|p| &p.0).unwrap_or(&self.g).source_changed(&e.name) {
                                Some(true) => {
                                    ui.colored_label(Color32::from_rgb(230, 190, 90), "The source file has changed since it was imported.");
                                }
                                Some(false) => {
                                    ui.small("Matches the source file.");
                                }
                                None => {
                                    ui.small("The source file is not available.");
                                }
                            }
                        }
                    }
                    ResStatus::Linked => {
                        ui.small(format!("Linked file: {}", e.resolved.as_ref().map(|p| p.display().to_string()).unwrap_or_default()));
                    }
                    ResStatus::Missing => {
                        ui.colored_label(Color32::from_rgb(235, 110, 100), format!("File not found: {}", e.name));
                    }
                }
                ui.add_space(4.0);
                ui.horizontal_wrapped(|ui| {
                    if e.kind != ResKind::Baked {
                        if e.status == ResStatus::Linked && ui.button("Embed").clicked() {
                            act = Some(ResAct::Embed(e.name.clone()));
                        }
                        if e.status == ResStatus::Embedded && ui.button("Extract to folder...").clicked() {
                            act = Some(ResAct::Extract(e.name.clone()));
                        }
                        if ui.button("Relink...").on_hover_text("Point the nodes at another file").clicked() {
                            act = Some(ResAct::Relink(e.name.clone()));
                        }
                        if e.status == ResStatus::Embedded && ui.button("Replace contents...").on_hover_text("Keep the name and the nodes, swap the data").clicked() {
                            act = Some(ResAct::Replace(e.name.clone()));
                        }
                        if e.status == ResStatus::Embedded && e.origin.is_some() && ui.button("Update from source").clicked() {
                            act = Some(ResAct::Update(e.name.clone()));
                        }
                    }
                    if e.status == ResStatus::Embedded && e.uses.is_empty() && ui.button("Delete").clicked() {
                        act = Some(ResAct::Delete(e.name.clone()));
                    }
                });
                if e.status == ResStatus::Embedded && e.kind != ResKind::Baked {
                    if self.res.rename_for.as_ref() != Some(&e.name) {
                        self.res.rename = e.name.clone();
                        self.res.rename_for = Some(e.name.clone());
                    }
                    ui.horizontal(|ui| {
                        ui.label("Name");
                        ui.add(egui::TextEdit::singleline(&mut self.res.rename).desired_width(160.0));
                        if ui.button("Rename").clicked() {
                            act = Some(ResAct::Rename(e.name.clone(), self.res.rename.clone()));
                        }
                    });
                }
                ui.add_space(6.0);
                ui.strong(format!("Used by {} node(s)", e.uses.len()));
                egui::ScrollArea::vertical().id_salt("res_uses").max_height(160.0).show(ui, |ui| {
                    for u in &e.uses {
                        let label = if u.scope.is_empty() { format!("Node {} ({}) - {}", u.node, u.node_kind, u.key) } else { format!("Node {} in '{}' ({}) - {}", u.node, u.scope, u.node_kind, u.key) };
                        if ui.link(label).clicked() {
                            act = Some(ResAct::Show(u.scope.clone(), u.node));
                        }
                    }
                    if e.uses.is_empty() {
                        ui.small("Nothing uses this resource.");
                    }
                });
            });
        });
        if !open {
            self.res.open = false;
        }
        if let Some(a) = act {
            self.run_res_act(a);
        }
    }

    fn run_res_act(&mut self, a: ResAct) {
        if let ResAct::Show(scope, node) = &a {
            self.goto_root();
            if !scope.is_empty() {
                self.acts.push(Act::EnterSub(scope.clone()));
            }
            if scope.is_empty() {
                if let Some(n) = self.g.nodes.get(node) {
                    let c = self.canvas_rect.size() * 0.5;
                    self.pan = c - vec2(n.pos[0] + CARD * 0.5, n.pos[1] + BAR) * self.zoom;
                    self.sel = std::iter::once(*node).collect();
                }
            }
            return;
        }
        self.goto_root();
        match a {
            ResAct::EmbedAll => {
                self.snap();
                let (n, problems) = self.g.embed_all();
                let msg = if problems.is_empty() { format!("Embedded {n} file(s)") } else { format!("Embedded {n} file(s); {}", problems.join("; ")) };
                self.res_done(msg, problems.is_empty());
            }
            ResAct::Embed(name) => {
                self.snap();
                match self.g.embed_file(&name) {
                    Ok(k) => {
                        self.res.sel = Some(k.clone());
                        self.res_done(format!("Embedded as '{k}'"), true)
                    }
                    Err(e) => {
                        self.undo.pop();
                        self.res_done(e, false)
                    }
                }
            }
            ResAct::Extract(name) => {
                let Some(dir) = rfd::FileDialog::new().set_directory(self.project_dir().unwrap_or_else(|| PathBuf::from("."))).pick_folder() else { return };
                self.snap();
                match self.g.extract(&name, &dir) {
                    Ok(p) => {
                        self.res.sel = Some(p.to_string_lossy().to_string());
                        self.res_done(format!("Saved {} and linked the nodes to it", p.display()), true)
                    }
                    Err(e) => {
                        self.undo.pop();
                        self.res_done(e, false)
                    }
                }
            }
            ResAct::Relink(name) => {
                let Some(p) = rfd::FileDialog::new().pick_file() else { return };
                self.snap();
                let path = p.to_string_lossy().to_string();
                match self.g.relink_file(&name, &path) {
                    Ok(n) => {
                        self.res.sel = Some(path);
                        self.res_done(format!("{n} reference(s) now point to {}", p.display()), true)
                    }
                    Err(e) => {
                        self.undo.pop();
                        self.res_done(e, false)
                    }
                }
            }
            ResAct::Replace(name) => {
                let Some(p) = rfd::FileDialog::new().pick_file() else { return };
                self.snap();
                match self.g.replace_resource(&name, &p.to_string_lossy()) {
                    Ok(()) => self.res_done(format!("'{name}' now holds the contents of {}", p.display()), true),
                    Err(e) => {
                        self.undo.pop();
                        self.res_done(e, false)
                    }
                }
            }
            ResAct::Update(name) => {
                self.snap();
                match self.g.update_resource(&name) {
                    Ok(()) => self.res_done(format!("'{name}' updated from its source file"), true),
                    Err(e) => {
                        self.undo.pop();
                        self.res_done(e, false)
                    }
                }
            }
            ResAct::UpdateAll => {
                self.snap();
                let done = self.g.update_all();
                if done.is_empty() {
                    self.undo.pop();
                    self.res.msg = "Everything is up to date".into();
                    self.res.ok = true;
                } else {
                    self.res_done(format!("Updated {}", done.join(", ")), true);
                }
            }
            ResAct::Collect => {
                let Some(dir) = self.project_dir() else {
                    return self.res_done("Save the project first: files are collected next to it.".into(), false);
                };
                self.snap();
                let (n, problems) = self.g.collect_files(&dir, "resources");
                let msg = if problems.is_empty() { format!("Copied {n} file(s) into {}", dir.join("resources").display()) } else { format!("Copied {n} file(s); {}", problems.join("; ")) };
                self.sync_search_dirs();
                self.res_done(msg, problems.is_empty());
            }
            ResAct::Rename(old, new) => {
                self.snap();
                match self.g.rename_resource(&old, &new) {
                    Ok(()) => {
                        self.res.sel = Some(new.clone());
                        self.res_done(format!("Renamed to '{new}'"), true)
                    }
                    Err(e) => {
                        self.undo.pop();
                        self.res_done(e, false)
                    }
                }
            }
            ResAct::Delete(name) => {
                self.snap();
                self.g.resources.remove(&name);
                self.res.sel = None;
                self.res_done(format!("Deleted '{name}'"), true);
            }
            ResAct::Prune => {
                self.snap();
                let gone = self.g.remove_unused();
                if gone.is_empty() {
                    self.undo.pop();
                    self.res.msg = "Nothing unused".into();
                    self.res.ok = true;
                } else {
                    self.res_done(format!("Removed {}", gone.join(", ")), true);
                }
            }
            ResAct::Show(..) => {}
        }
    }
}

enum ResAct {
    EmbedAll,
    Embed(String),
    Extract(String),
    Relink(String),
    Replace(String),
    Update(String),
    UpdateAll,
    Collect,
    Rename(String, String),
    Delete(String),
    Prune,
    Show(String, u32),
}
