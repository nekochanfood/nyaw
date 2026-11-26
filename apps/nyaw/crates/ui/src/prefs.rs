//! 設定の部品: 設定画面とセットアップウィザードが共有する、全体設定（[`Setting`]）選択用の UI 部品（[`appearance`] /
//! [`watch_faces`] / [`accessibility`] / [`keyboard_preview`]）。現在の設定を表示し、選択時に即座に反映する。背景（`background`）上に配置する。開閉等の小状態は呼び出し側が
//! 保持し、変更時は `set` で受け取る（ビューの型を問わない）。詳細は `crates/ui/AGENT.md` の「設定の部品」を参照。

use design::components::{self, Variant};
use design::{Accent, Mode, Theme, font_size, icons, radius, spacing};
use gpui::{
    App, ClickEvent, Context, Div, Entity, FontWeight, IntoElement, MouseDownEvent, Render,
    SharedString, Window, div, prelude::*, px, rgba,
};
use kernel::{
    PanelCommand, PanelId, PersonalColor, SCALE_STEP, Setting, TEXT_SCALE_MAX, TEXT_SCALE_MIN,
    ThemeMode, UI_SCALE_MAX, UI_SCALE_MIN, Vec2, WristDesign, WristFace,
};

use crate::keyboard::KeyboardView;
use crate::{PanelControl, PanelSpec, settings};

/// wrist の文字盤プレビュー（デザインと直径 px）を描画する関数。画面間で直接依存しないため、文字盤を持つ wrist が GPUI の
/// グローバルに設定し、選択画面（[`watch_faces`]）が参照する。未設定の場合、プレビューは背景色の円のみとなる。
#[derive(Clone, Copy)]
pub struct FacePreview(pub fn(WristDesign, f32, &App) -> Div);

impl gpui::Global for FacePreview {}

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

/// [`appearance`] の内部状態。
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Appearance {
    /// テーマ選択肢のドロップダウンが展開中。
    pub theme_open: bool,
}

/// 外観: 上からテーマプレビュー（`theme-preview`）、テーマ選択（`setting-theme` → `setting-theme-dark` / `-light`）、パーソナルカラー
/// （履歴プレビュー `accent-<名前>` と、カスタムカラー作成 `accent-custom`）。カラー作成ダイアログは `panel` の子パネルとして開く。
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
                crate::set(Setting::Theme(mode), cx);
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

    // 履歴: カスタムカラー（新しい順）の後にプリセットカラーを配置する。
    let custom = now.color_history.into_iter().flatten().map(accent);
    let colors = components::accent_row(
        &theme,
        current,
        &Accent::history(current, custom),
        |name| i18n::t(cx, &format!("color.{name}")),
        |a, _, cx| crate::set(Setting::PersonalColor(personal(a)), cx),
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

/// カラー作成ダイアログのサイズ（px。1px = 1mm）。縦に見出し・プレビュー・スライダー 3 本・ボタン 2 個（横 1 列）を配置する。
pub const COLOR_DIALOG_PX: (u32, u32) = (480, 674);

/// カラー作成ダイアログを `from` の子パネルとして開く起動仕様。`accent` を初期値として編集を開始する。
fn color_dialog(from: PanelId, accent: Accent) -> PanelSpec {
    let (w, h) = COLOR_DIALOG_PX;
    PanelSpec::child(from, true, move |id, _, _| ColorDialog::new(id, accent))
        .sized(Vec2::new(w as f32 / 1000.0, h as f32 / 1000.0), (w, h))
        // 応答して閉じるダイアログ。ナビゲーションバーは付けない。
        .without_nav_bar()
}

/// カスタムパーソナルカラー作成ダイアログ（[`appearance`] の `accent-custom` から開く子パネル）。スライダーで作成した色を
/// プレビューで確認し、`color-ok` でパーソナルカラーに設定して履歴に保存する（[`Setting::CustomColor`]）。`color-cancel` か外側の
/// クリックで変更せずに閉じる。
pub struct ColorDialog {
    panel: PanelId,
    /// 編集中の色。
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
        // プレビューは現在のテーマに編集中の色を適用したもの（確定まで他の画面の色は変更しない）。
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
            crate::set(Setting::CustomColor(personal(draft)), cx);
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
            // 見出しは左揃え、ボタンは横 1 列でキャンセルを左・OK を右に配置（`components::dialog` と同様）。
            .child(
                div()
                    .self_start()
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
                    .gap(px(spacing::SM))
                    .children([cancel.flex_1(), ok.flex_1()]),
            )
    }
}

