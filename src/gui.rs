//! MaterialCraft visual editor. Run: cargo run --release --features gui --bin materialcraft-gui
use eframe::egui::{
    self, epaint::CubicBezierShape, Align2, Color32, ColorImage, FontId, Id, Key, Modifiers, PointerButton, Pos2, Rect,
    Rounding, Sense, Stroke, TextStyle, Vec2, pos2, vec2,
};
use materialcraft::bake::{self, BakeSettings};
use materialcraft::graph::{category, label, spec, usage_label, Comment, Exposed, Graph, Node, EXAMPLE, SPECS, THUMB};
use materialcraft::render3d::{self, Maps, View};
use materialcraft::tex::Tex;
use materialcraft::export::{self, Format};
use materialcraft::{ops, png};
use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::path::PathBuf;
use std::sync::mpsc::{channel, Receiver, TryRecvError};

mod logo_data {
    include!(concat!(env!("OUT_DIR"), "/logo.rs"));
}

// ---------------------------------------------------------------- layout constants
const CARD: f32 = 92.0;
const BAR: f32 = 16.0;
const CARD_H: f32 = BAR + CARD;
const OUTPUT_NAMES: [&str; 11] = ["basecolor", "normal", "roughness", "metallic", "height", "ambient_occlusion", "opacity", "emissive", "diffuse", "specular", "glossiness"];
const COMMENT_COLORS: [[u8; 3]; 6] = [[90, 140, 200], [200, 160, 60], [90, 180, 110], [190, 90, 90], [150, 100, 200], [150, 150, 150]];

// ---------------------------------------------------------------- parameter tables
fn slots(kind: &str) -> Vec<(&'static str, bool)> {
    spec(kind)
        .map(|s| s.req.iter().map(|n| (*n, true)).chain(s.opt.iter().map(|n| (*n, false))).collect())
        .unwrap_or_default()
}

fn kind_color(kind: &str) -> Color32 {
    match kind {
        "subgraph" => Color32::from_rgb(214, 140, 56),
        "input" | "dot" => Color32::from_rgb(110, 116, 128),
        "bitmap" => Color32::from_rgb(188, 118, 70),
        "output" => Color32::from_rgb(74, 98, 120),
        _ => match category(kind) {
            "Generators" => Color32::from_rgb(176, 76, 66),
            "SDF" => Color32::from_rgb(140, 88, 184),
            "Material" => Color32::from_rgb(190, 150, 50),
            "Utility" => Color32::from_rgb(100, 110, 130),
            _ => Color32::from_rgb(64, 112, 176),
        },
    }
}

fn qimg(t: &Tex) -> ColorImage {
    let q = |v: f32| (v.clamp(0.0, 1.0) * 255.0 + 0.5) as u8;
    let bytes: Vec<u8> = t.px.iter().flat_map(|p| [q(p[0]), q(p[1]), q(p[2]), q(p[3])]).collect();
    ColorImage::from_rgba_unmultiplied([t.w, t.h], &bytes)
}

