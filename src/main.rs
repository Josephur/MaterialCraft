// SPDX-License-Identifier: MIT OR Apache-2.0
use materialcraft::bake::{self, BakeSettings};
use materialcraft::graph::{Graph, EXAMPLE, SPECS};
use materialcraft::export::{self, Format};
use materialcraft::render3d::{self, Maps, View};
use materialcraft::{ops, png, tex::Tex};
use std::collections::BTreeMap;
use std::fs;
use std::path::Path;
use std::process::ExitCode;

const HELP: &str = "materialcraft - procedural material authoring (pure Rust)

USAGE:
  materialcraft render <graph.mcg|.mca> <outdir> [--size N | --sizes 256,512] [--depth 8|16|32]
                  [--param \"Label=value\"]... [--variations N] [--preview] [--render3d]\n                  [--format png|jpg|tiff|exr|hdr] [--quality 1-100] [--only basecolor,normal]\n                  [--colorspace srgb|p3|wide|romm|rec2020|linear]
  materialcraft publish <graph.mcg> <out.mca>       embed bitmaps + flatten into a distributable package
  materialcraft params  <graph.mcg|.mca>            list exposed parameters
  materialcraft check   <graph.mcg>
  materialcraft bake    --low <mesh>... --high <mesh>... --out <dir> [options]    mesh map baker, see: materialcraft bake --help
  materialcraft trace   <graph.mcg> <node id> <out.spl> [--size N] [--threshold T] [--tolerance X]   outline the light areas of a node as a spline
  materialcraft testmesh <outdir>                   write a demo low/high-poly OBJ pair
  materialcraft example <outdir>
  materialcraft library [words...] [--kind material|generator|filter]   list or search the built-in ready-made graphs
  materialcraft library --export <id> <file.mcg>                          write one of them out to edit
  materialcraft nodes

The visual editor is a separate binary:  cargo run --release --features gui --bin materialcraft-gui
";

fn write_png(path: &Path, t: &Tex, depth: u8) -> Result<(), String> {
    materialcraft::safeio::write_file(path, &png::encode(t.w, t.h, &t.px, depth)).map_err(|e| format!("{}: {e}", path.display()))
}

fn load(path: &str) -> Result<Graph, String> {
    // live links are read relative to the file; a link that cannot be read is reported and the graph still loads
    let (g, issues) = Graph::load_file(std::path::Path::new(path)).map_err(|e| format!("{path}: {e}"))?;
    for i in &issues {
        eprintln!("warning: {i}");
    }
    g.validate().map_err(|e| e.to_string())?;
    Ok(g)
}

const BAKE_HELP: &str = "usage: materialcraft bake --low <mesh>... [--high <mesh>...] [--cage <mesh>] --out <dir> [options]
       materialcraft bake <low.obj> <high.obj> <outdir> [options]        (short form)
       materialcraft bake --set <file.bakeset> --out <dir>

Meshes: OBJ, PLY, STL, glTF, GLB. Repeat --low / --high for several files. Without --high the low-poly mesh is baked
against itself (ambient occlusion, curvature and so on of one mesh).

Options:
  --preset preview|game|final   starting point (512 / 2048 / 4096 px)
  --size N | WxH                output size           --aa N          supersampling grid (1, 2, 3, 4, 8)
  --padding N                   texels of padding     --maps a,b,c    normal,world_normal,position,ambient_occlusion,
  --front F --rear F            ray distances (fraction of the high-poly diagonal)  bent_normal,world_bent_normal,
  --dist F                      front = rear = F                                   curvature,thickness,height,object_id,
  --ao-rays N --ao-dist F       occlusion quality / reach                          material_id,group_id,vertex_color,
  --directx                     green channel down                                 opacity,world_direction,uv_layout
  --no-match                    do not match objects by name
  --device auto|cpu|gpu         where rays are traced (default auto: the GPU when there is one)
  --depth 8|16                  PNG bit depth (default 16)
  --set-opt key=value           any setting of a bake set (see a .bakeset file)
  --save-set <file>             write the settings and mesh list as a bake set";

fn bake_cli(args: &[String]) -> Result<(), String> {
    use materialcraft::bake::{BakeSet, Progress};
    if args.is_empty() || args.iter().any(|a| a == "--help" || a == "-h") {
        println!("{BAKE_HELP}");
        return Ok(());
    }
    let mut set = BakeSet::default();
    let (mut out, mut depth, mut save) = (None::<String>, 16u8, None::<String>);
    let mut overrides: Vec<(String, String)> = Vec::new();
    let mut preset: Option<String> = None;
    let mut positional: Vec<String> = Vec::new();
    let mut i = 0;
    let need = |i: usize| -> Result<&String, String> { args.get(i + 1).ok_or_else(|| format!("{} needs a value", args[i])) };
    while i < args.len() {
        let a = args[i].as_str();
        match a {
            "--low" => set.low.push(need(i)?.clone()),
            "--high" => set.high.push(need(i)?.clone()),
            "--cage" => set.cage = Some(need(i)?.clone()),
            "--out" => out = Some(need(i)?.clone()),
            "--depth" => depth = need(i)?.parse().map_err(|_| "bad --depth")?,
            "--preset" => preset = Some(need(i)?.clone()),
            "--set" => {
                let f = need(i)?;
                let text = fs::read_to_string(f).map_err(|e| format!("{f}: {e}"))?;
                set = BakeSet::parse("cli", &text).map_err(|e| format!("{f}: {e}"))?;
            }
            "--save-set" => save = Some(need(i)?.clone()),
            "--size" => overrides.push(("size".into(), need(i)?.replace('x', " "))),
            "--aa" | "--padding" | "--front" | "--rear" | "--maps" | "--device" => overrides.push((a[2..].into(), need(i)?.clone())),
            "--dist" => {
                overrides.push(("front".into(), need(i)?.clone()));
                overrides.push(("rear".into(), need(i)?.clone()));
            }
            "--ao-rays" => overrides.push(("ao_rays".into(), need(i)?.clone())),
            "--ao-dist" => overrides.push(("ao_dist".into(), need(i)?.clone())),
            "--set-opt" => {
                let kv = need(i)?;
                let (k, v) = kv.split_once('=').ok_or("--set-opt needs key=value")?;
                overrides.push((k.into(), v.into()));
            }
            "--directx" => {
                overrides.push(("flip_y".into(), "1".into()));
                i += 1;
                continue;
            }
            "--no-match" => {
                overrides.push(("match_by_name".into(), "0".into()));
                i += 1;
                continue;
            }
            x if x.starts_with("--") => return Err(format!("unknown option '{x}' (try: materialcraft bake --help)")),
            x => {
                positional.push(x.to_string());
                i += 1;
                continue;
            }
        }
        i += 2;
    }
    if positional.len() == 3 && set.low.is_empty() {
        set.low.push(positional[0].clone());
        set.high.push(positional[1].clone());
        out = Some(positional[2].clone());
    } else if !positional.is_empty() {
        return Err(format!("unexpected argument '{}' (try: materialcraft bake --help)", positional[0]));
    }
    if let Some(p) = preset {
        let base = match p.as_str() {
            "preview" => BakeSettings::preview(),
            "game" => BakeSettings::game(),
            "final" => BakeSettings::final_quality(),
            o => return Err(format!("unknown preset '{o}' (preview, game, final)")),
        };
        set.settings = BakeSettings { maps: set.settings.maps.clone(), ..base };
    }
    for (k, v) in &overrides {
        set.settings.set(k, v)?;
    }
    if let Some(f) = save {
        materialcraft::safeio::write_file(Path::new(&f), set.to_text().as_bytes()).map_err(|e| format!("{f}: {e}"))?;
        println!("wrote {f}");
    }
    if set.low.is_empty() {
        return Err("no low-poly mesh given (--low <file>)".into());
    }
    let out = out.ok_or("no output folder given (--out <dir>)")?;
    println!("baking {} map(s) at {}x{} px, {}x{} samples per texel ...", set.settings.maps.len(), set.settings.width, set.settings.height, set.settings.aa, set.settings.aa);
    let prog = Progress::new();
    let res = std::thread::scope(|sc| {
        let h = sc.spawn(|| bake::run_set(&set, Path::new("."), Some(&prog)));
        let mut last = String::new();
        while !h.is_finished() {
            std::thread::sleep(std::time::Duration::from_millis(250));
            let line = format!("{} {:.0}%", prog.stage(), prog.fraction() * 100.0);
            if line != last {
                eprint!("\r{line:<60}");
                last = line;
            }
        }
        eprintln!("\r{:<60}", "");
        h.join().unwrap_or_else(|_| Err("the baker stopped unexpectedly".into()))
    })?;
    fs::create_dir_all(&out).map_err(|e| e.to_string())?;
    for (name, t) in &res.maps {
        let p = Path::new(&out).join(format!("{name}.png"));
        write_png(&p, t, depth)?;
        println!("wrote {}", p.display());
    }
    let r = &res.report;
    println!(
        "{} low-poly and {} high-poly triangles, {:.1}% of the texels used, {} rays ({:.1}% of the samples hit), {:.1} s on {}",
        r.low_tris,
        r.high_tris,
        100.0 * r.covered as f32 / r.texels.max(1) as f32,
        r.rays,
        100.0 * r.hits as f32 / r.samples.max(1) as f32,
        r.seconds,
        r.device
    );
    for w in &r.warnings {
        eprintln!("warning: {w}");
    }
    Ok(())
}

fn run(args: &[String]) -> Result<(), String> {
    match args.first().map(|s| s.as_str()) {
        Some("nodes") => {
            use std::io::Write as _;
            let mut out = std::io::stdout().lock();
            for s in SPECS.iter().chain(materialcraft::func::FUNC_SPECS).chain(materialcraft::fxmap::FXM_SPECS) {
                // a closed pipe (`| head`) ends the listing quietly
                if writeln!(out, "{:<18} {:<11} inputs: {:<22} {}", s.kind, materialcraft::graph::category(s.kind), [s.req, s.opt].concat().join(","), s.doc).is_err() {
                    break;
                }
            }
            Ok(())
        }
        Some("check") => {
            let g = load(args.get(1).ok_or("usage: materialcraft check <graph.mcg>")?)?;
            println!("OK: {} nodes, {} subgraphs, {} outputs, {}x{}", g.nodes.len(), g.subgraphs.len(), g.outputs.len(), g.size, if g.is_square() { g.size } else { g.height });
            Ok(())
        }
        Some("trace") => {
            if args.len() < 4 {
                return Err("usage: materialcraft trace <graph.mcg> <node id> <out.spl> [--size N] [--threshold T] [--tolerance X]".into());
            }
            let g = load(&args[1])?;
            let id: u32 = args[2].trim_start_matches('@').parse().map_err(|_| "the node id must be a number")?;
            let (mut size, mut thr, mut tol) = (256usize, 0.5f32, 0.002f32);
            let mut i = 4;
            while i < args.len() {
                let v = args.get(i + 1).ok_or_else(|| format!("{} needs a value", args[i]))?;
                match args[i].as_str() {
                    "--size" => size = v.parse().map_err(|_| "bad --size")?,
                    "--threshold" => thr = v.parse().map_err(|_| "bad --threshold")?,
                    "--tolerance" => tol = v.parse().map_err(|_| "bad --tolerance")?,
                    o => return Err(format!("unknown option '{o}'")),
                }
                i += 2;
            }
            let t = g.evaluate_node(id, size).map_err(|e| e.to_string())?;
            let sp = materialcraft::spline::Spline::from_mask(&t, thr, tol)?;
            materialcraft::safeio::write_file(Path::new(&args[3]), sp.to_text().as_bytes()).map_err(|e| format!("{}: {e}", args[3]))?;
            println!("wrote {} paths with {} points to {}", sp.paths.len(), sp.point_count(), args[3]);
            Ok(())
        }
        Some("params") => {
            let g = load(args.get(1).ok_or("usage: materialcraft params <graph>")?)?;
            for e in &g.exposed {
                println!("\"{}\"  node {} . {}  range {}..{}", e.label, e.node, e.param, e.min, e.max);
            }
            Ok(())
        }
        Some("publish") => {
            let (i, o) = (args.get(1).ok_or("usage: materialcraft publish <in.mcg> <out.mca>")?, args.get(2).ok_or("missing <out.mca>")?);
            let g = load(i)?;
            let text = g.publish().map_err(|e| e.to_string())?;
            materialcraft::safeio::write_file(Path::new(o), text.as_bytes()).map_err(|e| e.to_string())?;
            println!("published {o} ({} bytes, {} exposed parameters)", text.len(), g.exposed.len());
            Ok(())
        }
        Some("library") => {
            let mut words: Vec<&str> = Vec::new();
            let (mut kind, mut export): (Option<materialcraft::library::AssetKind>, Option<(&str, &str)>) = (None, None);
            let mut i = 1;
            while i < args.len() {
                match args[i].as_str() {
                    "--kind" => {
                        let v = args.get(i + 1).ok_or("--kind needs a value")?;
                        kind = Some(materialcraft::library::AssetKind::parse(v).ok_or("unknown kind (use material, generator or filter)")?);
                        i += 2;
                    }
                    "--export" => {
                        let (id, file) = (args.get(i + 1).ok_or("--export needs an id and a file")?, args.get(i + 2).ok_or("--export needs a file")?);
                        export = Some((id.as_str(), file.as_str()));
                        i += 3;
                    }
                    w => {
                        words.push(w);
                        i += 1;
                    }
                }
            }
            if let Some((id, file)) = export {
                let a = materialcraft::library::find(id).ok_or_else(|| format!("no library entry '{id}' (try: materialcraft library)"))?;
                materialcraft::safeio::write_file(Path::new(file), a.source.as_bytes()).map_err(|e| format!("{file}: {e}"))?;
                println!("wrote {file} ({})", a.name);
                return Ok(());
            }
            let hits = materialcraft::library::search(&words.join(" "), kind);
            if hits.is_empty() {
                println!("nothing found");
            }
            for a in hits {
                println!("{:<24} {:<10} {:<20} {}", a.id, a.kind.label(), a.category, a.tags.join(", "));
            }
            Ok(())
        }
        Some("example") => {
            let dir = args.get(1).ok_or("usage: materialcraft example <outdir>")?;
            fs::create_dir_all(dir).map_err(|e| e.to_string())?;
            let p = Path::new(dir).join("brick_wall.mcg");
            materialcraft::safeio::write_file(&p, EXAMPLE.as_bytes()).map_err(|e| e.to_string())?;
            println!("wrote {}", p.display());
            Ok(())
        }
        Some("testmesh") => {
            let dir = args.get(1).ok_or("usage: materialcraft testmesh <outdir>")?;
            fs::create_dir_all(dir).map_err(|e| e.to_string())?;
            fs::write(Path::new(dir).join("low.obj"), bake::uv_sphere_obj(24, 16, 1.0, 0.0)).map_err(|e| e.to_string())?;
            fs::write(Path::new(dir).join("high.obj"), bake::uv_sphere_obj(160, 100, 1.0, 0.04)).map_err(|e| e.to_string())?;
            println!("wrote {dir}/low.obj and {dir}/high.obj");
            Ok(())
        }
        Some("bake") => bake_cli(&args[1..]),
        Some("render") => {
            let gp = args.get(1).ok_or("usage: materialcraft render <graph> <outdir> [options]")?;
            let dir = args.get(2).ok_or("missing <outdir>")?;
            let mut g = load(gp)?;
            let (mut sizes, mut depth, mut preview, mut r3d, mut variations) = (vec![g.size], 8u8, false, false, 1u32);
            #[allow(unused_mut, unused_variables)]
            let mut space = String::from("srgb");
            let mut params: BTreeMap<String, f32> = BTreeMap::new();
            let (mut fmt, mut quality, mut only): (Format, u8, Option<Vec<String>>) = (Format::Png, 92, None);
            let mut i = 3;
            while i < args.len() {
                match args[i].as_str() {
                    "--size" => {
                        i += 1;
                        sizes = vec![args.get(i).and_then(|s| s.parse().ok()).ok_or("--size needs a number")?];
                    }
                    "--sizes" => {
                        i += 1;
                        let v = args.get(i).ok_or("--sizes needs a list")?;
                        sizes = v.split(',').map(|s| s.trim().parse::<usize>().map_err(|_| format!("bad size '{s}'"))).collect::<Result<_, _>>()?;
                    }
                    "--depth" => {
                        i += 1;
                        depth = match args.get(i).map(|s| s.as_str()) {
                            Some("8") => 8,
                            Some("16") => 16,
                            Some("32") => 32,
                            _ => return Err("--depth must be 8, 16 or 32".into()),
                        };
                    }
                    "--param" => {
                        i += 1;
                        let kv = args.get(i).ok_or("--param needs Label=value")?;
                        let (k, v) = kv.rsplit_once('=').ok_or("--param must look like \"Label=value\"")?;
                        params.insert(k.to_string(), v.parse().map_err(|_| format!("bad value in '{kv}'"))?);
                    }
                    "--variations" => {
                        i += 1;
                        variations = args.get(i).and_then(|s| s.parse().ok()).filter(|n| (1..=1000).contains(n)).ok_or("--variations needs 1..1000")?;
                    }
                    "--format" => {
                        i += 1;
                        fmt = Format::parse(args.get(i).ok_or("--format needs png, jpg, tiff, exr or hdr")?).ok_or("--format must be png, jpg, tiff, exr or hdr")?;
                    }
                    "--quality" => {
                        i += 1;
                        quality = args.get(i).and_then(|s| s.parse().ok()).filter(|q| (1..=100).contains(q)).ok_or("--quality needs 1..100")?;
                    }
                    "--only" => {
                        i += 1;
                        only = Some(args.get(i).ok_or("--only needs a comma-separated list")?.split(',').map(|s| s.trim().to_string()).collect());
                    }
                    "--colorspace" => {
                        i += 1;
                        let v = args.get(i).ok_or("--colorspace needs a name")?;
                        #[cfg(any(feature = "cms", feature = "craft"))]
                        {
                            if materialcraft::colorspace::find(v).is_none() {
                                return Err(format!("--colorspace must be one of: {}", materialcraft::colorspace::SPACES.iter().map(|s| s.0).collect::<Vec<_>>().join(", ")));
                            }
                            space = v.to_string();
                        }
                        #[cfg(not(any(feature = "cms", feature = "craft")))]
                        return Err(format!("--colorspace {v}: this build has no colour management (build with --features cms)"));
                    }
                    "--preview" => preview = true,
                    "--render3d" => r3d = true,
                    o => return Err(format!("unknown option '{o}'")),
                }
                i += 1;
            }
            g.apply_params(&params)?;
            let base_seed = g.seed_offset;
            let multi = sizes.len() > 1;
            for var in 0..variations {
                g.seed_offset = base_seed.wrapping_add(var * 101);
                for &s in &sizes {
                    let out = g.evaluate(Some(s)).map_err(|e| e.to_string())?;
                    let mut d = Path::new(dir).to_path_buf();
                    if variations > 1 {
                        d = d.join(format!("variation_{var:03}"));
                    }
                    if multi {
                        d = d.join(format!("{s}"));
                    }
                    fs::create_dir_all(&d).map_err(|e| e.to_string())?;
                    for (name, t) in &out {
                        if only.as_ref().map(|o| !o.contains(name)).unwrap_or(false) {
                            continue;
                        }
                        let p = d.join(format!("{name}.{}", fmt.ext()));
                        #[cfg(any(feature = "cms", feature = "craft"))]
                        let conv;
                        #[cfg(any(feature = "cms", feature = "craft"))]
                        let t = if materialcraft::colorspace::is_color_output(name) {
                            conv = materialcraft::colorspace::convert(t, &space)?;
                            &conv
                        } else {
                            t
                        };
                        materialcraft::safeio::write_file(&p, &export::encode(fmt, t, if fmt == Format::Jpeg { 8 } else { depth }, quality)).map_err(|e| format!("{}: {e}", p.display()))?;
                        println!("wrote {}", p.display());
                    }
                    let flat = Tex::solid(s, s, [0.5, 0.5, 1.0, 1.0]);
                    let one = Tex::solid(s, s, [1.0; 4]);
                    let zero = Tex::solid(s, s, [0.0, 0.0, 0.0, 1.0]);
                    let half = Tex::solid(s, s, [0.5, 0.5, 0.5, 1.0]);
                    let (b, n, r, m, a) = (
                        out.get("basecolor").unwrap_or(&half),
                        out.get("normal").unwrap_or(&flat),
                        out.get("roughness").unwrap_or(&half),
                        out.get("metallic").unwrap_or(&zero),
                        out.get("ambient_occlusion").unwrap_or(&one),
                    );
                    if preview {
                        let pv = ops::lit_preview(b, n, r, m, a, [0.5, 0.5, 0.7]);
                        write_png(&d.join("preview.png"), &pv, 8)?;
                        println!("wrote {}", d.join("preview.png").display());
                    }
                    if r3d {
                        let maps = Maps { base: b, normal: n, rough: r, metal: m, ao: a, emissive: out.get("emissive"), height: out.get("height") };
                        for (label, id) in render3d::SHAPES.iter() {
                            let name: String = label.to_lowercase().chars().map(|c| if c.is_alphanumeric() { c } else { '_' }).collect();
                            let v = View { shape: *id, ..View::default() };
                            let img = render3d::render(&maps, &v, 512, 512);
                            write_png(&d.join(format!("render_{name}.png")), &img, 8)?;
                            println!("wrote {}", d.join(format!("render_{name}.png")).display());
                        }
                    }
                }
            }
            Ok(())
        }
        _ => {
            print!("{HELP}");
            Ok(())
        }
    }
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match run(&args) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("error: {e}");
            ExitCode::FAILURE
        }
    }
}
