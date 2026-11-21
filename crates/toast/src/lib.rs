//! Layer 5 画面: OS 通知のトースト表示。1 枚のパネルを再利用し、到着した通知を HUD → 手首の時計上部 →
//! ダッシュボード上部へ遷移させる（一方向のみ。[`flow`]）。表示位置の遷移時は、現在位置でフェードアウトしてから
//! 次の位置へ瞬間移動し、フェードインする（視界を横切らないようにする）。HUD はアイコンと見出しのみで非インタラクティブ、
//! 手首・ダッシュボードでは本文（行数制限あり）まで表示し、クリックで通知センター（ダッシュボードのクイック設定一覧、
//! 詳細と通知アクションはそこで表示）を開く。左右スワイプでその場で非表示にできる（[`ui::swipe`]、通知センターからは
//! 削除しない）。通知センター表示中はトーストを非表示にする（重複表示を避けるため）。開くのは wrist。

mod flow;

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;
use std::sync::Arc;
use std::time::{Duration, Instant};

use design::animation::Motion;
use design::components::{
    self,
    slide::{Entering, Slide, with_leaving},
};
use design::{PANEL_MARGIN, Theme, font_size, radius, spacing};
use gpui::{
    AnyElement, Bounds, Context, FontWeight, IntoElement, Pixels, SharedString, Window, canvas,
    div, prelude::*, px, rgba,
};
use kernel::subsystems::notifications::Notification;
use kernel::vr::{Device, Fov, Pose, Quat, Vec2, Vec3};
use kernel::{NotificationAnchor, PanelCommand, PanelId, Parent};
use ui::swipe::{Look, Swipes};
use ui::{PanelControl, PanelSpec};

pub use flow::Place;
use flow::{Flow, Scene};

/// パネルの解像度（px）。コンテンツは表示位置側（HUD はアンカー端、手首・ダッシュボードは下部）に寄せ、残りは透明。
pub const RES_PX: (u32, u32) = (420, 600);
/// 表示位置ごとの密度（px/m）。すべての位置でダッシュボード画面と同程度の見た目サイズとなるよう、遠い HUD は低密度、
/// 近い手首は高密度に設定する。HUD は視界の端でも気づけるようやや大きめにする。
const HUD_PX_PER_M: f32 = 600.0;
const WRIST_PX_PER_M: f32 = 3000.0;
const DASHBOARD_PX_PER_M: f32 = 1000.0;
/// HUD の頭からの距離（m）。
pub const HUD_DISTANCE: f32 = 1.5;
/// 手首の時計上部に立てる傾斜角（rad、顔方向へ起こす）。視点は文字盤の正面にあり、その上の通知はやや斜めに見えるため。
const WRIST_TILT: f32 = 0.3;
/// 表示上限数（超過分は「ほか n 件」として表示）。
const HUD_ROWS: usize = 3;
const CARD_ROWS: usize = 3;
/// HUD のアイコンの一辺（px）。
const HUD_ICON: f32 = 28.0;
/// カード群がイン・アウト時にスライドする距離（px）。
const SLIDE_PX: f32 = 56.0;
/// 追加されたカードが上部からドロップインする距離（px）。
const DROP_PX: f32 = 24.0;
/// アウト完了を待つマージン（アウト時間に加算する。フレーム遅延により完全消失前に遷移しないようにする）。
const OUT_MARGIN: Duration = Duration::from_millis(50);

/// トーストの起動仕様。手首の時計 `wrist` 上部およびダッシュボード（`PanelControl::dock`）上部にも遷移する。
/// `opened` は生成されたパネル ID を受け取るコールバック。
pub fn spec(wrist: PanelId, opened: impl FnOnce(PanelId, &mut gpui::App) + 'static) -> PanelSpec {
    PanelSpec::root(Pose::IDENTITY, move |panel, _, cx: &mut Context<Toast>| {
        opened(panel, cx);
        // 通知未到着時は非表示にしておく。遷移は不透明度のみで行い、スライド方向はコンテンツ側が描画する。
        ui::commands(
            panel,
            [PanelCommand::Hide(true), PanelCommand::FadeOnly(true)],
            cx,
        );
        Toast::new(panel, wrist, cx)
    })
    .sized(size(HUD_PX_PER_M), RES_PX)
    .locked()
    .without_resize()
    // 常駐パネルで、表示位置は自律的に決定する（閉じる・移動の対象外）。ナビゲーションバーは付与しない。
    .without_nav_bar()
}

