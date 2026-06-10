//! Liquid-glass lens optics.
//!
//! The lens is modeled the way Apple's Liquid Glass material works under the
//! hood: a glass slab whose top surface has a flat center and smoothly rounded
//! edges. For every covered pixel we:
//!
//! 1. evaluate a height (thickness) profile from the distance to the center,
//! 2. derive the surface normal from the profile's gradient,
//! 3. refract a top-down light ray through the surface with Snell's law and
//!    displace the background sample by where that ray lands,
//! 4. repeat per color channel with slightly different indices of refraction
//!    (dispersion) to get edge-only chromatic aberration,
//! 5. add Blinn-Phong specular and a Fresnel-style rim highlight,
//! 6. optionally frost (blur), magnify, and tint the result.
//!
//! Everything is parameterized through [`GlassParams`] / [`PARAMS`] so the UI
//! can expose every knob generically.

use crate::framebuffer::{Framebuffer, Rgb};

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct GlassParams {
    /// Lens radius in pixels.
    pub radius: f32,
    /// Glass thickness in pixels; scales the refraction displacement.
    pub depth: f32,
    /// Index of refraction (1.0 = no bending, ~1.5 = glass).
    pub ior: f32,
    /// Fraction of the radius that is optically flat (the slab "top").
    pub flatness: f32,
    /// Dispersion: per-channel IOR spread producing chromatic aberration.
    pub chroma: f32,
    /// Specular / rim highlight strength.
    pub specular: f32,
    /// Frosted-glass blur radius in pixels.
    pub frost: f32,
    /// Center magnification factor.
    pub magnify: f32,
    /// Glass tint: brightens and washes out the material slightly.
    pub tint: f32,
}

impl Default for GlassParams {
    fn default() -> Self {
        Self {
            radius: 24.0,
            depth: 12.0,
            ior: 1.5,
            flatness: 0.4,
            chroma: 0.04,
            specular: 1.0,
            frost: 0.0,
            magnify: 1.2,
            tint: 0.25,
        }
    }
}

/// Description of a tunable parameter, for generic UI editing.
pub struct ParamSpec {
    pub name: &'static str,
    pub min: f32,
    pub max: f32,
    pub step: f32,
    pub get: fn(&GlassParams) -> f32,
    pub set: fn(&mut GlassParams, f32),
}

impl ParamSpec {
    pub fn adjust(&self, params: &mut GlassParams, steps: f32) {
        let v = ((self.get)(params) + self.step * steps).clamp(self.min, self.max);
        (self.set)(params, v);
    }

    pub fn set_normalized(&self, params: &mut GlassParams, t: f32) {
        let t = t.clamp(0.0, 1.0);
        let raw = self.min + (self.max - self.min) * t;
        let snapped = (raw / self.step).round() * self.step;
        (self.set)(params, snapped.clamp(self.min, self.max));
    }

    pub fn normalized(&self, params: &GlassParams) -> f32 {
        let v = (self.get)(params);
        ((v - self.min) / (self.max - self.min)).clamp(0.0, 1.0)
    }
}

