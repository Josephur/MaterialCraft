# MaterialCraft Roadmap

MaterialCraft is a node-based procedural material editor written in pure Rust (Windows, Linux, macOS). The core library has zero dependencies; only the GUI binary uses eframe + rfd. File formats: `.mcg` (editable graph), `.mca` (published package).

Status note: everything has been built and tested on Linux only (CI for Windows, Linux and macOS is set up in `.github/workflows/ci.yml` but has not run yet). The GPU viewport has been verified on Mesa llvmpipe only, not on real GPUs.

---

## 1. Implemented (86 node types)

### Generators: noise
- White Noise
- Gaussian Noise
- Perlin Noise
- Fractal Noise (fbm, ridged, turbulence modes)
- Clouds
- Plasma
- Voronoi (distance, cell ID, edge, F2 modes)
- Crystal
- BnW Spots
- Directional Noise (seamless at any angle)
- Grunge

### Generators: shapes and gradients
- Circle, Rectangle, Polygon, Star, Shape
- Gradient (linear, radial), Axial Gradient, Circular Gradient
- Checker
- Uniform Color

### Generators: patterns
- Bricks
- Stripes (seamless at any angle)
- Weave
- Tile Sampler, Tile Generator
- Splatter, Circular Splatter

### Generators: custom
- Pixel Processor (per-pixel expression language)
- Bitmap import (PNG)

### Filters: blur and sharpen
- Blur, Blur HQ, Directional Blur, Slope Blur
- Sharpen, High Pass

### Filters: color and values
- Levels, Adjust, Invert, Threshold
- Histogram Scan, Histogram Range, Quantize, Clamp
- HSL, Tint, Gradient Map, Color Match
- To Gray, Channel, RGBA Merge

### Filters: geometry
- Transform 2D, Mirror, Swirl
- Warp, Directional Warp, Vector Warp
- Make It Tile Photo, Make It Tile Patch

### Filters: compositing and analysis
- Blend (with blend modes), Switch
- Flood Fill, Flood Fill Random, Flood Fill Color, Flood Fill Gradient
- Bevel, Distance, Edge Detect, Emboss

### Material and surface
- Normal From Height, Normal Blend
- Ambient Occlusion, Curvature, Curvature Smooth
- Height Blend
- Edge Wear, Dirt, Dust

### SDF
- SDF Shape, SDF Combine, SDF Mask, SDF 3D Form

### Graph structure
- Output (11 usages: basecolor, normal, roughness, metallic, height, ambient_occlusion, opacity, emissive, diffuse, specular, glossiness)
- Input (for subgraphs), Subgraph, Dot (link re-route)
- Comment, Frame

### Editor and workflow
- Packages workspace: any number of packages open at once, shown in the Explorer as roots (icon, italic bold name, `*` when modified) with their graphs underneath; each graph expands to list its outputs. File > New > Material Graph (Ctrl+N) adds a graph to the selected package; File > New > Empty Package creates a new unsaved package (preset chooser with graph name); File > Open adds packages without closing others. Right-click empty Explorer space: New Package / New Material Graph / Open Package(s); right-click a package: New > Material Graph, Save Package, Close Package (asks when unsaved). Graphs from any open package can be dragged onto the canvas to become nodes
- Graph nodes have one labelled output socket per output (Base Color, Normal, Roughness, ...); links remember which output they read (`in=@3.height` in the file format), sockets with links are filled orange
- Graphs as nodes: any saved graph (`.mcg`/`.mca`) can be dragged from the Explorer onto the canvas, dropped from the OS onto the window; it becomes a subgraph node (input sockets only appear if the graph defines inputs) whose exposed parameters and chosen output work like any node, so complex materials can be built from simpler ones
- Node graph with zoom, pan, rubber-band select, drag-to-connect from either end, cycle protection
- Tab search with auto-connect, undo/redo (100 steps), copy/paste (also via OS clipboard), duplicate, auto-layout
- Subgraph creation and editing, multiple graphs, exposed parameters
- Per-node property panels with color wheel and gradient editor
- Home screen with recent files, New Graph dialog, node library with thumbnails
- Resizable 2D and 3D views

### 3D viewport (GPU, OpenGL 3.3 via egui_glow)
- 13 preview objects: Cube, Cylinder, Hollow box, Inner box (seen from inside), Plane, Plane (hi-res), Sphere, Cloth, Mat ball, Rounded cube, Rounded cylinder, Sphere 2 tiles, Torus (mesh resolution up to 768 x 768; all closed shapes stay crack-free when displaced)
- True outward displacement, plus parallax relief and bump modes
- GGX PBR, mipmaps with anisotropic filtering, 4x MSAA (falls back to none), contact shadows, hue-preserving filmic tone mapping (bright oranges no longer wash out to pink), horizon-occluded reflections
- Light, camera, exposure and ambient controls, UV tiling, wireframe, normal strength; the key light follows the camera by default (Lights > Light follows the camera), a soft fill light keeps shadowed sides readable
- Studio reflection environment (softboxes, rim strip, horizon) that blurs with roughness, so roughness and metallic changes read clearly
- CPU renderer as automatic fallback (Renderer menu)
- Headless test: `cargo run --release --features gui --example gpu_shot -- out.png [shape] [relief] [w h] [nomsaa] [wire]`