// ---------------------------------------------------------------- types
#[derive(Clone)]
struct Entry {
    label: String,
    kind: String,
    sub: Option<String>,
    cat: &'static str,
    doc: String,
    nums: Vec<(&'static str, f32)>,
}

struct SearchState {
    screen: Pos2,
    world: Pos2,
    text: String,
    idx: usize,
    link_from: Option<u32>,
    link_to: Option<(u32, String)>,
    fresh: bool,
}

enum Act {
    Open,
    Save,
    SaveAs,
    Publish,
    ImportBitmap,
    Export,
    Bake,
    Undo,
    Redo,
    Copy,
    Cut,
    Paste(Option<Pos2>),
    Duplicate,
    Delete,
    SelectAll,
    Comment,
    Subgraph,
    EnterSub(String),
    ExitSub,
    AutoLayout,
    FrameAll,
    Search(Option<Pos2>),
    NewOutput(String, u32),
    NewGraph,
    Home,
    Drop(String, Pos2),
}

struct EvalResult {
    gen: u64,
    thumbs: HashMap<u32, Vec<u8>>,
    errors: HashMap<u32, String>,
    kept: HashMap<u32, Tex>,
    outputs: Vec<(String, u32)>,
}

struct ExportUi {
    open: bool,
    dir: Option<PathBuf>,
    prefix: String,
    sizes: [bool; 5],
    depth16: bool,
    fmt: Format,
    quality: u8,
    skip: BTreeSet<String>,
}

struct BakeUi {
    open: bool,
    low: Option<PathBuf>,
    high: Option<PathBuf>,
    out: Option<PathBuf>,
    s: BakeSettings,
    job: Option<Receiver<Result<Vec<PathBuf>, String>>>,
    msg: String,
}

struct App {
    g: Graph,
    parents: Vec<(Graph, String)>,
    undo: Vec<Graph>,
    redo: Vec<Graph>,
    edit_open: bool,
    sel: BTreeSet<u32>,
    active: Option<u32>,
    sel_comment: Option<usize>,
    clip: Vec<(u32, Node)>,
    pan: Vec2,
    zoom: f32,
    canvas_rect: Rect,
    connecting: Option<u32>,
    rubber: Option<Pos2>,
    comment_members: Vec<u32>,
    search: Option<SearchState>,
    acts: Vec<Act>,
    status: (String, bool),
    file: Option<PathBuf>,
    prev_size: usize,
    tile_preview: bool,
    thumbs: HashMap<u32, egui::TextureHandle>,
    node_err: HashMap<u32, String>,
    tex2d: Option<egui::TextureHandle>,
    tex2d_size: (usize, usize),
    maps: BTreeMap<String, Tex>,
    view3d: View,
    tex3d: Option<egui::TextureHandle>,
    r3d_size: usize,
    dirty: bool,
    dirty3d: bool,
    gen: u64,
    job: Option<Receiver<EvalResult>>,
    lib_search: String,
    text_focus: bool,
    export: ExportUi,
    bake: BakeUi,
    show_about: bool,
    screen: Screen,
    new_dlg: NewDlg,
    recent: Vec<PathBuf>,
    home_filter: String,
    home_page: u8,
    skip_home: bool,
    hide_suggestions: bool,
    banner: Option<egui::TextureHandle>,
    banner_rx: Option<Receiver<ColorImage>>,
    lib_thumbs: HashMap<String, egui::TextureHandle>,
    lib_rx: Option<Receiver<HashMap<String, ColorImage>>>,
    lib_cat: String,
    favorites: BTreeSet<String>,
    show: [bool; 5],
    modified: bool,
    last_title: String,
    z2d: f32,
    pan2d: Vec2,
    left_split: f32,
    need_fit: bool,
    w_left: f32,
    w_props: f32,
    h_views: f32,
    view_split: f32,
    connecting_in: Option<(u32, String)>,
    copy_out: Option<String>,
    logo: Option<egui::TextureHandle>,
    logo_note: String,
}

impl App {
    fn new(cc: &eframe::CreationContext<'_>) -> App {
        apply_theme(&cc.egui_ctx);
        let (logo_tex, logo_note) = logo().clone();
        let logo_handle = cc.egui_ctx.load_texture("logo", qimg(&logo_tex), egui::TextureOptions::LINEAR);
        let mut g = Graph::default();
        g.meta.insert("identifier".into(), "Graph".into());
        let active: Option<u32> = None;
        let settings = read_lines("settings.txt");
        let skip_home = settings.iter().any(|l| l == "skip_home=1");
        let favorites: BTreeSet<String> = settings.iter().find_map(|l| l.strip_prefix("favorites=")).map(|v| v.split(',').filter(|x| !x.is_empty()).map(String::from).collect()).unwrap_or_default();
        let recent: Vec<PathBuf> = read_lines("recent.txt").into_iter().map(PathBuf::from).collect();
        App {
            parents: Vec::new(),
            undo: Vec::new(),
            redo: Vec::new(),
            edit_open: false,
            sel: BTreeSet::new(),
            active,
            sel_comment: None,
            clip: Vec::new(),
            pan: vec2(30.0, 30.0),
            zoom: 0.9,
            canvas_rect: Rect::from_min_size(Pos2::ZERO, vec2(900.0, 500.0)),
            connecting: None,
            rubber: None,
            comment_members: Vec::new(),
            search: None,
            acts: Vec::new(),
            status: ("Press Tab in the graph to add a node at the cursor.".into(), true),
            file: None,
            prev_size: 256,
            tile_preview: false,
            thumbs: HashMap::new(),
            node_err: HashMap::new(),
            tex2d: None,
            tex2d_size: (0, 0),
            maps: BTreeMap::new(),
            view3d: View::default(),
            tex3d: None,
            r3d_size: 420,
            dirty: true,
            dirty3d: true,
            gen: 0,
            job: None,
            lib_search: String::new(),
            text_focus: false,
            export: ExportUi { open: false, dir: None, prefix: String::new(), sizes: [false, true, false, false, false], depth16: false, fmt: Format::Png, quality: 92, skip: BTreeSet::new() },
            bake: BakeUi { open: false, low: None, high: None, out: None, s: BakeSettings { size: 512, ..Default::default() }, job: None, msg: String::new() },
            show_about: false,
            screen: if skip_home { Screen::Editor } else { Screen::Home },
            new_dlg: NewDlg { open: false, sel: 1, name: "Graph".into(), size: 2048, depth16: false },
            recent,
            home_filter: String::new(),
            home_page: 0,
            skip_home,
            hide_suggestions: false,
            banner: None,
            banner_rx: Some(spawn_banner()),
            lib_thumbs: HashMap::new(),
            lib_rx: Some(spawn_library_thumbs()),
            lib_cat: "Texture generators/Patterns".into(),
            favorites,
            show: [true; 5],
            modified: false,
            last_title: String::new(),
            z2d: 1.0,
            pan2d: Vec2::ZERO,
            left_split: 0.33,
            need_fit: false,
            w_left: 380.0,
            w_props: 330.0,
            h_views: 330.0,
            view_split: 0.5,
            connecting_in: None,
            copy_out: None,
            logo: Some(logo_handle),
            logo_note,
            g,
        }
    }