/// 密度 `px_per_m` での大きさ（m。等倍）。
fn size(px_per_m: f32) -> Vec2 {
    Vec2::new(RES_PX.0 as f32, RES_PX.1 as f32) / px_per_m
}

/// 頭部基準の HUD 配置位置。コンテンツ外縁（パネル端から影余白の内側）が、両目の確実可視範囲 `safe`
/// （[`Fov::safe`]）の `anchor` 側の端に一致するよう、[`HUD_DISTANCE`] 先に配置して頭部方向へ向ける。`scale` は UI スケール。
// ponytail: 頭へ向けて回した分、外の端は 1° ほど外へずれる。セーフエリアの余裕（視野の 40%）に収まるので詰めない。
pub fn hud_pose(anchor: NotificationAnchor, safe: Fov, scale: f32) -> Pose {
    let (sx, sy) = anchor.side();
    let half = size(HUD_PX_PER_M) * scale / 2.0 - Vec2::splat(PANEL_MARGIN / HUD_PX_PER_M * scale);
    let d = HUD_DISTANCE;
    let edge = |s: f32, neg: f32, pos: f32, half: f32| {
        if s > 0.0 {
            d * pos.tan() - half
        } else if s < 0.0 {
            half - d * neg.tan()
        } else {
            0.0
        }
    };
    let p = Vec3::new(
        edge(sx, safe.left, safe.right, half.x),
        edge(sy, safe.down, safe.up, half.y),
        -d,
    );
    let to_eye = -p.normalize();
    Pose::new(
        p,
        Quat::from_rotation_y(to_eye.x.atan2(to_eye.z)) * Quat::from_rotation_x(-to_eye.y.asin()),
    )
}

/// パネルの配置先（同一であれば再配置しない）。
#[derive(Debug, Clone, Copy, PartialEq)]
enum Target {
    Hud(Pose),
    Parent(Parent),
}

/// イン・アウトの遷移フェーズ。アウト中（`Out`）は前のコンテンツのまま描画し、完全消失後に次の位置へ遷移する。
#[derive(Debug, Clone, Copy, PartialEq)]
enum Phase {
    Hidden,
    In(Instant),
    Shown,
    Out(Instant),
}

pub struct Toast {
    panel: PanelId,
    wrist: PanelId,
    flow: Flow,
    /// 現在のパネル配置先（遷移中も含む）。非表示時は `None`。
    target: Option<Target>,
    phase: Phase,
    /// 描画中の表示位置（アウト中も遷移前の位置を維持）。
    place: Place,
    /// 表示中の通知（新しい順）と、表示上限を超過した件数。
    shown: Vec<Arc<Notification>>,
    more: usize,
    /// 前フレームで表示していた通知（消去されたカードをフェードアウト完了まで描画し続ける）。
    prev: Vec<Arc<Notification>>,
    /// カードのイン・アウトアニメーション（上から積む）。
    stack: Entering,
    /// カードの配置矩形（ウィンドウ px 座標。前フレームで計測した自然サイズ）。イン・アウト時に高さの伸縮アニメーションに使用する。
    boxes: Rc<RefCell<HashMap<u32, Bounds<Pixels>>>>,
    /// 左右スワイプによる非表示操作（[`ui::swipe`]）。
    swipes: Swipes,
    icons: ui::notification::Icons,
}

