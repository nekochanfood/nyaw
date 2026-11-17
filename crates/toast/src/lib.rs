//! Layer 5 画面: OS の通知のトースト。1 枚のパネルを使い回し、届いた通知を視界の端（HUD）→ 手首の時計の上 →
//! ダッシュボードの上へ移す（進むだけで戻らない。[`flow`]）。場所を移るときは、今の場所で消えて（アウト）から次の場所へ
//! 瞬間的に移り、そこで現れる（イン）。目の前を横切って視界を遮らないように。HUD はアイコンと見出しだけで押せず、
//! 手首とダッシュボードでは本文（行数を限って）まで出し、押すと通知センター（ダッシュボードのクイック設定の一覧。
//! 詳しい中身と通知のボタンはそこで出す）が開く。左右にスワイプするとその場で片付く（[`ui::swipe`]。通知センターからは
//! 消さない）。通知センターが出ている間は出ない（同じものが並ぶので）。開くのは wrist。

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

/// パネルの解像度（px）。中身は置き場所の側（HUD は設定の端、手首とダッシュボードは下）に寄せ、残りは透明。
pub const RES_PX: (u32, u32) = (420, 600);
/// 置き場所ごとの密度（px/m）。どこでもダッシュボードの画面と同じくらいの見た目の大きさになるよう、遠い HUD は粗く、
/// 近い手首は細かく。HUD は視界の端でも気づけるよう少し大きく。
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
/// HUD のアイコンの一辺（px）。
const HUD_ICON: f32 = 28.0;
/// 並べたカードのまとまりが、出入りで滑る距離（px）。
const SLIDE_PX: f32 = 56.0;
/// 足されたカードが上から落ちてくる距離（px）。
const DROP_PX: f32 = 24.0;
/// アウトが終わりきるのを待つ余裕（アウトの長さに足す。フレームの遅れで消えきる前に移らないように）。
const OUT_MARGIN: Duration = Duration::from_millis(50);