/// テキストサンプルを配置するコンテナの高さ（px）。最大サイズのテキストでもサンプルが収まり、サイズ変更時に下の項目が動かない。
pub const SAMPLE_HEIGHT: f32 = 200.0;
/// 倍率行に配置するスライダーの長さ（px）。
const SCALE_SLIDER: f32 = 240.0;

/// 倍率 `scale`（`min..=max`）をスライダー上の位置（0..=1）に変換する。
fn stop(scale: f32, min: f32, max: f32) -> f32 {
    ((scale - min) / (max - min)).clamp(0.0, 1.0)
}

/// スライダー位置 `at`（0..=1）に最も近い刻み（[`SCALE_STEP`]）の倍率を返す。
fn scale_at(at: f32, min: f32, max: f32) -> f32 {
    let steps = ((max - min) / SCALE_STEP).round();
    min + (at.clamp(0.0, 1.0) * steps).round() * SCALE_STEP
}

/// 同一刻みの倍率かどうかを判定する（刻み計算の浮動小数点誤差を許容する）。
fn same_scale(a: f32, b: f32) -> bool {
    (a - b).abs() < SCALE_STEP / 2.0
}

fn percent(scale: f32) -> SharedString {
    format!("{:.0}%", scale * 100.0).into()
}

/// テキストサンプル（`a11y-sample`）: アイコン付き見出し・本文・補足のカード。テキストとアイコンには現在の設定（テキストサイズ・太字）が
/// そのまま適用される。`zoom` はサンプルの拡大率（操作中の UI スケールを、確定後に見えるサイズで表示する）。
fn text_sample(theme: &Theme, zoom: f32, cx: &App) -> Div {
    let s = |v: f32| px(v * zoom);
    let line = |size: f32| div().text_size(s(size));
    components::card(theme)
        .debug_selector(|| "a11y-sample".into())
        .flex_none()
        .flex()
        .flex_col()
        .gap(s(spacing::XS))
        .px(s(spacing::XL))
        .py(s(spacing::LG))
        .rounded(s(radius::LG))
        .whitespace_nowrap()
        .child(
            line(font_size::LG)
                .flex()
                .items_center()
                .gap(s(spacing::SM))
                .font_weight(FontWeight::BOLD)
                .child(design::icon(&icons::TEXT_AA).size(s(font_size::LG)))
                .child(i18n::t(cx, "accessibility.sample_title")),
        )
        .child(line(font_size::MD).child(i18n::t(cx, "accessibility.sample_body")))
        .child(
            line(font_size::SM)
                .text_color(rgba(theme.neutral_text_low))
                .child(i18n::t(cx, "accessibility.sample_caption")),
        )
}