impl Toast {
    pub fn new(panel: PanelId, wrist: PanelId, cx: &mut Context<Self>) -> Self {
        cx.observe_global::<PanelControl>(Self::sync).detach();
        cx.observe_global::<ui::notification::Read>(Self::sync)
            .detach();
        Self {
            panel,
            wrist,
            flow: Flow::default(),
            target: None,
            phase: Phase::Hidden,
            place: Place::Hidden,
            shown: Vec::new(),
            more: 0,
            prev: Vec::new(),
            // 画面のテストでは動かさない（動いている途中のカードを押さないように）。出たまま描き直さないことがあるので、
            // しばらく描かなくても積み直さない（出し直すときは `replay` と、隠れている間の `begin`）。
            // カードの高さ（下のカードを押し下げる・詰める）は、入りも出もゆっくり動き出してゆっくり止まる。
            stack: Entering::new(cfg!(test))
                .without_idle_replay()
                .with_grow(Motion::Morph),
            boxes: Rc::default(),
            swipes: Swipes::default(),
            icons: Default::default(),
        }
    }

    /// 現在の表示位置（アウト中も遷移前の位置を返す。非表示時は [`Place::Hidden`]）。
    pub fn place(&self) -> Place {
        if self.target.is_some() {
            self.place
        } else {
            Place::Hidden
        }
    }

    /// フレームループの状態書き戻しごとに、表示位置と対象通知を再決定する。位置が変わる場合は現在位置でフェードアウト完了後に
    /// 次の位置でフェードインする。同一位置ならコンテンツのみ入れ替える（カードが上から積まれ、消去分はフェードアウトする）。
    fn sync(&mut self, cx: &mut Context<Self>) {
        let ctl = cx.global::<PanelControl>();
        let status = |id: PanelId| ctl.panels.get(&id).map(|p| &p.status);
        let dock = ctl
            .dock
            .filter(|(bar, _)| status(*bar).is_some_and(|s| !s.hidden && !s.closed));
        let center = cx
            .try_global::<ui::notification::Center>()
            .and_then(|c| c.panel);
        let scene = Scene {
            dashboard: dock.is_some(),
            center: dock.is_some() && center.is_some() && ui::docked(cx) == center,
            gazing: status(self.wrist).is_some_and(|s| s.gazed),
            lockdown: ctl.settings.lockdown,
        };
        let notes = ctl.notifications.clone();
        let settings = ctl.settings;
        let safe = ctl.fov.unwrap_or(Fov::NARROW).safe();
        // 閲覧済み（通知センターに表示済み・アクティベート済み）の通知は表示対象外とする。
        let ids: Vec<u32> = notes
            .iter()
            .map(|n| n.id)
            .filter(|&id| !ui::notification::is_read(id, cx))
            .collect();
        let now = Instant::now();
        let (place, shown) = self.flow.step(&ids, scene, now);
        let target = match place {
            Place::Hidden => None,
            Place::Hud => Some(Target::Hud(hud_pose(
                settings.notification_anchor,
                safe,
                settings.ui_scale,
            ))),
            Place::Wrist => Some(Target::Parent(Parent {
                panel: self.wrist,
                docked: true,
                slot: Some(WRIST_TILT),
                ..Default::default()
            })),
            Place::Dashboard => dock.map(|(bar, tilt)| {
                Target::Parent(Parent {
                    panel: bar,
                    carry: true,
                    docked: true,
                    slot: Some(tilt),
                    stack: true,
                    ..Default::default()
                })
            }),
        };
        let rows = if place == Place::Hud {
            HUD_ROWS
        } else {
            CARD_ROWS
        };
        let shown: Vec<Arc<Notification>> = shown
            .iter()
            .filter_map(|id| notes.iter().find(|n| n.id == *id).cloned())
            .collect();
        let more = shown.len().saturating_sub(rows);
        let shown: Vec<_> = shown.into_iter().take(rows).collect();
        // アウト中: 前のコンテンツのまま、完全消失まで待機する。完全消失後に親から切り離す（親の再表示時に連動しないようにする）。
        if let Phase::Out(since) = self.phase {
            let out = Duration::from_millis(Motion::Exit.duration_ms()) + OUT_MARGIN;
            if now.saturating_duration_since(since) < out {
                return;
            }
            if matches!(self.target, Some(Target::Parent(_))) {
                ui::command(self.panel, PanelCommand::Parent(None), cx);
            }
            (self.target, self.phase) = (None, Phase::Hidden);
        }
        match (self.target, target) {
            (Some(at), Some(next)) if at == next => self.set_content(shown, more, cx),
            // 異なる位置への遷移・非表示化: まず現在位置でフェードアウトする（コンテンツは維持）。
            (Some(_), _) => {
                logger::debug!("toast leaves", from = ?self.place, to = ?place);
                ui::command(self.panel, PanelCommand::Hide(true), cx);
                self.phase = Phase::Out(now);
                ui::notify(self.panel, cx);
            }
            (None, Some(next)) => {
                logger::debug!("toast shows", at = ?place);
                self.show(place, next, now, cx);
                self.set_content(shown, more, cx);
            }
            (None, None) => {}
        }
    }

