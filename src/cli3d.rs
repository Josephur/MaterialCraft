// SPDX-License-Identifier: MIT OR Apache-2.0
//! `materialcraft render3d`: pictures and turntables of a material from the command line.
use crate::graph::Graph;
use crate::objects::CustomMesh;
use crate::render3d::{self, Maps, View, SHAPE_CUSTOM};
use crate::scene;
use crate::tex::Tex;
use std::collections::BTreeMap;
use std::path::Path;
use std::sync::Arc;

pub const HELP: &str = "usage: materialcraft render3d <graph.mcg|.mca> <out.png> [options]

  --size WxH | N          picture size (default 768x768)         --tex N     resolution the material is computed at
  --mode raster|pt        software rasteriser (default) or path tracer     --spp N   samples per pixel for the path tracer (default 64)
  --shape NAME|NUMBER     sphere, cube, plane, ... (see --list)  --mesh FILE   show an OBJ / glTF / GLB / PLY / STL mesh
  --scene single|floor|lookdev|pedestal                          --channel lit|base|roughness|metallic|normal|ao|height|emissive|world_normal|uv
  --env NAME|FILE         environment preset or .hdr / .exr / .png image    --rig NAME   light rig (three_point, key_only, top, sun, none)
  --camera NAME           three_quarter, front, back, left, right, top, bottom, close_up
  --post NAME             neutral, cinematic, punchy, soft       --view FILE  load a settings file (key value lines)
  --set key=value         any view setting, repeatable, e.g. --set post.bloom=0.4 --set light.0.intensity=5 --set env.rotation=1.2
  --param \"Label=value\"   graph parameter, repeatable
  --turntable N           write N frames around the object into the folder <out> (frame_0000.png ...)
  --list                  show the names the options accept
";

fn list() {
    println!("shapes:    {}", render3d::SHAPES.iter().map(|s| s.0.to_lowercase().replace(' ', "_")).collect::<Vec<_>>().join(", "));
    println!("scenes:    {}", scene::SceneKind::ALL.iter().map(|s| s.id()).collect::<Vec<_>>().join(", "));
    println!("channels:  {}", scene::Channel::ALL.iter().map(|s| s.id()).collect::<Vec<_>>().join(", "));
    println!("environments: {}", crate::hdri::PRESETS.iter().map(|s| s.0).collect::<Vec<_>>().join(", "));
    println!("light rigs: {}", scene::RIGS.iter().map(|s| s.0).collect::<Vec<_>>().join(", "));
    println!("cameras:   {}", scene::CAMERAS.iter().map(|s| s.0).collect::<Vec<_>>().join(", "));
    println!("post looks: {}", scene::POST_PRESETS.iter().map(|s| s.0).collect::<Vec<_>>().join(", "));
    println!("tone maps: {}", scene::ToneMap::ALL.iter().map(|s| s.id()).collect::<Vec<_>>().join(", "));
    println!("{}", View::default().to_text());
}

pub fn run(args: &[String], load: &dyn Fn(&str) -> Result<Graph, String>) -> Result<(), String> {
    run_inner(args, load, false)
}

/// Like [`run`] but never writes to standard output (the MCP server uses standard output for its protocol).
pub fn run_quiet(args: &[String], load: &dyn Fn(&str) -> Result<Graph, String>) -> Result<(), String> {
    run_inner(args, load, true)
}

