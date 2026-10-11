//! Node graph: text format, validation, subgraphs, packages and evaluation.
use crate::expr;
use crate::noise;
use crate::ops;
use crate::png;
use crate::tex::{lum, Tex};
use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::fmt;
use std::rc::Rc;

#[derive(Debug)]
pub enum Error {
    Parse(usize, String),
    UnknownNode(u32, String),
    UnknownSlot(u32, String),
    MissingInput(u32, String),
    MissingNode(u32, u32),
    Cycle(u32),
    BadParam(u32, String),
    NoOutputs,
    BadSize(usize),
    Io(String),
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::Parse(l, m) => write!(f, "line {l}: {m}"),
            Error::UnknownNode(id, k) => write!(f, "node {id}: unknown node type '{k}'"),
            Error::UnknownSlot(id, s) => write!(f, "node {id}: unknown input slot '{s}'"),
            Error::MissingInput(id, s) => write!(f, "node {id}: required input '{s}' is not connected"),
            Error::MissingNode(id, r) => write!(f, "node {id}: refers to missing node {r}"),
            Error::Cycle(id) => write!(f, "cycle detected at node {id}"),
            Error::BadParam(id, m) => write!(f, "node {id}: {m}"),
            Error::NoOutputs => write!(f, "graph has no outputs"),
            Error::BadSize(s) => write!(f, "invalid resolution {s} (use 1..=8192)"),
            Error::Io(m) => write!(f, "io error: {m}"),
        }
    }
}
impl std::error::Error for Error {}

pub struct Spec {
    pub kind: &'static str,
    pub req: &'static [&'static str],
    pub opt: &'static [&'static str],
    pub doc: &'static str,
}

macro_rules! s {
    ($k:expr, [$($r:expr),*], [$($o:expr),*], $d:expr) => {
        Spec { kind: $k, req: &[$($r),*], opt: &[$($o),*], doc: $d }
    };
}

pub const SPECS: &[Spec] = &[
    s!("color", [], [], "constant colour: r g b a"),
    s!("white_noise", [], [], "per-pixel random: seed"),
    s!("gaussian_noise", [], [], "gaussian-distributed per-pixel noise: seed"),
    s!("perlin", [], [], "perlin noise: scale scale_x scale_y seed"),
    s!("fbm", [], [], "fractal noise/clouds: scale octaves gain seed mode(0 fbm,1 ridged,2 turbulence)"),
    s!("voronoi", [], [], "cellular: scale jitter seed mode(0 dist,1 id,2 edge,3 f2)"),
    s!("gradient", [], [], "gradient: mode(0 linear,1 radial) angle"),
    s!("circle", [], [], "radius softness"),
    s!("rect", [], [], "width height softness"),
    s!("polygon", [], [], "sides radius softness rotation"),
    s!("checker", [], [], "tiles"),
    s!("bricks", [], [], "rows cols mortar bevel offset lum_var seed"),
    s!("tile_sampler", [], ["pattern"], "scatter pattern: count scale scale_var rot_var lum_var seed"),
    s!("pixel_processor", [], ["in"], "per-pixel expr / expr_g / expr_b / expr_a (string params)"),
    s!("sdf_shape", [], [], "2D signed distance shape: shape(0 circle,1 box,2 polygon,3 ring) size aspect sides rotation round x y range"),
    s!("sdf_op", ["a", "b"], [], "combine SDFs: op(0 union,1 subtract,2 intersect) smooth range"),
    s!("sdf_mask", ["in"], [], "SDF -> mask: offset softness"),
    s!("sdf_form", [], [], "procedural 3D SDF form as height: shape(0 sphere,1 box,2 torus,3 cylinder) size repeat round scatter seed height"),
    s!("bitmap", [], [], "imported PNG bitmap: file=\"path.png\""),
    s!("input", [], [], "subgraph input: idx"),
    s!("subgraph", [], ["in0", "in1", "in2", "in3"], "instance of a reusable subgraph: graph=\"name\""),
    s!("blur", ["in"], [], "gaussian blur: radius"),
    s!("directional_blur", ["in"], [], "angle length"),
    s!("sharpen", ["in"], [], "radius amount"),
    s!("levels", ["in"], [], "low high gamma out_low out_high"),
    s!("adjust", ["in"], [], "brightness contrast"),
    s!("invert", ["in"], [], "invert colours"),
    s!("threshold", ["in"], [], "threshold softness"),
    s!("histogram_scan", ["in"], [], "position width"),
    s!("quantize", ["in"], [], "steps"),
    s!("to_gray", ["in"], [], "luminance"),
    s!("channel", ["in"], [], "ch(0 r,1 g,2 b,3 a)"),
    s!("merge", [], ["r", "g", "b", "a"], "combine channels"),
    s!("tint", ["in"], [], "multiply by colour: r g b"),
    s!("colorize", ["in"], [], "gradient map: p0 r0 g0 b0 ... p7 r7 g7 b7"),
    s!("hsl", ["in"], [], "hue sat val"),
    s!("transform", ["in"], [], "offset_x offset_y scale_x scale_y rotation(turns)"),
    s!("mirror", ["in"], [], "axis(0 x,1 y,2 both)"),
    s!("warp", ["in", "warp"], [], "slope warp: intensity"),
    s!("directional_warp", ["in", "warp"], [], "angle intensity"),
    s!("blend", ["fg", "bg"], ["mask"], "mode(0 copy,1 add,2 sub,3 mul,4 screen,5 overlay,6 max,7 min,8 diff,9 softlight) opacity"),
    s!("normal", ["in"], [], "height->normal (OpenGL +Y): strength directx"),
    s!("normal_blend", ["a", "b"], [], "combine normal maps"),
    s!("ao", ["in"], [], "height-based ambient occlusion: radius intensity"),
    s!("curvature", ["in"], [], "radius intensity"),
    s!("edge_detect", ["in"], [], "intensity"),
    s!("emboss", ["in"], [], "angle intensity"),
];