    fn status(&mut self, msg: impl Into<String>, ok: bool) {
        self.status = (msg.into(), ok);
    }

    // ------------------------------------------------------------ undo / redo
    fn snap(&mut self) {
        self.modified = true;
        self.undo.push(self.g.clone());
        if self.undo.len() > 100 {
            self.undo.remove(0);
        }
        self.redo.clear();
    }

    fn undo(&mut self) {
        if let Some(prev) = self.undo.pop() {
            self.redo.push(std::mem::replace(&mut self.g, prev));
            self.after_restore();
        }
    }

    fn redo(&mut self) {
        if let Some(next) = self.redo.pop() {
            self.undo.push(std::mem::replace(&mut self.g, next));
            self.after_restore();
        }
    }

    fn after_restore(&mut self) {
        let g = &self.g;
        self.sel.retain(|i| g.nodes.contains_key(i));
        if self.active.map(|a| !g.nodes.contains_key(&a)).unwrap_or(false) {
            self.active = None;
        }
        self.sel_comment = None;
        self.dirty = true;
    }

    fn clear_history(&mut self) {
        self.undo.clear();
        self.redo.clear();
        self.sel.clear();
        self.active = None;
        self.sel_comment = None;
        self.dirty = true;
    }

    // ------------------------------------------------------------ graph editing
    fn depends_on(&self, a: u32, b: u32) -> bool {
        let mut stack = vec![a];
        let mut seen = BTreeSet::new();
        while let Some(x) = stack.pop() {
            if x == b {
                return true;
            }
            if !seen.insert(x) {
                continue;
            }
            if let Some(n) = self.g.nodes.get(&x) {
                stack.extend(n.ins.values().copied());
            }
        }
        false
    }

    fn try_connect(&mut self, src: u32, dst: u32, slot: &str) {
        if src == dst || self.depends_on(src, dst) {
            self.status("Connection rejected: it would create a cycle.", false);
            return;
        }
        self.snap();
        if let Some(n) = self.g.nodes.get_mut(&dst) {
            n.ins.insert(slot.to_string(), src);
            self.dirty = true;
        }
    }

    fn add_node_at(&mut self, kind: &str, sub: Option<&str>, pos: Pos2) -> u32 {
        self.snap();
        let id = self.g.nodes.keys().max().copied().unwrap_or(0) + 1;
        let mut n = Node { kind: kind.to_string(), pos: [pos.x, pos.y], ..Default::default() };
        match kind {
            "pixel_processor" => {
                n.strs.insert("expr".into(), "u".into());
            }
            "subgraph" => {
                n.strs.insert("graph".into(), sub.unwrap_or("").to_string());
            }
            "colorize" => {
                for (i, c) in [(0, 0.0f32), (1, 1.0)] {
                    n.nums.insert(format!("p{i}"), c);
                    for ch in ["r", "g", "b"] {
                        n.nums.insert(format!("{ch}{i}"), c);
                    }
                }
            }
            _ => {}
        }
        self.g.nodes.insert(id, n);
        self.sel = [id].into_iter().collect();
        self.active = Some(id);
        self.sel_comment = None;
        self.dirty = true;
        id
    }

    fn view_center_world(&self) -> Pos2 {
        let c = self.canvas_rect;
        let w = ((c.center() - c.min) - self.pan) / self.zoom;
        pos2(w.x - CARD * 0.5, w.y - CARD_H * 0.5)
    }

