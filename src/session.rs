// SPDX-License-Identifier: MIT OR Apache-2.0
//! The command layer: every action on a material graph as a named command with typed, documented parameters.
//!
//! One [`Session`] holds a graph with its undo history and runs commands against it. The command line (`exec`),
//! the MCP server (AI assistants), the control channel of the running editor and the tests all go through
//! [`Session::execute`], so they can never disagree about what an action does.
//!
//! The design follows the command-registry idea of the FilmCraft editor (MIT OR Apache-2.0, storytold/filmcraft):
//! one registry of commands with parameter docs, undoable edits, and a stdio MCP front end. The code here is written
//! for MaterialCraft's own graph model.
//!
//! Rules every command follows:
//! * it never panics: input comes from another program, a panic inside a command is caught and reported as an error;
//! * an edit is atomic: when it fails the graph is exactly as it was before;
//! * every successful edit is one undo step (`batch` makes many commands one step).

use crate::export::{self, Format};
use crate::graph::{self, Comment, Error as GError, Exposed, Graph, Node, NodeCache, OUTPUT_USAGES};
use crate::json::Json;
use crate::params::{self, PK};
use crate::tex::Tex;
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::sync::Arc;

/// Longest edge of an image the render commands return.
pub const MAX_RENDER: usize = 2048;
/// Undo steps kept.
pub const MAX_UNDO: usize = 200;
/// Commands in one `batch`.
pub const MAX_BATCH: usize = 2000;

// ------------------------------------------------------------------------------------------ descriptions

/// One documented parameter of a command.
pub struct Arg {
    pub name: &'static str,
    /// string, number, integer, boolean, object, array, node (a node id or name) or nodes (a list of those)
    pub ty: &'static str,
    pub req: bool,
    pub doc: &'static str,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Kind {
    /// reads the graph, changes nothing
    Query,
    /// changes the graph (one undo step)
    Edit,
    /// handled by the session itself (undo, redo, batch)
    Special,
}

type Run = fn(&mut Session, &Json) -> Result<Outcome, String>;

pub struct Command {
    pub id: &'static str,
    pub doc: &'static str,
    pub args: &'static [Arg],
    pub kind: Kind,
    run: Run,
}

/// A picture a command returns.
pub struct Image {
    pub label: String,
    pub png: Vec<u8>,
}

/// What a command returns: data, plus pictures for the commands that render.
pub struct Outcome {
    pub data: Json,
    pub images: Vec<Image>,
}

impl Outcome {
    fn data(d: Json) -> Result<Outcome, String> {
        Ok(Outcome { data: d, images: Vec::new() })
    }
}

macro_rules! arg {
    ($n:expr, $t:literal, $r:expr, $d:literal) => {
        Arg { name: $n, ty: $t, req: $r, doc: $d }
    };
}

const NODE: &str = "node";

pub const COMMANDS: &[Command] = &[
    // ---- discovery
    Command { id: "command.list", doc: "List every command with its parameters.", args: &[arg!("filter", "string", false, "only commands whose id or text contains this")], kind: Kind::Query, run: c_command_list },
    Command {
        id: "node.types",
        doc: "List the node types you can add: name, category and what it does. Filter by words (name, category or description).",
        args: &[arg!("query", "string", false, "words to look for, e.g. 'noise' or 'blur'"), arg!("category", "string", false, "only this category, e.g. Noises"), arg!("functions", "boolean", false, "list the function-graph nodes instead")],
        kind: Kind::Query,
        run: c_node_types,
    },
    Command { id: "node.describe", doc: "Everything about one node type: inputs, outputs, and every parameter with its default, range, choices and group.", args: &[arg!("kind", "string", true, "node type, e.g. voronoi")], kind: Kind::Query, run: c_node_describe },
    Command { id: "library.search", doc: "Search the built-in library of ready-made materials, generators and filters.", args: &[arg!("query", "string", false, "words, e.g. 'rusty metal'"), arg!("kind", "string", false, "material, generator or filter")], kind: Kind::Query, run: c_library_search },
    // ---- the graph
    Command { id: "graph.inspect", doc: "The whole graph: settings, every node with its parameters and connections, the outputs, and problems found.", args: &[arg!("compact", "boolean", false, "leave out positions and unchanged-looking details")], kind: Kind::Query, run: c_graph_inspect },
    Command { id: "graph.validate", doc: "Check the graph: missing inputs, wrong pin types, cycles, outputs, nodes that feed nothing. Returns a list of problems (empty = fine).", args: &[], kind: Kind::Query, run: c_graph_validate },
    Command { id: "graph.text", doc: "The graph as .mcg text (the file format).", args: &[], kind: Kind::Query, run: c_graph_text },
    Command { id: "graph.new", doc: "Start a new empty graph.", args: &[arg!("size", "integer", false, "width in pixels, a power of two from 16 to 8192 (default 512)"), arg!("height", "integer", false, "height in pixels; omit for square")], kind: Kind::Edit, run: c_graph_new },
    Command { id: "graph.open", doc: "Open a .mcg or .mca file.", args: &[arg!("path", "string", true, "file path")], kind: Kind::Edit, run: c_graph_open },
    Command { id: "graph.load_text", doc: "Replace the graph with .mcg text.", args: &[arg!("text", "string", true, "the .mcg text")], kind: Kind::Edit, run: c_graph_load_text },
    Command { id: "graph.save", doc: "Save the graph as .mcg (to `path`, or to the file it was opened from).", args: &[arg!("path", "string", false, "file path")], kind: Kind::Query, run: c_graph_save },
    Command { id: "graph.set", doc: "Change graph settings.", args: &[arg!("size", "integer", false, "width in pixels (16..8192)"), arg!("height", "integer", false, "height in pixels (0 = square)"), arg!("seed_offset", "integer", false, "added to every node's random seed"), arg!("precision", "integer", false, "0 = 32-bit float, 1 = 8 bit, 2 = 16 bit, 3 = 16-bit float")], kind: Kind::Edit, run: c_graph_set },
    Command { id: "graph.layout", doc: "Arrange all nodes in tidy columns from inputs to outputs (so the editor shows a readable graph).", args: &[], kind: Kind::Edit, run: c_graph_layout },
    Command { id: "library.load", doc: "Replace the graph with a library material, generator or filter, ready to edit.", args: &[arg!("id", "string", true, "library id from library.search")], kind: Kind::Edit, run: c_library_load },
    // ---- nodes
    Command {
        id: "node.add",
        doc: "Add a node. Returns its id. Parameters not given keep the node's defaults (see node.describe).",
        args: &[
            arg!("kind", "string", true, "node type (node.types)"),
            arg!("params", "object", false, "parameter values: numbers, choice names for dropdowns, true/false, or text for text parameters"),
            arg!("inputs", "object", false, "connections as {slot: node} or {slot: {node: id, output: name}}"),
            arg!("name", "string", false, "a short name you can use instead of the id in later commands"),
            arg!("x", "number", false, "position in the editor"),
            arg!("y", "number", false, "position in the editor"),
        ],
        kind: Kind::Edit,
        run: c_node_add,
    },
    Command {
        id: "node.set",
        doc: "Set parameters of a node. Numbers are range-checked (out-of-range values are clamped and reported); dropdowns take a choice name or index; null restores the default.",
        args: &[arg!(NODE, "node", true, "node id or name"), arg!("params", "object", true, "{parameter: value}"), arg!("allow_unknown", "boolean", false, "accept parameter names this node type does not list")],
        kind: Kind::Edit,
        run: c_node_set,
    },
    Command { id: "node.remove", doc: "Delete nodes. Links that used them are removed too.", args: &[arg!("nodes", "nodes", true, "node ids or names")], kind: Kind::Edit, run: c_node_remove },
    Command { id: "node.connect", doc: "Connect the output of one node to an input slot of another (replacing what was there).", args: &[arg!("from", "node", true, "source node"), arg!("to", "node", true, "target node"), arg!("slot", "string", false, "input slot of the target (default: the first free required slot, else 'in')"), arg!("output", "string", false, "which output of a multi-output source")], kind: Kind::Edit, run: c_node_connect },
    Command { id: "node.disconnect", doc: "Remove the link into an input slot.", args: &[arg!("to", "node", true, "target node"), arg!("slot", "string", true, "input slot")], kind: Kind::Edit, run: c_node_disconnect },
    Command { id: "node.move", doc: "Move a node in the editor.", args: &[arg!(NODE, "node", true, "node id or name"), arg!("x", "number", true, "x"), arg!("y", "number", true, "y")], kind: Kind::Edit, run: c_node_move },
    Command { id: "output.add", doc: "Make a node a material output (base colour, normal, roughness, metallic, height, ambient_occlusion, opacity, emissive ...). Replaces an existing output of the same usage.", args: &[arg!(NODE, "node", true, "node that provides the image"), arg!("usage", "string", true, "basecolor, normal, roughness, metallic, height, ambient_occlusion, opacity, emissive, diffuse, specular, glossiness or out"), arg!("output", "string", false, "which output of a multi-output node"), arg!("name", "string", false, "file name for the exported map (default: the usage)")], kind: Kind::Edit, run: c_output_add },
    Command { id: "output.remove", doc: "Remove the output with this usage.", args: &[arg!("usage", "string", true, "the usage to remove")], kind: Kind::Edit, run: c_output_remove },
    Command { id: "graph.expose", doc: "Expose a node parameter as a graph parameter (a slider for whoever uses the published material).", args: &[arg!("label", "string", true, "name shown to the user"), arg!(NODE, "node", true, "node"), arg!("param", "string", true, "parameter name"), arg!("min", "number", false, "smallest value (default: the parameter's range)"), arg!("max", "number", false, "largest value")], kind: Kind::Edit, run: c_graph_expose },
    Command { id: "graph.unexpose", doc: "Stop exposing a graph parameter.", args: &[arg!("label", "string", true, "its label")], kind: Kind::Edit, run: c_graph_unexpose },
    Command { id: "comment.add", doc: "Add a comment box to the editor canvas.", args: &[arg!("text", "string", true, "text"), arg!("x", "number", false, "x"), arg!("y", "number", false, "y"), arg!("width", "number", false, "width"), arg!("height", "number", false, "height")], kind: Kind::Edit, run: c_comment_add },
    // ---- looking at results
    Command {
        id: "render.node",
        doc: "Render one node and return a picture and statistics (min / max / mean per channel, whether it is flat, and a tiling check: seam values near 1 mean the edges match).",
        args: &[
            arg!(NODE, "node", true, "node id or name"),
            arg!("output", "string", false, "which output of a multi-output node"),
            arg!("size", "integer", false, "pixels on the longest side (16..2048, default 256)"),
            arg!("channel", "string", false, "rgb (default), r, g, b, alpha or luma"),
            arg!("normalize", "boolean", false, "stretch the values to 0..1 (for height maps)"),
            arg!("tile", "integer", false, "show the image repeated this many times per side (1..4) to judge seams"),
            arg!("image", "boolean", false, "set false to get only the statistics"),
        ],
        kind: Kind::Query,
        run: c_render_node,
    },
    Command { id: "render.outputs", doc: "A contact sheet of every material output (base colour, normal, roughness ...), labelled in the data.", args: &[arg!("size", "integer", false, "cell size in pixels (default 192)"), arg!("columns", "integer", false, "columns (default 3)")], kind: Kind::Query, run: c_render_outputs },
    Command { id: "render.sheet", doc: "A contact sheet of several nodes, to compare steps of the graph.", args: &[arg!("nodes", "nodes", true, "node ids or names"), arg!("size", "integer", false, "cell size in pixels (default 192)"), arg!("columns", "integer", false, "columns (default 3)")], kind: Kind::Query, run: c_render_sheet },
    Command {
        id: "render.3d",
        doc: "Render the material on a 3D object (software renderer) and return the picture.",
        args: &[
            arg!("size", "integer", false, "picture width and height (16..1024, default 512)"),
            arg!("shape", "string", false, "sphere, cube, plane, ... (default sphere)"),
            arg!("scene", "string", false, "single, floor, lookdev or pedestal"),
            arg!("channel", "string", false, "lit (default), base, roughness, metallic, normal, ao, height, emissive"),
            arg!("camera", "string", false, "three_quarter, front, top, close_up ..."),
            arg!("env", "string", false, "environment preset"),
            arg!("mode", "string", false, "raster (fast, default) or pt (path traced, slow)"),
            arg!("tex", "integer", false, "resolution the material is computed at (default 512)"),
        ],
        kind: Kind::Query,
        run: c_render_3d,
    },
    Command { id: "export", doc: "Write the material's output maps to a folder as image files.", args: &[arg!("dir", "string", true, "folder"), arg!("size", "integer", false, "resolution (default: the graph's size)"), arg!("format", "string", false, "png (default), jpg, tiff, exr, hdr"), arg!("depth", "integer", false, "8, 16 or 32")], kind: Kind::Query, run: c_export },
    // ---- history
    Command { id: "edit.undo", doc: "Undo the last change.", args: &[], kind: Kind::Special, run: c_noop },
    Command { id: "edit.redo", doc: "Redo the change that was undone.", args: &[], kind: Kind::Special, run: c_noop },
    Command { id: "edit.history", doc: "How many undo and redo steps there are, and the last commands.", args: &[], kind: Kind::Query, run: c_history },
    Command { id: "batch", doc: "Run several commands in order as ONE undoable step. All or nothing: if one fails, none of them is applied. Use node names (the `name` of node.add) to refer to nodes created earlier in the batch.", args: &[arg!("steps", "array", true, "list of {command, params}")], kind: Kind::Special, run: c_noop },
];