pub fn spec(kind: &str) -> Option<&'static Spec> {
    SPECS.iter().find(|s| s.kind == kind)
}

pub fn category(kind: &str) -> &'static str {
    match kind {
        "color" | "white_noise" | "gaussian_noise" | "perlin" | "fbm" | "voronoi" | "gradient" | "circle" | "rect" | "polygon" | "checker" | "bricks"
        | "tile_sampler" | "pixel_processor" => "Generators",
        "sdf_shape" | "sdf_op" | "sdf_mask" | "sdf_form" => "SDF",
        "bitmap" | "input" | "subgraph" => "Utility",
        _ => "Filters",
    }
}

#[derive(Debug, Clone, Default)]
pub struct Node {
    pub kind: String,
    pub nums: BTreeMap<String, f32>,
    pub strs: BTreeMap<String, String>,
    pub ins: BTreeMap<String, u32>,
    pub pos: [f32; 2],
}

#[derive(Debug, Clone)]
pub struct Comment {
    pub text: String,
    pub pos: [f32; 2],
    pub size: [f32; 2],
    pub color: u8,
}

#[derive(Debug, Clone)]
pub struct Exposed {
    pub label: String,
    pub node: u32,
    pub param: String,
    pub min: f32,
    pub max: f32,
}

#[derive(Debug, Clone)]
pub struct Graph {
    pub size: usize,
    pub nodes: BTreeMap<u32, Node>,
    pub outputs: Vec<(String, u32)>,
    pub subgraphs: BTreeMap<String, Graph>,
    pub resources: BTreeMap<String, Vec<u8>>,
    pub exposed: Vec<Exposed>,
    pub comments: Vec<Comment>,
    pub seed_offset: u32,
}

impl Default for Graph {
    fn default() -> Self {
        Graph {
            size: 512,
            nodes: BTreeMap::new(),
            outputs: Vec::new(),
            subgraphs: BTreeMap::new(),
            resources: BTreeMap::new(),
            exposed: Vec::new(),
            comments: Vec::new(),
            seed_offset: 0,
        }
    }
}

fn split_tokens(line: &str) -> Result<Vec<String>, String> {
    let mut out = Vec::new();
    let mut cur = String::new();
    let mut in_q = false;
    let mut had_q = false;
    for c in line.chars() {
        match c {
            '"' => {
                in_q = !in_q;
                had_q = true;
            }
            c if c.is_whitespace() && !in_q => {
                if !cur.is_empty() || had_q {
                    out.push(std::mem::take(&mut cur));
                    had_q = false;
                }
            }
            c => cur.push(c),
        }
    }
    if in_q {
        return Err("unterminated quote".into());
    }
    if !cur.is_empty() || had_q {
        out.push(cur);
    }
    Ok(out)
}

// ---------- base64 (for embedded resources) ----------
const B64: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

pub fn b64_encode(d: &[u8]) -> String {
    let mut s = String::with_capacity(d.len() * 4 / 3 + 4);
    for c in d.chunks(3) {
        let n = (c[0] as u32) << 16 | (*c.get(1).unwrap_or(&0) as u32) << 8 | *c.get(2).unwrap_or(&0) as u32;
        s.push(B64[(n >> 18) as usize & 63] as char);
        s.push(B64[(n >> 12) as usize & 63] as char);
        s.push(if c.len() > 1 { B64[(n >> 6) as usize & 63] as char } else { '=' });
        s.push(if c.len() > 2 { B64[n as usize & 63] as char } else { '=' });
    }
    s
}

pub fn b64_decode(s: &str) -> Result<Vec<u8>, String> {
    let mut out = Vec::with_capacity(s.len() * 3 / 4);
    let (mut acc, mut bits) = (0u32, 0);
    for ch in s.bytes() {
        if ch == b'=' {
            break;
        }
        let v = B64.iter().position(|&b| b == ch).ok_or_else(|| "invalid base64".to_string())? as u32;
        acc = (acc << 6) | v;
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            out.push((acc >> bits) as u8);
            acc &= (1 << bits) - 1;
        }
    }
    Ok(out)
}

pub const THUMB: usize = 96;

pub struct EvalAll {
    pub thumbs: HashMap<u32, Vec<u8>>,
    pub errors: HashMap<u32, String>,
    pub kept: HashMap<u32, Tex>,
}

struct Ctx<'a> {
    size: usize,
    lib: &'a BTreeMap<String, Graph>,
    res: &'a BTreeMap<String, Vec<u8>>,
    so: u32,
    depth: usize,
}

impl Graph {
    pub fn parse(text: &str) -> Result<Graph, Error> {
        let g = Graph::parse_unchecked(text)?;
        g.validate()?;
        Ok(g)
    }

