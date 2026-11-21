//! wrist の文字盤。直径 `d` px の円内に描画する（実物はウィンドウ全体、選択画面では小さなプレビュー）。
//! 色はテーマのロールのみを使用するため、テーマ（ライト / ダーク）とパーソナルカラー（塗りの `accent-solid` と、その上の `on-accent`）で変化する。
//!
//! デザインは 1 つ 1 関数（`fn(&Face) -> Div`）で、文字盤の種類ごとのファイルに配置する。追加方法は AGENT.md の「デザインを足す」。

mod analog;
mod digital;
mod standard;

use std::f32::consts::TAU;

use chrono::{DateTime, Local, Timelike};
use design::{Theme, radius};
use gpui::{
    App, Div, Pixels, Point, SharedString, Window, canvas, div, point, prelude::*, px, rgba,
};
use kernel::WristDesign;

/// 寸法を決定する基準の直径（実物の文字盤）。デザインはこのサイズを前提に記述し、他のサイズには [`Face`] がスケーリングする。
pub const BASE: f32 = 400.0;

/// 文字盤の内容（一辺 `d` の正方形。背景の丸いカードは呼び出し側が配置する）。`date` は現在の言語での日付。
pub fn dial(
    design: WristDesign,
    theme: &Theme,
    now: DateTime<Local>,
    date: SharedString,
    d: f32,
) -> Div {
    let f = Face {
        theme,
        now,
        date,
        k: d / BASE,
    };
    // デザインを追加したら、ここに 1 行追加する（追加漏れがあるとコンパイルエラーになる）。
    match design {
        WristDesign::Dots => standard::dots(&f),
        WristDesign::Ring => standard::ring(&f),
        WristDesign::Gauge => standard::gauge(&f),
        WristDesign::Lcd => digital::lcd(&f),
        WristDesign::Flip => digital::flip(&f),
        WristDesign::Terminal => digital::terminal(&f),
        WristDesign::Minimal => analog::minimal(&f),
        WristDesign::Dress => analog::dress(&f),
        WristDesign::Railway => analog::railway(&f),
    }
}

/// 選択画面に表示するプレビュー: 直径 `d` の丸いカードに、固定時刻（10:09:36）の文字盤。カードと区別可能な背景上に配置する。
pub fn preview(design: WristDesign, d: f32, cx: &App) -> Div {
    let theme = Theme::current(cx);
    let now = Local::now();
    let sample = now.with_hour(10).and_then(|t| t.with_minute(9));
    let sample = sample.and_then(|t| t.with_second(36)).unwrap_or(now);
    div()
        .flex_none()
        .size(px(d))
        .rounded(px(radius::FULL))
        .circular_corners()
        .bg(rgba(theme.card))
        .child(dial(
            design,
            &theme,
            sample,
            i18n::date(cx, sample).into(),
            d,
        ))
}

/// 文字盤 1 枚を描画するためのコンテキスト。デザイン関数はこれのみを受け取る。
/// 寸法は基準の直径（[`BASE`]）を前提に記述し、[`Face::px`] / [`Face::size`] で実際のサイズに変換する。
pub(crate) struct Face<'a> {
    pub theme: &'a Theme,
    pub now: DateTime<Local>,
    /// 現在の言語での日付。
    date: SharedString,
    /// 基準に対する縮尺。
    k: f32,
}

impl Face<'_> {
    /// 基準の寸法を実際のサイズ（px 値）に変換する。
    pub fn size(&self, base: f32) -> f32 {
        base * self.k
    }

    pub fn px(&self, base: f32) -> Pixels {
        px(self.size(base))
    }

    /// 文字盤のフレーム: 内容を上下左右中央に、縦に配置する。重ねる要素（[`Self::paint`] / [`Self::label`]）も子に追加する。
    pub fn frame(&self) -> Div {
        div()
            .relative()
            .flex_none()
            .size(self.px(BASE))
            .flex()
            .flex_col()
            .items_center()
            .justify_center()
            .text_color(rgba(self.theme.neutral_text))
            // 文字盤はグラフィカル要素。フォントサイズも直径に合わせて設定済みのため、全域のフォントサイズ・太字化には従わない（サイズは UI スケールで変化する）。
            .text_exact()
    }

    /// 日付テキスト（`date`）。色・サイズは呼び出し側が追加する。
    pub fn date(&self) -> Div {
        div()
            .debug_selector(|| "date".into())
            .child(self.date.clone())
    }

    /// 時刻の文字（`time`。`HH:MM`）。
    pub fn time(&self) -> Div {
        div()
            .debug_selector(|| "time".into())
            .child(self.clock("%H:%M"))
    }

    /// 現在の時刻を chrono のフォーマットで取得する（`%H` / `%M` / `%S`）。
    pub fn clock(&self, format: &str) -> String {
        self.now.format(format).to_string()
    }

    /// 現在の秒の 1 分内での進捗（0〜1）。
    pub fn minute_progress(&self) -> f32 {
        self.now.second() as f32 / 60.0
    }

    /// 時針・分針・秒針の向き（12 時から時計回りの周の割合）。
    pub fn hands(&self) -> [f32; 3] {
        turns(self.now.hour(), self.now.minute(), self.now.second())
    }

    /// 文字盤上のラベル: 12 時から時計回りに `turn` 周の方向・中心から `reach`（半径に対する割合）の点を中心に配置する。
    pub fn label(&self, turn: f32, reach: f32, text: impl Into<SharedString>) -> Div {
        // テキストを中央配置するボックスの一辺。
        const BOX: f32 = 96.0;
        let (a, r) = (turn * TAU, BASE / 2.0 * reach);
        div()
            .absolute()
            .left(self.px(BASE / 2.0 + r * a.sin() - BOX / 2.0))
            .top(self.px(BASE / 2.0 - r * a.cos() - BOX / 2.0))
            .size(self.px(BOX))
            .flex()
            .items_center()
            .justify_center()
            .child(text.into())
    }

    /// 文字盤全体に重ねる描画サーフェス（`hands`）。`draw` に渡る [`Pen`] で、中心からの方向と距離で線や円を描画する。
    pub fn paint(&self, draw: impl Fn(&mut Pen) + 'static) -> Div {
        let k = self.k;
        let surface = canvas(
            |_, _, _| {},
            move |bounds, _, window, _| {
                draw(&mut Pen {
                    window,
                    center: bounds.center(),
                    radius: f32::from(bounds.size.width) / 2.0,
                    k,
                })
            },
        );
        div().absolute().size_full().child(surface.size_full())
    }
}