pub fn find(id: &str) -> Option<&'static Command> {
    COMMANDS.iter().find(|c| c.id == id)
}

impl Command {
    /// The JSON Schema (draft 2020-12) of the command's parameters, for MCP.
    pub fn schema(&self) -> Json {
        let mut props = Json::obj();
        let mut req: Vec<Json> = Vec::new();
        for a in self.args {
            let mut p = match a.ty {
                "node" => Json::obj().with("type", Json::Arr(vec!["integer".into(), "string".into()])),
                "nodes" => Json::obj().with("type", "array").with("items", Json::obj().with("type", Json::Arr(vec!["integer".into(), "string".into()]))),
                "array" => Json::obj().with("type", "array").with("items", Json::obj()),
                t => Json::obj().with("type", t),
            };
            p.set("description", a.doc);
            props.set(a.name, p);
            if a.req {
                req.push(a.name.into());
            }
        }
        let mut s = Json::obj().with("type", "object").with("properties", props);
        if !req.is_empty() {
            s.set("required", Json::Arr(req));
        }
        s
    }

    /// Name used for the MCP tool: the id with `.` replaced by `_`.
    pub fn tool_name(&self) -> String {
        self.id.replace('.', "_")
    }
}

/// Command for an MCP tool name (`node_add`) or a command id (`node.add`).
pub fn find_tool(name: &str) -> Option<&'static Command> {
    COMMANDS.iter().find(|c| c.id == name || c.tool_name() == name)
}

// ------------------------------------------------------------------------------------------ the session

pub struct Session {
    pub graph: Graph,
    pub path: Option<PathBuf>,
    pub dirty: bool,
    undo: Vec<Graph>,
    redo: Vec<Graph>,
    aliases: BTreeMap<String, u32>,
    log: Vec<String>,
    cache: Arc<NodeCache>,
    /// when set, file commands may only touch paths inside this folder
    pub root: Option<PathBuf>,
    changed: bool,
}

impl Default for Session {
    fn default() -> Self {
        Session::new()
    }
}

fn panic_text(p: Box<dyn std::any::Any + Send>) -> String {
    p.downcast_ref::<&str>().map(|s| s.to_string()).or_else(|| p.downcast_ref::<String>().cloned()).unwrap_or_else(|| "unknown panic".into())
}

impl Session {
    pub fn new() -> Session {
        Session::from_graph(Graph::default(), None)
    }

    pub fn from_graph(graph: Graph, path: Option<PathBuf>) -> Session {
        Session { graph, path, dirty: false, undo: Vec::new(), redo: Vec::new(), aliases: BTreeMap::new(), log: Vec::new(), cache: Arc::new(NodeCache::with_default_budget()), root: None, changed: false }
    }

    /// A session over a graph that lives somewhere else (the open editor): `aliases` are the node names handed
    /// out so far and `cache` is shared between calls so repeated renders reuse earlier results. The editor keeps
    /// the real undo history; check [`Session::take_changed`] to see whether the graph must be taken back.
    pub fn from_parts(graph: Graph, path: Option<PathBuf>, dirty: bool, aliases: BTreeMap<String, u32>, cache: Arc<NodeCache>) -> Session {
        let mut s = Session::from_graph(graph, path);
        s.dirty = dirty;
        s.aliases = aliases;
        s.cache = cache;
        s
    }

    pub fn aliases(&self) -> &BTreeMap<String, u32> {
        &self.aliases
    }

    pub fn into_graph(self) -> Graph {
        self.graph
    }

    /// True once if an edit changed the graph since the last call.
    pub fn take_changed(&mut self) -> bool {
        std::mem::take(&mut self.changed)
    }

    pub fn can_undo(&self) -> bool {
        !self.undo.is_empty()
    }

    pub fn can_redo(&self) -> bool {
        !self.redo.is_empty()
    }

    /// Run one command. Never panics; an edit that fails leaves the graph unchanged.
    pub fn execute(&mut self, id: &str, params: &Json) -> Result<Outcome, String> {
        let cmd = find_tool(id).ok_or_else(|| format!("unknown command '{id}' (command.list shows them all)"))?;
        let empty = Json::obj();
        let params = match params {
            Json::Null => &empty,
            Json::Obj(_) => params,
            _ => return Err("the parameters must be an object".into()),
        };
        check_args(cmd, params)?;
        let r = match cmd.id {
            "edit.undo" => self.do_undo(),
            "edit.redo" => self.do_redo(),
            "batch" => self.do_batch(params),
            _ => self.run_one(cmd, params, true),
        };
        if r.is_ok() && cmd.kind != Kind::Query {
            self.log.push(cmd.id.to_string());
            if self.log.len() > 50 {
                self.log.remove(0);
            }
        }
        r
    }

