//! Layer 5 画面: Testing パネル。UI 層（`ui`）上の画面の一つで、各種機能を単一画面で検証するためのテスト画面。
//! E2E テストもこの画面を操作して検証する。
//!
//! - VR: クリック回数・HMD / コントローラー / トラッカーへのアタッチ・デタッチ・ロック・メニューボタンとグラブの表示
//! - GPUI: トグル・押下中の外観・ドラッグ＆ドロップ・スクロール可能なリスト・フォーカスとテキスト入力・ポップアップメニュー
//! - ポインタ方式: [`PointerTest`] でマウス・タッチエミュレーションを切り替える
//! - レーザー: [`SmoothingTest`] でレーザーの手ブレ抑制強度と線の長さを調整し、小さなターゲットで揺れと遅延を確認する
//! - ツールチップ: [`TooltipTest`] で、カーソルに追従する説明表示（方向・遅延・テキスト差し替え・幅の伸縮）を確認する
//! - ナビゲーションスタック: [`NavTest`] で、右から入る画面（push 遷移）と、押したまま右から左へスワイプして戻るスワイプバック（設定画面と共通の部品）
//! - 親子関係: [`DialogTest`] がダイアログ（[`Dialog`]）を子として表示する。表示中の親は暗転し、子をグラブすると親も追従する
//! - 描画: [`TestingAnim`]（毎フレーム更新）・[`TestingLod`]（現在の LOD を拡大表示）・[`TestingFont`]（フォントサンプル）・[`TestingScrollbar`]（縦・横・双方向スクロールバー）
//! - スケルタル入力: [`SkeletonTest`] で、手の関節をギズモ描画し、親指と他の指によるピンチ判定を確認する
//! - 通知: [`NotificationTest`] で、Windows に実際の通知を送信し、トースト（HUD → 手首 → ダッシュボード）と通知一覧を検証する
//! - 音声: [`AudioTest`] で、パネルスピーカーからの空間音響（同時再生・空間化の無効化・モノラル / ステレオ）を検証する
//! - 角でのサイズ変更: [`ResizeTest`] の角を掴んで動かし、解像度（表示領域）が範囲内で変わり中身が組み直されるのを確認する
//! - 操作ガイド: [`GuideTest`] で、使用中のコントローラの判定と、画像の上で操作部分を囲む円・テール・説明を確認する
//! - レイヤー: [`LayerTest`] で、面に重ねる子・ポップオーバー・ダイアログ・HUD・奥のパネルを出し、重なり方と確定した描画順・奥行きを確認する
//! - 負荷: [`StressTest`] で上限までパネルを生成し、閉じる・ランダム配置・整列を行う
//!
//! すべての要素に `debug_selector` で名前が付与されている（名前 = `id`）。

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
use kernel::{Language, PanelCommand, PanelId, Request, Setting};
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
mod notifications;
pub use notifications::NotificationTest;
mod skeleton;
pub use skeleton::{
    HandTuningTest, PINCH_DISTANCE, SKELETON_PX, SKELETON_SIZE, SkeletonTest, TUNING_PX,
    TUNING_SIZE, pinches,
};
mod guide;
pub use guide::{GUIDE_PX, GUIDE_SIZE, GuideTest, Shown};
mod anim;
mod dialog;
mod font;
mod layers;
mod locale;
mod lod;
mod nav;
mod panel;
mod pointer;
mod resize;
mod scrollbar;
mod smoothing;
mod stress;
mod tooltip;
mod transparent;
pub use anim::*;
pub use dialog::*;
pub use font::*;
pub use layers::*;
pub use locale::*;
pub use lod::*;
pub use nav::*;
pub use pointer::*;
pub use resize::*;
pub use scrollbar::*;
pub use smoothing::*;
pub use stress::*;
pub use tooltip::*;
pub use transparent::*;
#[cfg(test)]
mod test_util;

/// 「HMD に取り付け」で使用する相対姿勢: 正面 1m。
pub const HMD_OFFSET: Pose = Pose::new(Vec3::new(0.0, 0.0, -1.0), Quat::IDENTITY);
/// 「手・トラッカーに取り付け」で使用する相対姿勢: 0.3m 前方（手首の腕時計的な配置）。
pub const HAND_OFFSET: Pose = Pose::new(Vec3::new(0.0, 0.0, -0.3), Quat::IDENTITY);

