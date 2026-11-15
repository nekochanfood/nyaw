//! Layer 5 画面: OS の通知のトースト。1 枚のパネルを使い回し、届いた通知を視界の端（HUD）→ 手首の時計の上 →
//! ダッシュボードの上へ滑らかに動かす（進むだけで戻らない。[`flow`]）。HUD はアイコンと見出しだけで押せず、
//! 手首とダッシュボードでは本文まで出して押せる（ホバーすると本文が伸び、通知のボタンが出る）。開くのは wrist。

mod flow;

use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};

use design::components::{self, Variant};
use design::{PANEL_MARGIN, Theme, font_size, icons, radius, spacing};
use gpui::{Context, FontWeight, IntoElement, RenderImage, Window, div, img, prelude::*, px, rgba};
use kernel::subsystems::notifications::Notification;
use kernel::vr::{Device, Fov, Pose, Quat, Vec2, Vec3};
use kernel::{NotificationAnchor, PanelCommand, PanelId, Parent};
use ui::{PanelControl, PanelSpec};

pub use flow::Place;
use flow::{Flow, Scene};

/// パネルの解像度（px）。中身は置き場所の側（HUD は設定の端、手首とダッシュボードは下）に寄せ、残りは透明。
pub const RES_PX: (u32, u32) = (420, 600);
/// 置き場所ごとの密度（px/m）。どこでもダッシュボードの画面と同じくらいの見た目の大きさになるよう、遠い HUD は粗く、
/// 近い手首は細かく。
const HUD_PX_PER_M: f32 = 600.0;
const WRIST_PX_PER_M: f32 = 3000.0;
const DASHBOARD_PX_PER_M: f32 = 1000.0;
/// HUD の頭からの距離（m）。
pub const HUD_DISTANCE: f32 = 1.5;
/// 手首の時計の上に立てる角度（rad。顔の方へ起こす）。時計を見る目は文字盤の正面にあり、その上の通知は少し斜めに見えるので。
const WRIST_TILT: f32 = 0.3;
/// 並べる数（それより多ければ「ほか n 件」）。
const HUD_ROWS: usize = 3;
const CARD_ROWS: usize = 3;
/// 本文の行数（ふだん・ホバー中）。
const BODY_LINES: usize = 2;
const BODY_LINES_HOVER: usize = 6;
/// アイコンの一辺（px）。
const HUD_ICON: f32 = 28.0;
const CARD_ICON: f32 = 36.0;
/// 隠してから、ほかの場所に出し直すまで（消えるのを待つ。戻るように動いて見せない）。
const REAPPEAR: Duration = Duration::from_millis(200);

/// トーストの起動仕様。手首の時計 `wrist` の上と、ダッシュボード（`PanelControl::dock`）の上へも動く。
pub fn spec(wrist: PanelId) -> PanelSpec {
    PanelSpec::root(Pose::IDENTITY, move |panel, _, cx: &mut Context<Toast>| {
        // 何も届いていない間は隠しておく。
        ui::command(panel, PanelCommand::Hide(true), cx);
        Toast::new(panel, wrist, cx)
    })
    .sized(size(HUD_PX_PER_M), RES_PX)
    .locked()
    .without_resize()
    // 常駐で、置き場所は自分で決める（閉じる・動かす対象ではない）。ナビゲーションバーは付けない。
    .without_nav_bar()
}

/// 密度 `px_per_m` での大きさ（m。等倍）。
fn size(px_per_m: f32) -> Vec2 {
    Vec2::new(RES_PX.0 as f32, RES_PX.1 as f32) / px_per_m
}

/// 頭から見た HUD の置き場所。中身の外側の端（パネルの端から影の余白の内側）が、両目に確実に映る範囲 `safe`
/// （[`Fov::safe`]）の `anchor` の側の端に来るよう、[`HUD_DISTANCE`] 先に置いて頭の方へ向ける。`scale` は UI スケール。
// ponytail: 頭へ向けて回した分、外の端は 1° ほど外へずれる。セーフエリアの余裕（視野の 60%）に収まるので詰めない。
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

/// パネルを置いた先（同じなら置き直さない）。
#[derive(Debug, Clone, Copy, PartialEq)]
enum Target {
    Hud(Pose),
    Parent(Parent),
}

pub struct Toast {
    panel: PanelId,
    wrist: PanelId,
    flow: Flow,
    place: Place,
    target: Option<Target>,
    /// 最後に隠した時刻（[`REAPPEAR`]）。
    hidden_at: Option<Instant>,
    /// 出している通知（新しい順）と、出しきれなかった数。
    shown: Vec<Arc<Notification>>,
    more: usize,
    /// ホバーしている通知。
    hovered: Option<u32>,
    /// アプリごとのアイコンの画像。
    icons: HashMap<String, Option<Arc<RenderImage>>>,
}

impl Toast {
    pub fn new(panel: PanelId, wrist: PanelId, cx: &mut Context<Self>) -> Self {
        cx.observe_global::<PanelControl>(Self::sync).detach();
        Self {
            panel,
            wrist,
            flow: Flow::default(),
            place: Place::Hidden,
            target: None,
            hidden_at: None,
            shown: Vec::new(),
            more: 0,
            hovered: None,
            icons: HashMap::new(),
        }
    }