    fn run_one(&mut self, cmd: &'static Command, params: &Json, record: bool) -> Result<Outcome, String> {
        let before = (cmd.kind == Kind::Edit).then(|| self.graph.clone());
        let aliases = self.aliases.clone();
        let run = cmd.run;
        let res = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| run(self, params)));
        let res = match res {
            Ok(r) => r,
            Err(p) => Err(format!("internal error in {}: {}", cmd.id, panic_text(p))),
        };
        match (res, before) {
            (Ok(o), Some(b)) => {
                if record {
                    self.push_undo(b);
                }
                self.dirty = true;
                self.changed = true;
                Ok(o)
            }
            (Ok(o), None) => Ok(o),
            (Err(e), Some(b)) => {
                self.graph = b;
                self.aliases = aliases;
                Err(e)
            }
            (Err(e), None) => Err(e),
        }
    }

    fn push_undo(&mut self, g: Graph) {
        self.undo.push(g);
        if self.undo.len() > MAX_UNDO {
            self.undo.remove(0);
        }
        self.redo.clear();
    }

    fn do_undo(&mut self) -> Result<Outcome, String> {
        let prev = self.undo.pop().ok_or("nothing to undo")?;
        self.redo.push(std::mem::replace(&mut self.graph, prev));
        self.dirty = true;
        self.changed = true;
        Outcome::data(Json::obj().with("undone", true).with("undo_steps", self.undo.len()).with("redo_steps", self.redo.len()))
    }

    fn do_redo(&mut self) -> Result<Outcome, String> {
        let next = self.redo.pop().ok_or("nothing to redo")?;
        self.undo.push(std::mem::replace(&mut self.graph, next));
        self.dirty = true;
        self.changed = true;
        Outcome::data(Json::obj().with("redone", true).with("undo_steps", self.undo.len()).with("redo_steps", self.redo.len()))
    }

    fn do_batch(&mut self, p: &Json) -> Result<Outcome, String> {
        let steps = p.get("steps").and_then(Json::as_arr).ok_or("steps must be a list")?;
        if steps.len() > MAX_BATCH {
            return Err(format!("a batch can hold at most {MAX_BATCH} commands"));
        }
        let before = self.graph.clone();
        let aliases = self.aliases.clone();
        let mut results: Vec<Json> = Vec::new();
        let mut edits = 0;
        for (i, s) in steps.iter().enumerate() {
            let step = |m: String| format!("step {} ({}): {m}", i + 1, s.get("command").and_then(Json::as_str).unwrap_or("?"));
            let name = match s.get("command").or_else(|| s.get("id")).and_then(Json::as_str) {
                Some(n) => n,
                None => {
                    self.graph = before;
                    self.aliases = aliases;
                    return Err(step("every step needs a \"command\"".into()));
                }
            };
            let cmd = match find_tool(name) {
                Some(c) if !matches!(c.id, "batch" | "edit.undo" | "edit.redo") => c,
                _ => {
                    self.graph = before;
                    self.aliases = aliases;
                    return Err(step("not a command that can be used in a batch".into()));
                }
            };
            let empty = Json::obj();
            let sp = s.get("params").filter(|v| !v.is_null()).unwrap_or(&empty);
            let r = check_args(cmd, sp).and_then(|_| self.run_one(cmd, sp, false));
            match r {
                Ok(o) => {
                    if cmd.kind == Kind::Edit {
                        edits += 1;
                    }
                    results.push(o.data);
                }
                Err(e) => {
                    self.graph = before;
                    self.aliases = aliases;
                    return Err(step(e));
                }
            }
        }
        if edits > 0 {
            self.push_undo(before);
            self.dirty = true;
            self.changed = true;
        }
        Outcome::data(Json::obj().with("steps", steps.len()).with("results", Json::Arr(results)))
    }

    // ---------------------------------------------------------------- helpers

    fn node_ref(&self, v: &Json) -> Result<u32, String> {
        let id = match v {
            Json::Num(_) => v.as_u32().ok_or("a node id is a whole number")?,
            Json::Str(s) => {
                let t = s.trim().trim_start_matches('@');
                match t.parse::<u32>() {
                    Ok(n) => n,
                    Err(_) => *self.aliases.get(t).ok_or_else(|| format!("no node named '{t}'"))?,
                }
            }
            _ => return Err("a node is an id (number) or a name".into()),
        };
        if self.graph.nodes.contains_key(&id) {
            Ok(id)
        } else {
            Err(format!("there is no node {id}"))
        }
    }

    fn arg_node(&self, p: &Json, key: &str) -> Result<u32, String> {
        self.node_ref(p.get(key).ok_or_else(|| format!("missing '{key}'"))?).map_err(|e| format!("{key}: {e}"))
    }

    fn nodes_arg(&self, p: &Json, key: &str) -> Result<Vec<u32>, String> {
        let a = p.get(key).ok_or_else(|| format!("missing '{key}'"))?;
        match a {
            Json::Arr(v) => v.iter().map(|x| self.node_ref(x)).collect::<Result<_, _>>().map_err(|e| format!("{key}: {e}")),
            one => Ok(vec![self.node_ref(one).map_err(|e| format!("{key}: {e}"))?]),
        }
    }

    fn alias_of(&self, id: u32) -> Option<&str> {
        self.aliases.iter().find(|(_, v)| **v == id).map(|(k, _)| k.as_str())
    }

    fn resolve_path(&self, p: &str) -> Result<PathBuf, String> {
        match &self.root {
            Some(root) => confine(root, p),
            None => Ok(PathBuf::from(p)),
        }
    }

    fn eval(&self, id: u32, size: usize, out: Option<&str>) -> Result<Tex, String> {
        let mut g = &self.graph;
        let tmp;
        let mut target = id;
        if let Some(o) = out {
            let src = self.graph.nodes.get(&id).ok_or("no such node")?;
            let outs = graph::node_outputs(&src.kind);
            if !outs.contains(&o) {
                return Err(if outs.is_empty() { format!("{} has a single output", src.kind) } else { format!("{} has no output '{o}' (outputs: {})", src.kind, outs.join(", ")) });
            }
            let mut c = self.graph.clone();
            let nid = c.nodes.keys().max().copied().unwrap_or(0) + 1;
            let mut n = Node { kind: "dot".into(), ..Default::default() };
            n.ins.insert("in".into(), id);
            n.in_out.insert("in".into(), o.to_string());
            c.nodes.insert(nid, n);
            tmp = c;
            g = &tmp;
            target = nid;
        }
        g.check_cycles().map_err(|e| e.to_string())?;
        let r = g.evaluate_all_cached(size, &[target], 0, &self.cache, Some(target), false, &mut |_| {});
        if let Some(t) = r.kept.get(&target) {
            return Ok((**t).clone());
        }
        let mut errs: Vec<(&u32, &String)> = r.errors.iter().collect();
        errs.sort();
        match errs.first() {
            Some((n, e)) => Err(format!("node {n} ({}): {e}", g.nodes.get(n).map(|x| x.kind.as_str()).unwrap_or("?"))),
            None => Err("the node produced no image".into()),
        }
    }
}

/// `p` as a path inside `root` (relative paths are taken from `root`). `..` and paths outside are refused.
pub fn confine(root: &Path, p: &str) -> Result<PathBuf, String> {
    let path = PathBuf::from(p);
    let full = if path.is_absolute() { path } else { root.join(path) };
    if full.components().any(|c| matches!(c, std::path::Component::ParentDir)) {
        return Err(format!("{p}: paths may not contain '..' (files are limited to {})", root.display()));
    }
    if !full.starts_with(root) {
        return Err(format!("{p}: outside the allowed folder {}", root.display()));
    }
    Ok(full)
}

fn check_args(cmd: &Command, p: &Json) -> Result<(), String> {
    for a in cmd.args {
        match p.get(a.name) {
            None | Some(Json::Null) if a.req => return Err(format!("{}: missing required parameter '{}' ({})", cmd.id, a.name, a.doc)),
            None | Some(Json::Null) => {}
            Some(v) => {
                let ok = match a.ty {
                    "string" => matches!(v, Json::Str(_)),
                    "number" => v.as_f64().is_some(),
                    "integer" => v.as_f64().map(|n| n.fract() == 0.0).unwrap_or(false),
                    "boolean" => matches!(v, Json::Bool(_)),
                    "object" => matches!(v, Json::Obj(_)),
                    "array" => matches!(v, Json::Arr(_)),
                    "node" => matches!(v, Json::Num(_) | Json::Str(_)),
                    "nodes" => matches!(v, Json::Arr(_) | Json::Num(_) | Json::Str(_)),
                    _ => true,
                };
                if !ok {
                    return Err(format!("{}: parameter '{}' must be of type {}", cmd.id, a.name, a.ty));
                }
            }
        }
    }
    if let Json::Obj(o) = p {
        for (k, _) in o {
            if !cmd.args.iter().any(|a| a.name == k) {
                let names: Vec<&str> = cmd.args.iter().map(|a| a.name).collect();
                return Err(format!("{}: unknown parameter '{k}' (parameters: {})", cmd.id, if names.is_empty() { "none".into() } else { names.join(", ") }));
            }
        }
    }
    Ok(())
}

// ------------------------------------------------------------------------------------------ utilities

fn opt_usize(p: &Json, k: &str, lo: usize, hi: usize, def: usize) -> Result<usize, String> {
    match p.get(k).filter(|v| !v.is_null()) {
        None => Ok(def),
        Some(v) => {
            let n = v.as_f64().filter(|n| n.fract() == 0.0).ok_or_else(|| format!("{k} must be a whole number"))?;
            if n < lo as f64 || n > hi as f64 {
                return Err(format!("{k} must be between {lo} and {hi}"));
            }
            Ok(n as usize)
        }
    }
}

fn gerr(e: GError) -> String {
    e.to_string()
}

fn edit_distance(a: &str, b: &str) -> usize {
    let (a, b): (Vec<char>, Vec<char>) = (a.chars().collect(), b.chars().collect());
    let mut prev: Vec<usize> = (0..=b.len()).collect();
    for (i, ca) in a.iter().enumerate() {
        let mut cur = vec![i + 1];
        for (j, cb) in b.iter().enumerate() {
            cur.push((prev[j] + usize::from(ca != cb)).min(prev[j + 1] + 1).min(cur[j] + 1));
        }
        prev = cur;
    }
    prev[b.len()]
}

fn closest(word: &str, options: &[String]) -> Vec<String> {
    let mut v: Vec<(usize, &String)> = options.iter().map(|o| (edit_distance(&word.to_lowercase(), &o.to_lowercase()), o)).collect();
    v.sort();
    v.into_iter().filter(|(d, _)| *d <= 3.max(word.len() / 3)).take(3).map(|(_, o)| o.clone()).collect()
}

fn all_specs(functions: bool) -> Vec<&'static graph::Spec> {
    if functions {
        crate::func::FUNC_SPECS.iter().collect()
    } else {
        graph::SPECS.iter().collect()
    }
}

fn find_spec(kind: &str) -> Result<&'static graph::Spec, String> {
    if let Some(s) = graph::spec(kind) {
        return Ok(s);
    }
    let names: Vec<String> = graph::SPECS.iter().map(|s| s.kind.to_string()).collect();
    let hint = closest(kind, &names);
    Err(if hint.is_empty() { format!("unknown node type '{kind}' (node.types lists them)") } else { format!("unknown node type '{kind}'. Did you mean: {}?", hint.join(", ")) })
}

/// Words of a spec's doc line: the parameter names it mentions.
fn doc_words(doc: &str) -> BTreeSet<String> {
    doc.split(|c: char| !(c.is_ascii_alphanumeric() || c == '_')).filter(|w| !w.is_empty()).map(|w| w.to_string()).collect()
}

fn param_json(pd: &params::PD) -> Json {
    let mut j = Json::obj().with("name", pd.name).with("label", pd.label).with("group", params::param_group("", pd.name).0);
    match pd.k {
        PK::F { def, lo, hi } => {
            j.set("type", "float");
            j.set("default", def);
            j.set("min", lo);
            j.set("max", hi);
        }
        PK::I { def, lo, hi } => {
            j.set("type", "integer");
            j.set("default", def);
            j.set("min", lo);
            j.set("max", hi);
        }
        PK::E { def, opts } => {
            j.set("type", "choice");
            j.set("default", opts.get(def.max(0) as usize).copied().unwrap_or(""));
            j.set("options", Json::Arr(opts.iter().map(|o| Json::from(*o)).collect()));
        }
        PK::B { def } => {
            j.set("type", "boolean");
            j.set("default", def);
        }
    }
    j
}