    fn spawn(&mut self, e: &Entry, world: Pos2, link_from: Option<u32>) -> Option<u32> {
        match e.kind.as_str() {
            "comment" => {
                self.add_comment_at(Some(world));
                return None;
            }
            "frame" => {
                self.add_comment_at(None);
                return None;
            }
            _ => {}
        }
        let id = self.add_node_at(&e.kind, e.sub.as_deref(), world);
        let in_sub = !self.parents.is_empty();
        if let Some(n) = self.g.nodes.get_mut(&id) {
            for (k, v) in &e.nums {
                n.nums.insert(k.to_string(), *v);
            }
            if e.kind == "output" {
                n.strs.insert("usage".into(), if in_sub { "out" } else { "basecolor" }.into());
            }
        }
        if let Some(src) = link_from {
            let first = slots(&e.kind).first().map(|s| s.0.to_string());
            if let Some(slot) = first {
                if src != id && e.kind != "input" {
                    if let Some(n) = self.g.nodes.get_mut(&id) {
                        n.ins.insert(slot, src);
                    }
                }
            }
        }
        Some(id)
    }

    fn new_output(&mut self, usage: &str, from: u32) {
        self.snap();
        let id = self.g.nodes.keys().max().copied().unwrap_or(0) + 1;
        let pos = self.g.nodes.get(&from).map(|n| [n.pos[0] + CARD + 70.0, n.pos[1]]).unwrap_or([0.0, 0.0]);
        let mut node = Node { kind: "output".into(), pos, ..Default::default() };
        node.strs.insert("usage".into(), usage.to_string());
        node.ins.insert("in".into(), from);
        self.g.nodes.insert(id, node);
        self.sel = [id].into_iter().collect();
        self.active = Some(id);
        self.dirty = true;
    }

    fn delete_selected(&mut self) {
        if self.sel.is_empty() {
            if let Some(ci) = self.sel_comment.take() {
                if ci < self.g.comments.len() {
                    self.snap();
                    self.g.comments.remove(ci);
                }
            }
            return;
        }
        self.snap();
        let sel = std::mem::take(&mut self.sel);
        for id in &sel {
            self.g.nodes.remove(id);
        }
        for n in self.g.nodes.values_mut() {
            n.ins.retain(|_, s| !sel.contains(s));
        }
        self.g.outputs.retain(|(_, s)| !sel.contains(s));
        self.g.exposed.retain(|e| !sel.contains(&e.node));
        self.active = None;
        self.dirty = true;
    }

    fn copy(&mut self) {
        self.clip = self.sel.iter().filter_map(|i| self.g.nodes.get(i).map(|n| (*i, n.clone()))).collect();
        if self.clip.is_empty() {
            return;
        }
        let ids: BTreeSet<u32> = self.clip.iter().map(|(i, _)| *i).collect();
        let mut tmp = Graph::default();
        for (id, n) in &self.clip {
            let mut nn = n.clone();
            nn.ins.retain(|_, s| ids.contains(s));
            tmp.nodes.insert(*id, nn);
        }
        self.copy_out = Some(format!("MaterialCraft nodes\n{}", tmp.to_text()));
        self.status(format!("Copied {} node(s)", self.clip.len()), true);
    }

    fn paste(&mut self, at: Option<Pos2>) {
        if self.clip.is_empty() {
            return;
        }
        self.snap();
        let origin = self.clip.iter().fold([f32::MAX, f32::MAX], |a, (_, n)| [a[0].min(n.pos[0]), a[1].min(n.pos[1])]);
        let target = at.unwrap_or_else(|| pos2(origin[0] + 30.0, origin[1] + 30.0));
        let mut next = self.g.nodes.keys().max().copied().unwrap_or(0) + 1;
        let mut map = HashMap::new();
        for (id, _) in &self.clip {
            map.insert(*id, next);
            next += 1;
        }
        let mut new_sel = BTreeSet::new();
        for (id, n) in self.clip.clone() {
            let mut nn = n.clone();
            nn.pos = [target.x + n.pos[0] - origin[0], target.y + n.pos[1] - origin[1]];
            nn.ins.retain(|_, s| map.contains_key(s));
            for v in nn.ins.values_mut() {
                *v = map[v];
            }
            self.g.nodes.insert(map[&id], nn);
            new_sel.insert(map[&id]);
        }
        self.active = new_sel.iter().next_back().copied();
        self.sel = new_sel;
        self.dirty = true;
    }

    fn add_comment(&mut self) {
        self.add_comment_at(None);
    }

