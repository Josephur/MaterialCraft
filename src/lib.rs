// SPDX-License-Identifier: MIT OR Apache-2.0
//! MaterialCraft: procedural node-based material authoring in pure Rust.
//! The core library has no external dependencies; it builds on Windows, Linux and macOS.
pub mod bake;
pub mod export;
pub mod expr;
pub mod graph;
#[cfg(feature = "gui")]
pub mod gpu3d;
pub mod jpeg;
pub mod noise;
pub mod nodes2;
pub mod ops;
pub mod png;
pub mod render3d;
pub mod exr;
pub mod gpu;
pub mod grunge;
pub mod scatter;
pub mod tiff;
pub mod tex;
pub mod v3;
