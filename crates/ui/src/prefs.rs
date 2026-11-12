//! 設定の部品: 設定画面とセットアップウィザードが共有する、全体の設定（[`Setting`]）を選ぶ画面の一部（[`appearance`] /
//! [`watch_faces`]）。今の設定を見せ、選ぶとすぐ反映する。地（`background`）の上に置く。開閉などの小さな状態は呼び出し側が
//! 持ち、変えるときは `set` で受け取る（ビューの型を問わない）。詳しくは `crates/ui/AGENT.md` の「設定の部品」。

use design::components::{self, Variant};
use design::{Accent, Mode, Theme, font_size, icons, radius, spacing};
use gpui::{
    App, ClickEvent, Context, Div, FontWeight, IntoElement, MouseDownEvent, Render, SharedString,
    Window, div, prelude::*, px, rgba,
};
use kernel::{
    PanelCommand, PanelId, PersonalColor, Setting, Settings, ThemeMode, Vec2, WristDesign,
    WristFace,
};

use crate::{PanelControl, PanelSpec};

/// wrist の文字盤の見本（デザインと直径 px）を描く関数。画面どうしは直接依存しないので、文字盤を持つ wrist が GPUI の
/// グローバルに置き、選ぶ画面（[`watch_faces`]）が引く。置かれていなければ、見本は地の丸だけになる。
#[derive(Clone, Copy)]
pub struct FacePreview(pub fn(WristDesign, f32, &App) -> Div);

impl gpui::Global for FacePreview {}

fn settings(cx: &App) -> Settings {
    let control = cx.try_global::<PanelControl>();
    control.map(|c| c.settings).unwrap_or_default()
}

fn push(setting: Setting, cx: &mut App) {
    let control = cx.default_global::<PanelControl>();
    control.setting_changes.push(setting);
}

fn accent(c: PersonalColor) -> Accent {
    Accent::new(c.lightness, c.chroma, c.hue)
}

fn personal(a: Accent) -> PersonalColor {
    PersonalColor {
        lightness: a.lightness,
        chroma: a.chroma,
        hue: a.hue,
    }
}

/// [`appearance`] の状態。
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Appearance {
    /// テーマの選択肢を開いている。
    pub theme_open: bool,
}

/// 外観: 上からテーマの見本（`theme-preview`）、テーマ（`setting-theme` → `setting-theme-dark` / `-light`）、パーソナルカラー
/// （履歴の見本 `accent-<名前>` と、自分で色を作る `accent-custom`）。色を作るダイアログは `panel` の子として開く。
pub fn appearance(
    panel: PanelId,
    state: Appearance,
    set: impl Fn(Appearance, &mut Window, &mut App) + Clone + 'static,
    window: &mut Window,
    cx: &mut App,
) -> Div {
    let theme = Theme::current(cx);
    let now = settings(cx);
    let current = now.personal_color.map_or(Accent::DEFAULT, accent);
    let open = move |theme_open: bool| {
        let set = set.clone();
        move |window: &mut Window, cx: &mut App| set(Appearance { theme_open }, window, cx)
    };

    let modes = [
        (ThemeMode::Dark, "setting-theme-dark", "theme.dark"),
        (ThemeMode::Light, "setting-theme-light", "theme.light"),
    ]
    .map(|(mode, id, key)| (mode, id, i18n::t(cx, key)));
    let value = modes.iter().find(|(mode, ..)| *mode == now.theme);
    let value = value.map(|(.., label)| label.clone()).unwrap_or_default();
    let options = modes.map(|(mode, id, label)| {
        let close = open(false);
        components::select_option(&theme, id, label, mode == now.theme).on_click(
            move |_, window, cx| {
                push(Setting::Theme(mode), cx);
                close(window, cx)
            },
        )
    });
    let (show, dismiss) = (open(true), open(false));
    let popover = components::select_popover(
        &theme,
        window,
        cx,
        "setting-theme",
        state.theme_open,
        options,
        move |_: &MouseDownEvent, window, cx| dismiss(window, cx),
    );
    let label = i18n::t(cx, "settings.theme");
    let theme_row = components::select(&theme, "setting-theme", &icons::PALETTE, label, value)
        .on_click(move |_, window, cx| show(window, cx))
        .child(popover);

    // 履歴: 自分で作った色（新しい順）の後ろに、用意した色。
    let custom = now.color_history.into_iter().flatten().map(accent);
    let colors = components::accent_row(
        &theme,
        current,
        &Accent::history(current, custom),
        |name| i18n::t(cx, &format!("color.{name}")),
        |a, _, cx| push(Setting::PersonalColor(personal(a)), cx),
        move |_, _, cx| {
            let dialog = color_dialog(panel, current);
            cx.default_global::<PanelControl>().spawns.push(dialog);
        },
    );
    div()
        .flex()
        .flex_col()
        .gap(px(spacing::LG))
        .child(
            div()
                .flex()
                .justify_center()
                .child(components::theme_preview(&theme)),
        )
        .child(components::group(
            &theme,
            i18n::t(cx, "settings.style"),
            components::list(&theme, [theme_row]),
        ))
        .child(components::group(
            &theme,
            i18n::t(cx, "settings.personal_color"),
            components::card(&theme).p(px(spacing::MD)).child(colors),
        ))
}