pub const PARAMS: &[ParamSpec] = &[
    ParamSpec {
        name: "Radius",
        min: 6.0,
        max: 100.0,
        step: 2.0,
        get: |p| p.radius,
        set: |p, v| p.radius = v,
    },
    ParamSpec {
        name: "Depth",
        min: 0.0,
        max: 40.0,
        step: 1.0,
        get: |p| p.depth,
        set: |p, v| p.depth = v,
    },
    ParamSpec {
        name: "Distortion",
        min: 1.0,
        max: 2.5,
        step: 0.05,
        get: |p| p.ior,
        set: |p, v| p.ior = v,
    },
    ParamSpec {
        name: "Flatness",
        min: 0.0,
        max: 0.9,
        step: 0.05,
        get: |p| p.flatness,
        set: |p, v| p.flatness = v,
    },
    ParamSpec {
        name: "Chroma",
        min: 0.0,
        max: 0.3,
        step: 0.01,
        get: |p| p.chroma,
        set: |p, v| p.chroma = v,
    },
    ParamSpec {
        name: "Specular",
        min: 0.0,
        max: 2.0,
        step: 0.1,
        get: |p| p.specular,
        set: |p, v| p.specular = v,
    },
    ParamSpec {
        name: "Frost",
        min: 0.0,
        max: 3.0,
        step: 0.1,
        get: |p| p.frost,
        set: |p, v| p.frost = v,
    },
    ParamSpec {
        name: "Magnify",
        min: 0.5,
        max: 3.0,
        step: 0.05,
        get: |p| p.magnify,
        set: |p, v| p.magnify = v,
    },
    ParamSpec {
        name: "Tint",
        min: 0.0,
        max: 1.0,
        step: 0.05,
        get: |p| p.tint,
        set: |p, v| p.tint = v,
    },
];

type Vec3 = [f32; 3];