    /// 今の置き場所（テスト用）。
    pub fn place(&self) -> Place {
        self.place
    }

    /// フレームループが状態を書き戻すたびに、置き場所と出す通知を決め直す。変わったときだけパネルを動かし、描き直す。
    fn sync(&mut self, cx: &mut Context<Self>) {
        let ctl = cx.global::<PanelControl>();
        let status = |id: PanelId| ctl.panels.get(&id).map(|p| &p.status);
        let dock = ctl
            .dock
            .filter(|(bar, _)| status(*bar).is_some_and(|s| !s.hidden && !s.closed));
        let scene = Scene {
            dashboard: dock.is_some(),
            gazing: status(self.wrist).is_some_and(|s| s.gazed),
            lockdown: ctl.settings.lockdown,
        };
        let notes = ctl.notifications.clone();
        let settings = ctl.settings;
        let safe = ctl.fov.unwrap_or(Fov::NARROW).safe();
        let ids: Vec<u32> = notes.iter().map(|n| n.id).collect();
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
        self.move_to(place, target, now, cx);
        let same = |a: &[Arc<Notification>], b: &[Arc<Notification>]| {
            a.len() == b.len() && a.iter().zip(b).all(|(a, b)| Arc::ptr_eq(a, b))
        };
        if !same(&shown, &self.shown) || more != self.more {
            for n in &shown {
                self.icons.entry(n.app.id.clone()).or_insert_with(|| {
                    let icon = n.app.icon.as_ref()?;
                    ui::render_image(icon.width, icon.height, icon.rgba.to_vec())
                });
            }
            self.shown = shown;
            self.more = more;
            ui::notify(self.panel, cx);
        }
    }