    fn add_comment_at(&mut self, at: Option<Pos2>) {
        self.snap();
        let mut c = Comment { text: "Comment".into(), pos: [0.0, 0.0], size: [320.0, 220.0], color: 0 };
        if let Some(p) = at {
            c.pos = [p.x, p.y];
            c.size = [260.0, 130.0];
            c.text = "Comment".into();
        } else if self.sel.is_empty() {
            let p = self.view_center_world();
            c.pos = [p.x - 120.0, p.y - 90.0];
        } else {
            let (mut mn, mut mx) = ([f32::MAX; 2], [f32::MIN; 2]);
            for id in &self.sel {
                if let Some(n) = self.g.nodes.get(id) {
                    mn = [mn[0].min(n.pos[0]), mn[1].min(n.pos[1])];
                    mx = [mx[0].max(n.pos[0] + CARD), mx[1].max(n.pos[1] + CARD_H)];
                }
            }
            c.pos = [mn[0] - 24.0, mn[1] - 44.0];
            c.size = [mx[0] - mn[0] + 48.0, mx[1] - mn[1] + 68.0];
        }
        self.g.comments.push(c);
        self.sel_comment = Some(self.g.comments.len() - 1);
        self.sel.clear();
    }

    fn make_subgraph(&mut self) {
        let mut n = 1;
        let name = loop {
            let c = format!("subgraph_{n}");
            if !self.g.subgraphs.contains_key(&c) && !self.parents.iter().any(|(p, _)| p.subgraphs.contains_key(&c)) {
                break c;
            }
            n += 1;
        };
        self.snap();
        let sel = self.sel.clone();
        match self.g.extract_subgraph(&sel, &name) {
            Ok(id) => {
                self.sel = [id].into_iter().collect();
                self.active = Some(id);
                self.dirty = true;
                self.status(format!("Created '{name}' - double-click it to edit its contents"), true);
            }
            Err(e) => {
                self.undo.pop();
                self.status(e, false);
            }
        }
    }

    fn enter_sub(&mut self, name: &str) {
        let Some(sub) = self.g.subgraphs.get(name).cloned() else {
            return self.status(format!("subgraph '{name}' not found"), false);
        };
        let mut w = sub;
        w.subgraphs = self.g.subgraphs.clone();
        w.subgraphs.remove(name);
        w.resources = self.g.resources.clone();
        w.seed_offset = self.g.seed_offset;
        w.size = self.g.size;
        auto_layout_if_unplaced(&mut w);
        let parent = std::mem::replace(&mut self.g, w);
        self.parents.push((parent, name.to_string()));
        self.clear_history();
        self.fit_view();
    }

    fn exit_sub(&mut self) {
        if let Some((mut parent, name)) = self.parents.pop() {
            let mut sub = std::mem::take(&mut self.g);
            let libs = std::mem::take(&mut sub.subgraphs);
            sub.resources.clear();
            for (k, v) in libs {
                parent.subgraphs.entry(k).or_insert(v);
            }
            parent.subgraphs.insert(name, sub);
            self.g = parent;
            self.clear_history();
            self.fit_view();
        }
    }

    fn goto_root(&mut self) {
        while !self.parents.is_empty() {
            self.exit_sub();
        }
    }

    fn fit_view(&mut self) {
        if self.g.nodes.is_empty() {
            self.pan = vec2(30.0, 30.0);
            self.zoom = 0.9;
            return;
        }
        let (mut mn, mut mx) = ([f32::MAX; 2], [f32::MIN; 2]);
        for n in self.g.nodes.values() {
            mn = [mn[0].min(n.pos[0]), mn[1].min(n.pos[1])];
            mx = [mx[0].max(n.pos[0] + CARD), mx[1].max(n.pos[1] + CARD_H)];
        }
        let c = self.canvas_rect;
        let z = ((c.width() - 80.0) / (mx[0] - mn[0]).max(1.0)).min((c.height() - 80.0) / (mx[1] - mn[1]).max(1.0)).clamp(0.3, 1.2);
        self.zoom = z;
        self.pan = vec2(40.0 - mn[0] * z, 40.0 - mn[1] * z);
    }