// ------------------------------------------------------------------------------------------ discovery

fn c_noop(_: &mut Session, _: &Json) -> Result<Outcome, String> {
    Outcome::data(Json::obj())
}

fn c_command_list(_: &mut Session, p: &Json) -> Result<Outcome, String> {
    let f = p.get("filter").and_then(Json::as_str).unwrap_or("").to_lowercase();
    let list: Vec<Json> = COMMANDS
        .iter()
        .filter(|c| f.is_empty() || c.id.contains(&f) || c.doc.to_lowercase().contains(&f))
        .map(|c| {
            let args: Vec<Json> = c.args.iter().map(|a| Json::obj().with("name", a.name).with("type", a.ty).with("required", a.req).with("doc", a.doc)).collect();
            Json::obj().with("id", c.id).with("tool", c.tool_name()).with("kind", match c.kind { Kind::Query => "query", Kind::Edit => "edit", Kind::Special => "special" }).with("doc", c.doc).with("params", Json::Arr(args))
        })
        .collect();
    Outcome::data(Json::obj().with("commands", Json::Arr(list)))
}

fn c_node_types(_: &mut Session, p: &Json) -> Result<Outcome, String> {
    let q = p.get("query").and_then(Json::as_str).unwrap_or("").to_lowercase();
    let cat = p.get("category").and_then(Json::as_str).unwrap_or("").to_lowercase();
    let functions = p.get("functions").and_then(Json::as_bool).unwrap_or(false);
    let words: Vec<&str> = q.split_whitespace().collect();
    let mut cats: BTreeSet<&str> = BTreeSet::new();
    let mut list = Vec::new();
    for s in all_specs(functions) {
        let c = if functions { "Function" } else { graph::category(s.kind) };
        cats.insert(c);
        if !cat.is_empty() && c.to_lowercase() != cat {
            continue;
        }
        let hay = format!("{} {} {} {}", s.kind, graph::label(s.kind), c, s.doc).to_lowercase().replace('_', " ");
        if !words.iter().all(|w| hay.contains(&w.replace('_', " "))) {
            continue;
        }
        let mut j = Json::obj().with("kind", s.kind).with("label", graph::label(s.kind)).with("category", c);
        j.set("inputs", Json::Arr([s.req, s.opt].concat().iter().map(|x| Json::from(*x)).collect::<Vec<_>>()));
        let outs = graph::node_outputs(s.kind);
        if !outs.is_empty() {
            j.set("outputs", Json::Arr(outs.iter().map(|x| Json::from(*x)).collect::<Vec<_>>()));
        }
        j.set("doc", s.doc.split(':').next().unwrap_or(s.doc));
        list.push(j);
    }
    Outcome::data(Json::obj().with("count", list.len()).with("categories", Json::Arr(cats.into_iter().map(Json::from).collect())).with("nodes", Json::Arr(list)))
}

fn c_node_describe(_: &mut Session, p: &Json) -> Result<Outcome, String> {
    let kind = p.get("kind").and_then(Json::as_str).unwrap_or("");
    let s = match find_spec(kind) {
        Ok(s) => s,
        Err(e) => match crate::func::FUNC_SPECS.iter().find(|s| s.kind == kind) {
            Some(s) => s,
            None => return Err(e),
        },
    };
    let pd = params::pdefs(kind);
    let mut j = Json::obj().with("kind", s.kind).with("label", graph::label(s.kind)).with("category", graph::category(s.kind)).with("doc", s.doc);
    j.set("required_inputs", Json::Arr(s.req.iter().map(|x| Json::from(*x)).collect::<Vec<_>>()));
    j.set("optional_inputs", Json::Arr(s.opt.iter().map(|x| Json::from(*x)).collect::<Vec<_>>()));
    let outs = graph::node_outputs(kind);
    j.set("outputs", if outs.is_empty() { Json::Arr(vec!["output".into()]) } else { Json::Arr(outs.iter().map(|x| Json::from(*x)).collect::<Vec<_>>()) });
    j.set("params", Json::Arr(pd.iter().map(param_json).collect()));
    let known: BTreeSet<&str> = pd.iter().map(|d| d.name).collect();
    let others: Vec<Json> = doc_words(s.doc).into_iter().filter(|w| !known.contains(w.as_str()) && w.len() > 1 && !w.chars().all(|c| c.is_ascii_digit())).map(Json::from).collect();
    j.set("other_words_in_doc", Json::Arr(others));
    let dflt = params::new_node_defaults(kind);
    if !dflt.is_empty() {
        let mut d = Json::obj();
        for (k, v) in dflt {
            d.set(k, *v);
        }
        j.set("values_set_on_creation", d);
    }
    Outcome::data(j)
}

fn c_library_search(_: &mut Session, p: &Json) -> Result<Outcome, String> {
    let kind = match p.get("kind").and_then(Json::as_str) {
        Some(k) => Some(crate::library::AssetKind::parse(k).ok_or("kind must be material, generator or filter")?),
        None => None,
    };
    let hits = crate::library::search(p.get("query").and_then(Json::as_str).unwrap_or(""), kind);
    let list: Vec<Json> = hits.iter().map(|a| Json::obj().with("id", a.id).with("name", a.name).with("kind", a.kind.label()).with("category", a.category).with("tags", Json::Arr(a.tags.iter().map(|t| Json::from(*t)).collect::<Vec<_>>())).with("about", a.description)).collect();
    Outcome::data(Json::obj().with("count", list.len()).with("assets", Json::Arr(list)))
}

// ------------------------------------------------------------------------------------------ graph

fn describe_node(s: &Session, id: u32, n: &Node, compact: bool) -> Json {
    let mut j = Json::obj().with("id", id);
    if let Some(a) = s.alias_of(id) {
        j.set("name", a);
    }
    j.set("kind", n.kind.as_str());
    let mut pr = Json::obj();
    for (k, v) in &n.nums {
        if !k.starts_with('_') {
            pr.set(k, *v);
        }
    }
    for (k, v) in &n.strs {
        pr.set(k, v.as_str());
    }
    j.set("params", pr);
    let mut ins = Json::obj();
    for (slot, src) in &n.ins {
        ins.set(
            slot,
            match n.in_out.get(slot) {
                Some(o) => Json::obj().with("node", *src).with("output", o.as_str()),
                None => Json::from(*src),
            },
        );
    }
    j.set("inputs", ins);
    if !compact {
        j.set("x", n.pos[0]);
        j.set("y", n.pos[1]);
    }
    j
}

/// Problems with the graph, as readable lines (more than `Graph::validate`, which stops at the first).
fn problems(g: &Graph) -> Vec<String> {
    let mut v = Vec::new();
    for (id, n) in &g.nodes {
        match graph::spec(&n.kind) {
            None => v.push(format!("node {id}: unknown type '{}'", n.kind)),
            Some(sp) => {
                for r in sp.req {
                    if !n.ins.contains_key(*r) {
                        v.push(format!("node {id} ({}): required input '{r}' is not connected", n.kind));
                    }
                }
                for (slot, src) in &n.ins {
                    if !sp.req.contains(&slot.as_str()) && !sp.opt.contains(&slot.as_str()) {
                        v.push(format!("node {id} ({}): has no input '{slot}' (inputs: {})", n.kind, [sp.req, sp.opt].concat().join(", ")));
                    }
                    if !g.nodes.contains_key(src) {
                        v.push(format!("node {id} ({}): input '{slot}' comes from node {src}, which does not exist", n.kind));
                    }
                }
            }
        }
    }
    if let Err(e) = g.check_cycles() {
        v.push(e.to_string());
    }
    if g.output_nodes().is_empty() {
        v.push("the graph has no outputs (output.add makes a node an output)".into());
    }
    for (id, slot, m) in graph::connection_issues(g) {
        v.push(format!("node {id}: {slot}: {m}"));
    }
    if let Err(e) = g.validate() {
        let t = e.to_string();
        if !v.iter().any(|x| x.contains(&t)) && v.is_empty() {
            v.push(t);
        }
    }
    // nodes whose result nothing uses
    let mut used: BTreeSet<u32> = BTreeSet::new();
    let mut stack: Vec<u32> = g.output_nodes().iter().map(|o| o.2).collect();
    while let Some(i) = stack.pop() {
        if used.insert(i) {
            if let Some(n) = g.nodes.get(&i) {
                stack.extend(n.ins.values().copied());
            }
        }
    }
    let unused: Vec<String> = g.nodes.keys().filter(|i| !used.contains(i)).map(|i| i.to_string()).collect();
    if !unused.is_empty() && !g.output_nodes().is_empty() {
        v.push(format!("nodes {} do not feed any output", unused.join(", ")));
    }
    v
}

fn c_graph_inspect(s: &mut Session, p: &Json) -> Result<Outcome, String> {
    let compact = p.get("compact").and_then(Json::as_bool).unwrap_or(false);
    let g = &s.graph;
    let nodes: Vec<Json> = g.nodes.iter().map(|(id, n)| describe_node(s, *id, n, compact)).collect();
    let outs: Vec<Json> = g.output_nodes().into_iter().map(|(usage, ident, id)| Json::obj().with("usage", usage).with("name", ident).with("node", id)).collect();
    let exposed: Vec<Json> = g.exposed.iter().map(|e| Json::obj().with("label", e.label.as_str()).with("node", e.node).with("param", e.param.as_str()).with("min", e.min).with("max", e.max)).collect();
    let mut j = Json::obj().with("size", g.size).with("height", if g.is_square() { g.size } else { g.height });
    j.set("seed_offset", g.seed_offset);
    j.set("precision", g.default_format());
    j.set("file", s.path.as_ref().map(|p| p.display().to_string()));
    j.set("unsaved_changes", s.dirty);
    j.set("nodes", Json::Arr(nodes));
    j.set("outputs", Json::Arr(outs));
    if !exposed.is_empty() {
        j.set("exposed", Json::Arr(exposed));
    }
    if !g.subgraphs.is_empty() {
        j.set("subgraphs", Json::Arr(g.subgraphs.keys().map(|k| Json::from(k.as_str())).collect::<Vec<_>>()));
    }
    j.set("problems", Json::Arr(problems(g).into_iter().map(Json::from).collect::<Vec<_>>()));
    Outcome::data(j)
}

