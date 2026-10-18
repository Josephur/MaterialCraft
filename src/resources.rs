// SPDX-License-Identifier: MIT OR Apache-2.0
//! Resource manager: every file a graph depends on (bitmaps, vector art, fonts, lookup tables, splines, baked maps),
//! whether it is embedded in the package or linked from disk, which nodes use it, and the operations on it: embed,
//! extract, relink, replace, update from the original file, collect into the project folder, rename, clean up.
use crate::graph::Graph;
use crate::tex::Tex;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, RwLock};

// ------------------------------------------------------------------------------------------------ file lookup
static SEARCH: RwLock<Vec<PathBuf>> = RwLock::new(Vec::new());

/// Folders searched for linked files that are given as relative paths (or whose absolute path no longer exists).
/// The editor sets this to the project folder; the command line sets it to the graph's folder.
pub fn set_search_dirs(dirs: Vec<PathBuf>) {
    if let Ok(mut w) = SEARCH.write() {
        *w = dirs;
    }
}

pub fn search_dirs() -> Vec<PathBuf> {
    SEARCH.read().map(|r| r.clone()).unwrap_or_default()
}

fn file_name_of(p: &str) -> &str {
    p.rsplit(['/', '\\']).next().unwrap_or(p)
}

/// Finds a linked file: the path itself (backslashes are accepted on every platform), then relative to each search
/// folder, then by file name inside each search folder.
pub fn resolve(path: &str) -> Option<PathBuf> {
    if path.is_empty() {
        return None;
    }
    let alt = path.replace('\\', "/");
    for c in [path, alt.as_str()] {
        let p = Path::new(c);
        if p.is_file() {
            return Some(p.to_path_buf());
        }
    }
    for d in search_dirs() {
        for c in [path, alt.as_str()] {
            let p = Path::new(c);
            if p.is_relative() {
                let j = d.join(p);
                if j.is_file() {
                    return Some(j);
                }
            }
        }
        let j = d.join(file_name_of(path));
        if j.is_file() {
            return Some(j);
        }
    }
    None
}

pub fn read_file(path: &str) -> Result<Vec<u8>, String> {
    match resolve(path) {
        Some(p) => std::fs::read(&p).map_err(|e| format!("cannot read '{path}': {e}")),
        None => Err(format!("cannot read '{path}': file not found")),
    }
}

// ------------------------------------------------------------------------------------------------ kinds
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum ResKind {
    Image,
    Vector,
    Font,
    Lut,
    Spline,
    Baked,
    Other,
}

impl ResKind {
    pub fn label(self) -> &'static str {
        match self {
            ResKind::Image => "Bitmap",
            ResKind::Vector => "Vector art",
            ResKind::Font => "Font",
            ResKind::Lut => "Lookup table",
            ResKind::Spline => "Spline",
            ResKind::Baked => "Baked maps",
            ResKind::Other => "Other",
        }
    }
}

/// The node parameters that hold a file name or resource name, per node type.
pub fn file_keys(kind: &str) -> &'static [&'static str] {
    match kind {
        "bitmap" | "svg" | "color_lut" => &["file"],
        "text" => &["font"],
        k if k.starts_with("path_") => &["file", "file2", "file3", "file4"],
        _ => &[],
    }
}

pub fn classify(name: &str, node_kind: Option<&str>) -> ResKind {
    let l = name.to_lowercase();
    let ext = l.rsplit('.').next().unwrap_or("");
    match ext {
        "svg" => ResKind::Vector,
        "ttf" | "otf" | "ttc" | "woff" | "woff2" => ResKind::Font,
        "cube" | "3dl" => ResKind::Lut,
        "spl" => ResKind::Spline,
        _ => match node_kind {
            Some("color_lut") => ResKind::Lut,
            Some("svg") => ResKind::Vector,
            Some("text") => ResKind::Font,
            Some(k) if k.starts_with("path_") => ResKind::Spline,
            Some("bitmap") => ResKind::Image,
            _ => {
                if crate::imgio::has_image_extension(&l) {
                    ResKind::Image
                } else {
                    ResKind::Other
                }
            }
        },
    }
}

