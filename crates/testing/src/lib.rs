//! Layer 5 画面: Testing パネル。UI 層（`ui`）の上に載る画面の1つで、一連の機能を1画面で試せるテスト用の画面。
//! E2E テストもこの画面を操作して検証する。
//!
//! - VR: クリック数・HMD / 手 / トラッカーへの取り付け・取り外し・ロック・メニューボタンと掴みの表示
//! - GPUI: 切り替え・押している間の見た目・ドラッグ＆ドロップ・スクロールする一覧・フォーカスと文字入力・ポップアップメニュー
//! - ポインタの方式: [`PointerTest`] でマウス・タッチのエミュレートを切り替える
//! - レーザー: [`SmoothingTest`] でレーザーの手ブレ抑えの強さと線の長さを変え、小さな的で揺れと遅れを見る
//! - ツールチップ: [`TooltipTest`] で、カーソルに付いて出る説明（向き・猶予・文字の差し替え・幅の伸び縮み）を見る
//! - 積み重ね: [`NavTest`] で、右から入る画面（push 遷移）と、押したまま右から左へ動かして戻るスワイプバック（設定画面の中身と同じ部品）
//! - 親子: [`DialogTest`] がダイアログ（[`Dialog`]）を子として開く。開いている間の親は暗くなり、子を掴むと親も付いてくる
//! - 描画: [`TestingAnim`]（毎フレーム変わる）・[`TestingLod`]（今の LOD を大きく出す）・[`TestingFont`]（フォントの見本）・[`TestingScrollbar`]（縦・横・両方のスクロールバー）
//! - スケルタル入力: [`SkeletonTest`] で、手の関節をギズモで描き、親指とほかの指でつまめているかを見る
//! - 音: [`AudioTest`] で、パネルのスピーカーから立体的に鳴る音（同時再生・立体化のオプトアウト・モノラル / ステレオ）を聞く
//! - 負荷: [`StressTest`] で上限いっぱいまでパネルを出し、閉じる・ちりばめる・並べる
//!
//! 要素はすべて `debug_selector` で名前を付けてある（名前 = `id`）。

use std::cell::RefCell;
use std::rc::Rc;
use std::time::{Instant, SystemTime};

use design::components::{self, Variant};
use design::{Mode, Theme, font_size, radius, spacing};
use gpui::{
    App, ClickEvent, Context, Div, FocusHandle, FontWeight, KeyDownEvent, ScrollDelta,
    ScrollHandle, ScrollWheelEvent, SharedString, Stateful, WeakEntity, Window, anchored, deferred,
    div, hsla, prelude::*, px, relative, rgb, rgba,
};
use kernel::vr::{Device, Hand, Pose, Quat, Vec2, Vec3};
use kernel::{Language, PanelCommand, PanelId, Setting};
use ui::{PanelControl, PanelSpec, settings};

mod emoji;
pub use emoji::EmojiTest;
mod wizard;
pub use wizard::WizardTest;
mod logo;
pub use logo::LogoTest;
mod superellipse;
pub use superellipse::SuperellipseTest;
mod keyboard;
pub use keyboard::KeyboardTest;
mod desktop;
pub use desktop::{DesktopTest, MirrorPreview, MirrorShared, MirrorTest, spawn_mirror_test};
mod slide;
pub use slide::SlideTest;
mod audio;
pub use audio::AudioTest;
mod nyapps;
pub use nyapps::NyappTest;
mod skeleton;
pub use skeleton::{
    HandTuningTest, PINCH_DISTANCE, SKELETON_PX, SKELETON_SIZE, SkeletonTest, TUNING_PX,
    TUNING_SIZE, pinches,
};
mod anim;
mod dialog;
mod font;
mod locale;
mod lod;
mod nav;
mod panel;
mod pointer;
mod scrollbar;
mod smoothing;
mod stress;
mod tooltip;
mod transparent;
pub use anim::*;
pub use dialog::*;
pub use font::*;
pub use locale::*;
pub use lod::*;
pub use nav::*;
pub use pointer::*;
pub use scrollbar::*;
pub use smoothing::*;
pub use stress::*;
pub use tooltip::*;
pub use transparent::*;
#[cfg(test)]
mod test_util;

/// 「HMD に取り付け」で使う相対姿勢: 顔の正面 1m。
pub const HMD_OFFSET: Pose = Pose::new(Vec3::new(0.0, 0.0, -1.0), Quat::IDENTITY);
/// 「手・トラッカーに取り付け」で使う相対姿勢: 0.3m 前（手首の時計のような配置）。
pub const HAND_OFFSET: Pose = Pose::new(Vec3::new(0.0, 0.0, -0.3), Quat::IDENTITY);