/// アクセシビリティ: 上にテキストサンプル（`a11y-preview` コンテナ内に `a11y-sample`）、下に 3 つの対等な項目。テキストサイズ
/// （`setting-text-size`）は操作時に即座に反映する。UI スケール（`setting-ui-scale`）は確定時にパネル自体のサイズが変わるため、
/// 操作中はサンプルのみを拡縮して表示し、解放時に確定する。太字（`setting-bold-text`）はスイッチ。
/// 倍率は刻み（[`SCALE_STEP`]）にスナップする。操作中の値はこの部品が内部に保持する（呼び出し側に状態は不要）。
pub fn accessibility(window: &mut Window, cx: &mut App) -> Div {
    let theme = Theme::current(cx);
    let now = settings(cx);
    // 操作中の UI スケールと解放済みフラグ（解放時に確定とし、設定が追いつくまでサンプルと値はそのまま表示する）。
    let draft = window.use_keyed_state("a11y-ui-draft", cx, |_, _| None::<(f32, bool)>);
    let pending = *draft.read(cx);
    let pending = pending.filter(|&(to, released)| !(released && same_scale(to, now.ui_scale)));
    if pending.is_none() && draft.read(cx).is_some() {
        draft.update(cx, |d, _| *d = None);
    }
    let ui = pending.map_or(now.ui_scale, |(to, _)| to);

    let (min, max) = (TEXT_SCALE_MIN, TEXT_SCALE_MAX);
    let text_scale = now.text_scale;
    let text = components::slider_item(
        &theme,
        "setting-text-size",
        &icons::TEXT_AA,
        i18n::t(cx, "accessibility.text_size"),
        percent(text_scale),
        components::slider(
            &theme,
            "setting-text-size-slider",
            stop(text_scale, min, max),
            move |at, _, cx| {
                let to = scale_at(at, min, max);
                if !same_scale(to, text_scale) {
                    crate::set(Setting::TextScale(to), cx)
                }
            },
        )
        .length(SCALE_SLIDER),
    );

    let (min, max) = (UI_SCALE_MIN, UI_SCALE_MAX);
    let (moving, done) = (draft.clone(), draft);
    let ui_scale = components::slider_item(
        &theme,
        "setting-ui-scale",
        &icons::FRAME_CORNERS,
        i18n::t(cx, "accessibility.ui_scale"),
        percent(ui),
        components::slider(
            &theme,
            "setting-ui-scale-slider",
            stop(ui, min, max),
            move |at, _, cx| {
                let to = (scale_at(at, min, max), false);
                moving.update(cx, |d, cx| {
                    if *d != Some(to) {
                        *d = Some(to);
                        cx.notify();
                    }
                })
            },
        )
        .on_release(move |_, cx| {
            let to = done.update(cx, |d, cx| {
                cx.notify();
                d.as_mut().map(|(to, released)| {
                    *released = true;
                    *to
                })
            });
            if let Some(to) = to {
                crate::set(Setting::UiScale(to), cx)
            }
        })
        .length(SCALE_SLIDER),
    );

    let bold = components::switch_item(
        &theme,
        "setting-bold-text",
        &icons::TEXT_B,
        i18n::t(cx, "accessibility.bold_text"),
        now.bold_text,
    )
    .on_click(move |_, _, cx| crate::set(Setting::BoldText(!now.bold_text), cx));

    div()
        .flex()
        .flex_col()
        .gap(px(spacing::LG))
        .child(
            div()
                .debug_selector(|| "a11y-preview".into())
                .flex_none()
                .w_full()
                .h(px(SAMPLE_HEIGHT))
                .flex()
                .items_center()
                .justify_center()
                // 拡大しすぎたサンプルはコンテナでクリップする（下の項目に重ねない）。
                .overflow_hidden()
                .child(text_sample(&theme, ui / now.ui_scale, cx)),
        )
        .child(components::list(&theme, [text, ui_scale, bold]))
}

/// キーボードプレビューコンテナの高さ（px）。設定画面幅ではフルサイズは幅一杯、他の種類は高さ一杯に収まる。
const KEYBOARD_PREVIEW_HEIGHT: f32 = 200.0;

/// [`keyboard_preview`] に表示するキーボードを生成する（呼び出し側が保持する）。`panel` はプレビューを配置するパネル（キー音の再生元）。
pub fn new_keyboard_preview(panel: PanelId, cx: &mut App) -> Entity<KeyboardView> {
    cx.new(|cx| KeyboardView::preview(panel, cx))
}

/// キーボードプレビュー（高さ固定のコンテナ `keyboard-preview` の中央に、背景 `keyboard-preview-card` とキー `virtual-keyboard`）:
/// 現在の設定の種類・OS 配列のキーボードを、表示時と同じ背景に載せ、コンテナに収まるよう縮小表示する（幅変更時も
/// はみ出さない）。キーは押下可能で、キー音設定に従って再生される（入力は送出しない）。種類変更時も下の項目は動かない。
pub fn keyboard_preview(keys: &Entity<KeyboardView>, cx: &App) -> Div {
    div()
        .debug_selector(|| "keyboard-preview".into())
        .flex_none()
        .w_full()
        .h(px(KEYBOARD_PREVIEW_HEIGHT))
        .child(crate::keyboard::preview(keys, cx))
}

/// 文字盤プレビュー（中央）の直径と、使用中文字盤を囲むリングの太さ・プレビューとの間隔（px。中央サイズでの値）。
const FACE_PX: f32 = 200.0;
const FACE_RING: f32 = 4.0;
const FACE_RING_GAP: f32 = 6.0;

/// [`watch_faces`] の内部状態。
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct WatchFaces {
    /// フィルタする種類（`None` = 全種類）。
    pub face: Option<WristFace>,
    /// 中央に表示中のデザイン（`None` = 現在使用中のもの）。
    pub at: Option<WristDesign>,
    /// 種類選択ドロップダウンが展開中。
    pub open: bool,
}

impl WatchFaces {
    /// 表示するデザイン一覧（種類でフィルタ済み。定義順）。
    pub fn designs(&self) -> Vec<WristDesign> {
        let kinds = WristFace::ALL.iter();
        let kinds = kinds.filter(|&&f| self.face.is_none_or(|only| only == f));
        kinds.flat_map(|f| f.designs().iter().copied()).collect()
    }