fn c_graph_validate(s: &mut Session, _: &Json) -> Result<Outcome, String> {
    let pr = problems(&s.graph);
    Outcome::data(Json::obj().with("ok", pr.is_empty()).with("problems", Json::Arr(pr.into_iter().map(Json::from).collect::<Vec<_>>())))
}

fn c_graph_text(s: &mut Session, _: &Json) -> Result<Outcome, String> {
    Outcome::data(Json::obj().with("text", s.graph.to_text()))
}

fn check_size(v: usize) -> Result<(), String> {
    if (16..=8192).contains(&v) {
        Ok(())
    } else {
        Err("the size must be between 16 and 8192".into())
    }
}

fn c_graph_new(s: &mut Session, p: &Json) -> Result<Outcome, String> {
    let size = opt_usize(p, "size", 16, 8192, 512)?;
    let height = opt_usize(p, "height", 16, 8192, size)?;
    let mut g = Graph::default();
    g.size = size;
    g.height = if height == size { 0 } else { height };
    s.graph = g;
    s.path = None;
    s.aliases.clear();
    Outcome::data(Json::obj().with("size", size).with("height", height))
}

fn load_graph(path: &Path) -> Result<(Graph, Vec<String>), String> {
    if let Some(dir) = path.parent().filter(|p| !p.as_os_str().is_empty()) {
        crate::resources::set_search_dirs(vec![dir.to_path_buf(), dir.join("resources")]);
    }
    let (mut g, issues) = Graph::load_file(path).map_err(|e| format!("{}: {e}", path.display()))?;
    g.migrate_outputs();
    Ok((g, issues))
}

fn c_graph_open(s: &mut Session, p: &Json) -> Result<Outcome, String> {
    let path = s.resolve_path(p.get("path").and_then(Json::as_str).unwrap_or(""))?;
    let (g, issues) = load_graph(&path)?;
    let (n, o) = (g.nodes.len(), g.output_nodes().len());
    s.graph = g;
    s.path = Some(path.clone());
    s.aliases.clear();
    Outcome::data(Json::obj().with("file", path.display().to_string()).with("nodes", n).with("outputs", o).with("warnings", Json::Arr(issues.into_iter().map(Json::from).collect::<Vec<_>>())))
}

fn c_graph_load_text(s: &mut Session, p: &Json) -> Result<Outcome, String> {
    let text = p.get("text").and_then(Json::as_str).unwrap_or("");
    let mut g = Graph::parse_unchecked(text).map_err(gerr)?;
    g.migrate_outputs();
    let n = g.nodes.len();
    s.graph = g;
    s.aliases.clear();
    Outcome::data(Json::obj().with("nodes", n))
}

fn c_graph_save(s: &mut Session, p: &Json) -> Result<Outcome, String> {
    let path = match p.get("path").and_then(Json::as_str) {
        Some(x) => s.resolve_path(x)?,
        None => s.path.clone().ok_or("this graph has no file yet: give a path")?,
    };
    s.graph.validate().map_err(|e| format!("not saved, the graph is not valid yet: {e}"))?;
    crate::safeio::write_file(&path, s.graph.to_text().as_bytes()).map_err(|e| format!("{}: {e}", path.display()))?;
    s.path = Some(path.clone());
    s.dirty = false;
    Outcome::data(Json::obj().with("saved", path.display().to_string()))
}

fn c_graph_set(s: &mut Session, p: &Json) -> Result<Outcome, String> {
    if p.as_obj().map(|o| o.is_empty()).unwrap_or(true) {
        return Err("give at least one of size, height, seed_offset, precision".into());
    }
    if p.get("size").is_some() {
        let v = opt_usize(p, "size", 16, 8192, 512)?;
        check_size(v)?;
        s.graph.size = v;
    }
    if p.get("height").is_some() {
        let h = opt_usize(p, "height", 0, 8192, 0)?;
        if h != 0 {
            check_size(h)?;
        }
        s.graph.height = if h == s.graph.size { 0 } else { h };
    }
    if p.get("seed_offset").is_some() {
        s.graph.seed_offset = opt_usize(p, "seed_offset", 0, u32::MAX as usize, 0)? as u32;
    }
    if p.get("precision").is_some() {
        let v = opt_usize(p, "precision", 0, 3, 0)?;
        s.graph.meta.insert("ofmt".into(), v.to_string());
    }
    Outcome::data(Json::obj().with("size", s.graph.size).with("height", if s.graph.is_square() { s.graph.size } else { s.graph.height }))
}

fn c_graph_layout(s: &mut Session, _: &Json) -> Result<Outcome, String> {
    s.graph.check_cycles().map_err(gerr)?;
    let g = &mut s.graph;
    // column = longest path from a node without inputs
    let ids: Vec<u32> = g.nodes.keys().copied().collect();
    let mut depth: BTreeMap<u32, usize> = BTreeMap::new();
    for _ in 0..ids.len().max(1) {
        let mut changed = false;
        for id in &ids {
            let d = g.nodes[id].ins.values().filter_map(|i| depth.get(i)).map(|d| d + 1).max().unwrap_or(0);
            if depth.get(id) != Some(&d) {
                depth.insert(*id, d);
                changed = true;
            }
        }
        if !changed {
            break;
        }
    }
    let mut cols: BTreeMap<usize, Vec<u32>> = BTreeMap::new();
    for id in &ids {
        cols.entry(depth[id]).or_default().push(*id);
    }
    // order each column by the average row of its inputs so links stay short
    let mut row_of: BTreeMap<u32, f32> = BTreeMap::new();
    for (c, list) in cols.iter_mut() {
        if *c > 0 {
            list.sort_by(|a, b| {
                let avg = |i: &u32| {
                    let r: Vec<f32> = g.nodes[i].ins.values().filter_map(|x| row_of.get(x)).copied().collect();
                    if r.is_empty() {
                        f32::MAX
                    } else {
                        r.iter().sum::<f32>() / r.len() as f32
                    }
                };
                avg(a).partial_cmp(&avg(b)).unwrap_or(std::cmp::Ordering::Equal)
            });
        }
        for (r, id) in list.iter().enumerate() {
            row_of.insert(*id, r as f32);
        }
    }
    for (c, list) in &cols {
        for (r, id) in list.iter().enumerate() {
            if let Some(n) = g.nodes.get_mut(id) {
                n.pos = [*c as f32 * 170.0, r as f32 * 130.0];
            }
        }
    }
    Outcome::data(Json::obj().with("columns", cols.len()).with("nodes", ids.len()))
}

fn c_library_load(s: &mut Session, p: &Json) -> Result<Outcome, String> {
    let id = p.get("id").and_then(Json::as_str).unwrap_or("");
    let a = crate::library::find(id).ok_or_else(|| format!("no library entry '{id}' (library.search finds them)"))?;
    let g = a.graph().map_err(gerr)?;
    let n = g.nodes.len();
    s.graph = g;
    s.path = None;
    s.aliases.clear();
    Outcome::data(Json::obj().with("loaded", a.name).with("nodes", n))
}

// ------------------------------------------------------------------------------------------ nodes

fn next_id(g: &Graph) -> u32 {
    g.nodes.keys().max().copied().unwrap_or(0) + 1
}

/// A free spot to the right of the node's inputs (or below everything when it has none).
fn free_pos(g: &Graph, inputs: &BTreeMap<String, u32>) -> [f32; 2] {
    let srcs: Vec<[f32; 2]> = inputs.values().filter_map(|i| g.nodes.get(i)).map(|n| n.pos).collect();
    let mut pos = if srcs.is_empty() {
        let bottom = g.nodes.values().map(|n| n.pos[1]).fold(f32::MIN, f32::max);
        [0.0, if g.nodes.is_empty() { 0.0 } else { bottom + 130.0 }]
    } else {
        [srcs.iter().map(|p| p[0]).fold(f32::MIN, f32::max) + 170.0, srcs.iter().map(|p| p[1]).sum::<f32>() / srcs.len() as f32]
    };
    for _ in 0..200 {
        if g.nodes.values().any(|n| (n.pos[0] - pos[0]).abs() < 150.0 && (n.pos[1] - pos[1]).abs() < 120.0) {
            pos[1] += 130.0;
        } else {
            break;
        }
    }
    pos
}

fn parse_link(s: &Session, v: &Json) -> Result<(u32, Option<String>), String> {
    match v {
        Json::Obj(_) => {
            let id = s.node_ref(v.get("node").ok_or("a link object needs \"node\"")?)?;
            Ok((id, v.get("output").and_then(Json::as_str).map(str::to_string)))
        }
        other => Ok((s.node_ref(other)?, None)),
    }
}

/// Is `slot` of `kind` a valid input slot? Lists the valid ones in the error.
fn check_slot(kind: &str, slot: &str) -> Result<(), String> {
    let sp = find_spec(kind)?;
    if sp.req.contains(&slot) || sp.opt.contains(&slot) {
        return Ok(());
    }
    let all = [sp.req, sp.opt].concat();
    if all.is_empty() {
        Err(format!("{kind} has no inputs"))
    } else {
        Err(format!("{kind} has no input '{slot}' (inputs: {})", all.join(", ")))
    }
}

