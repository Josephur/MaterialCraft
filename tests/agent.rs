// SPDX-License-Identifier: MIT OR Apache-2.0
//! The command layer, the MCP server and the control channel.
use materialcraft::agent::{self, Backend, BridgeClient};
use materialcraft::json::Json;
use materialcraft::session::{self, Session};
use std::io::{BufRead, BufReader, Write};
use std::net::TcpStream;
use std::sync::{Arc, Mutex, PoisonError};

fn run(s: &mut Session, cmd: &str, p: Json) -> Json {
    match s.execute(cmd, &p) {
        Ok(o) => o.data,
        Err(e) => panic!("{cmd} failed: {e}"),
    }
}

fn fails(s: &mut Session, cmd: &str, p: Json) -> String {
    match s.execute(cmd, &p) {
        Ok(o) => panic!("{cmd} should have failed but returned {}", o.data.to_text()),
        Err(e) => e,
    }
}

/// Voronoi + Perlin blended into a height map, with a normal map.
fn small_material(s: &mut Session) {
    let steps = Json::parse(
        r#"[
        {"command":"node_add","params":{"kind":"voronoi","name":"cells","params":{"scale":6,"mode":"Edge"}}},
        {"command":"node_add","params":{"kind":"perlin","name":"grain","params":{"scale":14}}},
        {"command":"node_add","params":{"kind":"blend","name":"mix","inputs":{"fg":"cells","bg":"grain"}}},
        {"command":"node_add","params":{"kind":"normal","name":"nrm","inputs":{"in":"mix"}}},
        {"command":"output_add","params":{"node":"mix","usage":"height"}},
        {"command":"output_add","params":{"node":"nrm","usage":"normal"}}
    ]"#,
    )
    .unwrap();
    run(s, "batch", Json::obj().with("steps", steps));
}

#[test]
fn builds_a_material_with_one_batch_and_one_undo_step() {
    let mut s = Session::new();
    run(&mut s, "graph.set", Json::obj().with("size", 128));
    small_material(&mut s);
    let g = run(&mut s, "graph.inspect", Json::obj());
    assert_eq!(g.get("nodes").and_then(Json::as_arr).map(|a| a.len()), Some(6));
    assert_eq!(g.get("problems").and_then(Json::as_arr).map(|a| a.len()), Some(0));
    // the batch is a single undo step: graph.set was the first, the batch the second
    run(&mut s, "edit.undo", Json::obj());
    let g = run(&mut s, "graph.inspect", Json::obj());
    assert_eq!(g.get("nodes").and_then(Json::as_arr).map(|a| a.len()), Some(0));
    run(&mut s, "edit.redo", Json::obj());
    let g = run(&mut s, "graph.inspect", Json::obj());
    assert_eq!(g.get("nodes").and_then(Json::as_arr).map(|a| a.len()), Some(6));
}

#[test]
fn a_failing_batch_changes_nothing() {
    let mut s = Session::new();
    let steps = Json::parse(
        r#"[
        {"command":"node_add","params":{"kind":"perlin","name":"a"}},
        {"command":"node_add","params":{"kind":"blend","name":"b","inputs":{"fg":"a","nope":"a"}}}
    ]"#,
    )
    .unwrap();
    let e = fails(&mut s, "batch", Json::obj().with("steps", steps));
    assert!(e.contains("step 2") && e.contains("no input 'nope'"), "{e}");
    let g = run(&mut s, "graph.inspect", Json::obj());
    assert_eq!(g.get("nodes").and_then(Json::as_arr).map(|a| a.len()), Some(0), "the first step must be rolled back");
    assert!(!s.can_undo(), "a failed batch leaves no undo step");
}

