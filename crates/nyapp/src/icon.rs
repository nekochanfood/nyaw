//! ニャプリのアイコン。パッケージのアイコン（ベクターの `icon.svg` かラスターの `icon.png`。[`package::Icon`]）を
//! GPUI の画像にし、無い・読めないときは
//! nyaw が代わりを描く（アクセントの色の角丸の四角に、名前の頭文字。Android の既定のアイコンと同じ考え方）。

use std::sync::Arc;

use design::{Theme, radius};
use gpui::{AnyElement, FontWeight, RenderImage, div, img, prelude::*, px, rgba};
use resvg::{tiny_skia, usvg};

use crate::package;

/// 読む PNG の一辺の上限（px）。
const MAX_SIDE: u32 = 1024;
/// SVG を描く一辺（px）。ランチャーのタイルの倍あれば、どの大きさで出してもぼやけない。
const SVG_SIDE: u32 = 256;

/// アイコンを GPUI の画像にする。壊れている・大きすぎるなら `None`（代わりを描く）。
pub fn decode(icon: &package::Icon) -> Option<Arc<RenderImage>> {
    let rgba = match icon {
        package::Icon::Png(png) => {
            let decoded = image::load_from_memory_with_format(png, image::ImageFormat::Png).ok()?;
            if decoded.width() > MAX_SIDE || decoded.height() > MAX_SIDE {
                return None;
            }
            decoded.into_rgba8()
        }
        package::Icon::Svg(svg) => rasterize(svg)?,
    };
    let mut bgra = rgba;
    for p in bgra.chunks_exact_mut(4) {
        p.swap(0, 2);
    }
    Some(Arc::new(RenderImage::new(vec![image::Frame::new(bgra)])))
}

/// SVG を [`SVG_SIDE`] の正方形に、縦横比を保って中央に描く（RGBA。乗算済みでない）。
// ponytail: フォントを読まないので SVG の中の文字は描かない。要ればアイコンの文字はパスにしてもらう。
fn rasterize(svg: &[u8]) -> Option<image::RgbaImage> {
    let tree = usvg::Tree::from_data(svg, &usvg::Options::default()).ok()?;
    let size = tree.size();
    let side = SVG_SIDE as f32;
    let scale = side / size.width().max(size.height());
    let (dx, dy) = (
        (side - size.width() * scale) / 2.0,
        (side - size.height() * scale) / 2.0,
    );
    let mut pixmap = tiny_skia::Pixmap::new(SVG_SIDE, SVG_SIDE)?;
    let transform = tiny_skia::Transform::from_scale(scale, scale).post_translate(dx, dy);
    resvg::render(&tree, transform, &mut pixmap.as_mut());
    let rgba = pixmap
        .pixels()
        .iter()
        .flat_map(|p| {
            let c = p.demultiply();
            [c.red(), c.green(), c.blue(), c.alpha()]
        })
        .collect();
    image::RgbaImage::from_raw(SVG_SIDE, SVG_SIDE, rgba)
}

/// 一辺 `size` px のアイコン。`image` が無ければ `name` の頭文字の代わりのアイコン。
pub fn element(
    image: Option<Arc<RenderImage>>,
    name: &str,
    size: f32,
    theme: &Theme,
) -> AnyElement {
    match image {
        Some(image) => img(image).size(px(size)).into_any_element(),
        None => fallback(name, size, theme),
    }
}

/// 代わりのアイコン: アクセントの色の角丸の四角に、名前の頭文字（大文字）。
pub fn fallback(name: &str, size: f32, theme: &Theme) -> AnyElement {
    div()
        .debug_selector(|| "nyapp-icon-fallback".into())
        .flex_none()
        .size(px(size))
        .rounded(px(radius::LG.min(size / 4.0)))
        .bg(rgba(theme.accent_solid))
        .text_color(rgba(theme.on_accent))
        .flex()
        .items_center()
        .justify_center()
        .text_size(px(size / 2.0))
        .font_weight(FontWeight::BOLD)
        .child(initial(name))
        .into_any_element()
}

/// 名前の頭文字（英字は大文字。空なら `?`）。
fn initial(name: &str) -> String {
    name.trim()
        .chars()
        .next()
        .map_or_else(|| "?".into(), |c| c.to_uppercase().collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn initials_for_the_fallback_icon() {
        assert_eq!(initial("eyes"), "E");
        assert_eq!(initial(" 目玉"), "目");
        assert_eq!(initial(""), "?");
    }

    /// 正しい PNG・SVG は画像になり、壊れたものは代わりのアイコンに回す（`None`）。
    #[test]
    fn decodes_raster_and_vector_icons_and_rejects_garbage() {
        let mut png = Vec::new();
        image::RgbaImage::from_pixel(4, 4, image::Rgba([255, 0, 0, 255]))
            .write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png)
            .unwrap();
        assert!(decode(&package::Icon::Png(png)).is_some());
        assert!(decode(&package::Icon::Png(b"not a png".to_vec())).is_none());
        assert!(decode(&package::Icon::Svg(b"not an svg".to_vec())).is_none());
    }

    /// SVG は縦横比を保って正方形の中央に描き、周りは透明のまま。
    #[test]
    fn vector_icons_are_rasterized_centered() {
        let svg = br##"<svg xmlns="http://www.w3.org/2000/svg" width="20" height="10">
            <rect width="20" height="10" fill="#ff0000"/></svg>"##;
        let image = rasterize(svg).unwrap();
        assert_eq!(image.dimensions(), (SVG_SIDE, SVG_SIDE));
        let mid = SVG_SIDE / 2;
        assert_eq!(
            image.get_pixel(mid, mid).0,
            [255, 0, 0, 255],
            "中央は塗られる"
        );
        assert_eq!(image.get_pixel(mid, 2).0[3], 0, "横長なので上の端は空く");
    }
}