/// 色を作るダイアログの大きさ（px。1px = 1mm）。縦に、見出し・見本・スライダー 3 本・ボタン 2 個。
pub const COLOR_DIALOG_PX: (u32, u32) = (480, 736);

/// 色を作るダイアログを、`from` の子として開く起動仕様。`accent` から作り始める。
fn color_dialog(from: PanelId, accent: Accent) -> PanelSpec {
    let (w, h) = COLOR_DIALOG_PX;
    PanelSpec::child(from, true, move |id, _, _| ColorDialog::new(id, accent))
        .sized(Vec2::new(w as f32 / 1000.0, h as f32 / 1000.0), (w, h))
        // 答えて閉じるもの。ナビゲーションバーは付けない。
        .without_nav_bar()
}

/// 自分でパーソナルカラーを作るダイアログ（[`appearance`] の `accent-custom` から開く子パネル）。スライダーで作った色を
/// 見本で確かめ、`color-ok` でパーソナルカラーにして履歴に覚える（[`Setting::CustomColor`]）。`color-cancel` か、外側を
/// 押すと何も変えずに閉じる。
pub struct ColorDialog {
    panel: PanelId,
    /// 作りかけの色。
    pub accent: Accent,
}

impl ColorDialog {
    pub fn new(panel: PanelId, accent: Accent) -> Self {
        Self { panel, accent }
    }
}

impl Render for ColorDialog {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = Theme::current(cx);
        let (panel, draft) = (self.panel, self.accent);
        // 見本は、今のテーマを作りかけの色にしたもの（決めるまで、ほかの画面の色は変えない）。
        let mode = cx.try_global::<Mode>().copied().unwrap_or_default();
        let (w, _) = COLOR_DIALOG_PX;
        let width = w as f32 - 2.0 * (components::PANEL_MARGIN + spacing::XL);
        let this = cx.entity();
        let sliders = components::accent_sliders(
            &theme,
            draft,
            width,
            |name| i18n::t(cx, &format!("color.{name}")),
            move |a, _, cx| {
                this.update(cx, |dialog, cx| {
                    dialog.accent = a;
                    cx.notify();
                })
            },
        );
        let close = move |cx: &mut App| {
            let control = cx.default_global::<PanelControl>();
            let commands = &mut control.panels.entry(panel).or_default().commands;
            commands.push(PanelCommand::Close);
        };
        let ok = components::button(
            &theme,
            "color-ok",
            i18n::t(cx, "common.ok"),
            Variant::Primary,
        )
        .on_click(move |_, _, cx| {
            push(Setting::CustomColor(personal(draft)), cx);
            close(cx)
        });
        let cancel = components::button(
            &theme,
            "color-cancel",
            i18n::t(cx, "common.cancel"),
            Variant::Default,
        )
        .on_click(move |_, _, cx| close(cx));
        components::card_panel(&theme)
            .flex()
            .flex_col()
            .items_center()
            .justify_center()
            .gap(px(spacing::XL))
            .p(px(spacing::XL))
            .child(
                div()
                    .text_size(px(font_size::LG))
                    .font_weight(FontWeight::BOLD)
                    .child(i18n::t(cx, "color.custom")),
            )
            .child(components::theme_preview(&Theme::with(mode, draft)))
            .child(sliders)
            .child(
                div()
                    .w_full()
                    .flex()
                    .flex_col()
                    .gap(px(spacing::SM))
                    .children([ok.w_full(), cancel.w_full()]),
            )
    }
}

/// 文字盤の見本（中央）の直径と、使っている文字盤を囲む輪の太さ・見本との間（px。中央の大きさでの値）。
const FACE_PX: f32 = 200.0;
const FACE_RING: f32 = 4.0;
const FACE_RING_GAP: f32 = 6.0;