    /// Parse without validating connections (used by the editor for work-in-progress graphs).
    pub fn parse_unchecked(text: &str) -> Result<Graph, Error> {
        let mut g = Graph::default();
        let lines: Vec<&str> = text.lines().collect();
        let mut i = 0;
        while i < lines.len() {
            let ln = i + 1;
            let raw = lines[i];
            i += 1;
            let line = raw.split('#').next().unwrap_or("").trim();
            if line.is_empty() {
                continue;
            }
            if let Some(name) = line.strip_prefix("subgraph ") {
                let mut body = String::new();
                let mut closed = false;
                while i < lines.len() {
                    let l = lines[i];
                    i += 1;
                    if l.trim() == "end" {
                        closed = true;
                        break;
                    }
                    body.push_str(l);
                    body.push('\n');
                }
                if !closed {
                    return Err(Error::Parse(ln, "subgraph without matching 'end'".into()));
                }
                let sub = Graph::parse_unchecked(&body).map_err(|e| Error::Parse(ln, format!("in subgraph '{}': {e}", name.trim())))?;
                g.subgraphs.insert(name.trim().to_string(), sub);
                continue;
            }
            if let Some(rest) = line.strip_prefix("comment ") {
                let t: Vec<&str> = rest.split_whitespace().collect();
                if t.len() < 5 {
                    return Err(Error::Parse(ln, "usage: comment x y w h color text...".into()));
                }
                let f = |k: usize| t[k].parse::<f32>().map_err(|_| Error::Parse(ln, "bad comment number".into()));
                g.comments.push(Comment { text: t[5..].join(" "), pos: [f(0)?, f(1)?], size: [f(2)?.max(40.0), f(3)?.max(30.0)], color: t[4].parse().unwrap_or(0) });
                continue;
            }
            let t = split_tokens(line).map_err(|m| Error::Parse(ln, m))?;
            match t[0].as_str() {
                "size" => {
                    g.size = t.get(1).and_then(|s| s.parse::<usize>().ok()).ok_or_else(|| Error::Parse(ln, "size needs an integer".into()))?;
                }
                "seed_offset" => {
                    g.seed_offset = t.get(1).and_then(|s| s.parse::<u32>().ok()).ok_or_else(|| Error::Parse(ln, "seed_offset needs an integer".into()))?;
                }
                "resource" => {
                    if t.len() != 3 {
                        return Err(Error::Parse(ln, "usage: resource <name> <base64>".into()));
                    }
                    g.resources.insert(t[1].clone(), b64_decode(&t[2]).map_err(|m| Error::Parse(ln, m))?);
                }
                "expose" => {
                    if t.len() != 6 {
                        return Err(Error::Parse(ln, "usage: expose <label> <node> <param> <min> <max>".into()));
                    }
                    let p = |s: &str| s.parse::<f32>().map_err(|_| Error::Parse(ln, "bad number".into()));
                    g.exposed.push(Exposed {
                        label: t[1].clone(),
                        node: t[2].parse().map_err(|_| Error::Parse(ln, "bad node id".into()))?,
                        param: t[3].clone(),
                        min: p(&t[4])?,
                        max: p(&t[5])?,
                    });
                }
                "output" => {
                    if t.len() != 3 {
                        return Err(Error::Parse(ln, "usage: output <name> <node-id>".into()));
                    }
                    let id = t[2].parse::<u32>().map_err(|_| Error::Parse(ln, "bad node id".into()))?;
                    g.outputs.push((t[1].clone(), id));
                }
                "node" => {
                    if t.len() < 3 {
                        return Err(Error::Parse(ln, "usage: node <id> <type> [key=value ...]".into()));
                    }
                    let id = t[1].parse::<u32>().map_err(|_| Error::Parse(ln, "bad node id".into()))?;
                    if g.nodes.contains_key(&id) {
                        return Err(Error::Parse(ln, format!("duplicate node id {id}")));
                    }
                    let mut n = Node { kind: t[2].clone(), ..Default::default() };
                    for kv in &t[3..] {
                        let (k, v) = kv.split_once('=').ok_or_else(|| Error::Parse(ln, format!("expected key=value, got '{kv}'")))?;
                        if let Some(r) = v.strip_prefix('@') {
                            n.ins.insert(k.to_string(), r.parse::<u32>().map_err(|_| Error::Parse(ln, format!("bad reference '{v}'")))?);
                        } else if let Ok(x) = v.parse::<f32>() {
                            match k {
                                "_x" => n.pos[0] = x,
                                "_y" => n.pos[1] = x,
                                _ => {
                                    n.nums.insert(k.to_string(), x);
                                }
                            }
                        } else {
                            n.strs.insert(k.to_string(), v.to_string());
                        }
                    }
                    g.nodes.insert(id, n);
                }
                other => return Err(Error::Parse(ln, format!("unknown directive '{other}'"))),
            }
        }
        Ok(g)
    }

    pub fn validate(&self) -> Result<(), Error> {
        if self.size == 0 || self.size > 8192 {
            return Err(Error::BadSize(self.size));
        }
        if self.outputs.is_empty() {
            return Err(Error::NoOutputs);
        }
        self.validate_nodes()?;
        for (name, id) in &self.outputs {
            if !self.nodes.contains_key(id) {
                return Err(Error::MissingNode(0, *id));
            }
            if name.is_empty() || name.contains(['/', '\\', '.']) {
                return Err(Error::Parse(0, format!("invalid output name '{name}'")));
            }
        }
        self.check_cycles()
    }

    fn validate_nodes(&self) -> Result<(), Error> {
        for (id, n) in &self.nodes {
            let sp = spec(&n.kind).ok_or_else(|| Error::UnknownNode(*id, n.kind.clone()))?;
            for r in sp.req {
                if !n.ins.contains_key(*r) {
                    return Err(Error::MissingInput(*id, r.to_string()));
                }
            }
            for (slot, rid) in &n.ins {
                if !sp.req.contains(&slot.as_str()) && !sp.opt.contains(&slot.as_str()) {
                    return Err(Error::UnknownSlot(*id, slot.clone()));
                }
                if !self.nodes.contains_key(rid) {
                    return Err(Error::MissingNode(*id, *rid));
                }
            }
            if n.kind == "pixel_processor" {
                if !n.strs.contains_key("expr") {
                    return Err(Error::BadParam(*id, "pixel_processor needs expr=\"...\"".into()));
                }
                for (k, v) in &n.strs {
                    if k.starts_with("expr") {
                        expr::parse(v).map_err(|m| Error::BadParam(*id, format!("{k}: {m}")))?;
                    }
                }
            }
            if n.kind == "subgraph" {
                match n.strs.get("graph") {
                    Some(name) if self.subgraphs.contains_key(name) => {}
                    Some(name) => return Err(Error::BadParam(*id, format!("unknown subgraph '{name}'"))),
                    None => return Err(Error::BadParam(*id, "subgraph node needs graph=\"name\"".into())),
                }
            }
            if n.kind == "bitmap" && !n.strs.contains_key("file") {
                return Err(Error::BadParam(*id, "bitmap needs file=\"path.png\"".into()));
            }
        }
        Ok(())
    }