    /// 非表示のパネルを `next` に配置してフェードインする。カードは上から順に積まれる。
    fn show(&mut self, place: Place, next: Target, now: Instant, cx: &mut Context<Self>) {
        let height = RES_PX.1 as f32
            / match place {
                Place::Hud => HUD_PX_PER_M,
                Place::Wrist => WRIST_PX_PER_M,
                _ => DASHBOARD_PX_PER_M,
            };
        let mut commands = match next {
            // 頭部アタッチ前に、現在の頭部前方に配置しておく（頭部追従非対応のランタイム（novr）ではその位置に留まる）。
            Target::Hud(pose) => vec![
                PanelCommand::Place(pose),
                PanelCommand::Attach(Device::Hmd, pose),
                PanelCommand::PassThrough(true),
            ],
            Target::Parent(parent) => vec![
                PanelCommand::Parent(Some(parent)),
                PanelCommand::PassThrough(false),
            ],
        };
        commands.extend([PanelCommand::FitHeight(height), PanelCommand::Hide(false)]);
        ui::commands(self.panel, commands, cx);
        (self.target, self.place, self.phase) = (Some(next), place, Phase::In(now));
        // 前の位置のカードは引き継がず、新しい位置で上から順に積み直す。
        (self.shown, self.prev) = (Vec::new(), Vec::new());
        self.stack.replay();
        ui::notify(self.panel, cx);
    }

    /// 表示中の通知を更新する（変更があった場合のみ再描画する）。
    fn set_content(&mut self, shown: Vec<Arc<Notification>>, more: usize, cx: &mut Context<Self>) {
        let same = shown.len() == self.shown.len()
            && shown
                .iter()
                .zip(&self.shown)
                .all(|(a, b)| Arc::ptr_eq(a, b));
        if same && more == self.more {
            return;
        }
        (self.shown, self.more) = (shown, more);
        ui::notify(self.panel, cx);
    }

    /// カード群がスライドする隠れ側の方向（px 単位）: 手首・ダッシュボードは下（下→上にイン、上→下にアウト）、
    /// HUD はアンカー端の外側（上端なら上方向、左右端なら横方向）。
    fn hidden_side(&self, cx: &gpui::App) -> Vec2 {
        match self.place {
            Place::Hud => match ui::settings(cx).notification_anchor.side() {
                (_, sy) if sy != 0.0 => Vec2::new(0.0, -sy),
                (sx, _) => Vec2::new(sx, 0.0),
            },
            _ => Vec2::Y,
        }
    }

    /// カード群のオフセット（px）。イン・アウト遷移中であればアニメーション継続（`true`）。
    fn slide(&mut self, now: Instant, cx: &gpui::App) -> (Vec2, bool) {
        let side = self.hidden_side(cx) * SLIDE_PX;
        let progress = |since: Instant, motion: Motion| {
            let t = now.saturating_duration_since(since).as_secs_f32() * 1000.0
                / motion.duration_ms() as f32;
            (motion.ease(t.min(1.0)), t < 1.0)
        };
        match self.phase {
            Phase::Hidden => (side, false),
            Phase::Shown => (Vec2::ZERO, false),
            Phase::In(since) => {
                let (t, going) = progress(since, Motion::Enter);
                if !going {
                    self.phase = Phase::Shown;
                }
                (side * (1.0 - t), going)
            }
            Phase::Out(since) => {
                let (t, going) = progress(since, Motion::Exit);
                (side * t, going)
            }
        }
    }

