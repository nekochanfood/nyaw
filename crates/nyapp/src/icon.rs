//! ニャプリのアイコン。背景と前景の 2 枚を重ねる（Android のアダプティブアイコンと同じ考え方。[`package::Icon`]）。
//! 背景は nyaw が角丸の四角に切り、前景をその上に重ねる。前景だけでも出せる（進行中の作業の知らせなど）。
//! 無い・読めないものは nyaw が代わりを描く: 背景はアクセントの色、前景は名前の頭文字。

use std::sync::Arc;

use design::{Theme, radius};
use gpui::{AnyElement, FontWeight, RenderImage, div, img, prelude::*, px, rgba};
use resvg::{tiny_skia, usvg};

use crate::package;

/// 読む PNG の一辺の上限（px）。
const MAX_SIDE: u32 = 1024;
/// SVG を描く一辺（px）。ランチャーのタイルの倍あれば、どの大きさで出してもぼやけない。
const SVG_SIDE: u32 = 256;

/// GPUI の画像にしたアイコン（読めなかった層は `None`）。
#[derive(Clone, Default)]
pub struct AppIcon {
    pub background: Option<Arc<RenderImage>>,
    pub foreground: Option<Arc<RenderImage>>,
}

impl AppIcon {
    /// パッケージのものか（どちらも無ければ、全部 nyaw の代わりのもの）。
    pub fn is_packaged(&self) -> bool {
        self.background.is_some() || self.foreground.is_some()
    }
}

/// パッケージのアイコンを GPUI の画像にする。壊れている・大きすぎる層は無いものとする（代わりを描く）。
pub fn decode(icon: &package::Icon) -> AppIcon {
    AppIcon {
        background: icon.background.as_ref().and_then(decode_image),
        foreground: icon.foreground.as_ref().and_then(decode_image),
    }
}

fn decode_image(image: &package::Image) -> Option<Arc<RenderImage>> {
    let rgba = match image {
        package::Image::Png(png) => {
            let decoded = image::load_from_memory_with_format(png, image::ImageFormat::Png).ok()?;
            if decoded.width() > MAX_SIDE || decoded.height() > MAX_SIDE {
                return None;
            }
            decoded.into_rgba8()
        }
        package::Image::Svg(svg) => rasterize(svg)?,
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

/// 角丸の大きさ（一辺 `size` のアイコン）。
fn corner(size: f32) -> f32 {
    radius::LG.min(size / 4.0)
}

/// 一辺 `size` px のアイコン: 角丸に切った背景（無ければアクセントの色）に前景（無ければ名前 `name` の頭文字）。
pub fn element(icon: &AppIcon, name: &str, size: f32, theme: &Theme) -> AnyElement {
    let background = match &icon.background {
        Some(image) => img(image.clone())
            .absolute()
            .size_full()
            .rounded(px(corner(size)))
            .into_any_element(),
        None => div()
            .absolute()
            .size_full()
            .rounded(px(corner(size)))
            .bg(rgba(theme.accent_solid))
            .into_any_element(),
    };
    let foreground = match &icon.foreground {
        Some(image) => img(image.clone()).absolute().size_full().into_any_element(),
        None => initial_element(name, size, theme.on_accent),
    };
    div()
        .debug_selector(|| "nyapp-icon".into())
        .relative()
        .flex_none()
        .size(px(size))
        .child(background)
        .child(foreground)
        .into_any_element()
}

/// 前景だけの一辺 `size` px のアイコン（無ければ名前 `name` の頭文字を文字の色で）。背景の無い所に置くもの。
pub fn foreground(icon: &AppIcon, name: &str, size: f32, theme: &Theme) -> AnyElement {
    match &icon.foreground {
        Some(image) => img(image.clone())
            .flex_none()
            .size(px(size))
            .into_any_element(),
        None => div()
            .relative()
            .flex_none()
            .size(px(size))
            .child(initial_element(name, size, theme.neutral_text))
            .into_any_element(),
    }
}

/// 代わりの前景: 名前の頭文字（大文字）を `color` で真ん中に。
fn initial_element(name: &str, size: f32, color: u32) -> AnyElement {
    div()
        .debug_selector(|| "nyapp-icon-fallback".into())
        .absolute()
        .size_full()
        .flex()
        .items_center()
        .justify_center()
        .text_size(px(size / 2.0))
        .font_weight(FontWeight::BOLD)
        .text_color(rgba(color))
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

    fn png() -> Vec<u8> {
        let mut png = Vec::new();
        image::RgbaImage::from_pixel(4, 4, image::Rgba([255, 0, 0, 255]))
            .write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png)
            .unwrap();
        png
    }

    /// 背景と前景はそれぞれ読み、壊れた層だけを代わりに回す（`None`）。
    #[test]
    fn decodes_each_layer_and_drops_broken_ones() {
        let icon = decode(&package::Icon {
            background: Some(package::Image::Png(png())),
            foreground: Some(package::Image::Svg(b"not an svg".to_vec())),
        });
        assert!(icon.background.is_some() && icon.foreground.is_none());
        assert!(icon.is_packaged());
        let none = decode(&package::Icon::default());
        assert!(!none.is_packaged());
        let broken = decode(&package::Icon {
            background: Some(package::Image::Png(b"not a png".to_vec())),
            foreground: None,
        });
        assert!(!broken.is_packaged());
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

    /// 全部のアイコンは背景と前景を同じ大きさに重ね、前景だけのものはその大きさ。どちらも無ければ頭文字。
    #[gpui::test]
    fn layered_and_foreground_only_elements(cx: &mut gpui::TestAppContext) {
        let icon = AppIcon::default();
        let (_, cx) = cx.add_window_view(move |_, _| Icons(icon));
        cx.run_until_parked();
        let full = cx.debug_bounds("nyapp-icon").unwrap();
        assert_eq!(full.size.width, px(48.0));
        assert!(
            cx.debug_bounds("nyapp-icon-fallback").is_some(),
            "前景が無ければ頭文字"
        );
    }

    struct Icons(AppIcon);

    impl gpui::Render for Icons {
        fn render(
            &mut self,
            _: &mut gpui::Window,
            _: &mut gpui::Context<Self>,
        ) -> impl IntoElement {
            div()
                .child(element(&self.0, "eyes", 48.0, &Theme::DARK))
                .child(foreground(&self.0, "eyes", 24.0, &Theme::DARK))
        }
    }
}