fn set_link(g: &mut Graph, to: u32, slot: &str, from: u32, out: Option<String>) -> Result<(), String> {
    let kind = g.nodes.get(&to).map(|n| n.kind.clone()).ok_or("no such target node")?;
    check_slot(&kind, slot)?;
    if from == to {
        return Err("a node cannot feed itself".into());
    }
    if let Some(o) = &out {
        let sk = g.nodes.get(&from).map(|n| n.kind.clone()).ok_or("no such source node")?;
        let outs = graph::node_outputs(&sk);
        if !outs.contains(&o.as_str()) {
            return Err(if outs.is_empty() { format!("{sk} has a single output; leave 'output' out") } else { format!("{sk} has no output '{o}' (outputs: {})", outs.join(", ")) });
        }
    }
    let n = g.nodes.get_mut(&to).ok_or("no such target node")?;
    let old = (n.ins.get(slot).copied(), n.in_out.get(slot).cloned());
    n.ins.insert(slot.to_string(), from);
    match out {
        Some(o) => {
            n.in_out.insert(slot.to_string(), o);
        }
        None => {
            n.in_out.remove(slot);
        }
    }
    let undo = |g: &mut Graph| {
        if let Some(n) = g.nodes.get_mut(&to) {
            match &old.0 {
                Some(o) => {
                    n.ins.insert(slot.to_string(), *o);
                }
                None => {
                    n.ins.remove(slot);
                }
            }
            match &old.1 {
                Some(o) => {
                    n.in_out.insert(slot.to_string(), o.clone());
                }
                None => {
                    n.in_out.remove(slot);
                }
            }
        }
    };
    if let Err(e) = g.check_cycles() {
        undo(g);
        return Err(format!("that connection would make a loop ({e})"));
    }
    if let Some((_, _, m)) = graph::connection_issues(g).into_iter().find(|(i, sl, _)| *i == to && sl == slot) {
        undo(g);
        return Err(format!("connection rejected: {m}"));
    }
    Ok(())
}

/// Apply `values` to a node with range checks. Returns warnings (clamped values).
fn apply_params(node: &mut Node, kind: &str, values: &[(String, Json)], allow_unknown: bool) -> Result<Vec<String>, String> {
    let pd = params::pdefs(kind);
    let sp = graph::spec(kind);
    let mut known: BTreeSet<String> = pd.iter().map(|d| d.name.to_string()).collect();
    if let Some(sp) = sp {
        known.extend(doc_words(sp.doc));
    }
    known.extend(params::new_node_defaults(kind).iter().map(|(k, _)| k.to_string()));
    known.extend(node.nums.keys().cloned());
    known.extend(node.strs.keys().cloned());
    let mut warn = Vec::new();
    for (k, v) in values {
        let def = pd.iter().find(|d| d.name == k);
        if def.is_none() && !known.contains(k) && !allow_unknown {
            let names: Vec<String> = known.iter().cloned().collect();
            let hint = closest(k, &names);
            return Err(format!("{kind} has no parameter '{k}'.{} (node.describe shows them all; allow_unknown=true overrides)", if hint.is_empty() { String::new() } else { format!(" Did you mean: {}?", hint.join(", ")) }));
        }
        match v {
            Json::Null => {
                node.nums.remove(k);
                node.strs.remove(k);
            }
            Json::Bool(b) => {
                node.strs.remove(k);
                node.nums.insert(k.clone(), f32::from(u8::from(*b)));
            }
            Json::Str(t) => match def.map(|d| d.k) {
                Some(PK::E { opts, .. }) => {
                    let lower = t.trim().to_lowercase();
                    let idx = opts.iter().position(|o| o.to_lowercase() == lower).or_else(|| {
                        let m: Vec<usize> = opts.iter().enumerate().filter(|(_, o)| o.to_lowercase().starts_with(&lower)).map(|(i, _)| i).collect();
                        (m.len() == 1).then(|| m[0])
                    });
                    match idx {
                        Some(i) => {
                            node.strs.remove(k);
                            node.nums.insert(k.clone(), i as f32);
                        }
                        None => return Err(format!("{k}: '{t}' is not one of: {}", opts.join(", "))),
                    }
                }
                Some(PK::F { .. } | PK::I { .. } | PK::B { .. }) => return Err(format!("{k} is a number; got text")),
                None => {
                    if t.contains('"') || t.contains('#') || t.contains('\n') {
                        return Err(format!("{k}: text may not contain quotes, '#' or line breaks"));
                    }
                    node.nums.remove(k);
                    node.strs.insert(k.clone(), t.clone());
                }
            },
            Json::Num(_) => {
                let mut x = v.as_f32().ok_or_else(|| format!("{k}: not a finite number"))?;
                match def.map(|d| d.k) {
                    Some(PK::F { lo, hi, .. }) => {
                        let c = x.clamp(lo, hi);
                        if c != x {
                            warn.push(format!("{k}: {x} is outside {lo}..{hi}, used {c}"));
                        }
                        x = c;
                    }
                    Some(PK::I { lo, hi, .. }) => {
                        let c = x.round().clamp(lo as f32, hi as f32);
                        if c != x {
                            warn.push(format!("{k}: {x} is not a whole number inside {lo}..{hi}, used {c}"));
                        }
                        x = c;
                    }
                    Some(PK::E { opts, .. }) => {
                        let c = x.round().clamp(0.0, opts.len().saturating_sub(1) as f32);
                        if c != x {
                            warn.push(format!("{k}: choice {x} does not exist (0..{}), used {c}", opts.len().saturating_sub(1)));
                        }
                        x = c;
                    }
                    Some(PK::B { .. }) => x = f32::from(u8::from(x != 0.0)),
                    None => {}
                }
                node.strs.remove(k);
                node.nums.insert(k.clone(), x);
            }
            _ => return Err(format!("{k}: a parameter is a number, true/false or text")),
        }
    }
    Ok(warn)
}

fn c_node_add(s: &mut Session, p: &Json) -> Result<Outcome, String> {
    let kind = p.get("kind").and_then(Json::as_str).unwrap_or("").to_string();
    find_spec(&kind)?;
    if kind == "output" {
        return Err("use output.add to make a node an output".into());
    }
    let name = p.get("name").and_then(Json::as_str).map(str::to_string);
    if let Some(n) = &name {
        if n.is_empty() || n.parse::<u32>().is_ok() || n.starts_with('@') || n.len() > 64 {
            return Err("a node name must be text that is not a number (at most 64 characters)".into());
        }
        if let Some(old) = s.aliases.get(n) {
            if s.graph.nodes.contains_key(old) {
                return Err(format!("the name '{n}' is already used by node {old}"));
            }
        }
    }
    let id = next_id(&s.graph);
    let mut node = Node { kind: kind.clone(), ..Default::default() };
    match kind.as_str() {
        "pixel_processor" => {
            node.strs.insert("expr".into(), "u".into());
        }
        "colorize" => {
            for (i, c) in [(0, 0.0f32), (1, 1.0)] {
                node.nums.insert(format!("p{i}"), c);
                for ch in ["r", "g", "b"] {
                    node.nums.insert(format!("{ch}{i}"), c);
                }
            }
        }
        _ => {}
    }
    for (k, v) in params::new_node_defaults(&kind) {
        node.nums.insert((*k).to_string(), *v);
    }
    let mut warnings = Vec::new();
    if let Some(Json::Obj(o)) = p.get("params") {
        warnings = apply_params(&mut node, &kind, o, false)?;
    }
    let mut links: Vec<(String, u32, Option<String>)> = Vec::new();
    if let Some(Json::Obj(o)) = p.get("inputs") {
        for (slot, v) in o {
            let (src, out) = parse_link(s, v).map_err(|e| format!("inputs.{slot}: {e}"))?;
            links.push((slot.clone(), src, out));
        }
    } else if p.get("inputs").map(|v| !v.is_null()).unwrap_or(false) {
        return Err("inputs must be an object {slot: node}".into());
    }
    let map: BTreeMap<String, u32> = links.iter().map(|(k, v, _)| (k.clone(), *v)).collect();
    node.pos = match (p.get("x").and_then(Json::as_f32), p.get("y").and_then(Json::as_f32)) {
        (Some(x), Some(y)) => [x, y],
        _ => free_pos(&s.graph, &map),
    };
    s.graph.nodes.insert(id, node);
    for (slot, src, out) in links {
        set_link(&mut s.graph, id, &slot, src, out).map_err(|e| format!("inputs.{slot}: {e}"))?;
    }
    if let Some(n) = name {
        s.aliases.insert(n, id);
    }
    let mut j = Json::obj().with("id", id).with("kind", kind);
    if !warnings.is_empty() {
        j.set("warnings", Json::Arr(warnings.into_iter().map(Json::from).collect::<Vec<_>>()));
    }
    Outcome::data(j)
}

fn c_node_set(s: &mut Session, p: &Json) -> Result<Outcome, String> {
    let id = s.arg_node(p, NODE)?;
    let values = p.get("params").and_then(Json::as_obj).ok_or("params must be an object")?;
    if values.is_empty() {
        return Err("params is empty".into());
    }
    let allow = p.get("allow_unknown").and_then(Json::as_bool).unwrap_or(false);
    let node = s.graph.nodes.get_mut(&id).ok_or("no such node")?;
    let kind = node.kind.clone();
    let warnings = apply_params(node, &kind, values, allow)?;
    let mut j = Json::obj().with("id", id).with("updated", values.len());
    if !warnings.is_empty() {
        j.set("warnings", Json::Arr(warnings.into_iter().map(Json::from).collect::<Vec<_>>()));
    }
    Outcome::data(j)
}

fn c_node_remove(s: &mut Session, p: &Json) -> Result<Outcome, String> {
    let ids = s.nodes_arg(p, "nodes")?;
    let set: BTreeSet<u32> = ids.iter().copied().collect();
    let mut cut = 0;
    for n in s.graph.nodes.values_mut() {
        let dead: Vec<String> = n.ins.iter().filter(|(_, v)| set.contains(v)).map(|(k, _)| k.clone()).collect();
        for k in dead {
            n.ins.remove(&k);
            n.in_out.remove(&k);
            cut += 1;
        }
    }
    for id in &set {
        s.graph.nodes.remove(id);
    }
    s.graph.exposed.retain(|e| !set.contains(&e.node));
    s.graph.outputs.retain(|(_, i)| !set.contains(i));
    s.aliases.retain(|_, v| !set.contains(v));
    Outcome::data(Json::obj().with("removed", set.len()).with("links_removed", cut))
}

fn c_node_connect(s: &mut Session, p: &Json) -> Result<Outcome, String> {
    let from = s.arg_node(p, "from")?;
    let to = s.arg_node(p, "to")?;
    let out = p.get("output").and_then(Json::as_str).map(str::to_string);
    let kind = s.graph.nodes.get(&to).map(|n| n.kind.clone()).ok_or("no such node")?;
    let sp = find_spec(&kind)?;
    let slot = match p.get("slot").and_then(Json::as_str) {
        Some(x) => x.to_string(),
        None => {
            let n = &s.graph.nodes[&to];
            let free = sp.req.iter().chain(sp.opt.iter()).find(|x| !n.ins.contains_key(**x));
            match (free, [sp.req, sp.opt].concat().first()) {
                (Some(f), _) => (*f).to_string(),
                (None, Some(first)) => (*first).to_string(),
                (None, None) => return Err(format!("{kind} has no inputs")),
            }
        }
    };
    set_link(&mut s.graph, to, &slot, from, out)?;
    Outcome::data(Json::obj().with("from", from).with("to", to).with("slot", slot))
}