    /// Cycle detection (iterative DFS, 0=unseen 1=active 2=done).
    pub fn check_cycles(&self) -> Result<(), Error> {
        let mut state: HashMap<u32, u8> = HashMap::new();
        for &start in self.nodes.keys() {
            if state.get(&start).copied().unwrap_or(0) != 0 {
                continue;
            }
            let mut stack: Vec<(u32, Vec<u32>)> = vec![(start, self.nodes[&start].ins.values().copied().collect())];
            state.insert(start, 1);
            while let Some((id, deps)) = stack.last_mut() {
                if let Some(d) = deps.pop() {
                    if !self.nodes.contains_key(&d) {
                        continue;
                    }
                    match state.get(&d).copied().unwrap_or(0) {
                        1 => return Err(Error::Cycle(d)),
                        0 => {
                            state.insert(d, 1);
                            let nd: Vec<u32> = self.nodes[&d].ins.values().copied().collect();
                            stack.push((d, nd));
                        }
                        _ => {}
                    }
                } else {
                    state.insert(*id, 2);
                    stack.pop();
                }
            }
        }
        Ok(())
    }

    pub fn to_text(&self) -> String {
        let mut s = format!("size {}\n", self.size);
        if self.seed_offset != 0 {
            s.push_str(&format!("seed_offset {}\n", self.seed_offset));
        }
        for (k, v) in &self.resources {
            s.push_str(&format!("resource {k} {}\n", b64_encode(v)));
        }
        for (name, sub) in &self.subgraphs {
            s.push_str(&format!("subgraph {name}\n{}end\n", sub.to_text()));
        }
        for (id, n) in &self.nodes {
            s.push_str(&format!("node {id} {}", n.kind));
            if n.pos != [0.0, 0.0] {
                s.push_str(&format!(" _x={} _y={}", n.pos[0], n.pos[1]));
            }
            for (k, v) in &n.nums {
                s.push_str(&format!(" {k}={v}"));
            }
            for (k, v) in &n.strs {
                s.push_str(&format!(" {k}=\"{}\"", v.replace('"', "").replace('#', "")));
            }
            for (k, v) in &n.ins {
                s.push_str(&format!(" {k}=@{v}"));
            }
            s.push('\n');
        }
        for (name, id) in &self.outputs {
            s.push_str(&format!("output {name} {id}\n"));
        }
        for e in &self.exposed {
            s.push_str(&format!("expose \"{}\" {} {} {} {}\n", e.label.replace('"', ""), e.node, e.param, e.min, e.max));
        }
        for c in &self.comments {
            s.push_str(&format!("comment {} {} {} {} {} {}\n", c.pos[0], c.pos[1], c.size[0], c.size[1], c.color, c.text.replace('#', "").replace('\n', " ")));
        }
        s
    }

    /// Flatten into a distributable package (.mfa): embeds bitmaps, drops editor-only data.
    pub fn publish(&self) -> Result<String, Error> {
        self.validate()?;
        let mut g = self.clone();
        g.comments.clear();
        let mut res = std::mem::take(&mut g.resources);
        let mut seen: HashMap<String, String> = HashMap::new();
        fn embed(g: &mut Graph, res: &mut BTreeMap<String, Vec<u8>>, seen: &mut HashMap<String, String>, own: &BTreeMap<String, Vec<u8>>) -> Result<(), Error> {
            for (id, n) in g.nodes.iter_mut() {
                n.pos = [0.0, 0.0];
                if n.kind != "bitmap" {
                    continue;
                }
                let Some(f) = n.strs.get("file").cloned() else { continue };
                if let Some(k) = seen.get(&f) {
                    n.strs.insert("file".into(), k.clone());
                    continue;
                }
                let bytes = match res.get(&f).or_else(|| own.get(&f)) {
                    Some(b) => b.clone(),
                    None => std::fs::read(&f).map_err(|e| Error::BadParam(*id, format!("cannot embed '{f}': {e}")))?,
                };
                let base = std::path::Path::new(&f).file_name().map(|s| s.to_string_lossy().to_string()).unwrap_or_else(|| "bitmap.png".into());
                let mut key = base.clone();
                let mut c = 1;
                while res.contains_key(&key) && res.get(&key) != Some(&bytes) {
                    key = format!("{c}_{base}");
                    c += 1;
                }
                res.insert(key.clone(), bytes);
                seen.insert(f, key.clone());
                n.strs.insert("file".into(), key);
            }
            Ok(())
        }
        let own = BTreeMap::new();
        embed(&mut g, &mut res, &mut seen, &own)?;
        for sub in g.subgraphs.values_mut() {
            embed(sub, &mut res, &mut seen, &own)?;
        }
        g.resources = res;
        Ok(format!("# MatForge published asset\n{}", g.to_text()))
    }

    /// Apply exposed-parameter overrides by label.
    pub fn apply_params(&mut self, p: &BTreeMap<String, f32>) -> Result<(), String> {
        for (label, v) in p {
            let e = self.exposed.iter().find(|e| &e.label == label).cloned().ok_or_else(|| format!("no exposed parameter '{label}'"))?;
            let n = self.nodes.get_mut(&e.node).ok_or_else(|| format!("exposed node {} missing", e.node))?;
            n.nums.insert(e.param.clone(), v.clamp(e.min.min(e.max), e.min.max(e.max)));
        }
        Ok(())
    }

