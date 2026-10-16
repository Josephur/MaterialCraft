# MaterialCraft Roadmap

MaterialCraft is a node-based procedural material editor written in pure Rust (Windows, Linux, macOS). The core library has zero dependencies; only the GUI binary uses eframe + rfd. File formats: `.mcg` (editable graph), `.mca` (published package).

Status note: everything has been built and tested on Linux only. Windows and macOS have not been run yet. The GPU viewport has been verified on Mesa llvmpipe only, not on real GPUs.

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
- Node graph with zoom, pan, rubber-band select, drag-to-connect from either end, cycle protection
- Tab search with auto-connect, undo/redo (100 steps), copy/paste (also via OS clipboard), duplicate, auto-layout
- Subgraph creation and editing, multiple graphs, exposed parameters
- Per-node property panels with color wheel and gradient editor
- Home screen with recent files, New Graph dialog, node library with thumbnails
- Resizable 2D and 3D views

### 3D viewport (GPU, OpenGL 3.3 via egui_glow)
- Sphere, cube and high-resolution plane (mesh resolution up to 768 x 768)
- True outward displacement, plus parallax relief and bump modes
- GGX PBR, mipmaps with anisotropic filtering, 4x MSAA (falls back to none), contact shadows, ACES tone mapping
- Light, camera, exposure and ambient controls, UV tiling, wireframe, normal strength
- CPU renderer as automatic fallback (Renderer menu)
- Headless test: `cargo run --release --features gui --example gpu_shot -- out.png [shape] [relief] [w h] [nomsaa] [wire]`

### Export, files and tools
- Export to PNG, JPEG, TIFF at 8 or 16 bit
- Several resolutions at once (256 to 4096 px) and per-output selection
- `.mcg` editable graphs, `.mca` published packages with exposed parameters
- Mesh baking from OBJ (BVH): normal, world normal, AO, curvature, thickness, position, ID
- CLI: render, publish, params, bake

---

## 2. Not implemented yet

### Graph logic
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
- [ ] `.ico` resource for the Windows exe
- [ ] Visual check of the 2D/3D splitter and the export window
- [ ] Run and verify on Windows and macOS
- [ ] Test the GPU viewport on real GPUs (NVIDIA, AMD, Intel, Apple)
- [ ] GPU viewport: wireframe at mesh resolution 256+ renders as a solid blob; map preparation runs on the main thread

---

## 3. Suggested order

1. Function graphs + FX-Map (unlocks the most missing nodes)
2. Spline / path tools
3. HDRI image-based lighting and custom mesh loading
4. More nodes and a built-in material library
5. Color management, EXR/JPEG/TIFF import, HDR export
6. Scripting/batch API and engine output templates