#[test]
fn parameters_are_checked_and_explained() {
    let mut s = Session::new();
    run(&mut s, "node.add", Json::obj().with("kind", "voronoi").with("name", "v"));
    // a typo gets a suggestion
    let e = fails(&mut s, "node.set", Json::obj().with("node", "v").with("params", Json::obj().with("scalee", 3)));
    assert!(e.contains("Did you mean") && e.contains("scale"), "{e}");
    // an out-of-range number is clamped and reported
    let r = run(&mut s, "node.set", Json::obj().with("node", "v").with("params", Json::obj().with("scale", 9999)));
    assert!(r.get("warnings").and_then(Json::as_arr).map(|w| !w.is_empty()).unwrap_or(false));
    let g = run(&mut s, "graph.inspect", Json::obj());
    let v = &g.get("nodes").and_then(Json::as_arr).unwrap()[0];
    assert_eq!(v.get("params").and_then(|p| p.get("scale")).and_then(Json::as_f64), Some(64.0));
    // a dropdown takes its choice by name (also abbreviated) and is stored as its index
    run(&mut s, "node.set", Json::obj().with("node", "v").with("params", Json::obj().with("mode", "edge")));
    let g = run(&mut s, "graph.inspect", Json::obj());
    let v = &g.get("nodes").and_then(Json::as_arr).unwrap()[0];
    assert_eq!(v.get("params").and_then(|p| p.get("mode")).and_then(Json::as_f64), Some(2.0));
    let e = fails(&mut s, "node.set", Json::obj().with("node", "v").with("params", Json::obj().with("mode", "banana")));
    assert!(e.contains("Edge"), "the choices are listed: {e}");
    // null restores the default
    run(&mut s, "node.set", Json::obj().with("node", "v").with("params", Json::obj().with("mode", Json::Null)));
    // an unknown node type gets suggestions
    let e = fails(&mut s, "node.add", Json::obj().with("kind", "voronoii"));
    assert!(e.contains("voronoi"), "{e}");
}

#[test]
fn connections_are_validated() {
    let mut s = Session::new();
    run(&mut s, "node.add", Json::obj().with("kind", "perlin").with("name", "a"));
    run(&mut s, "node.add", Json::obj().with("kind", "blur").with("name", "b").with("inputs", Json::obj().with("in", "a")));
    run(&mut s, "node.add", Json::obj().with("kind", "blur").with("name", "c"));
    run(&mut s, "node.connect", Json::obj().with("from", "b").with("to", "c"));
    // a loop is refused, and so is a node feeding itself or a node without inputs receiving a link
    let e = fails(&mut s, "node.connect", Json::obj().with("from", "c").with("to", "b"));
    assert!(e.contains("loop") || e.contains("cycle"), "{e}");
    assert!(!fails(&mut s, "node.connect", Json::obj().with("from", "c").with("to", "c")).is_empty());
    let e = fails(&mut s, "node.connect", Json::obj().with("from", "b").with("to", "a"));
    assert!(e.contains("no inputs"), "{e}");
    let v = run(&mut s, "graph.validate", Json::obj());
    let problems = v.get("problems").and_then(Json::as_arr).unwrap();
    assert!(problems.iter().all(|p| !p.to_text().contains("loop")), "{problems:?}");
    let e = fails(&mut s, "output.add", Json::obj().with("node", "a").with("usage", "shiny"));
    assert!(e.contains("basecolor"), "{e}");
}

#[test]
fn removing_a_node_removes_the_links_that_used_it() {
    let mut s = Session::new();
    small_material(&mut s);
    let r = run(&mut s, "node.remove", Json::obj().with("nodes", Json::Arr(vec!["cells".into()])));
    assert_eq!(r.get("links_removed").and_then(Json::as_u32), Some(1));
    let v = run(&mut s, "graph.validate", Json::obj());
    assert_eq!(v.get("ok").and_then(Json::as_bool), Some(false), "blend lost an input only when it is required; the graph must still report honestly");
}