    /// Replace the selected nodes with a subgraph instance. Returns the new node id.
    pub fn extract_subgraph(&mut self, sel: &BTreeSet<u32>, name: &str) -> Result<u32, String> {
        let sel: BTreeSet<u32> = sel.iter().copied().filter(|i| self.nodes.contains_key(i)).collect();
        if sel.is_empty() {
            return Err("select at least one node".into());
        }
        if self.subgraphs.contains_key(name) {
            return Err(format!("subgraph '{name}' already exists"));
        }
        let mut sub = Graph { size: self.size, ..Default::default() };
        let mut map: HashMap<u32, u32> = HashMap::new();
        let mut next = 1u32;
        for id in &sel {
            map.insert(*id, next);
            next += 1;
        }
        let mut ext: Vec<u32> = Vec::new();
        let mut input_ids: HashMap<u32, u32> = HashMap::new();
        for id in &sel {
            let n = &self.nodes[id];
            let mut nn = n.clone();
            nn.ins.clear();
            for (slot, src) in &n.ins {
                if let Some(m) = map.get(src) {
                    nn.ins.insert(slot.clone(), *m);
                } else {
                    let iid = if let Some(x) = input_ids.get(src) {
                        *x
                    } else {
                        if ext.len() >= 4 {
                            return Err("a subgraph supports at most 4 external inputs".into());
                        }
                        let idx = ext.len();
                        ext.push(*src);
                        let iid = next;
                        next += 1;
                        let mut inode = Node { kind: "input".into(), ..Default::default() };
                        inode.nums.insert("idx".into(), idx as f32);
                        inode.pos = [n.pos[0] - 150.0, n.pos[1]];
                        sub.nodes.insert(iid, inode);
                        input_ids.insert(*src, iid);
                        iid
                    };
                    nn.ins.insert(slot.clone(), iid);
                }
            }
            sub.nodes.insert(map[id], nn);
        }
        let mut outs: Vec<u32> = Vec::new();
        for (oid, on) in &self.nodes {
            if sel.contains(oid) {
                continue;
            }
            for src in on.ins.values() {
                if sel.contains(src) && !outs.contains(src) {
                    outs.push(*src);
                }
            }
        }
        for (_, s) in &self.outputs {
            if sel.contains(s) && !outs.contains(s) {
                outs.push(*s);
            }
        }
        if outs.len() > 1 {
            return Err(format!("the selection has {} outputs used elsewhere; a subgraph supports exactly one", outs.len()));
        }
        let out_id = match outs.first() {
            Some(x) => *x,
            None => {
                let consumed: BTreeSet<u32> = sel.iter().flat_map(|i| self.nodes[i].ins.values().copied()).collect();
                *sel.iter().rev().find(|i| !consumed.contains(i)).unwrap_or_else(|| sel.iter().next_back().unwrap_or(&0))
            }
        };
        sub.outputs.push(("out".into(), map[&out_id]));
        let (mut cx, mut cy) = (0.0, 0.0);
        for id in &sel {
            cx += self.nodes[id].pos[0];
            cy += self.nodes[id].pos[1];
        }
        let new_id = self.nodes.keys().max().copied().unwrap_or(0) + 1;
        for id in &sel {
            self.nodes.remove(id);
        }
        let mut inst = Node { kind: "subgraph".into(), pos: [cx / sel.len() as f32, cy / sel.len() as f32], ..Default::default() };
        inst.strs.insert("graph".into(), name.to_string());
        for (i, e) in ext.iter().enumerate() {
            inst.ins.insert(format!("in{i}"), *e);
        }
        self.nodes.insert(new_id, inst);
        for n in self.nodes.values_mut() {
            for v in n.ins.values_mut() {
                if *v == out_id {
                    *v = new_id;
                }
            }
        }
        self.outputs.retain(|(_, s)| !sel.contains(s) || *s == out_id);
        for (_, s) in self.outputs.iter_mut() {
            if *s == out_id {
                *s = new_id;
            }
        }
        self.subgraphs.insert(name.to_string(), sub);
        Ok(new_id)
    }

    // ---------------- evaluation ----------------
    pub fn evaluate(&self, size: Option<usize>) -> Result<BTreeMap<String, Tex>, Error> {
        self.validate()?;
        let size = size.unwrap_or(self.size);
        if size == 0 || size > 8192 {
            return Err(Error::BadSize(size));
        }
        let ctx = Ctx { size, lib: &self.subgraphs, res: &self.resources, so: self.seed_offset, depth: 0 };
        let mut cache: HashMap<u32, Rc<Tex>> = HashMap::new();
        let mut res = BTreeMap::new();
        for (name, id) in &self.outputs {
            let t = eval_node(self, *id, &ctx, &[], &mut cache)?;
            res.insert(name.clone(), (*t).clone());
        }
        Ok(res)
    }

    /// Evaluate a single node (editor preview). Ignores unrelated incomplete nodes.
    pub fn evaluate_node(&self, id: u32, size: usize) -> Result<Tex, Error> {
        if size == 0 || size > 8192 {
            return Err(Error::BadSize(size));
        }
        self.check_cycles()?;
        let ctx = Ctx { size, lib: &self.subgraphs, res: &self.resources, so: self.seed_offset, depth: 0 };
        let mut cache: HashMap<u32, Rc<Tex>> = HashMap::new();
        Ok((*eval_node(self, id, &ctx, &[], &mut cache)?).clone())
    }

    /// Evaluate every node once (shared cache). Produces thumbnails, per-node errors and
    /// full-resolution textures for the ids in `keep`. Result is Send, so it can run on a worker thread.
    pub fn evaluate_all(&self, size: usize, keep: &[u32]) -> EvalAll {
        let mut r = EvalAll { thumbs: HashMap::new(), errors: HashMap::new(), kept: HashMap::new() };
        if size == 0 || size > 8192 {
            r.errors.insert(0, Error::BadSize(size).to_string());
            return r;
        }
        if let Err(e) = self.check_cycles() {
            r.errors.insert(0, e.to_string());
            return r;
        }
        let ctx = Ctx { size, lib: &self.subgraphs, res: &self.resources, so: self.seed_offset, depth: 0 };
        let mut cache: HashMap<u32, Rc<Tex>> = HashMap::new();
        for id in self.nodes.keys() {
            match eval_node(self, *id, &ctx, &[], &mut cache) {
                Ok(t) => {
                    let mut px = Vec::with_capacity(THUMB * THUMB * 4);
                    for y in 0..THUMB {
                        for x in 0..THUMB {
                            let p = t.sample((x as f32 + 0.5) / THUMB as f32, (y as f32 + 0.5) / THUMB as f32);
                            for c in &p[..3] {
                                px.push((c.clamp(0.0, 1.0) * 255.0 + 0.5) as u8);
                            }
                            px.push(255);
                        }
                    }
                    r.thumbs.insert(*id, px);
                    if keep.contains(id) {
                        r.kept.insert(*id, (*t).clone());
                    }
                }
                Err(e) => {
                    r.errors.insert(*id, e.to_string());
                }
            }
        }
        r
    }
}