/// 色は `design` のダークテーマの役割から引く（`color_at` は 0xRRGGBB なのでアルファを落とす）。
const THEME: Theme = Theme::of(Mode::Dark);
const fn rgb24(rgba: u32) -> u32 {
    rgba >> 8
}
/// カード 1 枚のパネルで使う地の色。
pub const PANEL_BG: u32 = rgb24(THEME.card);
/// Testing パネルのカードの外側にある地の色。
pub const PANEL_GROUND_BG: u32 = rgb24(THEME.background);
/// パネルの縁の影の余白。地はこの内側から始まる。
pub use design::components::PANEL_MARGIN;
pub const BUTTON_BG: u32 = rgb24(THEME.control);
/// レーザーが当たっている間のボタン色。VR 上でどこを指しているかの視覚的フィードバック。
pub const BUTTON_HOVER_BG: u32 = rgb24(THEME.control_hover);
/// 有効になっている切り替えボタン・表示や、押している間のボタンの色。
pub const BUTTON_ACTIVE_BG: u32 = rgb24(THEME.control_active);
/// ドラッグ中の値を落とせる場所の上にいるときの色。
pub const DROP_TARGET_BG: u32 = 0x30a050;
/// フォーカスのある入力欄の色。
pub const FIELD_FOCUS_BG: u32 = rgb24(THEME.accent_bg_active);
/// ダイアログの背景色（親より明るくして重なりが見えるように）。
pub const DIALOG_BG: u32 = rgb24(THEME.card);
/// 画面の地（`components::panel`）。設定画面の中身（右側）の地。
pub const BACKGROUND: u32 = rgb24(THEME.background);
/// Testing パネルの大きさ（m）と解像度（px）。ボタンが高い（最小ヒット領域）ので、既定の 0.8m × 0.6m では入りきらない。
pub const TESTING_SIZE: Vec2 = Vec2::new(1.2, 1.0);
pub const TESTING_PX: (u32, u32) = (1200, 1000);

/// パネル `from` から開く Testing パネルの起動仕様。開いたら `opened` で PanelId を知らせる。
pub fn spec(from: PanelId, opened: impl FnOnce(PanelId, &mut gpui::App) + 'static) -> PanelSpec {
    PanelSpec::spawn(from, move |id, _, cx| {
        opened(id, cx);
        Testing::new(id)
    })
    .sized(TESTING_SIZE, TESTING_PX)
    .titled("testing.open.testing")
}
/// セットアップウィザードを開く仕様（`from` の隣）。画面どうしは直接依存しないので、main が [`spec`] の `opened` で
/// グローバルに入れる。入っているときだけ Testing に `setup-wizard`（強制的にやり直す）が出る。
#[derive(Clone, Copy)]
pub struct SetupWizardSpawn(pub fn(PanelId) -> PanelSpec);
impl gpui::Global for SetupWizardSpawn {}

/// スクロールする一覧の行数と1行の高さ。
pub const LIST_ROWS: usize = 30;
const ROW_PX: f32 = 24.0;
#[derive(Default)]
pub struct Testing {
    panel: PanelId,
    pub clicks: usize,
    /// 受け取ったスクロール量の合計（行。下向きが負）。
    pub scrolled: f32,
    pub toggled: bool,
    /// ドロップ先に落とされた回数。
    pub dropped: usize,
    /// 入力欄に打たれた文字。
    pub text: String,
    pub menu_open: bool,
    /// ポップアップメニューで選んだ項目。
    pub picked: Option<&'static str>,
    /// スクロールする一覧の状態（`offset().y` が負 = 下へスクロール済み）。
    pub list: ScrollHandle,
    focus: Option<FocusHandle>,
}

impl Testing {
    pub fn new(panel: PanelId) -> Self {
        Self {
            panel,
            ..Self::default()
        }
    }
}

/// ドラッグ＆ドロップで運ぶ値。
struct Chip;

/// ドラッグ中にポインタに付いてくる見た目。
struct ChipPreview;

impl Render for ChipPreview {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div().size_4().bg(rgb(BUTTON_ACTIVE_BG))
    }
}

/// 押せるボタン（`design` の部品）。`active` は切り替えボタンが有効なとき。
fn button(
    theme: &Theme,
    id: &'static str,
    label: impl Into<SharedString>,
    active: bool,
) -> components::Button {
    themed_button(theme, id, label, active)
}

fn themed_button(
    theme: &Theme,
    id: &'static str,
    label: impl Into<SharedString>,
    active: bool,
) -> components::Button {
    components::button(theme, id, label.into(), Variant::Default).selected(active)
}

/// ボタンを押したらパネルへの操作要求を積む。
fn command(
    panel: PanelId,
    c: PanelCommand,
) -> impl Fn(&ClickEvent, &mut Window, &mut App) + 'static {
    move |_, _, cx| ui::command(panel, c.clone(), cx)
}

/// 押すと全体の設定の変更を積むクリックハンドラ。
fn change(s: Setting) -> impl Fn(&ClickEvent, &mut Window, &mut App) + 'static {
    move |_, _, cx| ui::set(s, cx)
}