#[test]
fn renders_pictures_and_statistics() {
    let mut s = Session::new();
    run(&mut s, "graph.set", Json::obj().with("size", 128));
    small_material(&mut s);
    let o = s.execute("render.node", &Json::obj().with("node", "mix").with("size", 64)).unwrap();
    assert_eq!(o.images.len(), 1);
    let img = materialcraft::png::decode(&o.images[0].png).expect("a valid PNG");
    assert_eq!((img.w, img.h), (64, 64));
    assert_eq!(o.data.get("flat").and_then(Json::as_bool), Some(false));
    assert!(o.data.get("seam_horizontal").and_then(Json::as_f64).is_some());
    // tile=2 repeats the picture
    let o = s.execute("render.node", &Json::obj().with("node", "mix").with("size", 64).with("tile", 2)).unwrap();
    let img = materialcraft::png::decode(&o.images[0].png).unwrap();
    assert_eq!((img.w, img.h), (64, 64), "a tiled preview keeps the requested size");
    // statistics only
    let o = s.execute("render.node", &Json::obj().with("node", "mix").with("size", 32).with("image", false)).unwrap();
    assert!(o.images.is_empty());
    // a second output of a multi-output node
    let o = s.execute("render.node", &Json::obj().with("node", "cells").with("output", "cell_id").with("size", 32)).unwrap();
    assert_eq!(o.images.len(), 1);
    let e = fails(&mut s, "render.node", Json::obj().with("node", "grain").with("output", "nope"));
    assert!(e.contains("single output"), "{e}");
    // contact sheet of all outputs
    let o = s.execute("render.outputs", &Json::obj().with("size", 48)).unwrap();
    assert_eq!(o.images.len(), 1);
    let o = s.execute("render.sheet", &Json::obj().with("nodes", Json::Arr(vec!["cells".into(), "grain".into(), "mix".into()])).with("size", 40).with("columns", 2)).unwrap();
    let img = materialcraft::png::decode(&o.images[0].png).unwrap();
    assert!(img.w > 80 && img.h > 80);
}

#[test]
fn a_flat_result_is_reported() {
    let mut s = Session::new();
    run(&mut s, "node.add", Json::obj().with("kind", "color").with("name", "c"));
    let o = s.execute("render.node", &Json::obj().with("node", "c").with("size", 32)).unwrap();
    assert_eq!(o.data.get("flat").and_then(Json::as_bool), Some(true));
}

#[test]
fn the_3d_render_returns_a_picture() {
    let mut s = Session::new();
    run(&mut s, "graph.set", Json::obj().with("size", 64));
    run(&mut s, "node.add", Json::obj().with("kind", "perlin").with("name", "p"));
    run(&mut s, "output.add", Json::obj().with("node", "p").with("usage", "basecolor"));
    let o = s.execute("render.3d", &Json::obj().with("size", 64).with("tex", 64)).unwrap();
    let img = materialcraft::png::decode(&o.images[0].png).unwrap();
    assert_eq!((img.w, img.h), (64, 64));
    // option values that could smuggle a command line option are refused
    let e = fails(&mut s, "render.3d", Json::obj().with("shape", "--mesh"));
    assert!(e.contains("invalid"), "{e}");
    let e = fails(&mut s, "render.3d", Json::obj().with("env", "../../etc/passwd"));
    assert!(!e.is_empty());
}