fn eval_node(g: &Graph, id: u32, ctx: &Ctx, ext: &[Option<Rc<Tex>>], cache: &mut HashMap<u32, Rc<Tex>>) -> Result<Rc<Tex>, Error> {
    if let Some(t) = cache.get(&id) {
        return Ok(t.clone());
    }
    let n = g.nodes.get(&id).ok_or(Error::MissingNode(0, id))?;
    let mut ins: BTreeMap<String, Rc<Tex>> = BTreeMap::new();
    for (slot, rid) in &n.ins {
        ins.insert(slot.clone(), eval_node(g, *rid, ctx, ext, cache)?);
    }
    let t = match n.kind.as_str() {
        "input" => {
            let idx = n.nums.get("idx").copied().unwrap_or(0.0).max(0.0) as usize;
            match ext.get(idx).cloned().flatten() {
                Some(t) => (*t).clone(),
                None => Tex::solid(ctx.size, ctx.size, [0.0, 0.0, 0.0, 1.0]),
            }
        }
        "subgraph" => run_sub(id, n, &ins, ctx)?,
        _ => compute(id, n, &ins, ctx)?,
    };
    let rc = Rc::new(t);
    cache.insert(id, rc.clone());
    Ok(rc)
}

fn run_sub(id: u32, n: &Node, ins: &BTreeMap<String, Rc<Tex>>, ctx: &Ctx) -> Result<Tex, Error> {
    if ctx.depth >= 8 {
        return Err(Error::BadParam(id, "subgraph nesting too deep (recursive?)".into()));
    }
    let name = n.strs.get("graph").ok_or_else(|| Error::BadParam(id, "subgraph needs graph=\"name\"".into()))?;
    let mut sub = ctx.lib.get(name).cloned().ok_or_else(|| Error::BadParam(id, format!("unknown subgraph '{name}'")))?;
    sub.check_cycles()?;
    for (k, v) in &n.nums {
        if let Some((nid, param)) = k.split_once('.') {
            if let Ok(nid) = nid.parse::<u32>() {
                if let Some(inner) = sub.nodes.get_mut(&nid) {
                    inner.nums.insert(param.to_string(), *v);
                }
            }
        }
    }
    let ext: Vec<Option<Rc<Tex>>> = (0..4).map(|i| ins.get(&format!("in{i}")).cloned()).collect();
    let out_id = sub
        .outputs
        .iter()
        .find(|(n, _)| n == "out")
        .or_else(|| sub.outputs.first())
        .map(|(_, i)| *i)
        .ok_or_else(|| Error::BadParam(id, format!("subgraph '{name}' has no output")))?;
    let c2 = Ctx { size: ctx.size, lib: ctx.lib, res: ctx.res, so: ctx.so, depth: ctx.depth + 1 };
    let mut cache = HashMap::new();
    Ok((*eval_node(&sub, out_id, &c2, &ext, &mut cache)?).clone())
}

