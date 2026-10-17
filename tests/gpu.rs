// SPDX-License-Identifier: MIT OR Apache-2.0
//! GPU-versus-CPU parity. Needs the `gpu` feature and an adapter (a software one is accepted here); skips otherwise.
#![cfg(feature = "gpu")]
use materialcraft::{gpu, noise, ops, tex::Tex};

fn setup() -> bool {
    std::env::set_var("MC_GPU", "force");
    gpu::set_min_pixels(0);
    gpu::adapter_name().is_some()
}

fn max_diff(a: &Tex, b: &Tex) -> f32 {
    assert_eq!((a.w, a.h), (b.w, b.h));
    a.px.iter().zip(&b.px).flat_map(|(p, q)| (0..4).map(move |c| (p[c] - q[c]).abs())).fold(0.0, f32::max)
}

#[test]
fn perlin_matches_cpu() {
    if !setup() { return; }
    let g = gpu::noise(96, 64, false, 5, 3, 1, 0.5, 7, 0).expect("gpu perlin");
    let c = Tex::gray_fn(96, 64, |u, v| noise::perlin(u, v, 5, 3, 7));
    assert!(max_diff(&g, &c) < 2e-3, "{}", max_diff(&g, &c));
}

#[test]
fn fractal_matches_cpu_in_every_mode() {
    if !setup() { return; }
    for mode in 0..3 {
        let g = gpu::noise(64, 64, true, 4, 4, 5, 0.5, 3, mode).expect("gpu fbm");
        let k = noise::Fbm::new(4, 4, 5, 0.5, 3, mode);
        let c = Tex::gray_fn(64, 64, |u, v| k.sample(u, v));
        assert!(max_diff(&g, &c) < 5e-3, "mode {mode}: {}", max_diff(&g, &c));
    }
}

#[test]
fn blur_matches_cpu() {
    if !setup() { return; }
    let src = Tex::from_fn(80, 48, |u, v| [u, v, (u * 7.0).sin() * 0.5 + 0.5, 1.0]);
    let g = gpu::blur(&src, 6.0).expect("gpu blur");
    assert!(max_diff(&g, &ops::blur(&src, 6.0)) < 1e-3);
}

#[test]
fn normal_matches_cpu() {
    if !setup() { return; }
    let src = Tex::gray_fn(64, 64, |u, v| noise::perlin(u, v, 4, 4, 1));
    for dx in [false, true] {
        let g = gpu::normal_from_height(&src, 2.0, dx).expect("gpu normal");
        assert!(max_diff(&g, &ops::normal_from_height(&src, 2.0, dx)) < 1e-3);
    }
}

#[test]
fn small_textures_stay_on_the_cpu_by_default() {
    gpu::set_min_pixels(1 << 20);
    assert!(gpu::noise(8, 8, false, 4, 4, 1, 0.5, 0, 0).is_none());
    gpu::set_min_pixels(0);
}

