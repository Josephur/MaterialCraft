// SPDX-License-Identifier: MIT OR Apache-2.0
//! Built-in library of ready-made graphs: complete materials, pattern generators and filters. Every entry is an
//! ordinary `.mcg` graph written for this project (see the `library/` folder), searchable by name, category and tag.
use crate::graph::{Error, Graph};
use crate::tex::Tex;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AssetKind {
    Material,
    Generator,
    Filter,
}

impl AssetKind {
    pub fn label(self) -> &'static str {
        match self {
            AssetKind::Material => "Material",
            AssetKind::Generator => "Generator",
            AssetKind::Filter => "Filter",
        }
    }

    pub fn parse(s: &str) -> Option<AssetKind> {
        match s.to_ascii_lowercase().as_str() {
            "material" | "materials" => Some(AssetKind::Material),
            "generator" | "generators" => Some(AssetKind::Generator),
            "filter" | "filters" => Some(AssetKind::Filter),
            _ => None,
        }
    }
}

pub struct Asset {
    /// short file-name style identifier, unique across the library
    pub id: &'static str,
    pub name: &'static str,
    pub kind: AssetKind,
    pub category: &'static str,
    pub tags: &'static [&'static str],
    pub description: &'static str,
    /// the graph in the `.mcg` text format
    pub source: &'static str,
}

macro_rules! asset {
    ($id:expr, $name:expr, $kind:expr, $cat:expr, [$($tag:expr),*], $desc:expr, $file:expr) => {
        Asset { id: $id, name: $name, kind: $kind, category: $cat, tags: &[$($tag),*], description: $desc, source: include_str!(concat!("../library/", $file)) }
    };
}

use AssetKind::{Filter as F, Generator as G, Material as M};

pub const ASSETS: &[Asset] = &[
    // ------------------------------------------------------------ materials
    asset!("cobblestone", "Cobblestone", M, "Stone", ["street", "paving", "road", "rock", "outdoor"], "Rounded paving stones with dark gaps.", "materials/cobblestone.mcg"),
    asset!("mossy_stone", "Mossy Stone", M, "Stone", ["rock", "moss", "wall", "wet", "outdoor", "ruin"], "Rough stones with moss growing in the cavities.", "materials/mossy_stone.mcg"),
    asset!("marble", "Marble", M, "Stone", ["polished", "veins", "floor", "white", "interior"], "Polished marble with thin dark veins.", "materials/marble.mcg"),
    asset!("concrete", "Concrete", M, "Stone", ["cement", "poured", "pores", "wall", "floor", "industrial"], "Poured concrete with pores and soft mottling.", "materials/concrete.mcg"),
    asset!("floor_tiles", "Floor Tiles", M, "Tiles", ["ceramic", "glazed", "grout", "bathroom", "kitchen", "interior"], "Glazed ceramic tiles with grout lines.", "materials/floor_tiles.mcg"),
    asset!("pool_caustics", "Pool Floor", M, "Tiles", ["water", "caustics", "swimming", "blue", "underwater"], "Small pool tiles lit by water caustics.", "materials/pool_caustics.mcg"),
    asset!("wood_planks", "Wood Planks", M, "Wood", ["floor", "boards", "grain", "timber", "interior"], "Brown planks with grain running along their length.", "materials/wood_planks.mcg"),
    asset!("rusty_metal", "Rusty Metal", M, "Metal", ["steel", "rust", "corroded", "scratches", "industrial", "old"], "Bare steel with spreading rust and fine scratches.", "materials/rusty_metal.mcg"),
    asset!("chipped_paint", "Chipped Paint", M, "Metal", ["painted", "peeling", "blue", "steel", "worn"], "Painted steel where the paint has chipped away.", "materials/chipped_paint.mcg"),
    asset!("chain_link_fence", "Chain-Link Fence", M, "Metal", ["fence", "wire", "mesh", "galvanised", "opacity", "outdoor"], "Galvanised diamond mesh with an opacity mask.", "materials/chain_link_fence.mcg"),
    asset!("leather", "Leather", M, "Fabric and leather", ["grain", "brown", "hide", "furniture", "organic"], "Fine-grained brown leather.", "materials/leather.mcg"),
    asset!("stitched_leather", "Stitched Leather", M, "Fabric and leather", ["seam", "stitches", "thread", "brown", "furniture", "path"], "Grained leather with a stitched seam along a path.", "materials/stitched_leather.mcg"),
    asset!("woven_fabric", "Woven Fabric", M, "Fabric and leather", ["cloth", "textile", "weave", "cotton", "blue"], "Plain-weave cloth with fuzzy fibres.", "materials/woven_fabric.mcg"),
    asset!("sand_ripples", "Sand Ripples", M, "Ground", ["dune", "desert", "beach", "wind", "outdoor", "terrain"], "Wind-blown sand with soft ripples.", "materials/sand_ripples.mcg"),
    asset!("dry_mud", "Dry Mud", M, "Ground", ["cracked", "earth", "drought", "soil", "terrain", "outdoor"], "Dried mud broken into plates by cracks.", "materials/dry_mud.mcg"),
    // ------------------------------------------------------------ generators
    asset!("wood_grain", "Wood Grain", G, "Natural", ["wood", "rings", "lines", "organic"], "Grayscale wood grain running vertically.", "generators/wood_grain.mcg"),
    asset!("crumpled_paper", "Crumpled Paper", G, "Natural", ["paper", "folds", "creases", "foil", "height"], "Sharp creases over soft folds, with a normal map.", "generators/crumpled_paper.mcg"),
    asset!("stone_blocks", "Stone Blocks", G, "Architecture", ["bricks", "masonry", "wall", "blocks", "ids"], "Offset stone block courses with height, normal and block ids.", "generators/stone_blocks.mcg"),
    asset!("scratch_overlay", "Scratch Overlay", G, "Wear", ["scratches", "metal", "mask", "damage", "wear"], "Fine, medium and deep scratches as a mask.", "generators/scratch_overlay.mcg"),
    asset!("rain_streaks", "Rain Streaks", G, "Wear", ["weathering", "streaks", "drips", "dirt", "mask", "stains"], "Vertical weathering streaks that fade towards the bottom.", "generators/rain_streaks.mcg"),
    asset!("speckle_dust", "Speckle Dust", G, "Wear", ["dust", "speckle", "dirt", "grain", "mask"], "Fine speckles under a loose, uneven coat of dust.", "generators/speckle_dust.mcg"),
    asset!("rope", "Rope", G, "Paths", ["cable", "twisted", "strands", "braid", "path", "height"], "Three-strand rope along a wavy path, with strand ids and normal.", "generators/rope.mcg"),
    asset!("railway_track", "Railway Track", G, "Paths", ["rails", "sleepers", "gravel", "ballast", "train", "path"], "Rails and sleepers on gravel, with masks for each part.", "generators/railway_track.mcg"),
    asset!("beaded_chain", "Beaded Chain", G, "Paths", ["beads", "pearls", "string", "necklace", "scatter", "path"], "Round beads threaded on a wavy string.", "generators/beaded_chain.mcg"),
    asset!("bevel_border", "Bevel Border", G, "Paths", ["frame", "border", "bevel", "edge", "closed", "path"], "Rounded bevel frame inside a closed shape, with a frame mask.", "generators/bevel_border.mcg"),
    // ------------------------------------------------------------ filters
    asset!("height_to_pbr", "Height To PBR", F, "Material utilities", ["normal", "ao", "curvature", "occlusion", "height"], "Normal map, ambient occlusion and curvature from a height map.", "filters/height_to_pbr.mcg"),
    asset!("edge_wear_mask", "Edge Wear Mask", F, "Material utilities", ["edges", "wear", "damage", "chipped", "mask", "height"], "Worn-edge mask from a height map, broken up by noise.", "filters/edge_wear_mask.mcg"),
    asset!("dust_on_top", "Dust On Top", F, "Material utilities", ["dust", "settled", "cavity", "mask", "height"], "Dust that settles on flat tops and in cavities.", "filters/dust_on_top.mcg"),
    asset!("make_tileable", "Make Tileable", F, "Photo tools", ["photo", "seamless", "tile", "repeat"], "Turns a photo into a seamless tile.", "filters/make_tileable.mcg"),
    asset!("region_color_variation", "Region Colour Variation", F, "Photo tools", ["random", "tint", "hue", "bricks", "tiles", "variation", "mask"], "Random brightness and hue for every region of a mask.", "filters/region_color_variation.mcg"),
];