    /// パネルを `target` へ置く。先へ進む（HUD → 手首 → ダッシュボード）ときは見えたまま滑らかに動かし、それ以外は
    /// いったん隠して、消えきってから出し直す（戻るようには動かさない）。
    fn move_to(
        &mut self,
        place: Place,
        target: Option<Target>,
        now: Instant,
        cx: &mut Context<Self>,
    ) {
        if target == self.target {
            return;
        }
        logger::debug!("toast moves", from = ?self.place, to = ?place);
        let forward = place > self.place && self.place != Place::Hidden;
        if self.target.is_some() && !forward {
            // 隠す。親（手首・ダッシュボード）から外しておく（親を出し直したときに一緒に出ないように）。
            let mut hide = vec![PanelCommand::Hide(true)];
            if matches!(self.target, Some(Target::Parent(_))) {
                hide.push(PanelCommand::Parent(None));
            }
            ui::commands(self.panel, hide, cx);
            (self.target, self.place, self.hidden_at) = (None, Place::Hidden, Some(now));
            ui::notify(self.panel, cx);
        }
        let Some(target) = target else {
            return;
        };
        if self.target.is_none() && self.hidden_at.is_some_and(|t| now - t < REAPPEAR) {
            return;
        }
        let height = RES_PX.1 as f32
            / match place {
                Place::Hud => HUD_PX_PER_M,
                Place::Wrist => WRIST_PX_PER_M,
                _ => DASHBOARD_PX_PER_M,
            };
        let mut commands = match target {
            // 頭に付ける前に、今の頭の前へ置いておく（頭を追えないランタイム（novr）では、そのままそこに残る）。
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
        // 見えたまま動くときは、動きの中で大きさも変わっていく（親を移した後に大きさを変える）。
        if place != self.place {
            commands.push(PanelCommand::FitHeight(height));
        }
        if self.target.is_none() {
            commands.push(PanelCommand::Hide(false));
        }
        ui::commands(self.panel, commands, cx);
        (self.target, self.place) = (Some(target), place);
        ui::notify(self.panel, cx);
    }

    /// 通知 `id` の本文（`action` が `None`）かボタンを押した。URL は確かめてから開き（`ui::link`）、ほかはアプリへ届ける。
    /// 押した通知はもう出さない。
    fn open(
        &mut self,
        id: u32,
        action: Option<usize>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(n) = self.shown.iter().find(|n| n.id == id) else {
            return;
        };
        let url = match action {
            None => n.url.clone(),
            Some(i) => n.actions.get(i).and_then(|a| a.url.clone()),
        };
        match url {
            Some(url) => design::link::open(url, window, cx),
            None => ui::command(self.panel, PanelCommand::Notification { id, action }, cx),
        }
        self.flow.done(id);
        self.shown.retain(|n| n.id != id);
        ui::notify(self.panel, cx);
    }

    fn icon(&self, n: &Notification, size: f32) -> gpui::AnyElement {
        match self.icons.get(&n.app.id).cloned().flatten() {
            Some(image) => img(image).size(px(size)).flex_none().into_any_element(),
            None => design::icon(&icons::BELL).size(px(size)).into_any_element(),
        }
    }

    /// HUD の 1 行: アイコンと見出し（1 行。長ければ省略）。押せない。
    fn pill(&self, theme: &Theme, n: &Notification) -> gpui::AnyElement {
        let id = n.id;
        components::card_panel_fit(theme)
            .debug_selector(move || format!("toast-hud-{id}"))
            .max_w_full()
            .flex()
            .items_center()
            .gap(px(spacing::SM))
            .px(px(spacing::MD))
            .py(px(spacing::SM))
            .rounded(px(radius::FULL))
            .child(self.icon(n, HUD_ICON))
            .child(
                div()
                    .min_w_0()
                    .truncate()
                    .text_size(px(font_size::MD))
                    .font_weight(FontWeight::BOLD)
                    .child(title(n)),
            )
            .into_any_element()
    }

    /// 手首・ダッシュボードの 1 枚: アイコン・見出し・アプリの名前・本文（行数を限って省略）。押すと開く。ホバー中は本文が
    /// 伸び、通知のボタンが並ぶ。
    fn card(&self, theme: &Theme, n: &Notification, cx: &mut Context<Self>) -> gpui::AnyElement {
        let id = n.id;
        let hovered = self.hovered == Some(id);
        let actions = n.actions.iter().enumerate().map(|(i, a)| {
            components::button(
                theme,
                format!("toast-action-{id}-{i}"),
                a.label.clone(),
                Variant::Default,
            )
            .on_click(cx.listener(move |this, _, window, cx| {
                cx.stop_propagation();
                this.open(id, Some(i), window, cx);
            }))
        });
        let body = (!n.body.is_empty()).then(|| {
            div()
                .w_full()
                .text_size(px(font_size::SM))
                .text_color(rgba(theme.neutral_text_low))
                .line_clamp(if hovered {
                    BODY_LINES_HOVER
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
            .child(self.icon(n, CARD_ICON))
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
        let tile = components::tile(theme, format!("toast-{id}"))
            .w_full()
            // ボタンの高さに決まっているので、中身に合わせて伸ばす。
            .h_auto()
            .items_start()
            .rounded(px(radius::XL))
            .child(header)
            .children(body)
            .when(hovered && !n.actions.is_empty(), |t| {
                t.child(
                    div()
                        .w_full()
                        .flex()
                        .flex_wrap()
                        .gap(px(spacing::SM))
                        .children(actions),
                )
            })
            .on_click(cx.listener(move |this, _, window, cx| this.open(id, None, window, cx)));
        div()
            .id(gpui::SharedString::from(format!("toast-card-{id}")))
            .w_full()
            .on_hover(cx.listener(move |this, on: &bool, _, cx| {
                let next = match (*on, this.hovered) {
                    (true, _) => Some(id),
                    (false, Some(h)) if h == id => None,
                    (false, h) => h,
                };
                if next != this.hovered {
                    this.hovered = next;
                    ui::notify(this.panel, cx);
                }
            }))
            .child(components::card_panel_fit(theme).w_full().child(tile))
            .into_any_element()
    }
}

/// 見出し（無ければアプリの名前）。
fn title(n: &Notification) -> gpui::SharedString {
    if n.title.is_empty() {
        n.app.name.clone().into()
    } else {
        n.title.clone().into()
    }
}

impl Render for Toast {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = Theme::current(cx);
        // HUD は設定の端に、手首・ダッシュボードは下（立てた所）に寄せる。新しいものほど寄せた端に近く。
        let (sx, sy) = match self.place {
            Place::Hud => ui::settings(cx).notification_anchor.side(),
            _ => (0.0, -1.0),
        };
        let mut items: Vec<gpui::AnyElement> = match self.place {
            Place::Hud => self.shown.iter().map(|n| self.pill(&theme, n)).collect(),
            _ => {
                let shown = self.shown.clone();
                shown.iter().map(|n| self.card(&theme, n, cx)).collect()
            }
        };
        if self.more > 0 {
            items.push(
                components::card_panel_fit(&theme)
                    .debug_selector(|| "toast-more".into())
                    .px(px(spacing::MD))
                    .py(px(spacing::XS))
                    .rounded(px(radius::FULL))
                    .text_size(px(font_size::SM))
                    .text_color(rgba(theme.neutral_text_low))
                    .child(i18n::tf(cx, "toast.more", &[("count", &self.more)]))
                    .into_any_element(),
            );
        }
        if sy < 0.0 {
            items.reverse();
        }
        let root = div()
            .size_full()
            .p(px(PANEL_MARGIN))
            .flex()
            .flex_col()
            .gap(px(spacing::SM))
            .text_color(rgba(theme.neutral_text));
        let root = match sy {
            y if y > 0.0 => root.justify_start(),
            y if y < 0.0 => root.justify_end(),
            _ => root.justify_center(),
        };
        let root = match sx {
            x if x < 0.0 => root.items_start(),
            x if x > 0.0 => root.items_end(),
            _ => root.items_center(),
        };
        root.children(items)
    }
}

#[cfg(test)]
mod tests;