fn normalize(v: Vec3) -> Vec3 {
    let len = (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt();
    if len <= 1e-9 {
        [0.0, 0.0, 1.0]
    } else {
        [v[0] / len, v[1] / len, v[2] / len]
    }
}

fn dot(a: Vec3, b: Vec3) -> f32 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

/// Normalized height profile of the lens top surface.
///
/// `d` is the normalized distance from the lens center (0..1). Returns the
/// height in 0..1: flat (1.0) inside `flatness`, then a quarter-ellipse
/// falloff to 0 at the edge — the classic "slab with rounded edges" shape.
pub fn height_profile(d: f32, flatness: f32) -> f32 {
    let f = flatness.clamp(0.0, 0.95);
    let d = d.clamp(0.0, 1.0);
    if d <= f {
        1.0
    } else {
        let t = ((d - f) / (1.0 - f)).min(1.0);
        (1.0 - t * t).max(0.0).sqrt()
    }
}

/// Derivative of `height_profile` with respect to `d` (clamped to avoid the
/// vertical-tangent singularity at the very edge).
pub fn height_slope(d: f32, flatness: f32) -> f32 {
    let f = flatness.clamp(0.0, 0.95);
    let d = d.clamp(0.0, 1.0);
    if d <= f {
        0.0
    } else {
        let t = ((d - f) / (1.0 - f)).clamp(0.0, 0.999);
        -t / ((1.0 - f) * (1.0 - t * t).sqrt())
    }
}

/// Surface normal of the lens at offset (`dx`, `dy`) px from the center.
pub fn surface_normal(dx: f32, dy: f32, params: &GlassParams) -> Vec3 {
    let r = (dx * dx + dy * dy).sqrt();
    if r <= 1e-6 || params.radius <= 1e-6 {
        return [0.0, 0.0, 1.0];
    }
    let d = (r / params.radius).min(1.0);
    // Height in pixels is h(d) * depth; slope per pixel of radius:
    let slope_px = height_slope(d, params.flatness) * params.depth / params.radius;
    let (ux, uy) = (dx / r, dy / r);
    normalize([-slope_px * ux, -slope_px * uy, 1.0])
}

/// Snell-law refraction of direction `incident` at surface normal `n`,
/// with `eta` = n1/n2. Returns `None` on total internal reflection.
pub fn refract(incident: Vec3, n: Vec3, eta: f32) -> Option<Vec3> {
    let cosi = -dot(incident, n);
    let k = 1.0 - eta * eta * (1.0 - cosi * cosi);
    if k < 0.0 {
        None
    } else {
        let ks = k.sqrt();
        Some(normalize([
            eta * incident[0] + (eta * cosi - ks) * n[0],
            eta * incident[1] + (eta * cosi - ks) * n[1],
            eta * incident[2] + (eta * cosi - ks) * n[2],
        ]))
    }
}

/// Where a top-down ray entering the glass at (`dx`, `dy`) lands on the
/// background, expressed as an (x, y) pixel offset from the entry point.
pub fn refraction_offset(dx: f32, dy: f32, params: &GlassParams, ior: f32) -> (f32, f32) {
    let n = surface_normal(dx, dy, params);
    let eta = 1.0 / ior.max(1.0);
    let t = match refract([0.0, 0.0, -1.0], n, eta) {
        Some(t) => t,
        None => return (0.0, 0.0),
    };
    if t[2] >= -1e-6 {
        return (0.0, 0.0);
    }
    let r = (dx * dx + dy * dy).sqrt();
    let d = if params.radius > 0.0 {
        (r / params.radius).min(1.0)
    } else {
        1.0
    };
    // Ray travels down through the local glass thickness to the flat bottom.
    let travel = height_profile(d, params.flatness) * params.depth;
    (t[0] / -t[2] * travel, t[1] / -t[2] * travel)
}

/// Per-pixel shading info for a point under the lens.
#[derive(Clone, Copy, Debug)]
pub struct LensSample {
    /// Background sample offsets per channel (R, G, B), in pixels.
    pub offset: [(f32, f32); 3],
    /// Additive specular highlight intensity, 0..~1.
    pub specular: f32,
    /// Fresnel-ish rim factor (1 at steep edges, 0 on the flat top).
    pub rim: f32,
    /// Anti-aliasing blend: 1 = fully glass, 0 = background.
    pub coverage: f32,
}

const LIGHT_MAIN: Vec3 = [-0.45, -0.70, 0.56];
const LIGHT_FILL: Vec3 = [0.45, 0.70, 0.56];

/// Evaluate the lens at offset (`dx`, `dy`) px from its center.
/// Returns `None` outside the lens.
pub fn sample(dx: f32, dy: f32, params: &GlassParams) -> Option<LensSample> {
    let r = (dx * dx + dy * dy).sqrt();
    if r > params.radius {
        return None;
    }

    let iors = [
        (params.ior - params.chroma).max(1.0),
        params.ior.max(1.0),
        (params.ior + params.chroma).max(1.0),
    ];
    let mut offset = [(0.0, 0.0); 3];
    for (i, ior) in iors.iter().enumerate() {
        offset[i] = refraction_offset(dx, dy, params, *ior);
        // With no dispersion, all channels are identical; skip recompute.
        if params.chroma == 0.0 {
            offset = [offset[0]; 3];
            break;
        }
    }

    let n = surface_normal(dx, dy, params);
    let view: Vec3 = [0.0, 0.0, 1.0];
    let mut spec = 0.0;
    for (light, weight) in [(LIGHT_MAIN, 1.0f32), (LIGHT_FILL, 0.45)] {
        let l = normalize(light);
        let h = normalize([l[0] + view[0], l[1] + view[1], l[2] + view[2]]);
        spec += dot(n, h).max(0.0).powf(48.0) * weight;
    }
    let rim = (1.0 - n[2]).clamp(0.0, 1.0).powf(0.75);

    let coverage = ((params.radius - r) / 1.5).clamp(0.0, 1.0);

    Some(LensSample {
        offset,
        specular: spec * params.specular,
        rim,
        coverage,
    })
}

fn sample_channel(src: &Framebuffer, fx: f32, fy: f32, frost: f32, channel: usize) -> f32 {
    let pick = |c: (f32, f32, f32)| match channel {
        0 => c.0,
        1 => c.1,
        _ => c.2,
    };
    if frost <= 0.01 {
        return pick(src.sample_bilinear(fx, fy));
    }
    let k = frost;
    let taps = [
        (0.0, 0.0),
        (k, k),
        (-k, k),
        (k, -k),
        (-k, -k),
        (1.6 * k, 0.0),
        (-1.6 * k, 0.0),
        (0.0, 1.6 * k),
        (0.0, -1.6 * k),
    ];
    let sum: f32 = taps
        .iter()
        .map(|(ox, oy)| pick(src.sample_bilinear(fx + ox, fy + oy)))
        .sum();
    sum / taps.len() as f32
}

/// Composite the lens over `src` into `dst` with its center at (`cx`, `cy`).
/// Pixels outside the lens are an untouched copy of `src`.
pub fn apply(src: &Framebuffer, dst: &mut Framebuffer, cx: f32, cy: f32, params: &GlassParams) {
    dst.copy_from(src);
    let r = params.radius.max(0.0);
    let x0 = (cx - r).floor().max(0.0) as usize;
    let x1 = ((cx + r).ceil() as usize).min(src.width().saturating_sub(1));
    let y0 = (cy - r).floor().max(0.0) as usize;
    let y1 = ((cy + r).ceil() as usize).min(src.height().saturating_sub(1));
    if src.width() == 0 || src.height() == 0 {
        return;
    }

    let mag = params.magnify.max(0.05);
    let tint = params.tint.clamp(0.0, 1.0);

    for y in y0..=y1 {
        for x in x0..=x1 {
            let px = x as f32 + 0.5;
            let py = y as f32 + 0.5;
            let dx = px - cx;
            let dy = py - cy;
            let Some(s) = sample(dx, dy, params) else {
                continue;
            };

            // Magnified base position plus per-channel refraction offset.
            let bx = cx + dx / mag;
            let by = cy + dy / mag;
            let mut rgb = [0.0f32; 3];
            for (i, v) in rgb.iter_mut().enumerate() {
                let (ox, oy) = s.offset[i];
                *v = sample_channel(src, bx + ox, by + oy, params.frost, i);
            }

            // Glass material: slight brighten + wash-out, then highlights.
            for v in rgb.iter_mut() {
                *v = *v * (1.0 - 0.25 * tint) + 255.0 * 0.16 * tint;
                *v += s.specular * 235.0;
                *v += s.rim * s.specular.min(1.0) * 26.0;
            }

            let glass = Rgb::from_f32(rgb[0], rgb[1], rgb[2]);
            let base = dst.get(x as isize, y as isize);
            dst.set(x, y, base.lerp(glass, s.coverage));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn close(a: f32, b: f32, eps: f32) -> bool {
        (a - b).abs() <= eps
    }

    #[test]
    fn height_profile_endpoints() {
        assert_eq!(height_profile(0.0, 0.4), 1.0);
        assert_eq!(height_profile(0.3, 0.4), 1.0); // flat region
        assert!(height_profile(1.0, 0.4) <= 1e-3);
    }

    #[test]
    fn height_profile_monotonic_after_flat() {
        let mut prev = f32::INFINITY;
        for i in 0..=100 {
            let d = i as f32 / 100.0;
            let h = height_profile(d, 0.4);
            assert!(h <= prev + 1e-6, "profile must not increase");
            prev = h;
        }
    }

    #[test]
    fn slope_zero_on_flat_negative_on_curve() {
        assert_eq!(height_slope(0.2, 0.4), 0.0);
        assert!(height_slope(0.7, 0.4) < 0.0);
        assert!(height_slope(0.99, 0.4) < height_slope(0.7, 0.4));
        assert!(height_slope(1.0, 0.4).is_finite());
    }

    #[test]
    fn normal_at_center_points_up() {
        let p = GlassParams::default();
        assert_eq!(surface_normal(0.0, 0.0, &p), [0.0, 0.0, 1.0]);
    }

    #[test]
    fn normal_is_unit_length_and_tilts_outward() {
        let p = GlassParams::default();
        let n = surface_normal(p.radius * 0.9, 0.0, &p);
        let len = (n[0] * n[0] + n[1] * n[1] + n[2] * n[2]).sqrt();
        assert!(close(len, 1.0, 1e-5));
        assert!(n[0] > 0.0, "edge normal should tilt outward (+x)");
        assert!(n[2] > 0.0);
    }

    #[test]
    fn refract_straight_through_flat_surface() {
        let t = refract([0.0, 0.0, -1.0], [0.0, 0.0, 1.0], 1.0 / 1.5).unwrap();
        assert!(close(t[0], 0.0, 1e-6));
        assert!(close(t[1], 0.0, 1e-6));
        assert!(close(t[2], -1.0, 1e-6));
    }

    #[test]
    fn refract_bends_toward_normal_entering_denser_medium() {
        // Tilted normal, entering glass: refracted ray bends toward the normal,
        // i.e. is less steep relative to it than the incident ray.
        let n = normalize([0.3, 0.0, 1.0]);
        let i = [0.0, 0.0, -1.0];
        let t = refract(i, n, 1.0 / 1.5).unwrap();
        let sin_i = (1.0f32 - dot(i, n).powi(2)).sqrt();
        let sin_t = (1.0f32 - dot(t, n).powi(2)).sqrt();
        assert!(close(sin_t, sin_i / 1.5, 1e-4), "Snell's law must hold");
    }

    #[test]
    fn total_internal_reflection_returns_none() {
        // Going from dense to thin at a grazing angle.
        let n = [0.0, 0.0, 1.0];
        let i = normalize([0.95, 0.0, -0.31]);
        assert!(refract(i, n, 1.5).is_none());
    }

    #[test]
    fn no_offset_at_center_or_with_unit_ior() {
        let p = GlassParams::default();
        let (ox, oy) = refraction_offset(0.0, 0.0, &p, p.ior);
        assert!(close(ox, 0.0, 1e-5) && close(oy, 0.0, 1e-5));

        let (ox, oy) = refraction_offset(p.radius * 0.8, 0.0, &p, 1.0);
        assert!(close(ox, 0.0, 1e-4) && close(oy, 0.0, 1e-4));
    }

    #[test]
    fn offset_grows_toward_edge_and_with_depth() {
        let p = GlassParams {
            flatness: 0.3,
            ..Default::default()
        };
        let (mid, _) = refraction_offset(p.radius * 0.6, 0.0, &p, p.ior);
        let (edge, _) = refraction_offset(p.radius * 0.92, 0.0, &p, p.ior);
        assert!(edge.abs() > mid.abs(), "edge {edge} vs mid {mid}");

        let deep = GlassParams { depth: 30.0, ..p };
        let (mid_deep, _) = refraction_offset(p.radius * 0.6, 0.0, &deep, p.ior);
        assert!(mid_deep.abs() > mid.abs());
    }

    #[test]
    fn offset_points_inward() {
        // A converging lens edge bends light toward the center: the sampled
        // background point lies further out than the pixel (offset is +x for
        // a +x pixel... actually the refracted ray tilts toward the center).
        let p = GlassParams {
            flatness: 0.3,
            ..Default::default()
        };
        let (ox, _) = refraction_offset(p.radius * 0.85, 0.0, &p, p.ior);
        // Normal tilts +x, incident straight down, so refracted ray tilts -x:
        assert!(ox < 0.0, "expected inward bend, got {ox}");
    }

    #[test]
    fn sample_outside_lens_is_none() {
        let p = GlassParams::default();
        assert!(sample(p.radius + 1.0, 0.0, &p).is_none());
        assert!(sample(0.0, 0.0, &p).is_some());
    }

    #[test]
    fn chroma_separates_channels_at_edge() {
        let p = GlassParams {
            chroma: 0.15,
            flatness: 0.2,
            ..Default::default()
        };
        let s = sample(p.radius * 0.9, 0.0, &p).unwrap();
        assert!(
            (s.offset[0].0 - s.offset[2].0).abs() > 1e-4,
            "R and B offsets should differ with dispersion"
        );

        let p0 = GlassParams { chroma: 0.0, ..p };
        let s0 = sample(p0.radius * 0.9, 0.0, &p0).unwrap();
        assert_eq!(s0.offset[0], s0.offset[2]);
    }

    #[test]
    fn coverage_full_inside_zero_at_rim() {
        let p = GlassParams::default();
        assert!(close(sample(0.0, 0.0, &p).unwrap().coverage, 1.0, 1e-6));
        let edge = sample(p.radius - 0.01, 0.0, &p).unwrap();
        assert!(edge.coverage < 0.1);
    }

    #[test]
    fn apply_leaves_outside_untouched() {
        let mut src = Framebuffer::new(80, 60);
        for y in 0..60 {
            for x in 0..80 {
                src.set(x, y, Rgb::new((x * 3) as u8, (y * 4) as u8, 128));
            }
        }
        let mut dst = Framebuffer::new(80, 60);
        let p = GlassParams {
            radius: 10.0,
            ..Default::default()
        };
        apply(&src, &mut dst, 40.0, 30.0, &p);
        assert_eq!(dst.get(2, 2), src.get(2, 2));
        assert_eq!(dst.get(79, 59), src.get(79, 59));
        // Just outside the lens circle:
        assert_eq!(dst.get(40 + 12, 30), src.get(40 + 12, 30));
    }

    #[test]
    fn apply_distorts_inside() {
        let mut src = Framebuffer::new(80, 60);
        for y in 0..60 {
            for x in 0..80 {
                let v = if (x / 4 + y / 4) % 2 == 0 { 255 } else { 0 };
                src.set(x, y, Rgb::new(v, v, v));
            }
        }
        let mut dst = Framebuffer::new(80, 60);
        let p = GlassParams {
            radius: 20.0,
            depth: 16.0,
            tint: 0.0,
            specular: 0.0,
            ..Default::default()
        };
        apply(&src, &mut dst, 40.0, 30.0, &p);
        let mut changed = 0;
        for y in 12..48 {
            for x in 22..58 {
                if dst.get(x, y) != src.get(x, y) {
                    changed += 1;
                }
            }
        }
        assert!(
            changed > 50,
            "lens should visibly rewrite pixels: {changed}"
        );
    }

    #[test]
    fn apply_handles_lens_off_canvas() {
        let src = Framebuffer::new(40, 30);
        let mut dst = Framebuffer::new(40, 30);
        let p = GlassParams::default();
        apply(&src, &mut dst, -50.0, -50.0, &p); // fully off-screen
        apply(&src, &mut dst, 39.0, 29.0, &p); // corner overlap
        apply(&src, &mut dst, 200.0, 200.0, &p);
    }

    #[test]
    fn apply_on_empty_framebuffer_does_not_panic() {
        let src = Framebuffer::new(0, 0);
        let mut dst = Framebuffer::new(0, 0);
        apply(&src, &mut dst, 0.0, 0.0, &GlassParams::default());
    }

    #[test]
    fn param_specs_adjust_and_clamp() {
        let mut p = GlassParams::default();
        let radius = &PARAMS[0];
        (radius.set)(&mut p, 10.0);
        radius.adjust(&mut p, 1.0);
        assert_eq!((radius.get)(&p), 12.0);
        radius.adjust(&mut p, -1000.0);
        assert_eq!((radius.get)(&p), radius.min);
        radius.adjust(&mut p, 1000.0);
        assert_eq!((radius.get)(&p), radius.max);
    }

    #[test]
    fn param_normalized_roundtrip() {
        let mut p = GlassParams::default();
        for spec in PARAMS {
            spec.set_normalized(&mut p, 0.5);
            let v = (spec.get)(&p);
            assert!(v >= spec.min && v <= spec.max, "{} out of range", spec.name);
            let t = spec.normalized(&p);
            assert!((t - 0.5).abs() < 0.1, "{} normalized {t}", spec.name);
        }
    }

    #[test]
    fn param_names_unique() {
        let mut names: Vec<_> = PARAMS.iter().map(|s| s.name).collect();
        names.sort();
        names.dedup();
        assert_eq!(names.len(), PARAMS.len());
    }
}