/// [`watch_faces`] の状態。
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct WatchFaces {
    /// 絞り込む種類（`None` = すべて）。
    pub face: Option<WristFace>,
    /// 中央に見せているデザイン（`None` = 今使っているもの）。
    pub at: Option<WristDesign>,
    /// 種類の選択肢を開いている。
    pub open: bool,
}

impl WatchFaces {
    /// 並ぶデザイン（種類で絞り込んだもの。表の順）。
    pub fn designs(&self) -> Vec<WristDesign> {
        let kinds = WristFace::ALL.iter();
        let kinds = kinds.filter(|&&f| self.face.is_none_or(|only| only == f));
        kinds.flat_map(|f| f.designs().iter().copied()).collect()
    }

    /// 中央に見せるデザイン: 見ているもの。並びに無ければ（絞り込みを替えた）使っているもの `used`、それも無ければ先頭。
    pub fn shown(&self, used: WristDesign) -> WristDesign {
        let designs = self.designs();
        let wanted = [self.at, Some(used)].into_iter().flatten();
        let found = wanted.into_iter().find(|d| designs.contains(d));
        found.unwrap_or(designs[0])
    }
}

/// 文字盤の見本 1 つ（一辺 `side` の正方形。`watch-face-<名前>`）。使っているものは、周りを塗りの色の輪で囲む。
fn face(design: WristDesign, used: bool, side: f32, theme: &Theme, cx: &App) -> Div {
    let k = side / FACE_PX;
    let (ring, gap) = (FACE_RING * k, FACE_RING_GAP * k);
    let color = if used { theme.accent_solid } else { 0 };
    let d = side - 2.0 * (ring + gap);
    let dial = match cx.try_global::<FacePreview>() {
        Some(preview) => preview.0(design, d, cx),
        None => div()
            .size(px(d))
            .rounded(px(radius::FULL))
            .circular_corners()
            .bg(rgba(theme.card)),
    };
    div()
        .debug_selector(move || format!("watch-face-{}", design.key()))
        .size(px(side))
        .flex()
        .items_center()
        .justify_center()
        .rounded(px(radius::FULL))
        // 輪は正円（全域のスーパーエリプスからは外す）。
        .circular_corners()
        .border(px(ring))
        .border_color(rgba(color))
        .child(dial)
}