/// トーストの起動仕様。手首の時計 `wrist` の上と、ダッシュボード（`PanelControl::dock`）の上へも移る。`opened` は作られた
/// パネルの ID を受け取る。
pub fn spec(wrist: PanelId, opened: impl FnOnce(PanelId, &mut gpui::App) + 'static) -> PanelSpec {
    PanelSpec::root(Pose::IDENTITY, move |panel, _, cx: &mut Context<Toast>| {
        opened(panel, cx);
        // 何も届いていない間は隠しておく。出入りは不透明度だけにして、どちらへ動くかは中身が描く。
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
    // 常駐で、置き場所は自分で決める（閉じる・動かす対象ではない）。ナビゲーションバーは付けない。
    .without_nav_bar()
}

/// 密度 `px_per_m` での大きさ（m。等倍）。
fn size(px_per_m: f32) -> Vec2 {
    Vec2::new(RES_PX.0 as f32, RES_PX.1 as f32) / px_per_m
}

/// 頭から見た HUD の置き場所。中身の外側の端（パネルの端から影の余白の内側）が、両目に確実に映る範囲 `safe`
/// （[`Fov::safe`]）の `anchor` の側の端に来るよう、[`HUD_DISTANCE`] 先に置いて頭の方へ向ける。`scale` は UI スケール。
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

/// パネルを置いた先（同じなら置き直さない）。
#[derive(Debug, Clone, Copy, PartialEq)]
enum Target {
    Hud(Pose),
    Parent(Parent),
}

/// 出入りの段階。出ていく間（`Out`）は前の中身のまま描き、消えきってから次の場所へ移る。
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
    /// 今パネルを置いている所（出入りしている間も）。隠れていれば `None`。
    target: Option<Target>,
    phase: Phase,
    /// 描いている置き場所（出ていく間も前のまま）。
    place: Place,
    /// 出している通知（新しい順）と、出しきれなかった数。
    shown: Vec<Arc<Notification>>,
    more: usize,
    /// 前のフレームに並べていた通知（無くなったカードを、消えていく間だけ描き残す）。
    prev: Vec<Arc<Notification>>,
    /// カードの出入り（上から積む）。
    stack: Entering,
    /// カードの置かれた所（ウィンドウの px。前のフレームに測った自然な大きさ）。出入りで高さを伸び縮みさせる。
    boxes: Rc<RefCell<HashMap<u32, Bounds<Pixels>>>>,
    /// 左右のスワイプで片付ける（[`ui::swipe`]）。
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

    /// 今の置き場所（出ていく間も前の場所。隠れていれば [`Place::Hidden`]）。
    pub fn place(&self) -> Place {
        if self.target.is_some() {
            self.place
        } else {
            Place::Hidden
        }
    }

    /// フレームループが状態を書き戻すたびに、置き場所と出す通知を決め直す。場所が変わるときは、今の場所で消えきってから
    /// 次の場所で現れる。同じ場所なら中身だけ入れ替える（カードが上から積まれ、無くなったものは消えていく）。
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
        // 読んだもの（通知センターに並んだ・押した）はもう出さない。
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
        // 出ていく途中: 前の中身のまま、消えきるまで待つ。消えきったら親から外す（親を出し直したときに一緒に出ないように）。
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
            // 違う所へ移る・隠す: まずこの場所で消える（中身はそのまま）。
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

    /// 隠れているパネルを `next` へ移して出す（イン）。カードは上から順に積まれる。
    fn show(&mut self, place: Place, next: Target, now: Instant, cx: &mut Context<Self>) {
        let height = RES_PX.1 as f32
            / match place {
                Place::Hud => HUD_PX_PER_M,
                Place::Wrist => WRIST_PX_PER_M,
                _ => DASHBOARD_PX_PER_M,
            };
        let mut commands = match next {
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
        commands.extend([PanelCommand::FitHeight(height), PanelCommand::Hide(false)]);
        ui::commands(self.panel, commands, cx);
        (self.target, self.place, self.phase) = (Some(next), place, Phase::In(now));
        // 前の場所のカードは引き継がず、ここで上から順に積み直す。
        (self.shown, self.prev) = (Vec::new(), Vec::new());
        self.stack.replay();
        ui::notify(self.panel, cx);
    }

    /// 出している通知を入れ替える（変わったときだけ描き直す）。
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

    /// 並べたカードのまとまりが隠れている側（px の向き）: 手首・ダッシュボードは下（下から上に入り、上から下に出る）、
    /// HUD は寄せた端の外（上の端なら上、左右の端なら横）。
    fn hidden_side(&self, cx: &gpui::App) -> Vec2 {
        match self.place {
            Place::Hud => match ui::settings(cx).notification_anchor.side() {
                (_, sy) if sy != 0.0 => Vec2::new(0.0, -sy),
                (sx, _) => Vec2::new(sx, 0.0),
            },
            _ => Vec2::Y,
        }
    }

    /// まとまりのずれ（px）。入り・出の途中なら動き続ける（`true`）。
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

    /// HUD の 1 行: アイコンと見出し（1 行。長ければ省略）。押せない。
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

    /// 手首・ダッシュボードの 1 枚（[`ui::notification::card`]。広げない）。押すと通知センターがその通知を広げて開く。
    /// 左右にスワイプしている間は横にずれて薄れる（[`ui::swipe`]）。
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

/// 揃え方（HUD は寄せた端の側。手首・ダッシュボードは中央）。
#[derive(Clone, Copy)]
enum Alignment {
    Start,
    Center,
    End,
}

/// カード 1 枚の出入り（上から積む）: 出具合 `s` に合わせて高さが伸び（下のカードを押し下げる）、上から落ちてきて現れる。
/// 消えるときは逆に縮んで上へ抜ける。高さは前のフレームに測った自然な高さ（`boxes`）。出きったら自然な高さに任せる
/// （ホバーで伸びても追従する）。カードとの隙間は寄せた端の反対側（`gap_top` なら上）に取る。
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
        // 高さは画素に揃える（端数で動くと、ほかのカードの文字だけが画素に吸い付いて震える）。
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
        // HUD は設定の端に、手首・ダッシュボードは下（立てた所）に寄せる。カードはどこでも新しいものが上。
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
        // スワイプで畳みきったカードは、もう出さない（消えていく動きも済んでいる。通知センターには残る）。
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
                // 横へ抜けたカードは高さを畳み、下のカードが滑らかに詰まる。
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
        // 「ほか n 件」は並べたカードの下。カードの隙間が上に付く並べ方（下に寄せる）では、ここにも同じ隙間を空ける
        // （空けないと一番下のカードに重なる）。
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
            // 押せる所でだけ、カードのスワイプを受ける。
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