fn c_node_disconnect(s: &mut Session, p: &Json) -> Result<Outcome, String> {
    let to = s.arg_node(p, "to")?;
    let slot = p.get("slot").and_then(Json::as_str).unwrap_or("");
    let n = s.graph.nodes.get_mut(&to).ok_or("no such node")?;
    if n.ins.remove(slot).is_none() {
        return Err(format!("input '{slot}' of node {to} is not connected"));
    }
    n.in_out.remove(slot);
    Outcome::data(Json::obj().with("to", to).with("slot", slot))
}

fn c_node_move(s: &mut Session, p: &Json) -> Result<Outcome, String> {
    let id = s.arg_node(p, NODE)?;
    let (x, y) = (p.get("x").and_then(Json::as_f32).ok_or("x must be a number")?, p.get("y").and_then(Json::as_f32).ok_or("y must be a number")?);
    if let Some(n) = s.graph.nodes.get_mut(&id) {
        n.pos = [x, y];
    }
    Outcome::data(Json::obj().with("id", id))
}

fn c_output_add(s: &mut Session, p: &Json) -> Result<Outcome, String> {
    let id = s.arg_node(p, NODE)?;
    let usage = p.get("usage").and_then(Json::as_str).unwrap_or("").to_string();
    if !OUTPUT_USAGES.contains(&usage.as_str()) {
        return Err(format!("unknown usage '{usage}' (use one of: {})", OUTPUT_USAGES.join(", ")));
    }
    let out = p.get("output").and_then(Json::as_str).map(str::to_string);
    let ident = p.get("name").and_then(Json::as_str).map(str::to_string);
    if let Some(i) = &ident {
        if i.is_empty() || i.contains(['/', '\\', '.', '"', '#', ' ']) {
            return Err("an output name may not be empty or contain / \\ . \" # or spaces".into());
        }
    }
    if s.graph.nodes.get(&id).map(|n| n.kind == "output").unwrap_or(false) {
        return Err("that node is already an output; give the node that feeds it".into());
    }
    // the same usage twice would clash when exporting
    let dup: Vec<u32> = s.graph.nodes.iter().filter(|(_, n)| n.kind == "output" && n.strs.get("usage").map(|u| u == &usage).unwrap_or(false)).map(|(i, _)| *i).collect();
    for d in &dup {
        s.graph.nodes.remove(d);
    }
    let nid = next_id(&s.graph);
    let src_pos = s.graph.nodes.get(&id).map(|n| n.pos).unwrap_or([0.0, 0.0]);
    let mut node = Node { kind: "output".into(), pos: [src_pos[0] + 170.0, src_pos[1]], ..Default::default() };
    node.strs.insert("usage".into(), usage.clone());
    if let Some(i) = ident {
        node.strs.insert("identifier".into(), i);
    }
    s.graph.nodes.insert(nid, node);
    set_link(&mut s.graph, nid, "in", id, out)?;
    Outcome::data(Json::obj().with("output_node", nid).with("usage", usage).with("replaced", dup.len()))
}

fn c_output_remove(s: &mut Session, p: &Json) -> Result<Outcome, String> {
    let usage = p.get("usage").and_then(Json::as_str).unwrap_or("");
    let ids: Vec<u32> = s.graph.nodes.iter().filter(|(_, n)| n.kind == "output" && n.strs.get("usage").map(|u| u == usage).unwrap_or(usage == "basecolor")).map(|(i, _)| *i).collect();
    if ids.is_empty() {
        return Err(format!("there is no output with usage '{usage}'"));
    }
    for i in &ids {
        s.graph.nodes.remove(i);
    }
    Outcome::data(Json::obj().with("removed", ids.len()))
}

fn c_graph_expose(s: &mut Session, p: &Json) -> Result<Outcome, String> {
    let id = s.arg_node(p, NODE)?;
    let label = p.get("label").and_then(Json::as_str).unwrap_or("").trim().to_string();
    let param = p.get("param").and_then(Json::as_str).unwrap_or("").to_string();
    if label.is_empty() || label.contains(['"', '#']) {
        return Err("the label may not be empty or contain quotes or '#'".into());
    }
    if s.graph.exposed.iter().any(|e| e.label == label) {
        return Err(format!("a parameter named '{label}' is already exposed"));
    }
    let kind = s.graph.nodes[&id].kind.clone();
    let def = params::pdefs(&kind).into_iter().find(|d| d.name == param);
    let (lo, hi) = match def.map(|d| d.k) {
        Some(PK::F { lo, hi, .. }) => (lo, hi),
        Some(PK::I { lo, hi, .. }) => (lo as f32, hi as f32),
        Some(PK::E { opts, .. }) => (0.0, opts.len().saturating_sub(1) as f32),
        Some(PK::B { .. }) => (0.0, 1.0),
        None => {
            if !s.graph.nodes[&id].nums.contains_key(&param) {
                return Err(format!("{kind} has no numeric parameter '{param}' (node.describe lists them)"));
            }
            (0.0, 1.0)
        }
    };
    let min = p.get("min").and_then(Json::as_f32).unwrap_or(lo);
    let max = p.get("max").and_then(Json::as_f32).unwrap_or(hi);
    if min >= max {
        return Err("min must be smaller than max".into());
    }
    s.graph.exposed.push(Exposed { label, node: id, param, min, max });
    Outcome::data(Json::obj().with("exposed", s.graph.exposed.len()))
}

fn c_graph_unexpose(s: &mut Session, p: &Json) -> Result<Outcome, String> {
    let label = p.get("label").and_then(Json::as_str).unwrap_or("");
    let n = s.graph.exposed.len();
    s.graph.exposed.retain(|e| e.label != label);
    if s.graph.exposed.len() == n {
        return Err(format!("no exposed parameter '{label}'"));
    }
    Outcome::data(Json::obj().with("exposed", s.graph.exposed.len()))
}

fn c_comment_add(s: &mut Session, p: &Json) -> Result<Outcome, String> {
    let text = p.get("text").and_then(Json::as_str).unwrap_or("").replace(['#', '\n'], " ");
    let f = |k: &str, d: f32| p.get(k).and_then(Json::as_f32).unwrap_or(d);
    s.graph.comments.push(Comment { text, pos: [f("x", 0.0), f("y", -160.0)], size: [f("width", 300.0).clamp(40.0, 4000.0), f("height", 120.0).clamp(30.0, 4000.0)], color: 0 });
    Outcome::data(Json::obj().with("comments", s.graph.comments.len()))
}

// ------------------------------------------------------------------------------------------ rendering

fn stats_json(t: &Tex) -> Json {
    let n = t.px.len().max(1) as f64;
    let (mut mn, mut mx, mut sum) = ([f32::MAX; 4], [f32::MIN; 4], [0f64; 4]);
    let mut bad = 0usize;
    for p in &t.px {
        for c in 0..4 {
            if !p[c].is_finite() {
                bad += 1;
                continue;
            }
            mn[c] = mn[c].min(p[c]);
            mx[c] = mx[c].max(p[c]);
            sum[c] += p[c] as f64;
        }
    }
    let ch = |a: [f32; 4]| Json::Arr(a.iter().map(|v| Json::from(if v.abs() > 1e30 { 0.0 } else { (*v * 10000.0).round() / 10000.0 })).collect());
    let mean: Vec<Json> = sum.iter().map(|v| Json::from(((v / n) * 10000.0).round() / 10000.0)).collect();
    let flat = (0..3).all(|c| mx[c] - mn[c] < 1e-5);
    let l = |p: &[f32; 4]| 0.2126 * p[0] + 0.7152 * p[1] + 0.0722 * p[2];
    let (w, h) = (t.w, t.h);
    // tiling check: how different the two opposite edges are, compared with how different neighbouring pixels are
    let seam = |horizontal: bool| -> f32 {
        let (mut edge, mut inner, mut cnt) = (0f64, 0f64, 0usize);
        let lines = if horizontal { h } else { w };
        let len = if horizontal { w } else { h };
        if len < 4 {
            return 1.0;
        }
        for i in 0..lines {
            let at = |k: usize| if horizontal { l(&t.px[i * w + k]) } else { l(&t.px[k * w + i]) };
            edge += (at(0) - at(len - 1)).abs() as f64;
            for k in 0..len - 1 {
                inner += (at(k) - at(k + 1)).abs() as f64;
            }
            cnt += 1;
        }
        let inner = inner / (cnt * (len - 1)).max(1) as f64;
        let edge = edge / cnt.max(1) as f64;
        if inner < 1e-6 {
            if edge < 1e-6 {
                1.0
            } else {
                99.0
            }
        } else {
            (edge / inner) as f32
        }
    };
    let r2 = |v: f32| (v * 100.0).round() / 100.0;
    let (sh, sv) = (r2(seam(true)), r2(seam(false)));
    let mut j = Json::obj().with("width", w).with("height", h).with("min", ch(mn)).with("max", ch(mx)).with("mean", Json::Arr(mean));
    j.set("flat", flat);
    j.set("seam_horizontal", sh);
    j.set("seam_vertical", sv);
    j.set("tiles_seamlessly", sh < 2.5 && sv < 2.5);
    if bad > 0 {
        j.set("non_finite_values", bad);
    }
    j
}

fn checker(x: usize, y: usize) -> f32 {
    if (x / 8 + y / 8) % 2 == 0 {
        0.55
    } else {
        0.4
    }
}

