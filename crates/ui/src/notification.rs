//! OS 通知のカード。トースト（`crates/toast`）とダッシュボードの通知センター（クイック設定一覧）で共有する
//! （画面間は直接依存しないため、共有部品は UI 層に配置する）。アイコン・見出し・アプリ名・本文（行数制限付き省略）を
//! 表示し、展開すると本文が伸び通知ボタンが並ぶ。押下時の開き方（[`open`]）、トーストから通知センターを開く要求
//! （[`Center`]）、既読管理（[`Read`]）、通知センターでのカード展開アニメーション（[`Expanding`]）を提供する。

use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use std::rc::Rc;
use std::sync::Arc;
use std::time::Instant;

use design::animation::{Motion, Transition};
use design::components::{self, Button, Variant};
use design::{Theme, font_size, icons, radius, spacing};
use gpui::{
    App, FontWeight, IntoElement, RenderImage, SharedString, Window, canvas, div, img, prelude::*,
    px, rgba,
};
use kernel::subsystems::notifications::Notification;
use kernel::{PanelCommand, PanelId};

/// 通知センター（ダッシュボードのクイック設定一覧）のオープン要求（GPUI グローバル）。トーストのカードを押すと
/// 該当通知に合わせて起動する（[`show_center`]）。ダッシュボードが検知して一覧を表示し、
/// 一覧が [`Center::focus`] を展開する。
#[derive(Debug, Default)]
pub struct Center {
    /// 要求回数（増加時に起動する）。
    pub asked: u32,
    /// 展開して表示する通知（一覧が取得後に消去する）。
    pub focus: Option<u32>,
    /// 一覧を表示中の画面パネル。このパネルがダッシュボードにドックされて表示中の間、トーストは表示しない（重複回避）。
    pub panel: Option<PanelId>,
}

impl gpui::Global for Center {}

/// 既読通知の集合（GPUI グローバル）。通知センターの一覧に表示されたもの、またはトーストで押下されたもの。
/// トーストは未読のみ表示し、バーの件数表示も未読数を使用する。
// ponytail: 消去済み通知の ID も残存する（u32 が漸増するのみ）。長時間稼働で問題になれば一覧から除外された ID を除去する。
#[derive(Debug, Default)]
pub struct Read(pub HashSet<u32>);

impl gpui::Global for Read {}

/// `id` の通知が既読か判定する。
pub fn is_read(id: u32, cx: &App) -> bool {
    cx.try_global::<Read>().is_some_and(|r| r.0.contains(&id))
}

/// `ids` を既読にする。新規の既読がある場合のみ、表示側（トースト・バー）に通知する。
pub fn mark_read(ids: impl IntoIterator<Item = u32>, cx: &mut App) {
    let new: Vec<u32> = ids.into_iter().filter(|&id| !is_read(id, cx)).collect();
    if !new.is_empty() {
        cx.default_global::<Read>().0.extend(new);
    }
}

/// 通知センターのオープンを要求する（`focus` 指定の通知を展開表示し、既読にする）。
pub fn show_center(focus: Option<u32>, cx: &mut App) {
    mark_read(focus, cx);
    let c = cx.default_global::<Center>();
    c.asked = c.asked.wrapping_add(1);
    c.focus = focus;
}

/// 本文の表示行数（通常時・展開時）。
pub const BODY_LINES: usize = 2;
pub const BODY_LINES_EXPANDED: usize = 6;
/// カードアイコンの一辺（px）。
pub const ICON_PX: f32 = 36.0;

/// アプリごとのアイコン画像キャッシュ（生成済みの画像を再利用する）。
#[derive(Default)]
pub struct Icons(HashMap<String, Option<Arc<RenderImage>>>);

impl Icons {
    /// 通知 `n` のアプリアイコンを取得する（アイコンがなければ `None`）。
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

/// アプリアイコン（未設定時はベルアイコン）。
pub fn icon(image: Option<Arc<RenderImage>>, size: f32) -> gpui::AnyElement {
    match image {
        Some(image) => img(image).size(px(size)).flex_none().into_any_element(),
        None => design::icon(&icons::BELL).size(px(size)).into_any_element(),
    }
}

/// 見出し（未設定時はアプリ名を使用する）。
pub fn title(n: &Notification) -> SharedString {
    if n.title.is_empty() {
        n.app.name.clone().into()
    } else {
        n.title.clone().into()
    }
}

/// カードの展開状態（[`Expanding::look`]）。トーストのカードは常に [`Expand::default`]（折りたたみ状態）。
#[derive(Clone, Default)]
pub struct Expand {
    /// 展開中の内容（本文 6 行とボタン）を描画する（展開中・展開遷移中・折りたたみ遷移中）。
    pub open: bool,
    /// 展開度（0 = 折りたたみ、1 = 展開済み）。ボタンはこの値に応じて表示される。
    pub p: f32,
    /// 遷移中の内容高さ（折りたたみ時・展開時の計測高さ間の補間値）。`None` なら内容に従う。
    height: Option<f32>,
    /// 内容高さの計測・記録先。
    measure: Option<(u32, Expanding)>,
}

/// 通知センターのカード展開・折りたたみアニメーション。描画のたびに内容高さを計測し、遷移中は折りたたみ時と
/// 展開時の高さ間で補間する（下のカードはそれに伴い押し下げられる）。高さはピクセル単位に丸める（端数があると文字だけがピクセルに吸着して震動する）。
#[derive(Clone, Default)]
pub struct Expanding(Rc<RefCell<ExpandState>>);

#[derive(Default)]
struct ExpandState {
    open: HashMap<u32, Transition>,
    /// 計測済みの内容高さ（通知 ID、展開状態）。
    heights: HashMap<(u32, bool), f32>,
}

impl Expanding {
    /// 通知 `id` の展開状態を `open` に設定する（変更時に `animated` ならアニメーション開始）。現在の展開状態を返す。
    pub fn look(&self, id: u32, open: bool, now: Instant, animated: bool) -> Expand {
        let mut s = self.0.borrow_mut();
        let to = if open { 1.0 } else { 0.0 };
        let t = s.open.entry(id).or_insert(Transition::rest(to));
        // 展開・折りたたみともに ease-in-out で遷移する。
        t.go(to, Motion::Morph, now, animated);
        let (p, moving) = t.at(now);
        let at = |expanded| s.heights.get(&(id, expanded)).copied();
        let height = match (moving, at(false), at(true)) {
            (false, _, _) => None,
            (true, Some(c), Some(o)) => Some(c + (o - c) * p),
            // 展開時の内容がまだ未計測（展開開始フレーム）。
            (true, Some(c), None) => Some(c),
            // ponytail: 展開状態で開いた（トースト経由の）カードは折りたたみ時の高さが未知のため、最後に収縮する。
            (true, None, _) => None,
        };
        Expand {
            open: open || p > 0.0,
            p,
            height: height.map(f32::round),
            measure: Some((id, self.clone())),
        }
    }