/// 文字盤に線や円を描画するユーティリティ（[`Face::paint`]）。方向は 12 時から時計回りの周の割合、中心からの距離は半径に対する割合
/// （負は反対側）、太さ・半径は基準の寸法。
pub(crate) struct Pen<'a> {
    window: &'a mut Window,
    center: Point<Pixels>,
    radius: f32,
    k: f32,
}

impl Pen<'_> {
    fn at(&self, turn: f32, reach: f32) -> Point<Pixels> {
        let (a, r) = (turn * TAU, self.radius * reach);
        point(
            self.center.x + px(r * a.sin()),
            self.center.y - px(r * a.cos()),
        )
    }

    /// 方向 `turn` の放射状の線（中心からの距離 `from`〜`to`）。両端は角のまま。目盛りや直線の針に使用する。
    pub fn spoke(&mut self, turn: f32, from: f32, to: f32, width: f32, color: u32) {
        let mut path = gpui::PathBuilder::stroke(px(width * self.k));
        path.move_to(self.at(turn, from));
        path.line_to(self.at(turn, to));
        if let Ok(path) = path.build() {
            self.window.paint_path(path, rgba(color));
        }
    }

    /// 両端の丸い [`Self::spoke`]。
    pub fn rounded_spoke(&mut self, turn: f32, from: f32, to: f32, width: f32, color: u32) {
        self.spoke(turn, from, to, width, color);
        for end in [from, to] {
            self.disc(turn, end, width / 2.0, color);
        }
    }

    /// 方向 `turn`・距離 `reach` の点を中心とした円。
    pub fn disc(&mut self, turn: f32, reach: f32, radius: f32, color: u32) {
        let (c, r) = (self.at(turn, reach), px(radius * self.k));
        let mut path = gpui::PathBuilder::fill();
        for i in 0..=32 {
            let a = i as f32 / 32.0 * TAU;
            let p = point(c.x + r * a.cos(), c.y + r * a.sin());
            if i == 0 {
                path.move_to(p)
            } else {
                path.line_to(p)
            }
        }
        if let Ok(path) = path.build() {
            self.window.paint_path(path, rgba(color));
        }
    }
}

/// 時針・分針・秒針の方向（12 時から時計回りの周の割合 0〜1）。時針は分で、分針は秒でスムーズに進む。
fn turns(hour: u32, minute: u32, second: u32) -> [f32; 3] {
    let second = second as f32 / 60.0;
    let minute = (minute as f32 + second) / 60.0;
    [(hour % 12) as f32 / 12.0 + minute / 12.0, minute, second]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hands_point_at_the_time() {
        assert_eq!(turns(0, 0, 0), [0.0, 0.0, 0.0]);
        assert_eq!(turns(15, 0, 0), [0.25, 0.0, 0.0], "15 時は 3 時の方向");
        let [hour, minute, second] = turns(6, 30, 30);
        assert!((second - 0.5).abs() < 1e-6);
        assert!((minute - 30.5 / 60.0).abs() < 1e-6, "分針は秒の分だけ進む");
        let want = 0.5 + 30.5 / 60.0 / 12.0;
        assert!((hour - want).abs() < 1e-6, "時針は分の分だけ進む");
    }

    /// 種類・デザインの名前（`wrist.face.*` / `wrist.design.*`）が、すべての言語の文言に存在する。
    #[test]
    fn every_face_and_design_has_a_name() {
        for &face in kernel::WristFace::ALL {
            let mut keys = vec![format!("wrist.face.{}", face.key())];
            let designs = face.designs().iter();
            keys.extend(designs.map(|d| format!("wrist.design.{}", d.key())));
            for key in keys {
                for lang in i18n::Lang::ALL {
                    assert!(i18n::get(lang, &key).is_some(), "{key} ({})", lang.code());
                }
            }
        }
    }
}