#[test]
fn files_save_open_and_stay_inside_the_root() {
    let dir = std::env::temp_dir().join(format!("mc_agent_files_{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let root = std::fs::canonicalize(&dir).unwrap();
    let mut s = Session::new();
    s.root = Some(root.clone());
    run(&mut s, "graph.set", Json::obj().with("size", 64));
    small_material(&mut s);
    run(&mut s, "graph.save", Json::obj().with("path", "wall.mcg"));
    assert!(root.join("wall.mcg").exists());
    let mut t = Session::new();
    t.root = Some(root.clone());
    run(&mut t, "graph.open", Json::obj().with("path", "wall.mcg"));
    let g = run(&mut t, "graph.inspect", Json::obj());
    assert_eq!(g.get("nodes").and_then(Json::as_arr).map(|a| a.len()), Some(6));
    // outside the root, and parent folders, are refused
    let e = fails(&mut t, "graph.save", Json::obj().with("path", "../escape.mcg"));
    assert!(e.contains(".."), "{e}");
    let outside = std::env::temp_dir().join("mc_agent_outside.mcg");
    let e = fails(&mut t, "graph.save", Json::obj().with("path", outside.display().to_string()));
    assert!(e.contains("outside"), "{e}");
    let e = fails(&mut t, "export", Json::obj().with("dir", "/tmp"));
    assert!(e.contains("outside") || e.contains(".."), "{e}");
    // export writes the maps
    run(&mut t, "export", Json::obj().with("dir", "maps").with("size", 32));
    assert!(root.join("maps").join("height.png").exists());
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn library_material_loads_and_validates() {
    let mut s = Session::new();
    let hits = run(&mut s, "library.search", Json::obj().with("query", "lava"));
    assert!(hits.get("count").and_then(Json::as_u32).unwrap_or(0) >= 1);
    run(&mut s, "library.load", Json::obj().with("id", "cooling_lava"));
    let v = run(&mut s, "graph.validate", Json::obj());
    assert_eq!(v.get("ok").and_then(Json::as_bool), Some(true), "{}", v.to_text());
    let e = fails(&mut s, "library.load", Json::obj().with("id", "no_such_thing"));
    assert!(e.contains("library.search"));
    run(&mut s, "edit.undo", Json::obj());
}

#[test]
fn describing_nodes_matches_the_editor_tables() {
    let mut s = Session::new();
    let d = run(&mut s, "node.describe", Json::obj().with("kind", "voronoi"));
    let params = d.get("params").and_then(Json::as_arr).unwrap();
    let scale = params.iter().find(|p| p.get("name").and_then(Json::as_str) == Some("scale")).unwrap();
    assert_eq!(scale.get("type").and_then(Json::as_str), Some("integer"));
    assert_eq!(scale.get("max").and_then(Json::as_f64), Some(64.0));
    let mode = params.iter().find(|p| p.get("name").and_then(Json::as_str) == Some("mode")).unwrap();
    assert!(mode.get("options").and_then(Json::as_arr).map(|o| o.len() >= 5).unwrap_or(false));
    // every node type describes itself without trouble
    let all = run(&mut s, "node.types", Json::obj());
    let nodes = all.get("nodes").and_then(Json::as_arr).unwrap();
    assert!(nodes.len() > 150, "{} node types", nodes.len());
    for n in nodes {
        let k = n.get("kind").and_then(Json::as_str).unwrap();
        run(&mut s, "node.describe", Json::obj().with("kind", k));
    }
    let q = run(&mut s, "node.types", Json::obj().with("query", "blur"));
    assert!(q.get("count").and_then(Json::as_u32).unwrap_or(0) >= 3);
}

#[test]
fn every_node_type_can_be_added_with_its_defaults() {
    // adding a node must never panic or leave the graph unusable, whatever the type
    let mut s = Session::new();
    let all = run(&mut s, "node.types", Json::obj());
    for n in all.get("nodes").and_then(Json::as_arr).unwrap() {
        let k = n.get("kind").and_then(Json::as_str).unwrap();
        if k == "output" {
            continue;
        }
        if let Err(e) = s.execute("node.add", &Json::obj().with("kind", k)) {
            panic!("adding {k} failed: {e}");
        }
    }
    let g = run(&mut s, "graph.text", Json::obj());
    let text = g.get("text").and_then(Json::as_str).unwrap();
    materialcraft::graph::Graph::parse_unchecked(text).expect("the graph text parses again");
}

#[test]
fn hostile_parameters_never_panic() {
    let junk = [
        Json::Null,
        Json::Bool(true),
        Json::Num(-1.0),
        Json::Num(1e300),
        Json::Num(0.5),
        Json::Num(4_294_967_296.0),
        Json::Str(String::new()),
        Json::Str("x\"y#z\n".into()),
        Json::Str("--mesh".into()),
        Json::Arr(vec![Json::Null, Json::Num(1.0)]),
        Json::Obj(vec![("a".into(), Json::Arr(vec![]))]),
    ];
    for cmd in session::COMMANDS {
        if matches!(cmd.id, "graph.save" | "export" | "graph.open") {
            continue; // these write or read files; covered separately
        }
        let mut s = Session::new();
        small_material(&mut s);
        for j in &junk {
            // every argument, once, set to the junk value
            for a in cmd.args {
                let p = Json::obj().with(a.name, j.clone());
                if let Err(e) = s.execute(cmd.id, &p) {
                    assert!(!e.starts_with("internal error"), "{} {} = {}: {e}", cmd.id, a.name, j.to_text());
                }
            }
            // and the same junk in all of them at once
            let mut p = Json::obj();
            for a in cmd.args {
                p.set(a.name, j.clone());
            }
            if let Err(e) = s.execute(cmd.id, &p) {
                assert!(!e.starts_with("internal error"), "{} all = {}: {e}", cmd.id, j.to_text());
            }
        }
        // the graph is still usable afterwards
        let g = s.execute("graph.text", &Json::obj()).unwrap();
        materialcraft::graph::Graph::parse_unchecked(g.data.get("text").and_then(Json::as_str).unwrap()).expect("graph text still parses");
    }
}

#[test]
fn unknown_commands_and_bad_parameter_shapes_are_explained() {
    let mut s = Session::new();
    assert!(fails(&mut s, "node.explode", Json::obj()).contains("command.list"));
    assert!(fails(&mut s, "node.add", Json::obj()).contains("missing required parameter 'kind'"));
    assert!(fails(&mut s, "node.add", Json::obj().with("kind", 3)).contains("must be of type string"));
    assert!(fails(&mut s, "node.add", Json::obj().with("kind", "perlin").with("colour", 1)).contains("unknown parameter 'colour'"));
    assert!(s.execute("node.add", &Json::Arr(vec![])).is_err());
    // the MCP spelling works too
    assert!(s.execute("node_types", &Json::Null).is_ok());
}

#[test]
fn undo_and_redo_walk_the_history() {
    let mut s = Session::new();
    for k in ["perlin", "voronoi", "clouds"] {
        run(&mut s, "node.add", Json::obj().with("kind", k));
    }
    let h = run(&mut s, "edit.history", Json::obj());
    assert_eq!(h.get("undo_steps").and_then(Json::as_u32), Some(3));
    run(&mut s, "edit.undo", Json::obj());
    run(&mut s, "edit.undo", Json::obj());
    run(&mut s, "node.add", Json::obj().with("kind", "gradient"));
    assert!(fails(&mut s, "edit.redo", Json::obj()).contains("nothing to redo"), "a new edit clears redo");
    run(&mut s, "edit.undo", Json::obj());
    run(&mut s, "edit.undo", Json::obj());
    assert!(fails(&mut s, "edit.undo", Json::obj()).contains("nothing to undo"));
}

#[test]
fn layout_puts_nodes_in_columns() {
    let mut s = Session::new();
    small_material(&mut s);
    run(&mut s, "graph.layout", Json::obj());
    let g = run(&mut s, "graph.inspect", Json::obj());
    let nodes = g.get("nodes").and_then(Json::as_arr).unwrap();
    let x = |name: &str| nodes.iter().find(|n| n.get("name").and_then(Json::as_str) == Some(name)).and_then(|n| n.get("x")).and_then(Json::as_f64).unwrap();
    assert!(x("cells") < x("mix") && x("mix") < x("nrm"));
}

#[test]
fn exposed_parameters_round_trip() {
    let mut s = Session::new();
    small_material(&mut s);
    run(&mut s, "graph.expose", Json::obj().with("label", "Cell size").with("node", "cells").with("param", "scale"));
    let g = run(&mut s, "graph.inspect", Json::obj());
    let e = &g.get("exposed").and_then(Json::as_arr).unwrap()[0];
    assert_eq!(e.get("max").and_then(Json::as_f64), Some(64.0));
    assert!(fails(&mut s, "graph.expose", Json::obj().with("label", "Cell size").with("node", "cells").with("param", "scale")).contains("already"));
    run(&mut s, "graph.unexpose", Json::obj().with("label", "Cell size"));
}

// ------------------------------------------------------------------------------------------ MCP

fn mcp(b: &mut dyn Backend, m: &str) -> Option<Json> {
    agent::mcp_handle(b, &Json::parse(m).unwrap())
}

#[test]
fn mcp_handshake_tools_and_calls() {
    let mut s = Session::new();
    let r = mcp(&mut s, r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2024-11-05"}}"#).unwrap();
    let res = r.get("result").unwrap();
    assert_eq!(res.get("protocolVersion").and_then(Json::as_str), Some("2024-11-05"), "an older client is answered in its own revision");
    assert!(res.get("instructions").and_then(Json::as_str).map(|t| t.contains("batch")).unwrap_or(false));
    let r = mcp(&mut s, r#"{"jsonrpc":"2.0","id":2,"method":"initialize","params":{"protocolVersion":"1999-01-01"}}"#).unwrap();
    assert_eq!(r.get("result").and_then(|x| x.get("protocolVersion")).and_then(Json::as_str), Some("2025-06-18"));
    assert!(mcp(&mut s, r#"{"jsonrpc":"2.0","method":"notifications/initialized"}"#).is_none(), "notifications get no reply");
    assert!(mcp(&mut s, r#"{"jsonrpc":"2.0","id":3,"method":"ping"}"#).is_some());

    let r = mcp(&mut s, r#"{"jsonrpc":"2.0","id":4,"method":"tools/list"}"#).unwrap();
    let tools = r.get("result").and_then(|x| x.get("tools")).and_then(Json::as_arr).unwrap();
    assert_eq!(tools.len(), session::COMMANDS.len());
    let mut names = std::collections::BTreeSet::new();
    for t in tools {
        let name = t.get("name").and_then(Json::as_str).unwrap();
        assert!(names.insert(name.to_string()), "duplicate tool {name}");
        assert!(name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_'), "{name}");
        assert!(t.get("description").and_then(Json::as_str).map(|d| !d.is_empty()).unwrap_or(false));
        let sch = t.get("inputSchema").unwrap();
        assert_eq!(sch.get("type").and_then(Json::as_str), Some("object"));
        assert!(sch.get("properties").is_some());
    }
    // a call returns text and, for renders, an image
    mcp(&mut s, r#"{"jsonrpc":"2.0","id":5,"method":"tools/call","params":{"name":"node_add","arguments":{"kind":"perlin","name":"p"}}}"#).unwrap();
    let r = mcp(&mut s, r#"{"jsonrpc":"2.0","id":6,"method":"tools/call","params":{"name":"render_node","arguments":{"node":"p","size":32}}}"#).unwrap();
    let res = r.get("result").unwrap();
    assert_eq!(res.get("isError").and_then(Json::as_bool), Some(false));
    let content = res.get("content").and_then(Json::as_arr).unwrap();
    assert!(content.iter().any(|c| c.get("type").and_then(Json::as_str) == Some("image") && c.get("mimeType").and_then(Json::as_str) == Some("image/png")));
    // errors are tool errors, not protocol errors
    let r = mcp(&mut s, r#"{"jsonrpc":"2.0","id":7,"method":"tools/call","params":{"name":"node_add","arguments":{"kind":"zzz"}}}"#).unwrap();
    assert_eq!(r.get("result").and_then(|x| x.get("isError")).and_then(Json::as_bool), Some(true));
    let r = mcp(&mut s, r#"{"jsonrpc":"2.0","id":8,"method":"tools/call","params":{"name":"nope","arguments":{}}}"#).unwrap();
    assert_eq!(r.get("result").and_then(|x| x.get("isError")).and_then(Json::as_bool), Some(true));
    let r = mcp(&mut s, r#"{"jsonrpc":"2.0","id":9,"method":"tools/call","params":{}}"#).unwrap();
    assert!(r.get("error").is_some());
    let r = mcp(&mut s, r#"{"jsonrpc":"2.0","id":10,"method":"does/not/exist"}"#).unwrap();
    assert_eq!(r.get("error").and_then(|e| e.get("code")).and_then(Json::as_f64), Some(-32601.0));
    let r = mcp(&mut s, r#"{"jsonrpc":"2.0","id":11}"#).unwrap();
    assert!(r.get("error").is_some());
}

// ------------------------------------------------------------------------------------------ control channel

struct Conn {
    r: BufReader<TcpStream>,
    w: TcpStream,
}

impl Conn {
    fn open(port: u16) -> Conn {
        let s = TcpStream::connect(("127.0.0.1", port)).unwrap();
        s.set_read_timeout(Some(std::time::Duration::from_secs(20))).unwrap();
        Conn { w: s.try_clone().unwrap(), r: BufReader::new(s) }
    }
    fn send(&mut self, line: &str) -> Option<Json> {
        writeln!(self.w, "{line}").ok()?;
        let mut l = String::new();
        match self.r.read_line(&mut l) {
            Ok(0) | Err(_) => None,
            Ok(_) => Json::parse(l.trim()).ok(),
        }
    }
}

fn start_server(s: Session) -> (agent::ControlServer, Arc<Mutex<Session>>) {
    let shared = Arc::new(Mutex::new(s));
    let h = shared.clone();
    let handler: Arc<agent::Handler> = Arc::new(move |c, p| h.lock().unwrap_or_else(PoisonError::into_inner).execute(c, p));
    (agent::start_control(0, Some("secret-token".into()), handler).unwrap(), shared)
}

#[test]
fn the_control_channel_requires_the_token() {
    let (srv, _s) = start_server(Session::new());
    // commands before logging in are refused and the connection is closed
    let mut c = Conn::open(srv.port);
    let r = c.send(r#"{"id":1,"method":"engine.execute","params":{"command":"graph.text"}}"#).unwrap();
    assert_eq!(r.get("ok").and_then(Json::as_bool), Some(false));
    assert!(c.send(r#"{"id":2,"method":"ping"}"#).is_none(), "the connection must be closed");
    // a wrong token too
    let mut c = Conn::open(srv.port);
    let r = c.send(r#"{"id":1,"method":"auth","params":{"token":"guess"}}"#).unwrap();
    assert_eq!(r.get("ok").and_then(Json::as_bool), Some(false));
    // text that is not a JSON request (such as an HTTP request line) closes the connection
    let mut c = Conn::open(srv.port);
    let r = c.send("GET / HTTP/1.1").unwrap();
    assert_eq!(r.get("ok").and_then(Json::as_bool), Some(false));
    assert!(c.send(r#"{"method":"ping"}"#).is_none());
    // the right token works, and then commands run
    let mut c = Conn::open(srv.port);
    let r = c.send(r#"{"id":1,"method":"auth","params":{"token":"secret-token"}}"#).unwrap();
    assert_eq!(r.get("ok").and_then(Json::as_bool), Some(true));
    let r = c.send(r#"{"id":2,"method":"engine.execute","params":{"command":"node.add","params":{"kind":"perlin"}}}"#).unwrap();
    assert_eq!(r.get("ok").and_then(Json::as_bool), Some(true), "{}", r.to_text());
    assert_eq!(r.get("id").and_then(Json::as_u32), Some(2));
    let r = c.send(r#"{"id":3,"method":"engine.execute","params":{"command":"node.add","params":{"kind":"zzz"}}}"#).unwrap();
    assert_eq!(r.get("ok").and_then(Json::as_bool), Some(false));
    let r = c.send(r#"{"id":4,"method":"engine.commands"}"#).unwrap();
    assert!(r.get("result").and_then(|x| x.get("commands")).is_some());
    let r = c.send(r#"{"id":5,"method":"made.up"}"#).unwrap();
    assert_eq!(r.get("ok").and_then(Json::as_bool), Some(false));
    // blank lines are ignored
    writeln!(c.w).unwrap();
    assert!(c.send(r#"{"id":6,"method":"ping"}"#).is_some());
    srv.stop();
}

#[test]
fn the_control_channel_stops() {
    let (srv, _s) = start_server(Session::new());
    let mut c = Conn::open(srv.port);
    assert!(c.send(r#"{"id":1,"method":"auth","params":{"token":"secret-token"}}"#).is_some());
    srv.stop();
    assert!(c.send(r#"{"id":2,"method":"ping"}"#).is_none(), "open connections are closed");
    std::thread::sleep(std::time::Duration::from_millis(200));
    assert!(TcpStream::connect(("127.0.0.1", srv.port)).is_err() || Conn::open(srv.port).send("{}").is_none(), "no new connection is served");
}

#[test]
fn the_bridge_client_runs_commands_and_returns_pictures() {
    let (srv, shared) = start_server(Session::new());
    let mut b = BridgeClient::new(agent::parse_addr(&format!("127.0.0.1:{}", srv.port)).unwrap(), "secret-token".into());
    b.call("graph.set", &Json::obj().with("size", 64)).unwrap();
    b.call("node.add", &Json::obj().with("kind", "perlin").with("name", "p")).unwrap();
    let o = b.call("render.node", &Json::obj().with("node", "p").with("size", 32)).unwrap();
    assert_eq!(o.images.len(), 1);
    assert!(materialcraft::png::decode(&o.images[0].png).is_ok());
    // the edit really happened in the served session
    assert_eq!(shared.lock().unwrap().graph.nodes.len(), 1);
    // errors come back as errors
    assert!(b.call("node.add", &Json::obj().with("kind", "zzz")).is_err());
    // a wrong token is refused
    let mut bad = BridgeClient::new(agent::parse_addr(&format!("127.0.0.1:{}", srv.port)).unwrap(), "nope".into());
    assert!(bad.call("graph.text", &Json::Null).is_err());
    // the server goes away: the client reports it instead of hanging
    srv.stop();
    std::thread::sleep(std::time::Duration::from_millis(200));
    assert!(b.call("graph.text", &Json::Null).is_err());
    assert!(agent::parse_addr("8.8.8.8:80").is_err());
}

#[test]
fn tokens_are_long_and_different() {
    let (a, b) = (agent::random_token(), agent::random_token());
    assert_eq!(a.len(), 64);
    assert_ne!(a, b);
    assert!(a.chars().all(|c| c.is_ascii_hexdigit()));
}

#[test]
fn command_line_values_parse_as_json_or_text() {
    let p = materialcraft::agent_cli::parse_params(&["kind=voronoi".to_string(), "params.scale=8".into(), "params.mode=Edge".into(), "x=1.5".into(), "ok=true".into(), "list=[1,2]".into(), "t=\"3\"".into()]).unwrap();
    assert_eq!(p.get("kind").and_then(Json::as_str), Some("voronoi"));
    assert_eq!(p.get("params").and_then(|x| x.get("scale")).and_then(Json::as_f64), Some(8.0));
    assert_eq!(p.get("params").and_then(|x| x.get("mode")).and_then(Json::as_str), Some("Edge"));
    assert_eq!(p.get("x").and_then(Json::as_f64), Some(1.5));
    assert_eq!(p.get("ok").and_then(Json::as_bool), Some(true));
    assert_eq!(p.get("list").and_then(Json::as_arr).map(|a| a.len()), Some(2));
    assert_eq!(p.get("t").and_then(Json::as_str), Some("3"));
    assert!(materialcraft::agent_cli::parse_params(&["novalue".to_string()]).is_err());
}
