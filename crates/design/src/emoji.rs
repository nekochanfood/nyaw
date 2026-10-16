//! Twemoji のカラー SVG。`emoji("🐈").size(px(24.))` はアイコンとして、
//! `emoji_text("こんにちは 🐈!")` は周りの文字サイズ・行高・折り返しで表示する。
//! 未対応の文字列・明示的なテキスト指定（VS15）はそのままフォントで描く。

use gpui::{
    AnyElement, App, Asset, AvailableSpace, Bounds, Element, ElementId, GlobalElementId,
    ImageCacheError, InspectorElementId, IntoElement, LayoutId, Pixels, RenderImage, RenderOnce,
    SharedString, StyledText, Window, div, img, point, prelude::*, px, size,
};
use std::sync::Arc;
use unicode_segmentation::UnicodeSegmentation;

include!(concat!(env!("OUT_DIR"), "/twemoji.rs"));
static SVG: &[u8] = include_bytes!("../twemoji/svg.bin");

fn lookup(text: &str) -> Option<usize> {
    if text.contains('\u{fe0e}') {
        return None;
    }
    let zwj = text.contains('\u{200d}');
    let key = text
        .chars()
        .filter(|c| zwj || *c != '\u{fe0f}')
        .map(|c| format!("{:x}", c as u32))
        .collect::<Vec<_>>()
        .join("-");
    INDEX
        .binary_search_by_key(&key.as_str(), |(key, _, _)| *key)
        .ok()
}

struct SvgAsset;
impl Asset for SvgAsset {
    type Source = (usize, u32);
    type Output = Result<Arc<RenderImage>, ImageCacheError>;

    fn load(
        (index, pixels): Self::Source,
        cx: &mut App,
    ) -> impl Future<Output = Self::Output> + Send + 'static {
        let renderer = cx.svg_renderer();
        async move {
            let (_, offset, len) = INDEX[index];
            let svg = renderer.parse_svg(&SVG[offset..offset + len])?;
            renderer
                .render_parsed(
                    &svg,
                    gpui::SvgSize::Size(size(
                        gpui::DevicePixels(pixels as i32),
                        gpui::DevicePixels(pixels as i32),
                    )),
                )
                .map_err(Into::into)
        }
    }
}

fn image(index: usize, edge: Pixels, window: &Window) -> gpui::Img {
    let pixels = (f32::from(edge) * window.scale_factor()).ceil().max(1.) as u32;
    img(move |window: &mut Window, cx: &mut App| {
        window.use_asset::<gpui::AssetLogger<SvgAsset>>(&(index, pixels), cx)
    })
    .size(edge)
    .flex_none()
}

/// カラーの絵文字部品。色は SVG 本来の配色を保つ。
#[derive(IntoElement)]
pub struct Emoji {
    text: SharedString,
    edge: Pixels,
}

pub fn emoji(text: impl Into<SharedString>) -> Emoji {
    Emoji {
        text: text.into(),
        edge: px(crate::font_size::MD),
    }
}

impl Emoji {
    pub fn size(mut self, edge: Pixels) -> Self {
        self.edge = edge;
        self
    }
}

impl RenderOnce for Emoji {
    fn render(self, _: &mut Window, _: &mut App) -> impl IntoElement {
        div()
            .size(self.edge)
            .flex_none()
            .text_size(self.edge)
            .line_height(self.edge)
            .child(emoji_text(self.text))
    }
}

/// 文章を一つのテキストとして組み、全角スペースで確保した位置に SVG を重ねる。
/// GPUI のテキストシェーパーを使うので、日本語・改行・折り返しも通常の文章と同じ。
pub struct EmojiText {
    original: SharedString,
    text: StyledText,
    icons: Vec<(usize, usize)>,
}

pub fn emoji_text(text: impl Into<SharedString>) -> EmojiText {
    let original = text.into();
    let mut shaped = String::new();
    let mut icons = Vec::new();
    for grapheme in original.graphemes(true) {
        match lookup(grapheme) {
            Some(index) => {
                icons.push((shaped.len(), index));
                shaped.push('\u{2003}');
            }
            None => shaped.push_str(grapheme),
        }
    }
    EmojiText {
        original,
        text: StyledText::new(shaped),
        icons,
    }
}

impl IntoElement for EmojiText {
    type Element = Self;
    fn into_element(self) -> Self {
        self
    }
}

impl Element for EmojiText {
    type RequestLayoutState = Vec<(usize, AnyElement)>;
    type PrepaintState = Vec<usize>;

    fn id(&self) -> Option<ElementId> {
        None
    }
    fn source_location(&self) -> Option<&'static std::panic::Location<'static>> {
        None
    }
    fn a11y_role(&self) -> Option<gpui::Role> {
        Some(gpui::Role::Label)
    }
    fn write_a11y_info(&self, node: &mut gpui::accesskit::Node) {
        node.set_value(self.original.to_string());
    }