    fn entries(&self) -> Vec<Entry> {
        let mk = |label: &str, kind: &str, cat: &'static str, doc: &str, nums: Vec<(&'static str, f32)>| Entry { label: label.to_string(), kind: kind.to_string(), sub: None, cat, doc: doc.to_string(), nums };
        let in_sub = !self.parents.is_empty();
        let mut v: Vec<Entry> = SPECS
            .iter()
            .filter(|s| !matches!(s.kind, "input" | "subgraph" | "output" | "dot"))
            .map(|s| mk(&label(s.kind), s.kind, category(s.kind), s.doc, vec![]))
            .collect();
        v.push(mk("Output", "output", "Utility", "graph output: drives the 3D view and exports a texture", vec![]));
        if in_sub {
            v.push(mk("Input Color", "input", "Utility", "color input of the subgraph", vec![("gray", 0.0)]));
            v.push(mk("Input Grayscale", "input", "Utility", "grayscale input of the subgraph", vec![("gray", 1.0)]));
        }
        v.push(mk("Comment", "comment", "Utility", "small note placed at the cursor", vec![]));
        v.push(mk("Frame", "frame", "Utility", "frame around the selected nodes", vec![]));
        v.push(mk("Dot Node", "dot", "Utility", "re-route a link", vec![]));
        let mut subs: BTreeSet<String> = self.g.subgraphs.keys().cloned().collect();
        for (p, _) in &self.parents {
            subs.extend(p.subgraphs.keys().cloned());
        }
        for name in subs {
            v.push(Entry { label: format!("subgraph: {name}"), kind: "subgraph".into(), sub: Some(name.clone()), cat: "Subgraphs", doc: format!("instance of subgraph '{name}'"), nums: vec![] });
        }
        v
    }

    // ------------------------------------------------------------ file operations
    fn open_file(&mut self) {
        let Some(p) = rfd::FileDialog::new().add_filter("MaterialCraft graph / package", &["mcg", "mca"]).pick_file() else { return };
        self.open_path(p);
    }

    fn save_to(&mut self, p: PathBuf) {
        self.goto_root();
        match std::fs::write(&p, self.g.to_text()) {
            Ok(()) => {
                self.status(format!("Saved {}", p.display()), true);
                self.push_recent(&p);
                self.file = Some(p);
                self.modified = false;
            }
            Err(e) => self.status(format!("Save failed: {e}"), false),
        }
    }

    fn save(&mut self) {
        match self.file.clone() {
            Some(p) => self.save_to(p),
            None => self.save_as(),
        }
    }

    fn save_as(&mut self) {
        if let Some(p) = rfd::FileDialog::new().add_filter("MaterialCraft graph", &["mcg"]).set_file_name("material.mcg").save_file() {
            self.save_to(p);
        }
    }

    fn publish(&mut self) {
        self.goto_root();
        match self.g.publish() {
            Ok(text) => {
                if let Some(p) = rfd::FileDialog::new().add_filter("MaterialCraft published asset", &["mca"]).set_file_name("material.mca").save_file() {
                    match std::fs::write(&p, &text) {
                        Ok(()) => self.status(format!("Published {} ({} KB, {} exposed parameters)", p.display(), text.len() / 1024, self.g.exposed.len()), true),
                        Err(e) => self.status(format!("Publish failed: {e}"), false),
                    }
                }
            }
            Err(e) => self.status(format!("Cannot publish: {e}"), false),
        }
    }

    fn import_bitmap(&mut self) {
        let Some(p) = rfd::FileDialog::new().add_filter("PNG image", &["png"]).pick_file() else { return };
        let pos = self.view_center_world();
        let id = self.add_node_at("bitmap", None, pos);
        if let Some(n) = self.g.nodes.get_mut(&id) {
            n.strs.insert("file".into(), p.to_string_lossy().to_string());
        }
    }

    fn run_export(&mut self) {
        self.goto_root();
        let Some(dir) = self.export.dir.clone() else {
            return self.status("Choose an export folder first.", false);
        };
        let sizes: Vec<usize> = [256usize, 512, 1024, 2048, 4096].iter().zip(self.export.sizes).filter(|(_, on)| *on).map(|(s, _)| *s).collect();
        if sizes.is_empty() {
            return self.status("Select at least one resolution.", false);
        }
        let idents: Vec<String> = self.g.output_nodes().into_iter().map(|(_, i, _)| i).collect();
        if idents.iter().all(|i| self.export.skip.contains(i)) {
            return self.status("Select at least one output to export.", false);
        }
        let fmt = self.export.fmt;
        let depth = if fmt != Format::Jpeg && (self.export.depth16 || self.g.meta.get("output_format").map(|f| f == "16" && false).unwrap_or(false)) { 16 } else { 8 };
        let mut count = 0;
        for &s in &sizes {
            match self.g.evaluate(Some(s)) {
                Ok(out) => {
                    let d = if sizes.len() > 1 { dir.join(s.to_string()) } else { dir.clone() };
                    if let Err(e) = std::fs::create_dir_all(&d) {
                        return self.status(format!("Export failed: {e}"), false);
                    }
                    for (name, t) in &out {
                        if self.export.skip.contains(name) {
                            continue;
                        }
                        let p = d.join(format!("{}{}.{}", self.export.prefix, name, fmt.ext()));
                        if let Err(e) = std::fs::write(&p, export::encode(fmt, t, depth, self.export.quality)) {
                            return self.status(format!("Export failed: {e}"), false);
                        }
                        count += 1;
                    }
                }
                Err(e) => return self.status(format!("Export blocked: {e}"), false),
            }
        }
        self.status(format!("Exported {count} {} file(s) to {}", fmt.name(), dir.display()), true);
    }