    /// HUD の 1 行: アイコンと見出し（1 行、長い場合は省略表示）。非インタラクティブ。
    fn pill(&mut self, theme: &Theme, n: &Notification) -> AnyElement {
        let id = n.id;
        let image = self.icons.of(n);
        components::card_panel_fit(theme)
            .debug_selector(move || format!("toast-hud-{id}"))
            .max_w_full()
            .flex()
            .items_center()
            .gap(px(spacing::SM))
            .px(px(spacing::MD))
            .py(px(spacing::SM))
            .rounded(px(radius::FULL))
            .child(ui::notification::icon(image, HUD_ICON))
            .child(
                div()
                    .min_w_0()
                    .truncate()
                    .text_size(px(font_size::MD))
                    .font_weight(FontWeight::BOLD)
                    .child(ui::notification::title(n)),
            )
            .into_any_element()
    }

    /// 手首・ダッシュボードの 1 枚（[`ui::notification::card`]、展開なし）。クリックで通知センターが該当通知を展開して開く。
    /// 左右スワイプ中は横方向にオフセットしてフェードする（[`ui::swipe`]）。
    fn card(&mut self, theme: &Theme, n: &Notification, look: Look) -> AnyElement {
        let id = n.id;
        let image = self.icons.of(n);
        let card = ui::notification::card(
            theme,
            "toast",
            n,
            image,
            Default::default(),
            move |_, _, cx| {
                ui::notification::show_center(Some(id), cx);
            },
        );
        self.swipes.item(
            id,
            look,
            components::card_panel_fit(theme).w_full().child(card),
        )
    }
}

/// 揃え方（HUD はアンカー端側、手首・ダッシュボードは中央）。
#[derive(Clone, Copy)]
enum Alignment {
    Start,
    Center,
    End,
}

/// カード 1 枚のドロップインアニメーション（上から積む）: 進捗 `s` に応じて高さを伸ばし（下のカードを押し下げる）、上部からドロップインで出現する。
/// 消去時は逆に縮小して上方へ退出する。高さは前フレームで計測した自然高さ（`boxes`）を使用する。イン完了後は自然高さに委ねる
/// （ホバーによる伸長にも追従する）。カード間のギャップはアンカー端の反対側（`gap_top` なら上）に配置する。
fn drop_in(
    s: Slide,
    id: u32,
    boxes: &Rc<RefCell<HashMap<u32, Bounds<Pixels>>>>,
    gap_top: bool,
    align: Alignment,
    child: AnyElement,
) -> AnyElement {
    let grow = s.grow.clamp(0.0, 1.0);
    let natural = boxes
        .borrow()
        .get(&id)
        .map_or(0.0, |b| f32::from(b.size.height));
    let measure = {
        let boxes = boxes.clone();
        canvas(
            move |bounds, _, _| {
                boxes.borrow_mut().insert(id, bounds);
            },
            |_, _, _, _| {},
        )
        .absolute()
        .inset_0()
    };
    let inner = div()
        .relative()
        .w_full()
        .flex_none()
        .flex()
        .map(|d| match align {
            Alignment::Start => d.justify_start(),
            Alignment::Center => d.justify_center(),
            Alignment::End => d.justify_end(),
        })
        .map(|d| {
            if gap_top {
                d.pt(px(spacing::SM))
            } else {
                d.pb(px(spacing::SM))
            }
        })
        .child(child)
        .child(measure);
    div()
        .w_full()
        .flex_none()
        // 高さはピクセルに揃える（端数のまま動くと、他カードのテキストのみがピクセルスナップして振動する）。
        .when(grow < 1.0, |d| {
            d.h(px((natural * grow).round())).overflow_hidden()
        })
        .child(
            div()
                .relative()
                .top(px(-(1.0 - s.rise) * DROP_PX))
                .opacity(s.rise.clamp(0.0, 1.0))
                .child(inner),
        )
        .into_any_element()
}