pub fn assets() -> &'static [Asset] {
    ASSETS
}

pub fn find(id: &str) -> Option<&'static Asset> {
    let id = id.trim().to_ascii_lowercase().replace([' ', '-'], "_");
    ASSETS.iter().find(|a| a.id == id)
}

/// Assets that match every word of `query` (in the name, id, category, a tag or the description, ignoring case),
/// optionally limited to one kind. An empty query matches everything. Name matches come first.
pub fn search(query: &str, kind: Option<AssetKind>) -> Vec<&'static Asset> {
    let words: Vec<String> = query.split_whitespace().map(|w| w.to_ascii_lowercase()).collect();
    let mut hits: Vec<(u32, &'static Asset)> = Vec::new();
    for a in ASSETS {
        if kind.map_or(false, |k| k != a.kind) {
            continue;
        }
        let (name, cat, desc) = (a.name.to_ascii_lowercase(), a.category.to_ascii_lowercase(), a.description.to_ascii_lowercase());
        let mut score = 0u32;
        let mut all = true;
        for w in &words {
            let s = if name.contains(w.as_str()) || a.id.contains(w.as_str()) {
                4
            } else if a.tags.iter().any(|t| t.contains(w.as_str())) {
                3
            } else if cat.contains(w.as_str()) {
                2
            } else if desc.contains(w.as_str()) {
                1
            } else {
                0
            };
            if s == 0 {
                all = false;
                break;
            }
            score += s;
        }
        if all {
            hits.push((score, a));
        }
    }
    hits.sort_by(|a, b| b.0.cmp(&a.0));
    hits.into_iter().map(|h| h.1).collect()
}

/// Categories of one kind, in library order.
pub fn categories(kind: AssetKind) -> Vec<&'static str> {
    let mut out: Vec<&'static str> = Vec::new();
    for a in ASSETS.iter().filter(|a| a.kind == kind) {
        if !out.contains(&a.category) {
            out.push(a.category);
        }
    }
    out
}

impl Asset {
    /// The graph, ready to edit: legacy outputs migrated to output nodes.
    pub fn graph(&self) -> Result<Graph, Error> {
        let mut g = Graph::parse_unchecked(self.source)?;
        g.migrate_outputs();
        Ok(g)
    }

    /// Small preview image: the base colour of a material, otherwise the first (or only) output.
    pub fn thumbnail(&self, size: usize) -> Result<Tex, Error> {
        let g = self.graph()?;
        let mut out = g.evaluate(Some(size.clamp(8, 1024)))?;
        for k in ["basecolor", "output", "height"] {
            if let Some(t) = out.remove(k) {
                return Ok(t);
            }
        }
        out.into_iter().next().map(|(_, t)| t).ok_or(Error::NoOutputs)
    }
}
