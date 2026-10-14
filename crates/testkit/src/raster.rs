//! A software rasterizer for tests. The test platform has no GPU renderer, so
//! this paints the scene GPUI drew (rectangles) into RGBA on the CPU, replacing production `ui::capture`.
//! The rest of the path (`Overlay::submit` -> VR) is the same as production.

use gpui::{App, Rgba, Window};
use ui::Frame;

/// Like `ui::capture`, redraws first and then paints the scene into RGBA.
// ponytail: only solid rectangles, rounded corners, borders (no edge anti-aliasing) and straight underlines. Wavy lines, shadows, text and images are not painted. Add them when something needs visual verification.
pub fn capture(window: &mut Window, cx: &mut App) -> Frame {
    window.draw(cx).clear(cx);
    let scale = window.scale_factor();
    let vp = window.viewport_size();
    let width = (f32::from(vp.width) * scale).round() as u32;
    let height = (f32::from(vp.height) * scale).round() as u32;
    let mut rgba = vec![0u8; (width * height * 4) as usize];

    let mut quads = window.painted_quads();
    quads.sort_by_key(|q| q.order);
    for q in quads {
        let fill = q.background.as_solid().map(Rgba::from);
        let border = Rgba::from(q.border_color);
        let w = &q.border_widths;
        let (w_t, w_r, w_b, w_l) = (w.top.0, w.right.0, w.bottom.0, w.left.0);
        let has_border = w_t > 0.0 || w_r > 0.0 || w_b > 0.0 || w_l > 0.0;
        if fill.is_none() && !has_border {
            continue;
        }
        let o = q.bounds;
        let outer = [
            o.origin.x.0,
            o.origin.y.0,
            o.origin.x.0 + o.size.width.0,
            o.origin.y.0 + o.size.height.0,
        ];
        let inner = [
            outer[0] + w_l,
            outer[1] + w_t,
            outer[2] - w_r,
            outer[3] - w_b,
        ];
        let r = &q.corner_radii;
        let radii = [
            r.top_left.0,
            r.top_right.0,
            r.bottom_right.0,
            r.bottom_left.0,
        ];
        let inner_radii = radii.map(|r| (r - w_t.max(w_r).max(w_b).max(w_l)).max(0.0));
        let b = q.bounds.intersect(&q.content_mask.bounds);
        let x0 = b.origin.x.0.max(0.0).round() as u32;
        let y0 = b.origin.y.0.max(0.0).round() as u32;
        let x1 = ((b.origin.x.0 + b.size.width.0).round().max(0.0) as u32).min(width);
        let y1 = ((b.origin.y.0 + b.size.height.0).round().max(0.0) as u32).min(height);
        // Plain rectangles (no corners, no border) skip the per-pixel tests, and opaque ones are a row copy.
        // The window-sized background alone is 8M pixels at 4K, so this is most of the capture time.
        if !has_border && radii == [0.0; 4] {
            let Some(f) = fill else { continue };
            for y in y0..y1 {
                let row =
                    &mut rgba[((y * width + x0) * 4) as usize..((y * width + x1) * 4) as usize];
                if f.a >= 1.0 {
                    let px = [to_u8(f.r), to_u8(f.g), to_u8(f.b), 255];
                    row.as_chunks_mut::<4>().0.fill(px);
                } else {
                    row.as_chunks_mut::<4>().0.iter_mut().for_each(|d| blend(d, f));
                }
            }
            continue;
        }
        for y in y0..y1 {
            for x in x0..x1 {
                let (cx, cy) = (x as f32 + 0.5, y as f32 + 0.5);
                if !inside(outer, radii, cx, cy) {
                    continue;
                }
                let color = if has_border && !inside(inner, inner_radii, cx, cy) {
                    border
                } else if let Some(f) = fill {
                    f
                } else {
                    continue;
                };
                let i = ((y * width + x) * 4) as usize;
                let px: &mut [u8; 4] = (&mut rgba[i..i + 4]).try_into().unwrap();
                blend(px, color);
            }
        }
    }
    // ponytail: underlines are just overlaid on the rectangles (draw order lives in a separate space from quads). Wavy lines are painted straight.
    for u in window.painted_underlines() {
        let color = Rgba::from(u.color);
        let b = u.bounds.intersect(&u.content_mask.bounds);
        let x0 = b.origin.x.0.max(0.0).round() as u32;
        let y0 = b.origin.y.0.max(0.0).round() as u32;
        let x1 = ((b.origin.x.0 + b.size.width.0).round().max(0.0) as u32).min(width);
        let y1 = ((b.origin.y.0 + b.size.height.0).round().max(0.0) as u32).min(height);
        for y in y0..y1 {
            for x in x0..x1 {
                let i = ((y * width + x) * 4) as usize;
                blend((&mut rgba[i..i + 4]).try_into().unwrap(), color);
            }
        }
    }
    Frame {
        width,
        height,
        rgba,
    }
}