    fn start_bake(&mut self) {
        let (Some(lo), Some(hi), Some(out)) = (self.bake.low.clone(), self.bake.high.clone(), self.bake.out.clone()) else {
            self.bake.msg = "Choose the low-poly mesh, high-poly mesh and output folder.".into();
            return;
        };
        let s = self.bake.s.clone();
        let (tx, rx) = channel();
        self.bake.job = Some(rx);
        self.bake.msg = "Baking...".into();
        std::thread::spawn(move || {
            let r = (|| -> Result<Vec<PathBuf>, String> {
                let low = bake::parse_obj(&std::fs::read_to_string(&lo).map_err(|e| format!("{}: {e}", lo.display()))?).map_err(|e| format!("low-poly: {e}"))?;
                let high = bake::parse_obj(&std::fs::read_to_string(&hi).map_err(|e| format!("{}: {e}", hi.display()))?).map_err(|e| format!("high-poly: {e}"))?;
                let maps = bake::bake(&low, &high, &s)?;
                std::fs::create_dir_all(&out).map_err(|e| e.to_string())?;
                let mut files = Vec::new();
                for (name, t) in &maps {
                    let p = out.join(format!("{name}.png"));
                    std::fs::write(&p, png::encode(t.w, t.h, &t.px, 8)).map_err(|e| e.to_string())?;
                    files.push(p);
                }
                Ok(files)
            })();
            let _ = tx.send(r);
        });
    }

    // ------------------------------------------------------------ background evaluation
    fn spawn_eval(&mut self) {
        let g = self.g.clone();
        let size = self.prev_size;
        let mut keep: Vec<u32> = g.output_nodes().iter().map(|o| o.2).collect();
        if let Some(a) = self.active {
            keep.push(a);
        }
        self.gen += 1;
        let gen = self.gen;
        let (tx, rx) = channel();
        self.job = Some(rx);
        std::thread::spawn(move || {
            let r = g.evaluate_all(size, &keep);
            let _ = tx.send(EvalResult { gen, thumbs: r.thumbs, errors: r.errors, kept: r.kept, outputs: g.output_nodes().into_iter().map(|(u, _, id)| (u, id)).collect() });
        });
    }

    fn poll_eval(&mut self, ctx: &egui::Context) {
        let Some(rx) = &self.job else { return };
        match rx.try_recv() {
            Ok(r) => {
                self.job = None;
                if r.gen != self.gen {
                    return;
                }
                let mut opts = egui::TextureOptions::LINEAR;
                opts.wrap_mode = egui::TextureWrapMode::Repeat;
                self.thumbs.retain(|id, _| r.thumbs.contains_key(id));
                for (id, bytes) in &r.thumbs {
                    let img = ColorImage::from_rgba_unmultiplied([THUMB, THUMB], bytes);
                    match self.thumbs.get_mut(id) {
                        Some(h) => h.set(img, egui::TextureOptions::LINEAR),
                        None => {
                            self.thumbs.insert(*id, ctx.load_texture(format!("thumb{id}"), img, egui::TextureOptions::LINEAR));
                        }
                    }
                }
                self.node_err = r.errors;
                if let Some(a) = self.active {
                    if let Some(t) = r.kept.get(&a) {
                        self.tex2d_size = (t.w, t.h);
                        self.tex2d = Some(ctx.load_texture("view2d", qimg(t), opts));
                    } else {
                        self.tex2d = None;
                    }
                } else {
                    self.tex2d = None;
                }
                self.maps.clear();
                for (name, id) in &r.outputs {
                    if let Some(t) = r.kept.get(id) {
                        self.maps.insert(name.clone(), t.clone());
                    }
                }
                if !self.maps.contains_key("basecolor") {
                    if let Some(t) = self.maps.get("out").cloned() {
                        self.maps.insert("basecolor".into(), t);
                    }
                }
                self.dirty3d = true;
            }
            Err(TryRecvError::Empty) => {}
            Err(TryRecvError::Disconnected) => {
                self.job = None;
                self.status("Evaluation failed unexpectedly.", false);
            }
        }
    }