    /// アニメーション中のカードが存在するか（次フレームも描画が必要）。
    pub fn busy(&self, now: Instant) -> bool {
        self.0.borrow().open.values().any(|t| t.at(now).1)
    }

    /// 表示されなくなった通知の状態を破棄する。
    pub fn forget(&self, drawn: &[u32]) {
        let mut s = self.0.borrow_mut();
        s.open.retain(|id, _| drawn.contains(id));
        s.heights.retain(|(id, _), _| drawn.contains(id));
    }
}

/// 通知センター専用のカード付加情報（トーストは [`Default`]）。
#[derive(Default)]
pub struct Detail {
    /// 展開状態。
    pub expand: Expand,
    /// 受信時刻（アプリ名の横に表示する）。
    pub stamp: Option<SharedString>,
    /// 展開時に通知ボタンの後に追加するボタン。
    pub extra: Vec<Button>,
}

/// 通知 `n` のカード（直接配置する押下可能なカード。`id` は `{prefix}-{通知 ID}`）。展開（[`Expand::open`]）すると
/// 本文を拡張し、通知ボタン（`{prefix}-action-{通知 ID}-{番号}`）と [`Detail::extra`] を並べる（展開中のカードはホバー・
/// 押下で色変化しない。変化させると内部ボタンと区別がつかなくなる）。本文またはボタンの押下時に `pressed` へ `None` またはボタン番号を送出する。
pub fn card(
    theme: &Theme,
    prefix: &str,
    n: &Notification,
    image: Option<Arc<RenderImage>>,
    detail: Detail,
    pressed: impl Fn(Option<usize>, &mut Window, &mut App) + Clone + 'static,
) -> Button {
    let Detail {
        expand,
        stamp,
        extra,
    } = detail;
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
            // カード本文への伝播を停止する。
            cx.stop_propagation();
            pressed(Some(i), window, cx);
        })
    });
    let expanded = expand.open;
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
                        .flex()
                        .gap(px(spacing::XS))
                        .text_size(px(font_size::SM))
                        .text_color(rgba(theme.neutral_text_low))
                        .child(div().min_w_0().truncate().child(n.app.name.clone()))
                        .children(stamp.map(|t| {
                            let selector = format!("{prefix}-time-{id}");
                            div()
                                .flex_none()
                                .debug_selector(|| selector)
                                .child(format!("· {t}"))
                        })),
                ),
        );
    let tile = components::tile(theme, format!("{prefix}-{id}"));
    if buttons.is_empty() {
        tile
    } else {
        tile.still()
    }
    .w_full()
    // ボタンの固定高さではなく、内容に合わせて伸縮する。
    .h_auto()
    .items_start()
    .rounded(px(radius::XL))
    .child(
        // 展開・折りたたみ遷移中は計測済み高さ間で内容をクリッピングする（内容は常に自然な高さで配置して計測する）。
        div()
            .w_full()
            .when_some(expand.height, |d, h| d.h(px(h)).overflow_hidden())
            .child(
                div()
                    .relative()
                    .w_full()
                    .flex_none()
                    .flex()
                    .flex_col()
                    .gap(px(spacing::SM))
                    .child(header)
                    .children(body)
                    .when(!buttons.is_empty(), |t| {
                        t.child(
                            div()
                                .w_full()
                                .flex()
                                .flex_wrap()
                                .gap(px(spacing::SM))
                                .opacity(expand.p)
                                .children(buttons),
                        )
                    })
                    .children(expand.measure.map(|(id, e)| {
                        canvas(
                            move |b, _, _| {
                                let h = f32::from(b.size.height);
                                e.0.borrow_mut().heights.insert((id, expanded), h);
                            },
                            |_, _, _, _| {},
                        )
                        .absolute()
                        .inset_0()
                    })),
            ),
    )
    .on_click(move |_, window, cx| pressed(None, window, cx))
}

/// 通知 `n` の本文（`action` が `None`）またはボタンの押下を処理する。URL は検証後に開き（[`crate::link`]）、
/// それ以外はアプリへ配送する（配送済みの通知は OS の通知センターからも消去され、一覧から除外される）。`panel` は押下元パネル。
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