/// The picture to show for a texture: a channel, optionally stretched, with transparency drawn over a checkerboard.
fn display(t: &Tex, channel: &str, normalize: bool) -> Result<Tex, String> {
    let mut out = Tex { w: t.w, h: t.h, px: Vec::with_capacity(t.px.len()) };
    let lum = |p: &[f32; 4]| 0.2126 * p[0] + 0.7152 * p[1] + 0.0722 * p[2];
    let pick = |p: &[f32; 4]| -> [f32; 3] {
        match channel {
            "r" => [p[0]; 3],
            "g" => [p[1]; 3],
            "b" => [p[2]; 3],
            "alpha" | "a" => [p[3]; 3],
            "luma" => [lum(p); 3],
            _ => [p[0], p[1], p[2]],
        }
    };
    if !matches!(channel, "rgb" | "r" | "g" | "b" | "alpha" | "a" | "luma") {
        return Err("channel must be rgb, r, g, b, alpha or luma".into());
    }
    let (mut lo, mut hi) = (f32::MAX, f32::MIN);
    if normalize {
        for p in &t.px {
            for v in pick(p) {
                if v.is_finite() {
                    lo = lo.min(v);
                    hi = hi.max(v);
                }
            }
        }
    }
    let scale = if normalize && hi > lo { 1.0 / (hi - lo) } else { 1.0 };
    let base = if normalize && hi > lo { lo } else { 0.0 };
    for (i, p) in t.px.iter().enumerate() {
        let c = pick(p);
        let mut o = [(c[0] - base) * scale, (c[1] - base) * scale, (c[2] - base) * scale];
        let a = if matches!(channel, "alpha" | "a") { 1.0 } else { p[3].clamp(0.0, 1.0) };
        if a < 0.999 {
            let k = checker(i % t.w.max(1), i / t.w.max(1));
            for v in o.iter_mut() {
                *v = *v * a + k * (1.0 - a);
            }
        }
        for v in o.iter_mut() {
            *v = if v.is_finite() { v.clamp(0.0, 1.0) } else { 1.0 };
        }
        out.px.push([o[0], o[1], o[2], 1.0]);
    }
    Ok(out)
}

fn tiled(t: &Tex, n: usize) -> Tex {
    if n <= 1 {
        return t.clone();
    }
    let (w, h) = (t.w * n, t.h * n);
    let mut px = Vec::with_capacity(w * h);
    for y in 0..h {
        for x in 0..w {
            px.push(t.px[(y % t.h) * t.w + (x % t.w)]);
        }
    }
    Tex { w, h, px }
}

fn png_of(t: &Tex) -> Vec<u8> {
    crate::png::encode(t.w, t.h, &t.px, 8)
}

fn fit_cell(t: &Tex, s: usize) -> Tex {
    let mut px = Vec::with_capacity(s * s);
    for y in 0..s {
        for x in 0..s {
            let sx = (x * t.w / s).min(t.w.saturating_sub(1));
            let sy = (y * t.h / s).min(t.h.saturating_sub(1));
            px.push(t.px[sy * t.w + sx]);
        }
    }
    Tex { w: s, h: s, px }
}

fn sheet(cells: &[Tex], size: usize, cols: usize) -> Tex {
    let cols = cols.clamp(1, cells.len().max(1));
    let rows = cells.len().div_ceil(cols).max(1);
    let gap = 4;
    let (w, h) = (cols * size + (cols + 1) * gap, rows * size + (rows + 1) * gap);
    let mut px = vec![[0.1, 0.1, 0.1, 1.0]; w * h];
    for (i, c) in cells.iter().enumerate() {
        let (cx, cy) = (gap + (i % cols) * (size + gap), gap + (i / cols) * (size + gap));
        for y in 0..size {
            for x in 0..size {
                px[(cy + y) * w + cx + x] = c.px[y * size + x];
            }
        }
    }
    Tex { w, h, px }
}

fn c_render_node(s: &mut Session, p: &Json) -> Result<Outcome, String> {
    let id = s.arg_node(p, NODE)?;
    let size = opt_usize(p, "size", 16, MAX_RENDER, 256)?;
    let tile = opt_usize(p, "tile", 1, 4, 1)?;
    let channel = p.get("channel").and_then(Json::as_str).unwrap_or("rgb");
    let normalize = p.get("normalize").and_then(Json::as_bool).unwrap_or(false);
    let want_image = p.get("image").and_then(Json::as_bool).unwrap_or(true);
    let out = p.get("output").and_then(Json::as_str);
    let base = if tile > 1 { (size / tile).max(32) } else { size };
    let t = s.eval(id, base, out)?;
    let mut j = stats_json(&t);
    j.set("node", id);
    j.set("kind", s.graph.nodes.get(&id).map(|n| n.kind.as_str()));
    let mut images = Vec::new();
    if want_image {
        let d = display(&tiled(&t, tile), channel, normalize)?;
        images.push(Image { label: format!("node {id}"), png: png_of(&d) });
    }
    Ok(Outcome { data: j, images })
}

fn cells_from(s: &Session, items: &[(String, u32)], size: usize) -> Result<Vec<Tex>, String> {
    let mut cells = Vec::new();
    for (label, id) in items {
        let t = s.eval(*id, size, None).map_err(|e| format!("{label}: {e}"))?;
        cells.push(fit_cell(&display(&t, "rgb", false)?, size));
    }
    Ok(cells)
}

fn c_render_outputs(s: &mut Session, p: &Json) -> Result<Outcome, String> {
    let size = opt_usize(p, "size", 32, 1024, 192)?;
    let cols = opt_usize(p, "columns", 1, 8, 3)?;
    let mut outs = s.graph.output_nodes();
    outs.sort_by_key(|o| OUTPUT_USAGES.iter().position(|x| *x == o.0).unwrap_or(99));
    if outs.is_empty() {
        return Err("the graph has no outputs yet (output.add)".into());
    }
    let items: Vec<(String, u32)> = outs.iter().map(|o| (o.1.clone(), o.2)).collect();
    let cells = cells_from(s, &items, size)?;
    let order: Vec<Json> = items.iter().map(|(n, _)| Json::from(n.as_str())).collect();
    Ok(Outcome { data: Json::obj().with("order_left_to_right_top_to_bottom", Json::Arr(order)), images: vec![Image { label: "outputs".into(), png: png_of(&sheet(&cells, size, cols)) }] })
}

fn c_render_sheet(s: &mut Session, p: &Json) -> Result<Outcome, String> {
    let size = opt_usize(p, "size", 32, 1024, 192)?;
    let cols = opt_usize(p, "columns", 1, 8, 3)?;
    let ids = s.nodes_arg(p, "nodes")?;
    if ids.is_empty() || ids.len() > 36 {
        return Err("give 1 to 36 nodes".into());
    }
    let items: Vec<(String, u32)> = ids.iter().map(|i| (format!("node {i}"), *i)).collect();
    let cells = cells_from(s, &items, size)?;
    let order: Vec<Json> = ids.iter().map(|i| Json::from(*i)).collect();
    Ok(Outcome { data: Json::obj().with("order_left_to_right_top_to_bottom", Json::Arr(order)), images: vec![Image { label: "sheet".into(), png: png_of(&sheet(&cells, size, cols)) }] })
}

fn c_render_3d(s: &mut Session, p: &Json) -> Result<Outcome, String> {
    let size = opt_usize(p, "size", 16, 1024, 512)?;
    let tex = opt_usize(p, "tex", 16, 2048, 512)?;
    s.graph.validate().map_err(|e| format!("cannot render yet: {e}"))?;
    let mut args: Vec<String> = Vec::new();
    let tmp = std::env::temp_dir().join(format!("materialcraft-{}-{}.png", std::process::id(), std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_nanos()).unwrap_or(0)));
    args.push("graph".into());
    args.push(tmp.display().to_string());
    args.extend(["--size".into(), format!("{size}x{size}"), "--tex".into(), tex.to_string()]);
    for (k, flag) in [("shape", "--shape"), ("scene", "--scene"), ("channel", "--channel"), ("camera", "--camera"), ("env", "--env"), ("mode", "--mode")] {
        if let Some(v) = p.get(k).and_then(Json::as_str) {
            if v.starts_with('-') || v.contains(char::is_whitespace) {
                return Err(format!("{k}: invalid value"));
            }
            if k == "env" && (v.contains(['/', '\\']) || v.contains('.')) {
                return Err("env: use a preset name".into());
            }
            args.push(flag.into());
            args.push(v.to_string());
        }
    }
    let g = s.graph.clone();
    let r = crate::cli3d::run_quiet(&args, &|_| Ok(g.clone()));
    let bytes = std::fs::read(&tmp);
    let _ = std::fs::remove_file(&tmp);
    r?;
    let bytes = bytes.map_err(|e| format!("render failed: {e}"))?;
    Ok(Outcome { data: Json::obj().with("size", size), images: vec![Image { label: "3d".into(), png: bytes }] })
}

fn c_export(s: &mut Session, p: &Json) -> Result<Outcome, String> {
    let dir = s.resolve_path(p.get("dir").and_then(Json::as_str).unwrap_or(""))?;
    let size = opt_usize(p, "size", 16, 8192, s.graph.size)?;
    let depth = match opt_usize(p, "depth", 8, 32, 8)? {
        8 => 8u8,
        16 => 16,
        32 => 32,
        _ => return Err("depth must be 8, 16 or 32".into()),
    };
    let fmt = match p.get("format").and_then(Json::as_str) {
        Some(f) => Format::parse(f).ok_or("format must be png, jpg, tiff, exr or hdr")?,
        None => Format::Png,
    };
    let maps = s.graph.evaluate(Some(size)).map_err(gerr)?;
    std::fs::create_dir_all(&dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    let mut files = Vec::new();
    for (name, t) in &maps {
        let path = dir.join(format!("{name}.{}", fmt.ext()));
        crate::safeio::write_file(&path, &export::encode(fmt, t, if fmt == Format::Jpeg { 8 } else { depth }, 92)).map_err(|e| format!("{}: {e}", path.display()))?;
        files.push(Json::from(path.display().to_string()));
    }
    Outcome::data(Json::obj().with("written", Json::Arr(files)))
}

// ------------------------------------------------------------------------------------------ history

fn c_history(s: &mut Session, _: &Json) -> Result<Outcome, String> {
    Outcome::data(Json::obj().with("undo_steps", s.undo.len()).with("redo_steps", s.redo.len()).with("recent_commands", Json::Arr(s.log.iter().rev().take(20).map(|c| Json::from(c.as_str())).collect::<Vec<_>>())))
}
