//! OS の通知のカード。通知のトースト（`crates/toast`）とダッシュボードの通知センター（クイック設定の一覧）が同じものを
//! 出す（画面どうしは直接依存しないので、共有する部品は UI 層に置く）。アイコン・見出し・アプリの名前・本文（行数を
//! 限って省略）。広げると本文が伸び、通知のボタンが並ぶ。押したときの開き方（[`open`]）と、トーストから通知センターを
//! 開く頼み（[`Center`]）と、既読（[`Read`]）もここ。

use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use design::components::{self, Button, Variant};
use design::{Theme, font_size, icons, radius, spacing};
use gpui::{
    App, FontWeight, IntoElement, RenderImage, SharedString, Window, div, img, prelude::*, px, rgba,
};
use kernel::subsystems::notifications::Notification;
use kernel::{PanelCommand, PanelId};

/// 通知センター（ダッシュボードのクイック設定の一覧）を開いてほしいという頼み（GPUI のグローバル）。トーストのカードを
/// 押すと、その通知に合わせて開く（[`show_center`]）。ダッシュボードが見て出し、一覧を開いて、一覧が [`Center::focus`]
/// を広げる。
#[derive(Debug, Default)]
pub struct Center {
    /// 頼まれた回数（増えたら開く）。
    pub asked: u32,
    /// 広げて見せる通知（一覧が受け取ったら空にする）。
    pub focus: Option<u32>,
    /// 一覧を出している画面のパネル。トーストは、これがダッシュボードにはまって出ている間は出ない（同じものが並ぶので）。
    pub panel: Option<PanelId>,
}

impl gpui::Global for Center {}

/// 既読の通知（GPUI のグローバル）: 通知センターにはまって出ている一覧に並んだか、トーストで押した（通知センターで
/// 開いた）もの。トーストは未読のものだけを出し、バーの件数は未読の数。
// ponytail: 消えた通知の ID も残る（u32 が数個ずつ増えるだけ）。長く動かして気になったら一覧に無いものを落とす。
#[derive(Debug, Default)]
pub struct Read(pub HashSet<u32>);

impl gpui::Global for Read {}

/// `id` の通知を読んだか。
pub fn is_read(id: u32, cx: &App) -> bool {
    cx.try_global::<Read>().is_some_and(|r| r.0.contains(&id))
}

/// `ids` を既読にする。新しく読んだものがあるときだけ、見ている側（トースト・バー）に知らせる。
pub fn mark_read(ids: impl IntoIterator<Item = u32>, cx: &mut App) {
    let new: Vec<u32> = ids.into_iter().filter(|&id| !is_read(id, cx)).collect();
    if !new.is_empty() {
        cx.default_global::<Read>().0.extend(new);
    }
}

/// 通知センターを開いてほしい（`focus` の通知を広げて。その通知は既読にする）。
pub fn show_center(focus: Option<u32>, cx: &mut App) {
    mark_read(focus, cx);
    let c = cx.default_global::<Center>();
    c.asked = c.asked.wrapping_add(1);
    c.focus = focus;
}

/// 本文の行数（ふだん・広げたとき）。
pub const BODY_LINES: usize = 2;
pub const BODY_LINES_EXPANDED: usize = 6;
/// カードのアイコンの一辺（px）。
pub const ICON_PX: f32 = 36.0;

/// アプリごとのアイコンの画像（作ったものを使い回す）。
#[derive(Default)]
pub struct Icons(HashMap<String, Option<Arc<RenderImage>>>);

impl Icons {
    /// 通知 `n` のアプリのアイコン（無ければ `None`）。
    pub fn of(&mut self, n: &Notification) -> Option<Arc<RenderImage>> {
        self.0
            .entry(n.app.id.clone())
            .or_insert_with(|| {
                let icon = n.app.icon.as_ref()?;
                crate::render_image(icon.width, icon.height, icon.rgba.to_vec())
            })
            .clone()
    }
}