fn run_inner(args: &[String], load: &dyn Fn(&str) -> Result<Graph, String>, quiet: bool) -> Result<(), String> {
    if quiet && (args.iter().any(|a| a == "--list") || args.len() < 2 || args[0].starts_with("--")) {
        return Err("usage: <graph> <out.png> [options]".into());
    }
    if args.iter().any(|a| a == "--list") {
        list();
        return Ok(());
    }
    if args.len() < 2 || args[0].starts_with("--") {
        print!("{HELP}");
        return Err("usage: materialcraft render3d <graph> <out.png> [options]".into());
    }
    let mut g = load(&args[0])?;
    let out = args[1].clone();
    let mut view = View { tess: 256, ..View::default() };
    let (mut w, mut h) = (768usize, 768usize);
    let (mut tex_size, mut mode, mut spp, mut turn) = (None::<usize>, "raster".to_string(), 64u32, 0u32);
    let mut params: BTreeMap<String, f32> = BTreeMap::new();
    let mut i = 2;
    let next = |i: &mut usize, what: &str| -> Result<String, String> {
        *i += 1;
        args.get(*i).cloned().ok_or_else(|| format!("{what} needs a value"))
    };
    // the settings file first, so the command line wins
    let mut sets: Vec<(String, String)> = Vec::new();
    while i < args.len() {
        match args[i].as_str() {
            "--size" => {
                let v = next(&mut i, "--size")?;
                let (a, b) = v.split_once('x').unwrap_or((&v, &v));
                w = a.parse().map_err(|_| format!("bad size '{v}'"))?;
                h = b.parse().map_err(|_| format!("bad size '{v}'"))?;
                if !(16..=8192).contains(&w) || !(16..=8192).contains(&h) {
                    return Err("the picture size must be between 16 and 8192".into());
                }
            }
            "--tex" => tex_size = Some(next(&mut i, "--tex")?.parse().map_err(|_| "--tex needs a number")?),
            "--mode" => {
                mode = next(&mut i, "--mode")?;
                if mode != "raster" && mode != "pt" {
                    return Err("--mode must be raster or pt".into());
                }
            }
            "--spp" => spp = next(&mut i, "--spp")?.parse().ok().filter(|n| (1..=65536).contains(n)).ok_or("--spp needs 1..65536")?,
            "--turntable" => turn = next(&mut i, "--turntable")?.parse().ok().filter(|n| (1..=3600).contains(n)).ok_or("--turntable needs 1..3600")?,
            "--shape" => sets.push(("shape".into(), next(&mut i, "--shape")?)),
            "--scene" => sets.push(("scene".into(), next(&mut i, "--scene")?)),
            "--channel" => sets.push(("channel".into(), next(&mut i, "--channel")?)),
            "--rig" => sets.push(("rig".into(), next(&mut i, "--rig")?)),
            "--camera" => sets.push(("camera".into(), next(&mut i, "--camera")?)),
            "--env" => {
                let v = next(&mut i, "--env")?;
                if crate::hdri::PRESETS.iter().any(|p| p.0 == v) {
                    sets.push(("env.preset".into(), v));
                } else {
                    sets.push(("env.file".into(), v));
                    sets.push(("env.background".into(), "environment".into()));
                }
            }
            "--post" => {
                let v = next(&mut i, "--post")?;
                if !scene::POST_PRESETS.iter().any(|p| p.0 == v) {
                    return Err(format!("unknown post look '{v}' ({})", scene::POST_PRESETS.iter().map(|p| p.0).collect::<Vec<_>>().join(", ")));
                }
                view.post = scene::post_preset(&v);
            }
            "--view" => {
                let p = next(&mut i, "--view")?;
                let text = std::fs::read_to_string(&p).map_err(|e| format!("{p}: {e}"))?;
                view.apply_text(&text).map_err(|e| format!("{p}: {e}"))?;
            }
            "--mesh" => {
                let p = next(&mut i, "--mesh")?;
                let cm = CustomMesh::load(Path::new(&p)).map_err(|e| format!("{p}: {e}"))?;
                view.custom = Some(Arc::new(cm));
                view.shape = SHAPE_CUSTOM;
            }
            "--set" => {
                let kv = next(&mut i, "--set")?;
                let (k, v) = kv.split_once('=').ok_or("--set must look like key=value")?;
                sets.push((k.to_string(), v.to_string()));
            }
            "--param" => {
                let kv = next(&mut i, "--param")?;
                let (k, v) = kv.rsplit_once('=').ok_or("--param must look like \"Label=value\"")?;
                params.insert(k.to_string(), v.parse().map_err(|_| format!("bad value in '{kv}'"))?);
            }
            o => return Err(format!("unknown option '{o}' (try --help)")),
        }
        i += 1;
    }
    for (k, v) in &sets {
        if k == "env.file" && !Path::new(v).exists() {
            return Err(format!("{v}: no such environment image (presets: {})", crate::hdri::PRESETS.iter().map(|p| p.0).collect::<Vec<_>>().join(", ")));
        }
        view.set(k, v)?;
    }
    if let Some(f) = &view.env.file {
        crate::hdri::load_env("studio", Some(f)).map_err(|e| format!("{f}: {e}"))?;
    }
    g.apply_params(&params)?;
    let outs = g.evaluate(tex_size.or(Some(g.size.min(1024)))).map_err(|e| e.to_string())?;
    let (flat, half, zero, one) = (Tex::solid(4, 4, [0.5, 0.5, 1.0, 1.0]), Tex::solid(4, 4, [0.5, 0.5, 0.5, 1.0]), Tex::solid(4, 4, [0.0, 0.0, 0.0, 1.0]), Tex::solid(4, 4, [1.0; 4]));
    let get = |k: &str| outs.get(k);
    let inv_gloss = get("glossiness").map(crate::ops::invert);
    let maps = Maps {
        base: get("basecolor").or_else(|| get("diffuse")).unwrap_or(&half),
        normal: get("normal").unwrap_or(&flat),
        rough: get("roughness").or(inv_gloss.as_ref()).unwrap_or(&half),
        metal: get("metallic").unwrap_or(&zero),
        ao: get("ambient_occlusion").unwrap_or(&one),
        emissive: get("emissive"),
        height: get("height"),
        env: None,
    };
    let frames = turn.max(1);
    if turn > 0 {
        std::fs::create_dir_all(&out).map_err(|e| format!("{out}: {e}"))?;
    }
    for f in 0..frames {
        let mut v = view.clone();
        if turn > 0 {
            v.yaw += std::f32::consts::TAU * f as f32 / turn as f32;
        }
        let img = if mode == "pt" { crate::pt::render(&maps, &v, w, h, spp) } else { render3d::render(&maps, &v, w, h) };
        let path = if turn > 0 { Path::new(&out).join(format!("frame_{f:04}.png")) } else { Path::new(&out).to_path_buf() };
        crate::safeio::write_file(&path, &crate::png::encode(img.w, img.h, &img.px, 8)).map_err(|e| format!("{}: {e}", path.display()))?;
        if !quiet {
            println!("wrote {}", path.display());
        }
    }
    Ok(())
}