fn spawn(panel: PanelId) -> impl Fn(&ClickEvent, &mut Window, &mut App) + 'static {
    move |_, _, cx| {
        cx.default_global::<PanelControl>().spawns.push(
            PanelSpec::spawn(panel, |id, _, _| Testing::new(id))
                .sized(TESTING_SIZE, TESTING_PX)
                .titled("testing.open.testing"),
        )
    }
}

fn spawn_dialog_test(panel: PanelId) -> impl Fn(&ClickEvent, &mut Window, &mut App) + 'static {
    move |_, _, cx| {
        cx.default_global::<PanelControl>().spawns.push(
            PanelSpec::spawn(panel, |id, _, _| DialogTest::new(id)).titled("testing.open.dialog"),
        )
    }
}

fn spawn_pointer_test(panel: PanelId) -> impl Fn(&ClickEvent, &mut Window, &mut App) + 'static {
    move |_, _, cx| {
        cx.default_global::<PanelControl>().spawns.push(
            PanelSpec::spawn(panel, |id, _, _| PointerTest::new(id)).titled("testing.open.pointer"),
        )
    }
}

fn spawn_transparent(panel: PanelId) -> impl Fn(&ClickEvent, &mut Window, &mut App) + 'static {
    move |_, _, cx| {
        cx.default_global::<PanelControl>().spawns.push(
            PanelSpec::spawn(panel, |id, _, _| TestingTransparent::new(id))
                .titled("testing.open.transparent"),
        )
    }
}

/// 隣に `build` の画面を開く。`title` はナビゲーションバーのタイトル（i18n のキー）。
fn spawn_view<V: Render>(
    panel: PanelId,
    title: &'static str,
    build: fn(PanelId, &mut App) -> V,
) -> impl Fn(&ClickEvent, &mut Window, &mut App) + 'static {
    move |_, _, cx| {
        cx.default_global::<PanelControl>()
            .spawns
            .push(PanelSpec::spawn(panel, move |id, _, cx| build(id, cx)).titled(title))
    }
}

/// design のカタログを隣に開く（Close はこのパネルを閉じる）。
fn spawn_catalog(panel: PanelId) -> impl Fn(&ClickEvent, &mut Window, &mut App) + 'static {
    move |_, _, cx| {
        cx.default_global::<PanelControl>().spawns.push(
            PanelSpec::spawn(panel, |id, _, cx| {
                // 動きの見本を毎フレーム描く。
                cx.default_global::<PanelControl>()
                    .panels
                    .entry(id)
                    .or_default()
                    .live = true;
                design::catalog::Catalog::default()
            })
            .titled("testing.open.catalog"),
        )
    }
}

/// 言語の切り替えボタンが次に選ぶ言語（i18n の対応言語を順に回る）。
fn next_language(l: Language) -> Language {
    ui::language(ui::lang(l).next())
}

fn attach_offset(panel: PanelId, base: Pose) -> Pose {
    Pose::new(
        base.position + Vec3::new(1.2 * panel as f32, 0.0, 0.0),
        base.orientation,
    )
}

fn row() -> Div {
    div().flex().flex_wrap().gap_2().items_center()
}

/// 見出し付きのカード。中身は縦に並べる。
fn section(theme: &Theme, title: impl Into<SharedString>) -> Div {
    components::card(theme)
        .p(px(spacing::LG))
        .flex()
        .flex_col()
        .gap(px(spacing::LG))
        .child(
            div()
                .text_size(px(font_size::MD))
                .font_weight(FontWeight::BOLD)
                .child(title.into()),
        )
}

/// 小見出しと、その下の中身。
fn labeled(theme: &Theme, label: impl Into<SharedString>, body: impl IntoElement) -> Div {
    div()
        .flex()
        .flex_col()
        .gap(px(spacing::XS))
        .child(
            div()
                .text_size(px(font_size::SM))
                .text_color(rgba(theme.neutral_text_low))
                .child(label.into()),
        )
        .child(body)
}

/// 押せない状態の表示。ボタンと見分けられるよう低く、先頭に点を置く（点は入っている間だけ強調色）。
/// 面の色は入っている間 `control-active`、それ以外 `control`（テストは中心の色を見る）。
fn indicator(
    theme: &Theme,
    id: &'static str,
    label: impl Into<SharedString>,
    on: bool,
) -> Stateful<Div> {
    div()
        .id(id)
        .debug_selector(move || id.into())
        .h(px(36.0))
        .px(px(spacing::MD))
        .flex()
        .items_center()
        .gap(px(spacing::SM))
        .rounded(px(radius::FULL))
        .bg(rgba(if on {
            theme.control_active
        } else {
            theme.control
        }))
        .child(
            div()
                .size(px(10.0))
                .rounded(px(radius::FULL))
                .bg(rgba(if on {
                    theme.accent_solid
                } else {
                    theme.neutral_text_low
                })),
        )
        .child(label.into())
}