    fn render_3d(&mut self, ctx: &egui::Context) {
        self.dirty3d = false;
        let (flat, half, zero, one) = (Tex::solid(4, 4, [0.5, 0.5, 1.0, 1.0]), Tex::solid(4, 4, [0.5, 0.5, 0.5, 1.0]), Tex::solid(4, 4, [0.0, 0.0, 0.0, 1.0]), Tex::solid(4, 4, [1.0; 4]));
        let inv_gloss = self.maps.get("glossiness").map(ops::invert);
        let m = Maps {
            base: self.maps.get("basecolor").or_else(|| self.maps.get("diffuse")).unwrap_or(&half),
            normal: self.maps.get("normal").unwrap_or(&flat),
            rough: self.maps.get("roughness").or(inv_gloss.as_ref()).unwrap_or(&half),
            metal: self.maps.get("metallic").unwrap_or(&zero),
            ao: self.maps.get("ambient_occlusion").unwrap_or(&one),
            emissive: self.maps.get("emissive"),
            height: self.maps.get("height"),
        };
        let img = render3d::render(&m, &self.view3d, self.r3d_size, self.r3d_size);
        self.tex3d = Some(ctx.load_texture("view3d", qimg(&img), egui::TextureOptions::LINEAR));
    }
}

// ---------------------------------------------------------------- free helpers
fn auto_layout(g: &mut Graph) {
    let ids: Vec<u32> = g.nodes.keys().copied().collect();
    let mut level: HashMap<u32, usize> = ids.iter().map(|i| (*i, 0)).collect();
    for _ in 0..=ids.len() {
        let mut changed = false;
        for id in &ids {
            let l = g.nodes[id].ins.values().map(|s| level.get(s).copied().unwrap_or(0) + 1).max().unwrap_or(0);
            if l > level[id] && l <= ids.len() {
                level.insert(*id, l);
                changed = true;
            }
        }
        if !changed {
            break;
        }
    }
    let mut rows: HashMap<usize, f32> = HashMap::new();
    for id in &ids {
        let l = level[id];
        let y = rows.entry(l).or_insert(0.0);
        if let Some(n) = g.nodes.get_mut(id) {
            n.pos = [l as f32 * (CARD + 70.0), *y];
        }
        *y += CARD_H + 40.0;
    }
}

fn auto_layout_if_unplaced(g: &mut Graph) {
    if g.nodes.len() > 1 && g.nodes.values().all(|n| n.pos == [0.0, 0.0]) {
        auto_layout(g);
    }
}

fn apply_theme(ctx: &egui::Context) {
    let mut style = (*ctx.style()).clone();
    let mut v = egui::Visuals::dark();
    v.panel_fill = Color32::from_rgb(46, 46, 46);
    v.window_fill = Color32::from_rgb(52, 52, 52);
    v.extreme_bg_color = Color32::from_rgb(30, 30, 30);
    v.faint_bg_color = Color32::from_rgb(52, 52, 52);
    v.widgets.noninteractive.bg_stroke = Stroke::new(1.0, Color32::from_rgb(28, 28, 28));
    v.widgets.inactive.bg_fill = Color32::from_rgb(66, 66, 66);
    v.widgets.inactive.weak_bg_fill = Color32::from_rgb(62, 62, 62);
    v.widgets.hovered.bg_fill = Color32::from_rgb(88, 88, 88);
    v.widgets.hovered.weak_bg_fill = Color32::from_rgb(84, 84, 84);
    v.widgets.active.bg_fill = Color32::from_rgb(235, 130, 20);
    v.widgets.active.weak_bg_fill = Color32::from_rgb(235, 130, 20);
    v.selection.bg_fill = Color32::from_rgb(220, 120, 20);
    v.selection.stroke = Stroke::new(1.0, Color32::WHITE);
    v.window_rounding = Rounding::same(3.0);
    v.menu_rounding = Rounding::same(3.0);
    style.visuals = v;
    style.text_styles.insert(TextStyle::Body, FontId::proportional(12.5));
    style.text_styles.insert(TextStyle::Button, FontId::proportional(12.5));
    style.text_styles.insert(TextStyle::Small, FontId::proportional(10.5));
    style.text_styles.insert(TextStyle::Heading, FontId::proportional(14.0));
    style.spacing.item_spacing = vec2(6.0, 4.0);
    style.spacing.button_padding = vec2(6.0, 2.0);
    style.spacing.slider_width = 150.0;
    ctx.set_style(style);
}

include!("gui_params.rs");
include!("gui_ui.rs");
include!("gui_home.rs");