fn compute(id: u32, n: &Node, ins: &BTreeMap<String, Rc<Tex>>, ctx: &Ctx) -> Result<Tex, Error> {
    let f = |k: &str, d: f32| n.nums.get(k).copied().unwrap_or(d);
    let tex = |slot: &str| -> Result<&Tex, Error> {
        ins.get(slot).map(|r| r.as_ref()).ok_or_else(|| Error::MissingInput(id, slot.to_string()))
    };
    let seed = (f("seed", 0.0).max(0.0) as u32).wrapping_add(ctx.so);
    let per = |k: &str, d: f32| f(k, d).round().clamp(1.0, 4096.0) as i32;
    let s = ctx.size;
    let out = match n.kind.as_str() {
        "color" => Tex::solid(s, s, [f("r", 0.0), f("g", 0.0), f("b", 0.0), f("a", 1.0)]),
        "white_noise" => Tex::gray_fn(s, s, |u, v| noise::rnd((u * s as f32) as i32, (v * s as f32) as i32, seed)),
        "gaussian_noise" => Tex::gray_fn(s, s, |u, v| {
            let (x, y) = ((u * s as f32) as i32, (v * s as f32) as i32);
            let sum: f32 = (0..4).map(|i| noise::rnd(x, y, seed.wrapping_add(i * 17))).sum();
            ((sum / 4.0 - 0.5) * 3.46 + 0.5).clamp(0.0, 1.0)
        }),
        "perlin" => {
            let (px, py) = (per("scale_x", f("scale", 4.0)), per("scale_y", f("scale", 4.0)));
            Tex::gray_fn(s, s, move |u, v| noise::perlin(u, v, px, py, seed))
        }
        "fbm" => {
            let (px, py) = (per("scale_x", f("scale", 4.0)), per("scale_y", f("scale", 4.0)));
            let (oct, gain, mode) = (f("octaves", 5.0).max(1.0) as u32, f("gain", 0.5), f("mode", 0.0).max(0.0) as u32);
            Tex::gray_fn(s, s, move |u, v| noise::fbm(u, v, px, py, oct, gain, seed, mode))
        }
        "voronoi" => {
            let (p, j, mode) = (per("scale", 6.0), f("jitter", 1.0).clamp(0.0, 1.0), f("mode", 0.0) as u32);
            Tex::gray_fn(s, s, move |u, v| {
                let (a, b, c) = noise::voronoi(u, v, p, p, seed, j);
                match mode {
                    1 => c,
                    2 => ((b - a) * 4.0).clamp(0.0, 1.0),
                    3 => b,
                    _ => a,
                }
            })
        }
        "gradient" => {
            let (mode, (sa, ca)) = (f("mode", 0.0) as u32, (f("angle", 0.0) * std::f32::consts::TAU).sin_cos());
            Tex::gray_fn(s, s, move |u, v| {
                if mode == 1 {
                    (((u - 0.5).powi(2) + (v - 0.5).powi(2)).sqrt() * 2.0).clamp(0.0, 1.0)
                } else {
                    ((u - 0.5) * ca + (v - 0.5) * sa + 0.5).clamp(0.0, 1.0)
                }
            })
        }
        "circle" => {
            let (r, so) = (f("radius", 0.4), f("softness", 0.01).max(1e-4));
            Tex::gray_fn(s, s, move |u, v| 1.0 - smooth(r - so, r, ((u - 0.5).powi(2) + (v - 0.5).powi(2)).sqrt()))
        }
        "rect" => {
            let (w, h, so) = (f("width", 0.6), f("height", 0.4), f("softness", 0.01).max(1e-4));
            Tex::gray_fn(s, s, move |u, v| {
                let d = ((u - 0.5).abs() - w * 0.5).max((v - 0.5).abs() - h * 0.5);
                1.0 - smooth(-so, 0.0, d)
            })
        }
        "polygon" => {
            let (sd, r, so, rot) = (f("sides", 5.0), f("radius", 0.4), f("softness", 0.01).max(1e-4), f("rotation", 0.0));
            Tex::gray_fn(s, s, move |u, v| 1.0 - smooth(-so, 0.0, ops::polygon_sdf(u - 0.5, v - 0.5, sd, r, rot)))
        }
        "checker" => {
            let t = f("tiles", 8.0).round().max(1.0);
            Tex::gray_fn(s, s, move |u, v| (((u * t).floor() + (v * t).floor()) as i64 & 1) as f32)
        }
        "bricks" => {
            let (r, c, m, b, o, lv) = (f("rows", 8.0), f("cols", 4.0), f("mortar", 0.03), f("bevel", 0.05), f("offset", 0.5), f("lum_var", 0.3));
            Tex::gray_fn(s, s, move |u, v| ops::bricks(u, v, r, c, m, b, o, lv, seed))
        }
        "tile_sampler" => ops::tile_sampler(
            s,
            s,
            ins.get("pattern").map(|p| p.as_ref()),
            f("count", 40.0).max(0.0) as u32,
            f("scale", 0.1),
            f("scale_var", 0.3),
            f("rot_var", 1.0),
            f("lum_var", 0.0),
            seed,
        ),
        "pixel_processor" => {
            let parse = |k: &str| -> Result<Option<expr::Ex>, Error> {
                match n.strs.get(k) {
                    Some(src) => expr::parse(src).map(Some).map_err(|m| Error::BadParam(id, format!("{k}: {m}"))),
                    None => Ok(None),
                }
            };
            let e0 = parse("expr")?.ok_or_else(|| Error::BadParam(id, "missing expr".into()))?;
            let (eg, eb, ea) = (parse("expr_g")?, parse("expr_b")?, parse("expr_a")?);
            let input = ins.get("in").map(|t| t.as_ref());
            Tex::from_fn(s, s, |u, v| {
                let c = input.map(|t| t.sample(u, v)).unwrap_or([0.0, 0.0, 0.0, 1.0]);
                let env = expr::Env { vars: [u, v, u * s as f32, v * s as f32, s as f32, s as f32, c[0], c[1], c[2], lum(c)], tex: input };
                let r = expr::eval(&e0, &env);
                let g = eg.as_ref().map(|e| expr::eval(e, &env)).unwrap_or(r);
                let b = eb.as_ref().map(|e| expr::eval(e, &env)).unwrap_or(r);
                let a = ea.as_ref().map(|e| expr::eval(e, &env)).unwrap_or(1.0);
                [r.clamp(0.0, 1.0), g.clamp(0.0, 1.0), b.clamp(0.0, 1.0), a.clamp(0.0, 1.0)]
            })
        }
        "sdf_shape" => {
            let (shape, size, aspect, sides, rot, round) = (f("shape", 0.0) as u32, f("size", 0.25), f("aspect", 1.0), f("sides", 5.0), f("rotation", 0.0), f("round", 0.0));
            let (cx, cy, range) = (f("x", 0.0), f("y", 0.0), f("range", 0.25));
            Tex::gray_fn(s, s, move |u, v| ops::sdf_to_value(ops::sdf2d(shape, u - 0.5 - cx, v - 0.5 - cy, size, aspect, sides, rot, round), range))
        }
        "sdf_op" => {
            let (op, k, range) = (f("op", 0.0) as u32, f("smooth", 0.0), f("range", 0.25));
            let (a, b) = (tex("a")?, tex("b")?);
            Tex::gray_fn(s, s, move |u, v| {
                let (da, db) = (ops::value_to_sdf(lum(a.sample(u, v)), range), ops::value_to_sdf(lum(b.sample(u, v)), range));
                ops::sdf_to_value(ops::sdf_combine(op, da, db, k), range)
            })
        }
        "sdf_mask" => {
            let (off, soft) = (f("offset", 0.0), f("softness", 0.02).max(1e-4));
            let a = tex("in")?;
            Tex::gray_fn(s, s, move |u, v| smooth(0.5 + off - soft * 0.5, 0.5 + off + soft * 0.5, lum(a.sample(u, v))))
        }
        "sdf_form" => {
            let (shape, size, rep, round, scatter, hs) = (f("shape", 0.0) as u32, f("size", 0.3), f("repeat", 3.0), f("round", 0.05), f("scatter", 0.3), f("height", 1.5));
            Tex::gray_fn(s, s, move |u, v| ops::sdf_form(u, v, shape, size, rep, round, scatter, seed, hs))
        }
        "bitmap" => {
            let file = n.strs.get("file").ok_or_else(|| Error::BadParam(id, "bitmap needs file".into()))?;
            let bytes = match ctx.res.get(file) {
                Some(b) => b.clone(),
                None => std::fs::read(file).map_err(|e| Error::BadParam(id, format!("cannot read '{file}': {e}")))?,
            };
            png::decode(&bytes).map_err(|e| Error::BadParam(id, format!("'{file}': {e}")))?
        }
        "blur" => ops::blur(tex("in")?, f("radius", 4.0)),
        "directional_blur" => ops::directional_blur(tex("in")?, f("angle", 0.0), f("length", 0.05)),
        "sharpen" => ops::sharpen(tex("in")?, f("radius", 2.0), f("amount", 1.0)),
        "levels" => ops::levels(tex("in")?, f("low", 0.0), f("high", 1.0), f("gamma", 1.0), f("out_low", 0.0), f("out_high", 1.0)),
        "adjust" => ops::adjust(tex("in")?, f("brightness", 0.0), f("contrast", 1.0)),
        "invert" => ops::invert(tex("in")?),
        "threshold" => ops::threshold(tex("in")?, f("threshold", 0.5), f("softness", 0.0)),
        "histogram_scan" => ops::histogram_scan(tex("in")?, f("position", 0.5), f("width", 0.2)),
        "quantize" => ops::quantize(tex("in")?, f("steps", 4.0)),
        "to_gray" => ops::to_gray(tex("in")?),
        "channel" => ops::channel(tex("in")?, f("ch", 0.0).max(0.0) as usize),
        "merge" => {
            let g = |slot: &str, ch: usize| ins.get(slot).map(|t| ops::channel(t, ch));
            let (r, gg, b, a) = (g("r", 0), g("g", 1), g("b", 2), g("a", 3));
            Tex::from_fn(s, s, |u, v| {
                let q = |t: &Option<Tex>, d: f32| t.as_ref().map(|t| t.sample(u, v)[0]).unwrap_or(d);
                [q(&r, 0.0), q(&gg, 0.0), q(&b, 0.0), q(&a, 1.0)]
            })
        }
        "tint" => {
            let c = [f("r", 1.0), f("g", 1.0), f("b", 1.0)];
            tex("in")?.map(move |p| [p[0] * c[0], p[1] * c[1], p[2] * c[2], p[3]])
        }
        "colorize" => {
            let mut stops = Vec::new();
            for i in 0..8 {
                if let Some(p) = n.nums.get(&format!("p{i}")) {
                    let g = |c: &str| n.nums.get(&format!("{c}{i}")).copied().unwrap_or(0.0);
                    stops.push((*p, [g("r"), g("g"), g("b")]));
                }
            }
            if stops.is_empty() {
                return Err(Error::BadParam(id, "colorize needs at least one stop (p0 r0 g0 b0)".into()));
            }
            ops::colorize(tex("in")?, &stops)
        }
        "hsl" => ops::hsl_adjust(tex("in")?, f("hue", 0.0), f("sat", 1.0), f("val", 1.0)),
        "transform" => ops::transform(tex("in")?, f("offset_x", 0.0), f("offset_y", 0.0), f("scale_x", 1.0), f("scale_y", 1.0), f("rotation", 0.0)),
        "mirror" => ops::mirror(tex("in")?, f("axis", 0.0) as u32),
        "warp" => ops::warp(tex("in")?, tex("warp")?, f("intensity", 0.05)),
        "directional_warp" => ops::directional_warp(tex("in")?, tex("warp")?, f("angle", 0.0), f("intensity", 0.1)),
        "blend" => ops::blend(tex("fg")?, tex("bg")?, ins.get("mask").map(|m| m.as_ref()), f("mode", 0.0).max(0.0) as u32, f("opacity", 1.0)),
        "normal" => ops::normal_from_height(tex("in")?, f("strength", 2.0), f("directx", 0.0) > 0.5),
        "normal_blend" => ops::normal_blend(tex("a")?, tex("b")?),
        "ao" => ops::ambient_occlusion(tex("in")?, f("radius", 6.0), f("intensity", 2.0)),
        "curvature" => ops::curvature(tex("in")?, f("radius", 3.0), f("intensity", 4.0)),
        "edge_detect" => ops::edge_detect(tex("in")?, f("intensity", 2.0)),
        "emboss" => ops::emboss(tex("in")?, f("angle", 0.125), f("intensity", 4.0)),
        other => return Err(Error::UnknownNode(id, other.to_string())),
    };
    if out.w != s || out.h != s {
        return Ok(Tex::from_fn(s, s, |u, v| out.sample(u, v)));
    }
    Ok(out)
}