/// wrist の文字盤: 上から種類の絞り込み（`watch-kind` → `watch-kind-all` / `-<種類>`）、弧に並ぶ見本（`watch-faces`。押したまま
/// 左右に動かす。端はつながる）、中央のデザインの名前（左右の `watch-prev` / `watch-next` でも回す）、`watch-set`（押すとその
/// デザインにする。使っているデザインでは押せない見た目で「設定中」）。使っているデザインの見本は輪で囲む。
pub fn watch_faces(
    state: WatchFaces,
    set: impl Fn(WatchFaces, &mut Window, &mut App) + Clone + 'static,
    window: &mut Window,
    cx: &mut App,
) -> Div {
    let theme = Theme::current(cx);
    let used = settings(cx).wrist_design;
    let designs = state.designs();
    let shown = state.shown(used);
    let at = designs.iter().position(|&d| d == shown).unwrap_or(0);
    let len = designs.len();

    // 種類の絞り込み。
    let kind = |face: Option<WristFace>, cx: &App| match face {
        Some(f) => i18n::t(cx, &format!("wrist.face.{}", f.key())),
        None => i18n::t(cx, "watch.all"),
    };
    let kinds = [None]
        .into_iter()
        .chain(WristFace::ALL.iter().copied().map(Some));
    let options: Vec<_> = kinds
        .map(|face| {
            let set = set.clone();
            let id = format!("watch-kind-{}", face.map_or("all", WristFace::key));
            components::select_option(&theme, id, kind(face, cx), face == state.face).on_click(
                move |_, window, cx| {
                    let next = WatchFaces {
                        face,
                        open: false,
                        ..state
                    };
                    set(next, window, cx)
                },
            )
        })
        .collect();
    let toggle = |open: bool| {
        let set = set.clone();
        move |window: &mut Window, cx: &mut App| set(WatchFaces { open, ..state }, window, cx)
    };
    let (show, dismiss) = (toggle(true), toggle(false));
    let popover = components::dropdown_popover(
        &theme,
        window,
        cx,
        "watch-kind",
        state.open,
        false,
        options,
        move |_: &MouseDownEvent, window, cx| dismiss(window, cx),
    );
    let kinds = components::dropdown(
        &theme,
        "watch-kind",
        Variant::OnBackground,
        None,
        kind(state.face, cx),
    )
    .on_click(move |_, window, cx| show(window, cx))
    .child(popover);

    // 弧に並ぶ見本。中央に見せるデザインを替えるのは `pick`（番号は並びの中の位置）。
    let pick = {
        let (set, designs) = (set.clone(), designs.clone());
        move |i: usize, window: &mut Window, cx: &mut App| {
            let next = WatchFaces {
                at: Some(designs[i]),
                ..state
            };
            set(next, window, cx)
        }
    };
    let faces = {
        let designs = designs.clone();
        move |i: usize, side: f32, _: &mut Window, cx: &mut App| {
            let theme = Theme::current(cx);
            face(designs[i], designs[i] == used, side, &theme, cx).into_any_element()
        }
    };
    let carousel = components::carousel(
        "watch-faces",
        len,
        at,
        components::arc(FACE_PX),
        faces,
        pick.clone(),
    );
    let turn = |id: &'static str, glyph, tip: SharedString, to: usize| {
        let pick = pick.clone();
        components::icon_button(&theme, id, glyph, tip, Variant::OnBackground)
            .on_click(move |_: &ClickEvent, window, cx| pick(to, window, cx))
    };
    let previous = turn(
        "watch-prev",
        &icons::CARET_LEFT,
        i18n::t(cx, "watch.previous"),
        (at + len - 1) % len,
    );
    let next = turn(
        "watch-next",
        &icons::CARET_RIGHT,
        i18n::t(cx, "watch.next"),
        (at + 1) % len,
    );

    // 使っているデザインでは、押せない見た目で「設定中」。
    let apply = if shown == used {
        components::disabled_button(&theme, "watch-set", i18n::t(cx, "watch.in_use"))
    } else {
        components::button(
            &theme,
            "watch-set",
            i18n::t(cx, "watch.set"),
            Variant::Primary,
        )
        .on_click(move |_, _, cx| push(Setting::WristDesign(shown), cx))
    };
    // 名前と種類。幅は見本と同じに決めておき、名前の長さで左右のボタンが動かないようにする。
    let name = div()
        .w(px(FACE_PX))
        .flex()
        .flex_col()
        .items_center()
        .text_center()
        .child(
            div()
                .debug_selector(|| "watch-name".into())
                .max_w_full()
                .truncate()
                .text_size(px(font_size::LG))
                .font_weight(FontWeight::BOLD)
                .child(i18n::t(cx, &format!("wrist.design.{}", shown.key()))),
        )
        .child(
            div()
                .text_size(px(font_size::SM))
                .text_color(rgba(theme.neutral_text_low))
                .child(kind(Some(shown.face()), cx)),
        );
    // 見本の上下は同じだけ空ける（上は中央の見本の上端、下は弧で下がった隣の下端から）。
    div()
        .flex()
        .flex_col()
        .items_center()
        .child(kinds)
        .child(div().w_full().my(px(spacing::XL)).child(carousel))
        .child(
            div()
                .flex()
                .items_center()
                .gap(px(spacing::XL))
                .child(previous)
                .child(name)
                .child(next),
        )
        .child(div().mt(px(spacing::LG)).child(apply))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn faces_are_filtered_by_kind_and_keep_what_is_shown() {
        let all = WatchFaces::default();
        assert_eq!(all.designs().len(), 9);
        assert_eq!(
            all.shown(WristDesign::Lcd),
            WristDesign::Lcd,
            "使っているもの"
        );
        let looking = WatchFaces {
            at: Some(WristDesign::Railway),
            ..all
        };
        assert_eq!(looking.shown(WristDesign::Lcd), WristDesign::Railway);
        // 絞り込むと、その種類のデザインだけが表の順に並ぶ。
        let digital = WatchFaces {
            face: Some(WristFace::Digital),
            ..looking
        };
        assert_eq!(digital.designs(), WristFace::Digital.designs());
        // 見ていたものが並びから外れたら使っているもの、それも無ければ先頭。
        assert_eq!(digital.shown(WristDesign::Flip), WristDesign::Flip);
        assert_eq!(digital.shown(WristDesign::Dots), WristDesign::Lcd);
    }

    #[test]
    fn colors_round_trip_between_the_setting_and_the_look() {
        let color = PersonalColor {
            lightness: 0.66,
            chroma: 0.21,
            hue: 350.0,
        };
        assert_eq!(personal(accent(color)), color);
        assert_eq!(accent(color).name(), Some("pink"));
    }
}