    fn request_layout(
        &mut self,
        id: Option<&GlobalElementId>,
        inspector: Option<&InspectorElementId>,
        window: &mut Window,
        cx: &mut App,
    ) -> (LayoutId, Self::RequestLayoutState) {
        let (layout, ()) = self.text.request_layout(id, inspector, window, cx);
        let edge = window.text_style().font_size.to_pixels(window.rem_size());
        let icons = self
            .icons
            .iter()
            .map(|&(offset, index)| {
                let mut image = image(index, edge, window).into_any_element();
                image.layout_as_root(
                    size(AvailableSpace::MinContent, AvailableSpace::MinContent),
                    window,
                    cx,
                );
                (offset, image)
            })
            .collect();
        (layout, icons)
    }

    fn prepaint(
        &mut self,
        id: Option<&GlobalElementId>,
        inspector: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        icons: &mut Self::RequestLayoutState,
        window: &mut Window,
        cx: &mut App,
    ) -> Self::PrepaintState {
        self.text
            .prepaint(id, inspector, bounds, &mut (), window, cx);
        let edge = window.text_style().font_size.to_pixels(window.rem_size());
        let line_height = self.text.layout().line_height();
        let inset = (line_height - edge) / 2.;
        let mut shifts = Vec::new();
        for line in self.text.layout().line_layouts() {
            let layout = &line.unwrapped_layout;
            let mut start = px(0.);
            for end in line
                .wrap_boundaries
                .iter()
                .map(|b| layout.runs[b.run_ix].glyphs[b.glyph_ix].position.x)
                .chain([layout.width])
            {
                let remaining = bounds.size.width - (end - start);
                shifts.push(match window.text_style().text_align {
                    gpui::TextAlign::Left => px(0.),
                    gpui::TextAlign::Center => remaining / 2.,
                    gpui::TextAlign::Right => remaining,
                });
                start = end;
            }
        }
        let mut visible = Vec::new();
        for (i, (offset, image)) in icons.iter_mut().enumerate() {
            // 折り返し境界の開始位置は前行の末尾を返すため、全角スペースの終端から戻す。
            if let Some(end) = self
                .text
                .layout()
                .position_for_index(*offset + '\u{2003}'.len_utf8())
            {
                let row =
                    (f32::from(end.y - bounds.origin.y) / f32::from(line_height)).round() as usize;
                image.prepaint_at(end + point(shifts[row] - edge, inset), window, cx);
                visible.push(i);
            }
        }
        visible
    }

    fn paint(
        &mut self,
        id: Option<&GlobalElementId>,
        inspector: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        icons: &mut Self::RequestLayoutState,
        visible: &mut Self::PrepaintState,
        window: &mut Window,
        cx: &mut App,
    ) {
        self.text
            .paint(id, inspector, bounds, &mut (), &mut (), window, cx);
        for &i in visible.iter() {
            icons[i].1.paint(window, cx);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lookup_handles_sequences_and_presentation() {
        for emoji in ["😀", "❤️", "❤", "👍🏽", "👩‍💻", "🏳️‍🌈", "🇯🇵", "1️⃣", "🫩"]
        {
            assert!(lookup(emoji).is_some(), "{emoji}");
        }
        for text in ["", "hello", "😀😀", "❤\u{fe0e}", "A", "1", "#"] {
            assert!(lookup(text).is_none(), "{text}");
        }
    }

    #[test]
    fn mixed_text_preserves_unknown_graphemes_and_newlines() {
        let text = emoji_text("日本語 😀👍🏽👩‍💻🇯🇵1️⃣\nABC e\u{301} ❤\u{fe0e}");
        assert_eq!(text.icons.len(), 5);
        assert_eq!(
            text.icons
                .iter()
                .map(|(offset, _)| *offset)
                .collect::<Vec<_>>(),
            [10, 13, 16, 19, 22]
        );
        assert!(
            emoji_text("plain\ntext e\u{301} ❤\u{fe0e}")
                .icons
                .is_empty()
        );
    }

    #[test]
    fn packed_assets_are_sorted_valid_svg() {
        let renderer = gpui::SvgRenderer::new(Arc::new(()));
        assert_eq!(INDEX.len(), 4009);
        assert!(INDEX.windows(2).all(|w| w[0].0 < w[1].0));
        for &(_, offset, len) in INDEX {
            let svg = std::str::from_utf8(&SVG[offset..offset + len]).unwrap();
            assert!(svg.contains("<svg") && svg.contains("</svg>"));
            renderer
                .parse_svg(svg.as_bytes())
                .expect("valid Twemoji SVG");
        }
    }
}