/// アプリのアイコン（無ければ鐘）。
pub fn icon(image: Option<Arc<RenderImage>>, size: f32) -> gpui::AnyElement {
    match image {
        Some(image) => img(image).size(px(size)).flex_none().into_any_element(),
        None => design::icon(&icons::BELL).size(px(size)).into_any_element(),
    }
}

/// 見出し（無ければアプリの名前）。
pub fn title(n: &Notification) -> SharedString {
    if n.title.is_empty() {
        n.app.name.clone().into()
    } else {
        n.title.clone().into()
    }
}

/// 通知 `n` のカード（地に直接置く押せるカード。`id` は `{prefix}-{通知の ID}`）。`expanded` なら本文を伸ばし、通知の
/// ボタン（`{prefix}-action-{通知の ID}-{番号}`）と `extra` を並べる（そのときカードはホバー・押下で色を変えない。
/// 変えると中のボタンが紛れる）。本文か通知のボタンを押すと `pressed` に `None` かボタンの番号が届く。
pub fn card(
    theme: &Theme,
    prefix: &str,
    n: &Notification,
    image: Option<Arc<RenderImage>>,
    expanded: bool,
    extra: impl IntoIterator<Item = Button>,
    pressed: impl Fn(Option<usize>, &mut Window, &mut App) + Clone + 'static,
) -> Button {
    let id = n.id;
    let actions = n.actions.iter().enumerate().map(|(i, a)| {
        let pressed = pressed.clone();
        components::button(
            theme,
            format!("{prefix}-action-{id}-{i}"),
            a.label.clone(),
            Variant::Default,
        )
        .on_click(move |_, window, cx| {
            // カードの本文を押したことにしない。
            cx.stop_propagation();
            pressed(Some(i), window, cx);
        })
    });
    let buttons: Vec<Button> = if expanded {
        actions.chain(extra).collect()
    } else {
        Vec::new()
    };
    let body = (!n.body.is_empty()).then(|| {
        div()
            .w_full()
            .text_size(px(font_size::SM))
            .text_color(rgba(theme.neutral_text_low))
            .line_clamp(if expanded {
                BODY_LINES_EXPANDED
            } else {
                BODY_LINES
            })
            .text_ellipsis()
            .child(n.body.clone())
    });
    let header = div()
        .w_full()
        .flex()
        .items_center()
        .gap(px(spacing::SM))
        .child(icon(image, ICON_PX))
        .child(
            div()
                .flex_1()
                .min_w_0()
                .flex()
                .flex_col()
                .child(
                    div()
                        .truncate()
                        .text_size(px(font_size::MD))
                        .font_weight(FontWeight::BOLD)
                        .child(title(n)),
                )
                .child(
                    div()
                        .truncate()
                        .text_size(px(font_size::SM))
                        .text_color(rgba(theme.neutral_text_low))
                        .child(n.app.name.clone()),
                ),
        );
    let tile = components::tile(theme, format!("{prefix}-{id}"));
    if buttons.is_empty() {
        tile
    } else {
        tile.still()
    }
    .w_full()
    // ボタンの高さに決まっているので、中身に合わせて伸ばす。
    .h_auto()
    .items_start()
    .rounded(px(radius::XL))
    .child(header)
    .children(body)
    .when(!buttons.is_empty(), |t| {
        t.child(
            div()
                .w_full()
                .flex()
                .flex_wrap()
                .gap(px(spacing::SM))
                .children(buttons),
        )
    })
    .on_click(move |_, window, cx| pressed(None, window, cx))
}

/// 通知 `n` の本文（`action` が `None`）かボタンを押した。URL は確かめてから開き（[`crate::link`]）、ほかはアプリへ届ける
/// （届けた通知は OS の通知センターからも消え、一覧から落ちる）。`panel` は押したパネル。
pub fn open(
    panel: PanelId,
    n: &Notification,
    action: Option<usize>,
    window: &Window,
    cx: &mut App,
) {
    let url = match action {
        None => n.url.clone(),
        Some(i) => n.actions.get(i).and_then(|a| a.url.clone()),
    };
    match url {
        Some(url) => design::link::open(url, window, cx),
        None => crate::command(panel, PanelCommand::Notification { id: n.id, action }, cx),
    }
}