    /// 中央に表示するデザイン: 閲覧中のもの。一覧にない場合（フィルタ変更時）は使用中の `used`、それもなければ先頭。
    pub fn shown(&self, used: WristDesign) -> WristDesign {
        let designs = self.designs();
        let wanted = [self.at, Some(used)].into_iter().flatten();
        let found = wanted.into_iter().find(|d| designs.contains(d));
        found.unwrap_or(designs[0])
    }
}

/// 文字盤プレビュー 1 つ（一辺 `side` の正方形。`watch-face-<名前>`）。使用中のものは控えめなテキスト色のリングで囲む（ライトは
/// 濃い灰、ダークは明るい灰。塗り色だと文字盤デザインの一部に見えるものがあり、テキスト色そのままでは文字盤より目立つ）。
fn face(design: WristDesign, used: bool, side: f32, theme: &Theme, cx: &App) -> Div {
    let k = side / FACE_PX;
    let (ring, gap) = (FACE_RING * k, FACE_RING_GAP * k);
    let color = if used { theme.neutral_text_low } else { 0 };
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

/// wrist の文字盤: 上から種類フィルタ（`watch-kind` → `watch-kind-all` / `-<種類>`）、弧状に並ぶプレビュー（`watch-faces`。押下したまま
/// 左右に移動。端はループする）、中央のデザイン名（左右の `watch-prev` / `watch-next` でも切替可能）、`watch-set`（押下で該当
/// デザインに設定。使用中デザインでは無効表示で「設定中」）。使用中デザインのプレビューはリングで囲む。
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

    // 種類のフィルタ。
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

    // 弧状に並ぶプレビュー。中央に表示するデザインの切替は `pick`（引数は一覧内のインデックス）。
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

    // 使用中デザインでは無効状態で「設定中」を表示する。
    let apply = if shown == used {
        components::disabled_button(&theme, "watch-set", i18n::t(cx, "watch.in_use"))
    } else {
        components::button(
            &theme,
            "watch-set",
            i18n::t(cx, "watch.set"),
            Variant::Primary,
        )
        .on_click(move |_, _, cx| crate::set(Setting::WristDesign(shown), cx))
    };
    // 名前と種類。幅をプレビューと同一に固定し、名前の長さで左右のボタンが移動しないようにする。
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
    // プレビューの上下に同じ余白を設ける（上は中央プレビューの上端、下は弧で下がった隣接プレビューの下端から）。
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
            "使用中のデザイン"
        );
        let looking = WatchFaces {
            at: Some(WristDesign::Railway),
            ..all
        };
        assert_eq!(looking.shown(WristDesign::Lcd), WristDesign::Railway);
        // フィルタすると、該当種類のデザインのみが定義順に並ぶ。
        let digital = WatchFaces {
            face: Some(WristFace::Digital),
            ..looking
        };
        assert_eq!(digital.designs(), WristFace::Digital.designs());
        // 閲覧中のものが一覧から外れた場合は使用中のもの、それもなければ先頭を表示する。
        assert_eq!(digital.shown(WristDesign::Flip), WristDesign::Flip);
        assert_eq!(digital.shown(WristDesign::Dots), WristDesign::Lcd);
    }

    /// スライダー位置と倍率は、範囲の端から端まで刻みごとに往復可能（任意の位置で範囲内の刻みにスナップする）。
    #[test]
    fn scale_stops_snap_to_steps_and_round_trip() {
        for (min, max) in [
            (TEXT_SCALE_MIN, TEXT_SCALE_MAX),
            (UI_SCALE_MIN, UI_SCALE_MAX),
        ] {
            assert!(same_scale(scale_at(0.0, min, max), min));
            assert!(same_scale(scale_at(1.0, min, max), max));
            assert!(
                same_scale(scale_at(-3.0, min, max), min),
                "範囲外は端に丸める"
            );
            assert!(same_scale(scale_at(9.0, min, max), max));
            let steps = ((max - min) / SCALE_STEP).round() as usize;
            for i in 0..=steps {
                let scale = min + i as f32 * SCALE_STEP;
                let at = stop(scale, min, max);
                assert!(same_scale(scale_at(at, min, max), scale), "{scale}");
                // 刻みの間の値は近い刻みにスナップする。
                let nudged = at + 0.3 / steps as f32;
                assert!(same_scale(scale_at(nudged, min, max), scale), "{scale}");
            }
            assert!(same_scale(scale_at(stop(1.0, min, max), min, max), 1.0));
        }
        assert_eq!(percent(1.0), "100%");
        assert_eq!(percent(0.7 + 2.0 * SCALE_STEP), "90%");
        assert_eq!(percent(1.3000001), "130%");
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
