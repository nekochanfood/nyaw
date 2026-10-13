//! テスト用のソフトウェアラスタライザ。テストプラットフォームには GPU レンダラが無いので、
//! GPUI が描いたシーン（矩形）を CPU で RGBA に塗り、本番の `ui::capture` の代わりにする。
//! 以降の経路（`Overlay::submit` → VR）は本番と同じ。

use gpui::{App, Rgba, Window};
use ui::Frame;

/// `ui::capture` と同じく描き直してから、シーンを RGBA に塗る。
// ponytail: 単色の矩形のみ。角丸・枠線・影・文字・画像は塗らない。見た目の検証が要るものが出たら足す。
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
        let Some(color) = q.background.as_solid() else {
            continue;
        };
        let color = Rgba::from(color);
        let b = q.bounds.intersect(&q.content_mask.bounds);
        let x0 = b.origin.x.0.max(0.0).round() as u32;
        let y0 = b.origin.y.0.max(0.0).round() as u32;
        let x1 = ((b.origin.x.0 + b.size.width.0).round().max(0.0) as u32).min(width);
        let y1 = ((b.origin.y.0 + b.size.height.0).round().max(0.0) as u32).min(height);
        for y in y0..y1 {
            for x in x0..x1 {
                let i = ((y * width + x) * 4) as usize;
                let px: &mut [u8; 4] = (&mut rgba[i..i + 4]).try_into().unwrap();
                blend(px, color);
            }
        }
    }
    Frame {
        width,
        height,
        rgba,
    }
}

/// `src` を `dst` に重ねる（ストレートアルファの source-over）。
fn blend(dst: &mut [u8; 4], src: Rgba) {
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
