# MaterialCraft - handoff notes

Pure-Rust node-based procedural material editor (engine + egui editor). User is on Windows; everything so far was only
built/tested on Linux (Rust 1.91, eframe/egui 0.29, rfd 0.15). Never run on Windows yet.

## Build / run
    cargo test                                                    # 22 tests, all pass
    cargo run --release --features gui --bin materialcraft-gui    # editor
    cargo run --release -- render graph.mcg out --size 1024 --render3d [--format png|jpg|tiff] [--quality 92] [--only basecolor,normal]
Files: .mcg = editable graph, .mca = published package (old .mfg/.mfa still open). Config/recent files live in the
OS config dir under "MaterialCraft". Core library has zero dependencies; only the GUI binary uses eframe + rfd.

## Source map (what each file does)
- src/graph.rs        graph model, text format, SPECS (all node types), compute() for every node, subgraphs, output nodes,
                      packages (.mca), exposed params, demo_graph_for() (used for library thumbnails + smoke test)
- src/ops.rs          image operators (blend, blur, warps, flood fill, bevel/distance, tile grid, SDF, color match, ...)
- src/noise.rs        seeded tileable noise, voronoi (+voronoi_ext)
- src/expr.rs         expression language for the Pixel Processor node
- src/png.rs          PNG encoder + decoder (palette/tRNS, 1-16 bit, Adam7)
- src/jpeg.rs, tiff.rs, export.rs   JPEG (baseline) and TIFF (8/16-bit) encoders + Format enum  [new this session]
- src/bake.rs         OBJ import + BVH baker (normal, AO, curvature, thickness, position, ID)
- src/render3d.rs     CPU PBR renderer: sphere/cube/plane, GGX, normal map, height parallax + bump, fill light, pan  [rewritten]
- src/main.rs         CLI (render, publish, params, bake, ...)
- src/gui.rs          App state, entries(), spawn, copy (writes nodes to OS clipboard as text), export run, eval jobs, logo module
- src/gui_ui.rs       graph canvas (cards, ports, reverse connect, rubber band), actions, Tab search popup, Export/Bake windows
- src/gui_home.rs     Home screen, New Graph dialog, Library, Explorer, 2D/3D panels, editor layout + splitters, update loop,
                      logo/window-icon loading + built-in fallback squid logo, main()
- src/gui_params.rs   per-node parameter schema (pdefs), color wheel, gradient editor, node/graph properties panels
  (gui.rs textually include!()s gui_params.rs, gui_ui.rs, gui_home.rs - they share one scope)
- build.rs + assets/README.txt   embeds assets/MaterialCraftLogo.png when present
- tests/core.rs       all tests; examples/enc.rs, examples/h3d.rs are throwaway dev helpers

## Done this session (user's 8 requests)
1. Logo: pipeline done (build.rs embeds assets/MaterialCraftLogo.png, runtime search fallback, window icon, home logo,
   built-in purple-squid fallback). THE USER'S REAL SQUID FILE WAS NEVER RECEIVED (only a title-bar crop came through).
   Not verified with the real file. Exe icon in Windows Explorer (.ico resource) NOT done.
2. 2D/3D views resizable sideways: implemented (draggable divider in editor_ui). *** UNVERIFIED / PROBABLY BUGGY: my one
   automated drag test did not move the divider. Check first. ***
3. Drag from an input port out and connect to an output port (or release on empty space -> search -> auto-connect): DONE, tested.
4. Ctrl+C / Ctrl+V: DONE, tested (nodes are copied to the OS clipboard as text, so it also works across windows).
5. Height map now shows in 3D: parallax relief + bump (View.height_scale, View.bump; sliders in 3D > Materials). Renderer
   verified with a CLI contact sheet; the GUI sliders themselves not yet looked at.
6. Dark underside fixed (brighter ground + fill light, Ambient slider). JPEG + TIFF export written and verified against a
   real decoder (JPEG mean error 0.75/255, 8-bit TIFF pixel-exact). CLI has --format/--quality.
7. Export window: format (PNG/JPEG/TIFF), JPEG quality, 16-bit, resolutions, per-output checkboxes (skip set). Code written;
   window not visually tested.
8. Alt+drag (or middle-drag) pans the object in the 3D view: implemented, untested (openbox WM swallows Alt+drag in my rig).
Earlier this session: panel-resize snap-back bug fixed (own drag handles), Output nodes drive the 3D view, color wheel +
grayscale on Uniform Color, gradient-map editor, scrollable Tab menu, ~86 nodes incl. all priority nodes, property panels per node.

## Next chat: start here
A. Verify/fix the unverified items above, in this order: (2) 3D/2D splitter, (7) Export window UI (try JPEG + TIFF + unchecking
   outputs), (5) height depth/bump sliders, (8) Alt+drag pan on Windows, (1) drop the user's MaterialCraftLogo.png in
   assets/ (ask them to upload it in chat) and confirm window/taskbar/home logo; consider embedding an .ico into the exe.
B. Windows pass: build + run on Windows, check rfd dialogs, DPI, Ctrl shortcuts, clipboard.
C. Visually review the ~35 new nodes (only smoke-tested for NaN/errors): tile sampler/generator, splatter circular, crystal,
   bnw spots, flood fill family, slope blur, vector warp, bevel, distance, height blend, make-it-tile photo/patch.
D. Not built yet: function nodes + FxMap, spline/path tools, Pin node, Input Value, non-square graphs, true displacement
   (only parallax), HDRI/environment maps, loading custom meshes into the 3D view, SVG/font import, OCIO/ACE color management,
   scripting API, PBR material packs, mesh-baker GUI polish. Home "Learn" page is minimal.
E. Known rough edges: library thumbnails are 64px and generated once at startup; 'Frame'/'Comment' use the same frame data;
   Directional noise/Stripes only tile at 0/90 degrees; blur-type radii are in 512px units (resolution independent).