/// Whether the rounded rectangle `[x0, y0, x1, y1]` (corners in the order top-left, top-right, bottom-right, bottom-left) contains the point.
fn inside([x0, y0, x1, y1]: [f32; 4], [tl, tr, br, bl]: [f32; 4], x: f32, y: f32) -> bool {
    if x < x0 || x >= x1 || y < y0 || y >= y1 {
        return false;
    }
    // Center of the corner circle. Only check inside/outside the circle when the point is in that corner's square region.
    for (r, cx, cy) in [
        (tl, x0 + tl, y0 + tl),
        (tr, x1 - tr, y0 + tr),
        (br, x1 - br, y1 - br),
        (bl, x0 + bl, y1 - bl),
    ] {
        let in_corner = (x - cx).abs() <= r
            && (y - cy).abs() <= r
            && (x < x0 + x1) == (cx < x0 + x1)
            && (y < y0 + y1) == (cy < y0 + y1);
        if r > 0.0 && in_corner && (x - cx).hypot(y - cy) > r {
            return false;
        }
    }
    true
}

fn to_u8(c: f32) -> u8 {
    (c * 255.0).round() as u8
}

/// Composites `src` over `dst` (source-over with straight alpha).
fn blend(dst: &mut [u8; 4], src: Rgba) {
    if src.a >= 1.0 {
        *dst = [to_u8(src.r), to_u8(src.g), to_u8(src.b), 255];
        return;
    }
    let da = dst[3] as f32 / 255.0;
    let out_a = src.a + da * (1.0 - src.a);
    if out_a <= 0.0 {
        return;
    }
    let mix = |s: f32, d: u8| {
        let d = d as f32 / 255.0;
        ((s * src.a + d * da * (1.0 - src.a)) / out_a * 255.0).round() as u8
    };
    *dst = [
        mix(src.r, dst[0]),
        mix(src.g, dst[1]),
        mix(src.b, dst[2]),
        (out_a * 255.0).round() as u8,
    ];
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rgba(r: f32, g: f32, b: f32, a: f32) -> Rgba {
        Rgba { r, g, b, a }
    }

    #[test]
    fn rounded_corner_cuts_the_corner_only() {
        let rect = [0.0, 0.0, 10.0, 10.0];
        let r = [4.0, 0.0, 0.0, 0.0];
        assert!(!inside(rect, r, 0.5, 0.5));
        assert!(inside(rect, r, 5.0, 5.0));
        assert!(
            inside(rect, r, 9.5, 0.5),
            "a corner with radius 0 is not clipped"
        );
        assert!(!inside(rect, r, 10.5, 5.0));
    }

    #[test]
    fn opaque_replaces() {
        let mut px = [10, 20, 30, 255];
        blend(&mut px, rgba(1.0, 0.0, 0.0, 1.0));
        assert_eq!(px, [255, 0, 0, 255]);
    }

    #[test]
    fn half_alpha_mixes_over_opaque() {
        let mut px = [0, 0, 0, 255];
        blend(&mut px, rgba(1.0, 1.0, 1.0, 0.5));
        assert_eq!(px, [128, 128, 128, 255]);
    }

    #[test]
    fn over_transparent_keeps_color() {
        let mut px = [0, 0, 0, 0];
        blend(&mut px, rgba(0.0, 0.0, 1.0, 0.5));
        assert_eq!(px, [0, 0, 255, 128]);
    }
}