### Export, files and tools
- Export to PNG, JPEG, TIFF at 8 or 16 bit
- Several resolutions at once (256 to 8192 px) and per-output selection
- `.mcg` editable graphs, `.mca` published packages with exposed parameters
- Mesh baking from OBJ (BVH): normal, world normal, AO, curvature, thickness, position, ID
- CLI: render, publish, params, bake

### Quality and resolution
- Graph size 128 to 8192 px; the editor previews at the graph's own size (2048 px by default for a 2K graph, capped at 4096 for the preview), shows a quick 256 px pass first and refines in the background. All parameters are resolution independent (checked by a test)
- Every node tiles seamlessly: centred shapes and gradients wrap around the tile border, checker/weave use even counts, flood fill regions that cross the border keep one continuous gradient, Transform 2D cross-fades only when the matrix cannot tile by itself. `tests/seam.rs` renders every node and 100+ parameter variants and fails on any visible seam
- Anti-aliased hard edges (shapes, checker, bricks), bicubic resampling in Transform/Warp, Scharr-filter normals, fast gaussian blur (cost independent of radius, multi-threaded), 4096-entry gradient table for noise, rotated fractal octaves (no visible grid), spots as a union of discs (no cell-edge cuts)
- 2D view shows true texels like Designer: crisp squares when zoomed in, mip-mapped when zoomed out. The shipped example graph is 2048 px. Maps above 4096 px are box-averaged (not point-sampled) when they go to the 3D viewport
- Responsive previews: editing cancels the running evaluation at the next node and starts the new one immediately; a 512 px pass shows the result quickly and the full size refines in the background; switching or entering a graph clears the old previews at once
- Memory: intermediate textures are freed as soon as every node that reads them has finished, so large graphs at 4K fit

### Quality checks
- 42 automated tests, including: a wrap-around check that rolls the input of 30 filters and requires the output to roll identically (proves every filter wraps at the borders), seam checks on every node, every node at four resolutions (finite, sane range, not blank), every documented parameter fuzzed with extreme values (no panics/NaN), and whole-graph import and per-output links
- `cargo run --release --example node_sheet -- out_dir` renders a PNG for every node type for visual review

---

## 2. Not implemented yet

### Graph logic
- [ ] Package context menu: Rename, Save As, Duplicate, Reveal in file explorer, Delete; graph context menu: Rename, Duplicate, Delete, Close Graph
- [ ] Several graph tabs open at once (one graph is shown at a time today)
- [ ] Imported graphs are embedded copies; no live link or "update from source" when the package file changes
- [ ] Size/format caption (e.g. 2048x2048 - C8) on graph nodes
- [ ] Function graphs (variables, math, conditionals, function node family)
- [ ] FX-Map (recursive, rule-based pattern generation)
- [ ] Pin node, Input Value node
- [ ] Non-square graphs
- [ ] Graph instances and dependency management between packages
- [ ] Graph templates, parameter presets, randomized variations

### Nodes
- [ ] Spline and path tools (shapes along curves, tracks, seams, borders)
- [ ] More of Designer's several hundred nodes, for example:
  - more weaves and tile patterns
  - Non-Uniform Blur
  - Multi-Directional Warp
  - Cells and fur patterns
  - Emboss with Gradient
  - Histogram Equalize
  - Gradient Dynamic
  - Auto Levels
  - Cube Map tools
- [ ] Built-in libraries of ready-made materials, filters and generators

### 3D and rendering
- Custom light rigs (several lights, HDR environment images), a true seamless-edge check in the 3D view
- [ ] HDRI and image-based lighting (only an analytic studio light today)
- [ ] Load custom meshes and 3D scenes into the viewport
- [ ] Path-traced rendering and post-processing (bloom, DOF, SSAO)
- [ ] GPU tessellation (displacement is per vertex on a pre-built mesh today)
- [ ] Camera and light presets, turntable rendering

### Import and color
- [ ] SVG import, font import, text nodes
- [ ] Bitmap import for JPEG, TIFF, EXR (only PNG today)
- [ ] AxF material import
- [ ] OCIO and Adobe Color Engine color management (sRGB only today)
- [ ] Linear/HDR 32-bit float output and EXR export

### Publishing and automation
- [ ] `.sbs` / `.sbsar` compatibility and Unreal / Unity plugins
- [ ] Scripting API and batch automation
- [ ] Engine-specific output templates (Unreal ORM packing and similar)
- [ ] Texture atlases for the shape/splatter workflow

### Polish and verification
- [ ] Library thumbnails are only 64 px and generated once at startup
- [ ] `.ico` resource for the Windows exe file (the window and taskbar icon already work on all platforms; the release build hides the console window on Windows)
- [ ] Visual check of the 2D/3D splitter and the export window
- [ ] Run and verify on Windows and macOS
- [ ] Test the GPU viewport on real GPUs (NVIDIA, AMD, Intel, Apple)
- [ ] CPU fallback renderer still uses the old plain environment (roughness/metallic look flatter than on the GPU viewport)
- [ ] GPU viewport: wireframe at mesh resolution 256+ renders as a solid blob; map preparation runs on the main thread

---

## 3. Suggested order

1. Function graphs + FX-Map (unlocks the most missing nodes)
2. Spline / path tools
3. HDRI image-based lighting and custom mesh loading
4. More nodes and a built-in material library
5. Color management, EXR/JPEG/TIFF import, HDR export
6. Scripting/batch API and engine output templates