// ------------------------------------------------------------------------------------------------ report
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Use {
    /// "" for the main graph, otherwise the subgraph's name
    pub scope: String,
    pub node: u32,
    pub node_kind: String,
    pub key: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Status {
    Embedded,
    Linked,
    Missing,
}

#[derive(Clone, Debug)]
pub struct Entry {
    /// the value stored in the nodes: a resource name or a file path
    pub name: String,
    pub kind: ResKind,
    pub status: Status,
    pub resolved: Option<PathBuf>,
    pub size: Option<usize>,
    pub uses: Vec<Use>,
    /// the file an embedded resource was imported from, when known
    pub origin: Option<String>,
}

const ORIGIN: &str = "origin:";

pub(crate) fn sanitize(name: &str) -> String {
    let base = file_name_of(name);
    let s: String = base.chars().map(|c| if c.is_whitespace() || c == '"' || c == '#' || c.is_control() { '_' } else { c }).collect();
    if s.is_empty() {
        "resource".into()
    } else {
        s
    }
}

impl Graph {
    fn scopes(&self) -> Vec<(String, &Graph)> {
        let mut v = vec![(String::new(), self)];
        for (k, g) in &self.subgraphs {
            v.push((k.clone(), g));
        }
        v
    }

    /// Every file reference of every node, with its value.
    pub fn file_uses(&self) -> Vec<(Use, String)> {
        let mut out = Vec::new();
        for (scope, g) in self.scopes() {
            for (id, n) in &g.nodes {
                for key in file_keys(&n.kind) {
                    if let Some(v) = n.strs.get(*key).filter(|v| !v.is_empty()) {
                        out.push((Use { scope: scope.clone(), node: *id, node_kind: n.kind.clone(), key: key.to_string() }, v.clone()));
                    }
                }
            }
        }
        out
    }

    /// Changes every reference to `from` into `to`; returns how many changed. Read-only linked copies are left alone.
    fn rewrite_uses(&mut self, from: &str, to: &str) -> usize {
        let mut count = 0;
        let managed = self.managed.clone();
        let mut fix = |g: &mut Graph| {
            for n in g.nodes.values_mut() {
                for key in file_keys(&n.kind) {
                    if n.strs.get(*key).map(|v| v == from).unwrap_or(false) {
                        n.strs.insert(key.to_string(), to.to_string());
                        count += 1;
                    }
                }
            }
        };
        fix(self);
        for (k, g) in self.subgraphs.iter_mut() {
            if !managed.contains(k) {
                fix(g);
            }
        }
        count
    }

    pub fn resource_entries(&self) -> Vec<Entry> {
        let mut map: BTreeMap<String, Entry> = BTreeMap::new();
        for (u, v) in self.file_uses() {
            let kind = classify(&v, Some(&u.node_kind));
            let e = map.entry(v.clone()).or_insert_with(|| {
                let embedded = self.resources.get(&v);
                let resolved = if embedded.is_none() { resolve(&v) } else { None };
                let size = embedded.map(|b| b.len()).or_else(|| resolved.as_ref().and_then(|p| std::fs::metadata(p).ok()).map(|m| m.len() as usize));
                let status = if embedded.is_some() {
                    Status::Embedded
                } else if resolved.is_some() {
                    Status::Linked
                } else {
                    Status::Missing
                };
                Entry { name: v.clone(), kind, status, resolved, size, uses: Vec::new(), origin: self.meta.get(&format!("{ORIGIN}{v}")).cloned() }
            });
            e.uses.push(u);
        }
        // embedded resources no node refers to
        for (k, b) in &self.resources {
            if k.starts_with("bake/") || map.contains_key(k) {
                continue;
            }
            map.insert(
                k.clone(),
                Entry { name: k.clone(), kind: classify(k, None), status: Status::Embedded, resolved: None, size: Some(b.len()), uses: Vec::new(), origin: self.meta.get(&format!("{ORIGIN}{k}")).cloned() },
            );
        }
        // baked maps, one entry per set
        let mut baked: BTreeMap<String, (usize, bool)> = BTreeMap::new();
        for (k, b) in &self.resources {
            if let Some(rest) = k.strip_prefix("bake/") {
                let set = rest.split('/').next().unwrap_or("").trim_end_matches(".cfg").to_string();
                let e = baked.entry(set).or_insert((0, true));
                e.0 += b.len();
            }
        }
        for (set, (size, _)) in baked {
            let name = format!("bake/{set}");
            let mut uses = Vec::new();
            for (scope, g) in self.scopes() {
                for (id, n) in &g.nodes {
                    if n.kind == "baked_map" && n.strs.get("set").map(|s| s.trim() == set).unwrap_or(false) {
                        uses.push(Use { scope: scope.clone(), node: *id, node_kind: n.kind.clone(), key: "set".into() });
                    }
                }
            }
            map.insert(name.clone(), Entry { name, kind: ResKind::Baked, status: Status::Embedded, resolved: None, size: Some(size), uses, origin: None });
        }
        let mut v: Vec<Entry> = map.into_values().collect();
        v.sort_by(|a, b| (a.kind, a.name.to_lowercase()).cmp(&(b.kind, b.name.to_lowercase())));
        v
    }

    /// Linked files that cannot be found, with the nodes that need them.
    pub fn missing_files(&self) -> Vec<Entry> {
        self.resource_entries().into_iter().filter(|e| e.status == Status::Missing).collect()
    }

    fn unique_key(&self, base: &str, bytes: &Arc<Vec<u8>>) -> String {
        let mut key = base.to_string();
        let mut c = 1;
        while self.resources.get(&key).map(|b| b != bytes).unwrap_or(false) {
            key = format!("{c}_{base}");
            c += 1;
        }
        key
    }

    /// Reads a linked file into the package and points its nodes at the embedded copy. Returns the resource name.
    pub fn embed_file(&mut self, name: &str) -> Result<String, String> {
        if self.resources.contains_key(name) {
            return Ok(name.to_string());
        }
        let path = resolve(name).ok_or_else(|| format!("'{name}' not found"))?;
        let bytes = Arc::new(std::fs::read(&path).map_err(|e| format!("{}: {e}", path.display()))?);
        let key = self.unique_key(&sanitize(name), &bytes);
        self.resources.insert(key.clone(), bytes);
        let origin = path.canonicalize().unwrap_or(path).to_string_lossy().to_string();
        if !origin.contains(['"', '#', '\n']) {
            self.meta.insert(format!("{ORIGIN}{key}"), origin);
        }
        self.rewrite_uses(name, &key);
        Ok(key)
    }

    /// Embeds every linked file that can be found; returns (embedded, problems).
    pub fn embed_all(&mut self) -> (usize, Vec<String>) {
        let mut n = 0;
        let mut problems = Vec::new();
        for e in self.resource_entries() {
            match e.status {
                Status::Linked => match self.embed_file(&e.name) {
                    Ok(_) => n += 1,
                    Err(m) => problems.push(m),
                },
                Status::Missing => problems.push(format!("'{}' is missing", e.name)),
                Status::Embedded => {}
            }
        }
        (n, problems)
    }

    /// Writes an embedded resource to `dir` and turns it into a link to that file.
    pub fn extract(&mut self, name: &str, dir: &Path) -> Result<PathBuf, String> {
        let bytes = self.resources.get(name).cloned().ok_or_else(|| format!("'{name}' is not an embedded resource"))?;
        std::fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
        let target = dir.join(sanitize(name));
        crate::safeio::write_file(&target, &bytes).map_err(|e| format!("{}: {e}", target.display()))?;
        let value = target.to_string_lossy().to_string();
        self.rewrite_uses(name, &value);
        self.resources.remove(name);
        self.meta.remove(&format!("{ORIGIN}{name}"));
        Ok(target)
    }

    /// Points every node using `name` at another file (which must exist). An embedded copy that nothing else needs
    /// is dropped.
    pub fn relink_file(&mut self, name: &str, new_path: &str) -> Result<usize, String> {
        if resolve(new_path).is_none() {
            return Err(format!("'{new_path}' not found"));
        }
        let n = self.rewrite_uses(name, new_path);
        if self.resources.contains_key(name) && !self.file_uses().iter().any(|(_, v)| v == name) {
            self.resources.remove(name);
            self.meta.remove(&format!("{ORIGIN}{name}"));
        }
        Ok(n)
    }

    /// Replaces the contents of an embedded resource with another file, keeping its name. For a linked file this is a relink.
    pub fn replace_resource(&mut self, name: &str, new_path: &str) -> Result<(), String> {
        if !self.resources.contains_key(name) {
            return self.relink_file(name, new_path).map(|_| ());
        }
        let path = resolve(new_path).ok_or_else(|| format!("'{new_path}' not found"))?;
        let bytes = std::fs::read(&path).map_err(|e| format!("{}: {e}", path.display()))?;
        self.resources.insert(name.to_string(), Arc::new(bytes));
        let origin = path.canonicalize().unwrap_or(path).to_string_lossy().to_string();
        if !origin.contains(['"', '#', '\n']) {
            self.meta.insert(format!("{ORIGIN}{name}"), origin);
        }
        Ok(())
    }

    /// Re-reads an embedded resource from the file it was imported from.
    pub fn update_resource(&mut self, name: &str) -> Result<(), String> {
        let origin = self.meta.get(&format!("{ORIGIN}{name}")).cloned().ok_or_else(|| format!("'{name}' has no recorded source file"))?;
        let bytes = read_file(&origin)?;
        self.resources.insert(name.to_string(), Arc::new(bytes));
        Ok(())
    }

    /// Updates every embedded resource whose source file is still there and has changed; returns the names updated.
    pub fn update_all(&mut self) -> Vec<String> {
        let mut done = Vec::new();
        let names: Vec<String> = self.resources.keys().cloned().collect();
        for n in names {
            let Some(origin) = self.meta.get(&format!("{ORIGIN}{n}")).cloned() else { continue };
            if let Ok(b) = read_file(&origin) {
                if self.resources.get(&n).map(|o| o.as_slice() != b.as_slice()).unwrap_or(true) {
                    self.resources.insert(n.clone(), Arc::new(b));
                    done.push(n);
                }
            }
        }
        done
    }

    /// Whether the source file of an embedded resource has changed since it was imported.
    pub fn source_changed(&self, name: &str) -> Option<bool> {
        let origin = self.meta.get(&format!("{ORIGIN}{name}"))?;
        let b = read_file(origin).ok()?;
        Some(self.resources.get(name).map(|o| o.as_slice() != b.as_slice()).unwrap_or(true))
    }

    /// Copies every linked file into `project_dir/sub` and links the nodes to the copies by relative path, so the
    /// project folder can be moved or shared as a whole. Returns (copied, problems).
    pub fn collect_files(&mut self, project_dir: &Path, sub: &str) -> (usize, Vec<String>) {
        let mut copied = 0;
        let mut problems = Vec::new();
        let dir = if sub.is_empty() { project_dir.to_path_buf() } else { project_dir.join(sub) };
        for e in self.resource_entries() {
            match (e.status, &e.resolved) {
                (Status::Linked, Some(src)) => {
                    if let Err(m) = std::fs::create_dir_all(&dir) {
                        problems.push(format!("{}: {m}", dir.display()));
                        continue;
                    }
                    let base = sanitize(&e.name);
                    let mut target = dir.join(&base);
                    let mut c = 1;
                    // a different file with the same name gets a numbered name
                    while target.exists() && std::fs::read(&target).ok() != std::fs::read(src).ok() {
                        target = dir.join(format!("{c}_{base}"));
                        c += 1;
                    }
                    if !target.exists() {
                        if let Err(m) = std::fs::copy(src, &target) {
                            problems.push(format!("{}: {m}", src.display()));
                            continue;
                        }
                    }
                    let rel = target.strip_prefix(project_dir).unwrap_or(&target).to_string_lossy().replace('\\', "/");
                    self.rewrite_uses(&e.name, &rel);
                    copied += 1;
                }
                (Status::Missing, _) => problems.push(format!("'{}' is missing and was not copied", e.name)),
                _ => {}
            }
        }
        (copied, problems)
    }

    /// Renames an embedded resource (or a link value) everywhere it is used.
    pub fn rename_resource(&mut self, old: &str, new: &str) -> Result<(), String> {
        let new = sanitize(new);
        if new == old {
            return Ok(());
        }
        if let Some(b) = self.resources.remove(old) {
            if self.resources.contains_key(&new) {
                self.resources.insert(old.to_string(), b);
                return Err(format!("a resource named '{new}' already exists"));
            }
            self.resources.insert(new.clone(), b);
            if let Some(o) = self.meta.remove(&format!("{ORIGIN}{old}")) {
                self.meta.insert(format!("{ORIGIN}{new}"), o);
            }
        }
        self.rewrite_uses(old, &new);
        Ok(())
    }

    /// Removes embedded resources that no node uses (baked map sets are kept). Returns the removed names.
    pub fn remove_unused(&mut self) -> Vec<String> {
        let used: std::collections::BTreeSet<String> = self.file_uses().into_iter().map(|(_, v)| v).collect();
        let dead: Vec<String> = self.resources.keys().filter(|k| !k.starts_with("bake/") && !used.contains(*k)).cloned().collect();
        for k in &dead {
            self.resources.remove(k);
            self.meta.remove(&format!("{ORIGIN}{k}"));
        }
        dead
    }

    /// Information and a small picture for one resource, for the resource list.
    pub fn resource_info(&self, e: &Entry) -> Info {
        let bytes: Option<Arc<Vec<u8>>> = match e.status {
            Status::Embedded => self.resources.get(&e.name).cloned(),
            Status::Linked => e.resolved.as_ref().and_then(|p| std::fs::read(p).ok()).map(Arc::new),
            Status::Missing => None,
        };
        match (e.kind, bytes) {
            (ResKind::Baked, _) => {
                let set = e.name.trim_start_matches("bake/");
                let maps = self.resources.keys().filter(|k| k.starts_with(&format!("bake/{set}/"))).count();
                Info { summary: format!("{maps} baked map(s)"), thumb: None }
            }
            (_, None) => Info { summary: "file not found".into(), thumb: None },
            (kind, Some(b)) => describe(&e.name, kind, &b),
        }
    }
}

pub struct Info {
    pub summary: String,
    pub thumb: Option<Tex>,
}

/// Longest side of a thumbnail.
pub const THUMB: usize = 96;

fn shrink(t: &Tex, max: usize) -> Tex {
    let s = (t.w.max(t.h) as f32 / max as f32).max(1.0);
    let (w, h) = (((t.w as f32 / s).round() as usize).max(1), ((t.h as f32 / s).round() as usize).max(1));
    if (w, h) == (t.w, t.h) {
        return t.clone();
    }
    // box filter
    let mut out = Tex::solid(w, h, [0.0; 4]);
    for y in 0..h {
        for x in 0..w {
            let (x0, x1) = (x * t.w / w, ((x + 1) * t.w / w).max(x * t.w / w + 1).min(t.w));
            let (y0, y1) = (y * t.h / h, ((y + 1) * t.h / h).max(y * t.h / h + 1).min(t.h));
            let mut acc = [0.0f32; 4];
            let mut n = 0.0;
            for yy in y0..y1 {
                for xx in x0..x1 {
                    let p = t.px[yy * t.w + xx];
                    for k in 0..4 {
                        acc[k] += p[k];
                    }
                    n += 1.0;
                }
            }
            out.px[y * w + x] = [acc[0] / n, acc[1] / n, acc[2] / n, acc[3] / n];
        }
    }
    out
}

/// Describes a file's contents and renders a thumbnail where that makes sense.
pub fn describe(name: &str, kind: ResKind, b: &[u8]) -> Info {
    match kind {
        ResKind::Image => match crate::imgio::decode_full(name, b) {
            Ok(d) => {
                let mut t = d.tex;
                if d.float {
                    // tone map for display
                    t = t.map(|p| {
                        let f = |v: f32| (v.max(0.0) / (1.0 + v.max(0.0))).powf(1.0 / 2.2) * 1.6;
                        [f(p[0]).min(1.0), f(p[1]).min(1.0), f(p[2]).min(1.0), p[3]]
                    });
                }
                Info { summary: format!("{} {}x{}{}", d.format.name(), t.w, t.h, if d.float { ", floating point" } else { "" }), thumb: Some(shrink(&t, THUMB)) }
            }
            Err(e) => Info { summary: format!("unreadable: {e}"), thumb: None },
        },
        ResKind::Vector => {
            let src = String::from_utf8_lossy(b);
            match crate::vector::render_svg(&src, THUMB, THUMB, 0, 1.0, 0.0, 0.0) {
                Ok(t) => Info { summary: "SVG vector image".into(), thumb: Some(t) },
                Err(e) => Info { summary: format!("unreadable: {e}"), thumb: None },
            }
        }
        ResKind::Font => match crate::font::Font::load(b) {
            Ok(f) => {
                let xf = crate::vector::Xf::translate(THUMB as f32 * 0.5, THUMB as f32 * 0.5);
                let polys = f.layout("Aa", THUMB as f32 * 0.55, 1, 0.0, 1.0, &xf);
                let m = crate::vector::fill(&polys, THUMB, THUMB, false);
                let mut t = Tex::solid(THUMB, THUMB, [0.0, 0.0, 0.0, 1.0]);
                for (p, c) in t.px.iter_mut().zip(&m.a) {
                    *p = [*c, *c, *c, 1.0];
                }
                Info { summary: f.describe(), thumb: Some(t) }
            }
            Err(e) => Info { summary: format!("unreadable: {e}"), thumb: None },
        },
        ResKind::Lut => match crate::lut::Lut::load(name, b) {
            Ok(l) => {
                // a colour ramp pushed through the table shows what it does
                let ramp = Tex::from_fn(THUMB, THUMB / 2, |u, v| {
                    let h = u * 6.0;
                    let (r, g, bl) = ((h - 3.0).abs() - 1.0, 2.0 - (h - 2.0).abs(), 2.0 - (h - 4.0).abs());
                    let (r, g, bl) = (r.clamp(0.0, 1.0), g.clamp(0.0, 1.0), bl.clamp(0.0, 1.0));
                    let k = 1.0 - v * 0.8;
                    [r * k + (1.0 - k) * 0.5, g * k + (1.0 - k) * 0.5, bl * k + (1.0 - k) * 0.5, 1.0]
                });
                Info { summary: l.describe(), thumb: Some(l.apply_tex(&ramp, 1.0)) }
            }
            Err(e) => Info { summary: format!("unreadable: {e}"), thumb: None },
        },
        ResKind::Spline => match crate::spline::Spline::from_bytes(b) {
            Ok(s) => Info { summary: format!("{} path(s)", s.paths.len()), thumb: None },
            Err(e) => Info { summary: format!("unreadable: {e}"), thumb: None },
        },
        _ => Info { summary: format!("{} bytes", b.len()), thumb: None },
    }
}

pub fn human_size(n: usize) -> String {
    if n >= 1 << 20 {
        format!("{:.1} MB", n as f64 / (1 << 20) as f64)
    } else if n >= 1 << 10 {
        format!("{:.1} KB", n as f64 / 1024.0)
    } else {
        format!("{n} B")
    }
}
