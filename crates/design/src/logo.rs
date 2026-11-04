//! nyaw のロゴ。`logo(Logo::Mini).height(px(48.))` は今のテーマ（[`Mode`]）に合う SVG を描く。
//! SVG の実体は `assets/logos/`（ビルドに埋め込む）。読み込みと描画は GPUI のアセットで裏のスレッドが行う。
//! exe に埋め込むアイコンは別（`assets/icons/*.ico`、ルートの `build.rs`）。

use std::sync::Arc;

use gpui::{
    App, Asset, ImageCacheError, IntoElement, Pixels, RenderImage, RenderOnce, Window, img,
    prelude::*, px, size,
};

use crate::Mode;

/// ロゴの種類。
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Logo {
    /// 文字入りの横長（352.49 × 150.19）。
    Full,
    /// 耳だけの縦長（172.62 × 213.42）。アイコン向き。
    Mini,
}

impl Logo {
    /// `mode` の背景に合う SVG の中身。
    pub fn svg(self, mode: Mode) -> &'static [u8] {
        match (self, mode) {
            (Logo::Full, Mode::Light) => {
                include_bytes!("../../../assets/logos/Logo_Full_Light.svg")
            }
            (Logo::Full, Mode::Dark) => include_bytes!("../../../assets/logos/Logo_Full_Dark.svg"),
            (Logo::Mini, Mode::Light) => {
                include_bytes!("../../../assets/logos/Logo_Mini_Light.svg")
            }
            (Logo::Mini, Mode::Dark) => include_bytes!("../../../assets/logos/Logo_Mini_Dark.svg"),
        }
    }

    /// 幅 ÷ 高さ。
    pub fn aspect(self) -> f32 {
        match self {
            Logo::Full => 352.49 / 150.19,
            Logo::Mini => 172.62 / 213.42,
        }
    }
}

struct LogoAsset;
impl Asset for LogoAsset {
    /// 種類・モード・描く画素数（幅, 高さ）
    type Source = (Logo, Mode, u32, u32);
    type Output = Result<Arc<RenderImage>, ImageCacheError>;

    fn load(
        (kind, mode, w, h): Self::Source,
        cx: &mut App,
    ) -> impl Future<Output = Self::Output> + Send + 'static {
        let renderer = cx.svg_renderer();
        async move {
            let svg = renderer.parse_svg(kind.svg(mode))?;
            renderer
                .render_parsed(
                    &svg,
                    gpui::SvgSize::Size(size(
                        gpui::DevicePixels(w as i32),
                        gpui::DevicePixels(h as i32),
                    )),
                )
                .map_err(Into::into)
        }
    }
}

/// ロゴの部品。高さで大きさを決め、幅は縦横比から出す。
#[derive(IntoElement)]
pub struct LogoView {
    kind: Logo,
    height: Pixels,
    mode: Option<Mode>,
}

pub fn logo(kind: Logo) -> LogoView {
    LogoView {
        kind,
        height: px(crate::font_size::LG),
        mode: None,
    }
}

impl LogoView {
    pub fn height(mut self, height: Pixels) -> Self {
        self.height = height;
        self
    }

    /// 今のテーマではなく、このモード用のロゴを使う（見比べるとき）。
    pub fn mode(mut self, mode: Mode) -> Self {
        self.mode = Some(mode);
        self
    }
}

impl RenderOnce for LogoView {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let mode = self
            .mode
            .unwrap_or_else(|| cx.try_global::<Mode>().copied().unwrap_or_default());
        let scale = window.scale_factor();
        let width = self.height * self.kind.aspect();
        let (w, h) = (
            (f32::from(width) * scale).ceil().max(1.) as u32,
            (f32::from(self.height) * scale).ceil().max(1.) as u32,
        );
        let kind = self.kind;
        img(move |window: &mut Window, cx: &mut App| {
            window.use_asset::<gpui::AssetLogger<LogoAsset>>(&(kind, mode, w, h), cx)
        })
        .w(width)
        .h(self.height)
        .flex_none()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 縦横比の定数は SVG の viewBox と合っている（差し替えで食い違うと歪む）。
    #[test]
    fn aspect_matches_view_box() {
        for kind in [Logo::Full, Logo::Mini] {
            for mode in [Mode::Light, Mode::Dark] {
                let svg = std::str::from_utf8(kind.svg(mode)).unwrap();
                let vb = svg
                    .split("viewBox=\"")
                    .nth(1)
                    .unwrap()
                    .split('"')
                    .next()
                    .unwrap();
                let v: Vec<f32> = vb.split_whitespace().map(|n| n.parse().unwrap()).collect();
                assert!(
                    (v[2] / v[3] - kind.aspect()).abs() < 1e-3,
                    "{kind:?} {mode:?}"
                );
            }
        }
    }
}