fn smooth(a: f32, b: f32, x: f32) -> f32 {
    let d = b - a;
    let t = if d.abs() < 1e-9 { if x >= b { 1.0 } else { 0.0 } } else { ((x - a) / d).clamp(0.0, 1.0) };
    t * t * (3.0 - 2.0 * t)
}

pub const EXAMPLE: &str = r#"# Brick wall PBR material
size 512
node 1 bricks rows=8 cols=4 mortar=0.04 bevel=0.10 lum_var=0.45 seed=3
node 2 fbm scale=8 octaves=5 seed=7
node 3 fbm scale=32 octaves=3 seed=9
node 4 blend fg=@2 bg=@1 mode=5 opacity=0.35
node 5 blend fg=@3 bg=@4 mode=1 opacity=0.06
node 6 normal in=@5 strength=3
node 7 ao in=@5 radius=6 intensity=2.5
node 8 colorize in=@5 p0=0 r0=0.10 g0=0.09 b0=0.08 p1=0.5 r1=0.55 g1=0.27 b1=0.19 p2=1 r2=0.78 g2=0.46 b2=0.34
node 9 blend fg=@7 bg=@8 mode=3 opacity=1
node 10 levels in=@3 low=0.2 high=0.9 out_low=0.55 out_high=0.95
node 11 color r=0 g=0 b=0
output basecolor 9
output height 5
output normal 6
output ambient_occlusion 7
output roughness 10
output metallic 11
expose "Brick count" 1 cols 1 12
expose "Mortar width" 1 mortar 0 0.15
expose "Normal strength" 6 strength 0 10
"#;