impl Render for Toast {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let now = Instant::now();
        let theme = Theme::current(cx);
        let hud = self.place == Place::Hud;
        // HUD はアンカー端に、手首・ダッシュボードは下部（起立位置）に寄せる。カードはすべての位置で新しいものが上。
        let (sx, sy) = if hud {
            ui::settings(cx).notification_anchor.side()
        } else {
            (0.0, -1.0)
        };
        let align = match sx {
            x if x < 0.0 => Alignment::Start,
            x if x > 0.0 => Alignment::End,
            _ => Alignment::Center,
        };
        let (offset, sliding) = self.slide(now, cx);
        // スワイプで完全に畳まれたカードは以降表示しない（フェードアウトも完了済み。通知センターには残留する）。
        for id in self.swipes.take_gone(now) {
            self.flow.done(id);
            self.shown.retain(|n| n.id != id);
            self.prev.retain(|n| n.id != id);
        }
        self.stack.begin(self.phase == Phase::Hidden, now);
        let items = with_leaving(
            self.shown.clone(),
            &self.prev,
            |n| SharedString::from(n.id.to_string()),
            &mut self.stack,
            now,
        );
        self.prev = self.shown.clone();
        let boxes = self.boxes.clone();
        let mut rows: Vec<AnyElement> = Vec::new();
        for (slot, (n, leaving)) in items.iter().enumerate() {
            let key = SharedString::from(n.id.to_string());
            let mut s = if *leaving {
                self.stack.progress_out(&key, now)
            } else {
                self.stack.progress(&key, slot, now)
            };
            let content = if hud {
                self.pill(&theme, n)
            } else {
                // 横方向に退出したカードは高さを畳み、下のカードが滑らかに詰まる。
                let look = self.swipes.look(n.id, now);
                s.grow *= look.height;
                self.card(&theme, n, look)
            };
            rows.push(drop_in(s, n.id, &boxes, sy < 0.0, align, content));
        }
        let drawn: Vec<u32> = items.iter().map(|(n, _)| n.id).collect();
        self.swipes.forget(&drawn);
        boxes.borrow_mut().retain(|id, _| drawn.contains(id));
        if self.stack.finish() || sliding || self.swipes.busy() {
            components::keep_drawing(window, cx);
        }
        // 「ほか n 件」はカード列の下部に表示する。ギャップが上に付くレイアウト（下寄せ）では、ここにも同じギャップを設ける
        // （設けないと最下部のカードに重なる）。
        let more = (self.more > 0).then(|| {
            div().when(sy < 0.0, |d| d.pt(px(spacing::SM))).child(
                components::card_panel_fit(&theme)
                    .debug_selector(|| "toast-more".into())
                    .px(px(spacing::MD))
                    .py(px(spacing::XS))
                    .rounded(px(radius::FULL))
                    .text_size(px(font_size::SM))
                    .text_color(rgba(theme.neutral_text_low))
                    .child(i18n::tf(cx, "toast.more", &[("count", &self.more)])),
            )
        });
        let column = div()
            .relative()
            .left(px(offset.x))
            .top(px(offset.y))
            .w_full()
            .flex()
            .flex_col()
            .map(|d| match align {
                Alignment::Start => d.items_start(),
                Alignment::Center => d.items_center(),
                Alignment::End => d.items_end(),
            })
            .children(rows)
            .children(more);
        div()
            .size_full()
            .p(px(PANEL_MARGIN))
            .flex()
            .flex_col()
            .text_color(rgba(theme.neutral_text))
            .map(|d| match sy {
                y if y > 0.0 => d.justify_start(),
                y if y < 0.0 => d.justify_end(),
                _ => d.justify_center(),
            })
            .child(column)
            // インタラクティブな位置でのみ、カードのスワイプ操作を受け付ける。
            .when(!hud, |d| {
                let (swipes, panel) = (self.swipes.clone(), self.panel);
                d.child(
                    canvas(
                        |_, _, _| {},
                        move |_, _, window, _| swipes.listen(panel, window),
                    )
                    .absolute()
                    .size_full(),
                )
            })
    }
}

#[cfg(test)]
mod tests;