/// 色は `design` のダークテーマのロールから取得する（`color_at` は 0xRRGGBB のためアルファを除去）。
const THEME: Theme = Theme::of(Mode::Dark);
const fn rgb24(rgba: u32) -> u32 {
    rgba >> 8
}
/// カード 1 枚構成のパネルで使用する背景色。
pub const PANEL_BG: u32 = rgb24(THEME.card);
/// Testing パネルのカード外側の背景色。
pub const PANEL_GROUND_BG: u32 = rgb24(THEME.background);
/// パネル縁のシャドウマージン。背景描画はこの内側から開始される。
pub use design::components::PANEL_MARGIN;
pub const BUTTON_BG: u32 = rgb24(THEME.control);
/// レーザーホバー中のボタン色。VR 上でのポインティング位置を示す視覚フィードバック。
pub const BUTTON_HOVER_BG: u32 = rgb24(THEME.control_hover);
/// 有効状態のトグル・インジケーター、または押下中のボタンの色。
pub const BUTTON_ACTIVE_BG: u32 = rgb24(THEME.control_active);
/// ドラッグ中のアイテムがドロップターゲット上にあるときの色。
pub const DROP_TARGET_BG: u32 = 0x30a050;
/// フォーカス中の入力フィールドの色。
pub const FIELD_FOCUS_BG: u32 = rgb24(THEME.accent_bg_active);
/// ダイアログの背景色（親より明るくし、重なりを視認可能にする）。
pub const DIALOG_BG: u32 = rgb24(THEME.card);
/// 画面の背景色（`components::panel`）。設定画面のコンテンツ領域（右側）の背景。
pub const BACKGROUND: u32 = rgb24(THEME.background);
/// Testing パネルのサイズ（m）と解像度（px）。ボタンの最小ヒット領域が大きいため、既定の 0.8m × 0.6m では収まらない。
pub const TESTING_SIZE: Vec2 = Vec2::new(1.2, 1.0);
pub const TESTING_PX: (u32, u32) = (1200, 1000);

/// パネル `from` から開く Testing パネルの起動仕様。起動後に `opened` で PanelId を通知する。
pub fn spec(from: PanelId, opened: impl FnOnce(PanelId, &mut gpui::App) + 'static) -> PanelSpec {
    PanelSpec::spawn(from, move |id, _, cx| {
        opened(id, cx);
        Testing::new(id)
    })
    .sized(TESTING_SIZE, TESTING_PX)
    .titled("testing.open.testing")
}
/// セットアップウィザードを開く仕様（`from` の隣）。画面間に直接依存がないため、main が [`spec`] の `opened` で
/// グローバルに登録する。登録時のみ Testing に `setup-wizard`（強制再実行）ボタンが表示される。
#[derive(Clone, Copy)]
pub struct SetupWizardSpawn(pub fn(PanelId) -> PanelSpec);
impl gpui::Global for SetupWizardSpawn {}

/// スクロール可能なリストの行数と 1 行の高さ。
pub const LIST_ROWS: usize = 30;
const ROW_PX: f32 = 24.0;
#[derive(Default)]
pub struct Testing {
    panel: PanelId,
    pub clicks: usize,
    /// 累計スクロール量（行単位。下方向が負）。
    pub scrolled: f32,
    pub toggled: bool,
    /// ドロップターゲットへのドロップ回数。
    pub dropped: usize,
    /// 入力フィールドに入力されたテキスト。
    pub text: String,
    pub menu_open: bool,
    /// ポップアップメニューで選択された項目。
    pub picked: Option<&'static str>,
    /// スクロール可能なリストの状態（`offset().y` が負 = 下方向にスクロール済み）。
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

/// ドラッグ＆ドロップで搬送する値。
struct Chip;

/// ドラッグ中にポインタに追従するプレビュー。
struct ChipPreview;

impl Render for ChipPreview {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div().size_4().bg(rgb(BUTTON_ACTIVE_BG))
    }
}

/// クリック可能なボタン（`design` のコンポーネント）。`active` はトグルが有効な場合に true。
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

/// ボタン押下時にパネルへの操作コマンドをキューイングする。
fn command(
    panel: PanelId,
    c: PanelCommand,
) -> impl Fn(&ClickEvent, &mut Window, &mut App) + 'static {
    move |_, _, cx| ui::command(panel, c.clone(), cx)
}

/// クリック時にグローバル設定の変更をキューイングするハンドラ。
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

/// 隣に `build` の画面を開く。`title` はナビゲーションバーのタイトル（i18n キー）。
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

/// design カタログを隣に開く（Close でこのパネルを閉じる）。
fn spawn_catalog(panel: PanelId) -> impl Fn(&ClickEvent, &mut Window, &mut App) + 'static {
    move |_, _, cx| {
        cx.default_global::<PanelControl>().spawns.push(
            PanelSpec::spawn(panel, |id, _, cx| {
                // アニメーションのサンプルを毎フレーム描画する。
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

/// 言語切り替えボタンが次に選択する言語（i18n の対応言語をサイクルする）。
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

/// 見出し付きカード。内容は縦方向に配置する。
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

/// 小見出しとその下のコンテンツ。
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

/// 非インタラクティブなインジケーター。ボタンと区別するため高さを低くし、先頭にドットを配置する（ドットは有効時のみアクセントカラー）。
/// 面の色は有効時 `control-active`、無効時 `control`（テストは中心の色で判定する）。
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
