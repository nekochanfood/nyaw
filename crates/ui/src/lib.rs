//! Layer 5: UI 層。各オーバーレイパネルを GPUI のウィンドウか、Tauri の WebView（[`web`]）で実体化する。
//! どちらも同じフレームループ（[`run`]）・同じカーネル・同じ PanelId の空間に載り、GPUI の画面から Web のページを開ける。
//!
//! - GPUI: オーバーレイ層のレーザーイベントは通常の GPUI マウス入力に変換される。
//!   そのためビューはデスクトップの GPUI ビューと全く同じように書け、VR 上にあることを意識しない。
//!   逆向きは [`capture`] でウィンドウをオフスクリーン描画して読み戻し、`Kernel::submit` で VR パネルへ送る。
//! - Web: レーザーイベントはブリッジが DOM のポインタイベントにする（[`web`]）。ページは React 等で書く。
//!
//! イベントループは GPUI が持ち、Tauri は最初の Web のパネルを開いたときに起こして毎フレーム 1 回ずつ回す。

pub mod web;
use kernel::rendering::gpui::read_back;
pub use kernel::rendering::gpui::{Frame, capture, set_render_lod};
mod panel_window;
pub use panel_window::{open_panel_window, panel_window_options};

use std::{
    collections::HashMap,
    time::{Duration, Instant},
};

use gpui::{
    AnyView, AnyWindowHandle, App, Context, Entity, MouseButton, MouseDownEvent, MouseExitEvent,
    MouseMoveEvent, MouseUpEvent, Pixels, PlatformInput, Point, ScrollDelta, ScrollWheelEvent,
    TouchPhase, Window, div, prelude::*, px, rgb,
};
use kernel::vr::{Hand, Quat, Vec3};
use kernel::{
    Anchor, Device, Kernel, PanelCommand, PanelId, PanelStatus, Pointer, PointerEvent, PointerMode,
    Pose, ThemeMode, UiEvent, Vec2, VrBackend,
};
use profiler::{Profiler, ProfilerView, Stages};

/// 1つの手のレーザーイベントを GPUI のプラットフォーム入力に変換する。GPUI のイベントは
/// 位置とボタン状態を持つがオーバーレイのイベントは持たないので、直前の値を保持する。
///
/// [`PointerMode::Touch`] では押したまま動かすとスワイプ（ピクセル単位のスクロール。離すと慣性で滑る）になり、
/// 動かさずに離すとタップ（押して離す）、動かさずに [`LONG_PRESS`] 押し続けるとマウスの押下になる（そのまま動かせば
/// ドラッグ＆ドロップ）。GPUI にはタッチ入力が無いので、どれと決まるまで押下を送らずに待つ。
/// 方式は押した瞬間に決め、離すまで変えない。
#[derive(Debug, Default)]
pub struct PointerBridge {
    pos: Point<Pixels>,
    pressed: bool,
    touch: Option<Touch>,
    /// スワイプを離した後の慣性（宛先の位置と、1 フレームあたりの移動量）。
    momentum: Option<(Point<Pixels>, Point<Pixels>)>,
    /// GPUI が知っているこの手の位置（最後に送ったイベントの位置。離れたら `None`）。同じ位置への Move は送らない
    /// （オーバーレイは指している間 毎 tick Move を出すので、止まっていても描き直しと読み戻しが毎フレーム走ってしまう）。
    sent: Option<Point<Pixels>>,
}

/// タッチで押している間の状態。
#[derive(Debug)]
struct Touch {
    start: Point<Pixels>,
    /// 押した時刻（長押しの判定）。
    at: Instant,
    /// スワイプと決まった。
    swiping: bool,
    /// 長押しでマウスの押下になった（以降はマウスと同じ）。
    held: bool,
    /// スワイプの 1 イベントあたりの移動量（慣性の初速。直近を重く見て均す）。
    velocity: Point<Pixels>,
}

/// これだけ動かさずに押し続けると、タッチでもマウスの押下になる（ドラッグ＆ドロップを始められる）。
pub const LONG_PRESS: Duration = Duration::from_millis(400);
/// 長押しでドラッグを始めるときに押したまま動かす量（論理 px）。GPUI のドラッグの閾値（2px）を越える。
const DRAG_NUDGE: f32 = 3.0;
/// 慣性の 1 フレームあたりの減衰（約 90Hz で 1 秒後に 1% 程度）。
const FRICTION: f32 = 0.95;
/// 慣性がこれより遅くなったら止める（論理 px / フレーム）。
const MOMENTUM_STOP: f32 = 0.5;

/// これより動いたらタップではなくスワイプ（論理 px）。
// ponytail: 手ブレを見込んだ固定値。指す距離で変えたくなったら角度で決める。
pub const SWIPE_SLOP: f32 = 12.0;

impl PointerBridge {
    pub fn translate(&mut self, ev: PointerEvent, mode: PointerMode) -> Vec<PlatformInput> {
        let at = |p: Vec2| Point::new(px(p.x), px(p.y));
        if let Some(t) = &mut self.touch {
            match ev {
                PointerEvent::Move { .. } if t.held => return self.mouse(ev),
                PointerEvent::Move { pos } => {
                    let (last, pos) = (self.pos, at(pos));
                    let d = pos - t.start;
                    let was_swiping = t.swiping;
                    t.swiping |= f32::from(d.x).hypot(f32::from(d.y)) > SWIPE_SLOP;
                    if !t.swiping {
                        if t.at.elapsed() < LONG_PRESS {
                            return Vec::new();
                        }
                        // 長押し: 押した所でマウスの押下にする。以降はマウスと同じ（動かせばドラッグ）。
                        // 押したまま GPUI のドラッグの閾値（2px）を越えて動かし、カーソルを動かさなくてもすぐドラッグを始めて
                        // その見た目を出す。最後に今の位置へ戻す。
                        t.held = true;
                        let start = Vec2::new(t.start.x.into(), t.start.y.into());
                        let mut out = self.mouse(PointerEvent::Move { pos: start });
                        out.extend(self.mouse(PointerEvent::Down { pos: start }));
                        out.extend(self.mouse(PointerEvent::Move {
                            pos: start + Vec2::new(DRAG_NUDGE, 0.0),
                        }));
                        out.extend(self.mouse(ev));
                        return out;
                    }
                    let delta = pos - last;
                    t.velocity = t.velocity * 0.5 + delta * 0.5;
                    self.pos = pos;
                    self.sent = Some(t.start);
                    // 中身が指に付いてくる向き（下へ動かすと上へスクロール）。宛先は押した所に固定する。
                    return vec![PlatformInput::ScrollWheel(ScrollWheelEvent {
                        position: t.start,
                        delta: ScrollDelta::Pixels(pos - last),
                        modifiers: Default::default(),
                        // スクロールバーなどが、指のスライドの始まりと終わりを知れるように段階を付ける。
                        touch_phase: if was_swiping {
                            TouchPhase::Moved
                        } else {
                            TouchPhase::Started
                        },
                    })];
                }
                PointerEvent::Up { .. } if t.held => {
                    self.touch = None;
                    return self.mouse(ev);
                }
                PointerEvent::Up { pos } => {
                    let t = self.touch.take().expect("pressed");
                    if t.swiping {
                        // 離した後も慣性で滑る（フレームごとの momentum）。GPUI のマウスは離した所へ。
                        self.momentum = Some((t.start, t.velocity));
                        let mut out = vec![PlatformInput::ScrollWheel(ScrollWheelEvent {
                            position: t.start,
                            delta: ScrollDelta::Pixels(Point::default()),
                            modifiers: Default::default(),
                            touch_phase: TouchPhase::Ended,
                        })];
                        out.extend(self.mouse(PointerEvent::Move { pos }));
                        return out;
                    }
                    // タップ: 押して離す。
                    let mut out = self.mouse(PointerEvent::Move { pos });
                    out.extend(self.mouse(PointerEvent::Down { pos }));
                    out.extend(self.mouse(PointerEvent::Up { pos }));
                    return out;
                }
                PointerEvent::Leave => {
                    // スワイプ中に外れたら、指のスライドが終わったことを知らせる。
                    let swiped = t.swiping.then_some(t.start);
                    self.touch = None;
                    if let Some(position) = swiped {
                        let mut out = vec![PlatformInput::ScrollWheel(ScrollWheelEvent {
                            position,
                            delta: ScrollDelta::Pixels(Point::default()),
                            modifiers: Default::default(),
                            touch_phase: TouchPhase::Ended,
                        })];
                        out.extend(self.mouse(ev));
                        return out;
                    }
                }
                // スティックのスクロールはマウスと同じ。押し直しは来ない。
                PointerEvent::Scroll { .. } | PointerEvent::Down { .. } => {}
            }
        } else if let (PointerMode::Touch, PointerEvent::Down { pos }) = (mode, ev) {
            // 触れたら慣性は止まる。
            self.momentum = None;
            self.touch = Some(Touch {
                start: at(pos),
                at: Instant::now(),
                swiping: false,
                held: false,
                velocity: Point::default(),
            });
            self.pos = at(pos);
            return Vec::new();
        }
        self.mouse(ev)
    }

    /// タッチで押して動かしていない間の、長押し（[`LONG_PRESS`]）までの進み具合（0〜1）。押していない・スワイプ中・
    /// 長押しが済んだ後は `None`。ポインタのリングに出す。
    pub fn hold_progress(&self) -> Option<f32> {
        let t = self.touch.as_ref().filter(|t| !t.swiping && !t.held)?;
        // 普通のタップでリングがちらつかないよう、押して少しの間は出さず、残りの時間で 0〜1 に満ちる。
        const RING_DELAY: Duration = Duration::from_millis(150);
        let e = t.at.elapsed().checked_sub(RING_DELAY)?;
        Some((e.as_secs_f32() / (LONG_PRESS - RING_DELAY).as_secs_f32()).min(1.0))
    }

    /// この手のレーザーがパネルの上にある（GPUI がこの手のマウスの位置を知っている）。
    pub fn on_panel(&self) -> bool {
        self.sent.is_some()
    }

    /// タッチの長押しでマウスの押下になっている（ドラッグできる）。
    pub fn held(&self) -> bool {
        self.touch.as_ref().is_some_and(|t| t.held)
    }

    /// スワイプを離した後の慣性の 1 フレーム分（スクロール）。滑っていなければ `None`。フレームごとに 1 回呼ぶ。
    pub fn momentum(&mut self) -> Option<PlatformInput> {
        let (position, v) = self.momentum.as_mut()?;
        *v *= FRICTION;
        if f32::from(v.x).hypot(f32::from(v.y)) < MOMENTUM_STOP {
            self.momentum = None;
            return None;
        }
        Some(PlatformInput::ScrollWheel(ScrollWheelEvent {
            position: *position,
            delta: ScrollDelta::Pixels(*v),
            modifiers: Default::default(),
            // 指を離した後の惰性（指のスライドそのものではない）。
            touch_phase: TouchPhase::Ended,
        }))
    }

    /// マウスとして変換する。
    fn mouse(&mut self, ev: PointerEvent) -> Vec<PlatformInput> {
        let at = |p: Vec2| Point::new(px(p.x), px(p.y));
        let held = |pressed: bool| pressed.then_some(MouseButton::Left);
        match ev {
            PointerEvent::Move { pos } if self.sent == Some(at(pos)) => return Vec::new(),
            PointerEvent::Leave => self.sent = None,
            PointerEvent::Move { pos }
            | PointerEvent::Down { pos }
            | PointerEvent::Up { pos }
            | PointerEvent::Scroll { pos, .. } => self.sent = Some(at(pos)),
        }
        vec![match ev {
            PointerEvent::Move { pos } => {
                self.pos = at(pos);
                PlatformInput::MouseMove(MouseMoveEvent {
                    position: self.pos,
                    pressed_button: held(self.pressed),
                    modifiers: Default::default(),
                })
            }
            PointerEvent::Down { pos } => {
                (self.pos, self.pressed) = (at(pos), true);
                PlatformInput::MouseDown(MouseDownEvent {
                    button: MouseButton::Left,
                    position: self.pos,
                    modifiers: Default::default(),
                    click_count: 1,
                    first_mouse: false,
                })
            }
            PointerEvent::Up { pos } => {
                (self.pos, self.pressed) = (at(pos), false);
                PlatformInput::MouseUp(MouseUpEvent {
                    button: MouseButton::Left,
                    position: self.pos,
                    modifiers: Default::default(),
                    click_count: 1,
                })
            }
            PointerEvent::Scroll { pos, delta } => {
                self.pos = at(pos);
                PlatformInput::ScrollWheel(ScrollWheelEvent {
                    position: self.pos,
                    delta: ScrollDelta::Lines(Point::new(delta.x, delta.y)),
                    modifiers: Default::default(),
                    touch_phase: Default::default(),
                })
            }
            PointerEvent::Leave => {
                let pressed_button = held(std::mem::take(&mut self.pressed));
                PlatformInput::MouseExited(MouseExitEvent {
                    position: self.pos,
                    pressed_button,
                    modifiers: Default::default(),
                })
            }
        }]
    }
}

/// 1パネル分の操作要求と状態。
#[derive(Debug, Default)]
pub struct PanelState {
    pub commands: Vec<PanelCommand>,
    pub status: PanelStatus,
    /// 見えている間は毎フレーム描き直す（アニメーションし続けるビューが立てる）。
    pub live: bool,
}

/// ビューとフレームループの受け渡し口（GPUI のグローバル）。ビューは対象パネルの `commands` に積み、
/// フレームループが [`sync_panel`] でオーバーレイに反映して `status` を書き戻す。
/// ビューの型に依存しないので、本番・testkit のどのキットも同じ経路を通る。
#[derive(Default)]
pub struct PanelControl {
    pub panels: HashMap<PanelId, PanelState>,
    pub spawns: Vec<PanelSpec>,
    /// 計測するか。ビューが切り替え、フレームループが計測とプロファイラパネルの開閉を合わせる。
    /// パネルの閉じるボタンでも `false` になる。
    pub profiling: bool,
    /// 手ごとの、タッチの長押しの進み具合（0〜1。[`PointerBridge::hold_progress`]）。ポインタ（カーソル）がリングで見せる。
    pub hold: HashMap<Hand, f32>,
    /// 手ごとの、ツールチップの文字（指している要素が [`design::components::Tooltips`] に置いたもの）と、
    /// カーソルの上下どちらに出すか。フレームループが [`sync_tooltips`] で書き、ツールチップのパネルが描く。
    pub tooltip: HashMap<Hand, gpui::SharedString>,
    pub anchor: HashMap<Hand, Anchor>,
    /// 終了を頼む。フレームループが WebView を閉じてからアプリを終える（テストのハーネスもこれで終える）。
    pub quit: bool,
    /// 開いているパネルの数（閉じたものは数えない）と、同時に開ける数の上限（[`MAX_PANELS`] とランタイムの上限の小さい方）。
    /// フレームループが毎フレーム書く。
    pub open: usize,
    pub limit: usize,
}

impl gpui::Global for PanelControl {}

/// ビューが積んだ操作要求を `panel` に反映し、状態を書き戻す。変わったらウィンドウを描き直す（`true` を返す）。
/// 入力を配送した後・描画する前に毎フレーム呼ぶ。
pub fn sync_panel<B: VrBackend>(kernel: &mut Kernel<B>, panel: PanelId, cx: &mut App) -> bool {
    let ctl = cx
        .default_global::<PanelControl>()
        .panels
        .entry(panel)
        .or_default();
    for c in ctl.commands.drain(..) {
        kernel.apply(panel, c);
    }
    let status = kernel.status(panel);
    if ctl.status == status {
        return false;
    }
    let theme = status.theme;
    ctl.status = status;
    // テーマは全パネル共通。画面は `design::Theme::current` で引く。
    cx.set_global(match theme {
        ThemeMode::Dark => design::Mode::Dark,
        ThemeMode::Light => design::Mode::Light,
    });
    cx.refresh_windows();
    true
}

/// ウィンドウごとのツールチップの要求（[`design::components::Tooltips`]）を、そのパネルを指している手に振り分け、
/// 手ごとの向き（[`Kernel::tooltip_anchor`]）とともに [`PanelControl`] へ書く。変わったら `true`（ツールチップを描き直す）。
/// 入力を配送した後に毎フレーム呼ぶ。本番と testkit 共通。
pub fn sync_tooltips<B: VrBackend>(
    kernel: &Kernel<B>,
    bridges: &HashMap<(PanelId, Hand), PointerBridge>,
    windows: impl IntoIterator<Item = (PanelId, gpui::WindowId)>,
    cx: &mut App,
) -> bool {
    let requests = cx.default_global::<design::components::Tooltips>();
    let mut tooltip = HashMap::new();
    for (panel, window) in windows {
        let Some((_, text)) = requests.0.get(&window) else {
            continue;
        };
        // ponytail: 両手で同じパネルを指すと GPUI のマウスは 1 つなので、どちらの手の要求か見分けられない。両方に出す。
        for hand in [Hand::Left, Hand::Right] {
            if bridges.get(&(panel, hand)).is_some_and(|b| b.on_panel()) {
                tooltip.insert(hand, text.clone());
            }
        }
    }
    let anchor: HashMap<Hand, Anchor> = [Hand::Left, Hand::Right]
        .into_iter()
        .map(|h| (h, kernel.tooltip_anchor(h)))
        .collect();
    let ctl = cx.default_global::<PanelControl>();
    let changed = ctl.tooltip != tooltip || ctl.anchor != anchor;
    (ctl.tooltip, ctl.anchor) = (tooltip, anchor);
    changed
}

/// 1フレーム分: 下位パイプラインを回し、パネル宛てのイベントを GPUI 入力に変換して返す。
/// 本番の [`run`] とテスト用の testkit が共通で通る経路。
pub fn pump<B: VrBackend>(
    kernel: &mut Kernel<B>,
    bridges: &mut HashMap<(PanelId, Hand), PointerBridge>,
) -> Vec<(PanelId, PlatformInput)> {
    pump_with(kernel, bridges, |_| false).0
}

/// [`pump`] に Web のパネルを混ぜたもの。`is_web` なパネル宛てのイベントは変換せずにそのまま返す（[`web::Host::deliver`] へ）。
pub fn pump_with<B: VrBackend>(
    kernel: &mut Kernel<B>,
    bridges: &mut HashMap<(PanelId, Hand), PointerBridge>,
    is_web: impl Fn(PanelId) -> bool,
) -> (Vec<(PanelId, PlatformInput)>, Vec<UiEvent>) {
    let mode = kernel.pointer_mode();
    let mut gpui = Vec::new();
    let mut web = Vec::new();
    for e in kernel.tick() {
        if is_web(e.panel) {
            web.push(e);
            continue;
        }
        let bridge = bridges.entry((e.panel, e.hand)).or_default();
        let input = bridge.translate(e.event, mode);
        if bridge.held() {
            // 長押しでドラッグになった。ほかのパネルへ運べるよう、オーバーレイのキャプチャを外す。
            kernel.release_capture(e.hand);
        }
        gpui.extend(input.into_iter().map(|input| (e.panel, input)));
    }
    for (&(panel, _), b) in bridges.iter_mut() {
        if let Some(input) = b.momentum() {
            gpui.push((panel, input));
        }
    }
    (gpui, web)
}

const PANEL_PX: (u32, u32) = (800, 600);

/// ドラッグの見た目を描くウィンドウ（[`Pointer::Drag`] のパネル）。無ければ（novr）どのウィンドウでも描く。
#[derive(Default)]
struct DragWindows(Vec<AnyWindowHandle>);

impl gpui::Global for DragWindows {}

/// ドラッグ中の見た目（`on_drag` が返すビュー）を包む。GPUI はこれをどのウィンドウにもマウス位置に描くが、
/// VR ではカーソル側のウィンドウ（[`Pointer::Drag`]）だけに描く。パネルの外へ出てもウィンドウをまたいで見える。
pub struct DragPreview(AnyView);

/// `.on_drag(value, |_, _, _, cx| ui::drag_preview(cx, MyPreview))` のように使う。
pub fn drag_preview<V: Render>(cx: &mut App, view: V) -> Entity<DragPreview> {
    let view = cx.new(|_| view).into();
    cx.new(|_| DragPreview(view))
}

impl Render for DragPreview {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let here = window.window_handle();
        let shown = cx
            .try_global::<DragWindows>()
            .is_none_or(|d| d.0.is_empty() || d.0.contains(&here));
        div().when(shown, |d| d.child(self.0.clone()))
    }
}
/// プロファイラパネルのグラフを描き直す間隔（約 10Hz）。
const PROFILER_REDRAW: Duration = Duration::from_millis(100);
/// プロファイラパネルの位置。開いた瞬間の頭から見て正面 1m、少し上（Testing と重ならないように）。
/// 親パネルから見た、スポーンしたパネルの位置（親の中心の手前 5cm）。
const SPAWN_GAP: Vec3 = Vec3::new(0.0, 0.0, 0.05);
const PROFILER_POSE: Pose = Pose::new(Vec3::new(0.0, 0.25, -1.0), Quat::IDENTITY);
/// 入力も状態の変化も無いパネルを描き直す間隔。時計のようにビューが自分で描き直すものはこれで反映される。
// ponytail: GPUI はウィンドウの dirty を公開していないので推測で描く。公開されたら dirty なときだけにする。
pub(crate) const IDLE_REDRAW: Duration = Duration::from_millis(250);
/// 同時に開いておけるパネルの数（閉じたものは数えない）。OpenVR はランタイム全体で 128 枚までなので、
/// レーザーや他のアプリのオーバーレイの分を残す。ランタイムの上限（OpenXR のレイヤー数など）が小さければそちら。
/// 超えた分は開かずにエラーを出す。
pub const MAX_PANELS: usize = 64;
/// 描き直しても絵が変わらないパネルの、描き直す間隔の上限（時計の分の切り替わりはこの遅れで見える）。
const IDLE_MAX: Duration = Duration::from_secs(1);

/// `panel` で始まったドラッグの見た目を、押している手の [`Pointer::Drag`] のパネルに出す。
/// 大きさは元のパネルと同じ縮尺（1m あたりのピクセル）にする。
fn start_drag(
    kernel: &mut Kernel,
    bridges: &HashMap<(PanelId, Hand), PointerBridge>,
    windows: &[(PanelId, Option<AnyWindowHandle>)],
    panel: PanelId,
) -> Option<(Hand, PanelId)> {
    let hand = bridges
        .iter()
        .find(|((p, _), b)| *p == panel && b.pressed)
        .map(|((_, h), _)| *h)?;
    let (dp, _) = windows
        .iter()
        .find(|(id, _)| kernel.panel(*id).pointer == Some((hand, Pointer::Drag)))?;
    let src = kernel.panel(panel);
    let px_per_m = src.resolution.0 as f32 / src.size.x;
    let width = kernel.panel(*dp).resolution.0 as f32 / px_per_m;
    kernel.set_drag(hand, Some(width));
    Some((hand, *dp))
}

type PanelBuilder = dyn FnOnce(PanelId, &mut Window, &mut App) -> AnyView;

/// パネルの中身。
enum Content {
    /// GPUI のビュー。
    Gpui(Box<PanelBuilder>),
    /// Web のページ（ページ名。`web::Files` の `<ページ名>/index.html`）。
    Web(String),
}

/// 画面と VR 空間での配置。異なる画面型を同じ起動リストに入れられる。
pub struct PanelSpec {
    pose: Pose,
    from: Option<PanelId>,
    /// `pose` を開いた瞬間の頭（向きは水平方向だけ）からの相対姿勢とする。
    in_view: bool,
    size: Vec2,
    resolution: (u32, u32),
    /// 起動時に取り付ける先と相対姿勢。
    attach: Option<(Device, Pose)>,
    /// 起動時から掴めない。
    locked: bool,
    /// 親子関係（[`Self::child`]）。
    parent: Option<kernel::Parent>,
    animated: bool,
    /// レーザーを描くパネル。
    pointer: Option<(Hand, Pointer)>,
    content: Content,
}

impl PanelSpec {
    pub fn new<V: Render>(
        pose: Pose,
        build: impl FnOnce(PanelId, &mut Window, &mut Context<V>) -> V + 'static,
    ) -> Self {
        Self::with_content(
            pose,
            Content::Gpui(Box::new(move |panel, window, cx| {
                cx.new(|cx| build(panel, window, cx)).into()
            })),
        )
    }

    fn with_content(pose: Pose, content: Content) -> Self {
        Self {
            pose,
            from: None,
            in_view: false,
            size: Vec2::new(0.8, 0.6),
            resolution: PANEL_PX,
            attach: None,
            locked: false,
            parent: None,
            animated: true,
            pointer: None,
            content,
        }
    }

    /// Web のページ `page` のパネル（Tauri の WebView）。置き場所や大きさは GPUI のパネルと同じく指定する。
    pub fn web(pose: Pose, page: impl Into<String>) -> Self {
        Self::with_content(pose, Content::Web(page.into()))
    }

    /// `from` の隣に Web のページを開く。
    pub fn spawn_web(from: PanelId, page: impl Into<String>) -> Self {
        Self {
            from: Some(from),
            ..Self::web(Pose::IDENTITY, page)
        }
    }

    /// Web のパネルならページ名。
    pub fn web_page(&self) -> Option<&str> {
        match &self.content {
            Content::Web(page) => Some(page),
            Content::Gpui(_) => None,
        }
    }

    pub fn spawn<V: Render>(
        from: PanelId,
        build: impl FnOnce(PanelId, &mut Window, &mut Context<V>) -> V + 'static,
    ) -> Self {
        Self {
            from: Some(from),
            ..Self::new(Pose::IDENTITY, build)
        }
    }

    /// `from` の子として出す（ダイアログなど）。親に追従し、開いている間の親は暗くなって入力を受けない。
    /// 子を掴んだとき、`carry` なら親も連れていき、そうでなければ子だけが離れる。
    pub fn child<V: Render>(
        from: PanelId,
        carry: bool,
        build: impl FnOnce(PanelId, &mut Window, &mut Context<V>) -> V + 'static,
    ) -> Self {
        Self {
            parent: Some(kernel::Parent {
                panel: from,
                carry,
                dismiss: true,
            }),
            ..Self::spawn(from, build)
        }
    }

    /// 子の外側を押しても閉じないようにする（既定は閉じる）。親がなければ何もしない。
    pub fn without_dismiss(mut self) -> Self {
        if let Some(p) = &mut self.parent {
            p.dismiss = false;
        }
        self
    }

    /// 親子関係を作る要求（親がいなければ `None`）。開いた直後に [`PanelCommand::Parent`] で適用する。
    pub fn parent(&self) -> Option<kernel::Parent> {
        self.parent
    }

    /// 開いた瞬間の目の前に出す。`offset` は頭から見た相対姿勢（頭の傾きは無視し、パネルは立てて置く）。
    /// トラッキング空間の原点は人の居場所と一致しないので、召喚するパネルはこれで出す。
    pub fn in_view(mut self, offset: Pose) -> Self {
        (self.pose, self.from, self.in_view) = (offset, None, true);
        self
    }

    /// 大きさ（m）と解像度（px）を指定する。既定は 0.8m × 0.6m / 800 × 600。
    pub fn sized(mut self, size: Vec2, resolution: (u32, u32)) -> Self {
        (self.size, self.resolution) = (size, resolution);
        self
    }

    /// 起動時にデバイスへ取り付ける。
    pub fn attached(mut self, device: Device, offset: Pose) -> Self {
        self.attach = Some((device, offset));
        self
    }

    /// 起動時から掴めなくする。
    pub fn locked(mut self) -> Self {
        self.locked = true;
        self
    }

    /// このパネルのイン・アウトアニメーションを無効にする。
    pub fn without_animation(mut self) -> Self {
        self.animated = false;
        self
    }

    /// `hand` のレーザー（線またはカーソル）を描くパネルにする。置き場所はオーバーレイ層が決める。
    pub fn pointer(mut self, hand: Hand, part: Pointer) -> Self {
        self.pointer = Some((hand, part));
        self
    }

    pub fn pose_for<B: VrBackend>(&self, kernel: &Kernel<B>, _next: PanelId) -> Pose {
        match self.from {
            // 親の中心をアンカーに、親の手前へ少しだけ離して重ねる（子の子はさらに手前へ積まれる）。
            Some(from) => kernel.panel(from).pose * Pose::new(SPAWN_GAP, Quat::IDENTITY),
            None if self.in_view => kernel.hmd().leveled() * self.pose,
            None => self.pose,
        }
    }

    /// GPUI のビューを組み立てる。Web のパネル（[`Self::web_page`] が `Some`）には使えない。
    pub fn build(self, panel: PanelId, window: &mut Window, cx: &mut App) -> AnyView {
        match self.content {
            Content::Gpui(build) => build(panel, window, cx),
            Content::Web(page) => panic!("web page {page} cannot become a GPUI view"),
        }
    }
}

pub struct PanelRoot(AnyView);

impl PanelRoot {
    pub fn new(view: AnyView) -> Self {
        Self(view)
    }

    pub fn view(&self) -> AnyView {
        self.0.clone()
    }
}

impl Render for PanelRoot {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        // 全パネル共通の書体（子は継承する）。
        div()
            .size_full()
            .font(design::font::font())
            .child(self.0.clone())
    }
}

/// テストなどがフレームループに差し込む口。
#[derive(Default)]
pub struct Hooks {
    /// Web（Tauri）を起こした直後に呼ぶ（最初の Web のパネルを開くとき）。テストがイベントを購読するのに使う。
    pub on_web: Option<OnWeb>,
    /// 毎フレーム、メインスレッドで `App` を触る処理を受け取る（別スレッドのテストが GPUI を操作する）。
    pub remote: Option<std::sync::mpsc::Receiver<Remote>>,
    /// 計測はするがプロファイラパネルを出さない（ベンチマークが自分の描画で計測を汚さないように）。
    pub hide_profiler: bool,
}

/// Web（Tauri）を起こした直後に呼ぶ処理。
pub type OnWeb = Box<dyn FnOnce(&tauri::AppHandle)>;

/// フレームループ（メインスレッド）で実行してもらう処理。
pub type Remote = Box<dyn FnOnce(&mut App) + Send>;

/// Web のパネル。Tauri は重いので、最初の Web のパネルを開くときに起こす。
struct WebSlot {
    files: web::Files,
    host: Option<web::Host>,
    on_web: Option<OnWeb>,
    failed: bool,
}

impl WebSlot {
    fn new(files: web::Files, on_web: Option<OnWeb>) -> Self {
        Self {
            files,
            host: None,
            on_web,
            failed: false,
        }
    }

    fn host(&mut self) -> Option<&mut web::Host> {
        if self.host.is_none() && !self.failed {
            match web::Host::new(self.files) {
                Ok(h) => {
                    if let Some(f) = self.on_web.take() {
                        f(h.handle());
                    }
                    self.host = Some(h);
                }
                Err(e) => {
                    logger::error!("cannot start web (tauri)", error = e);
                    self.failed = true;
                }
            }
        }
        self.host.as_mut()
    }

    /// 終わる前に WebView を閉じる（[`web::Host::shutdown`]）。
    fn shutdown(&mut self) {
        if let Some(h) = self.host.as_mut() {
            h.shutdown();
        }
    }

    fn is_web(&self, panel: PanelId) -> bool {
        self.host.as_ref().is_some_and(|h| h.contains(panel))
    }
}

/// メインスレッドを占有する。GPUI のイベントループがランタイムのフレームループも回す。
/// どのランタイム（OpenVR / OpenXR / novr）でも同じ経路を通る（周期は `poll` が vsync まで待って作る）。
/// パネルに載せる画面（上の層）は `panels` で受け取る。UI 層はどの画面かを知らない。
/// Web のパネル（[`PanelSpec::web`]）のページは `web` から配信する。
/// `profiler` があれば起動時から計測し（`PanelControl::profiling`）、段階ごとの所要時間をプロファイラパネルにグラフで出す。
pub fn run(kernel: Kernel, panels: Vec<PanelSpec>, profiler: Option<Profiler>, web: web::Files) {
    run_with(kernel, panels, profiler, web, Hooks::default())
}

/// [`run`] にテスト用の差し込み口（[`Hooks`]）を足したもの。
pub fn run_with(
    mut kernel: Kernel,
    panels: Vec<PanelSpec>,
    profiler: Option<Profiler>,
    web: web::Files,
    hooks: Hooks,
) {
    let mut webs = WebSlot::new(web, hooks.on_web);
    let remote = hooks.remote;
    let hide_profiler = hooks.hide_profiler;
    // novr はフレームの周期が決まっているので、ドロップをその長さで数える（間隔からの推定は揺れに弱い）。
    let budget = kernel.frame_interval();
    let with_budget = move |p: Profiler| match budget {
        Some(b) => p.with_budget(b),
        None => p,
    };
    gpui_platform::application().run(move |cx: &mut App| {
        // 部品のトランジションを有効にする（テストのキットは置かない）。
        cx.set_global(design::Motion::default());
        design::font::install(cx);
        if let Some(p) = profiler {
            cx.set_global(with_budget(p));
            cx.default_global::<PanelControl>().profiling = true;
        }
        let mut profiler_panel: Option<PanelId> = None;
        // 添字は PanelId。Web のパネルは None（ウィンドウは web::Host が持つ）。
        let mut windows: Vec<(PanelId, Option<AnyWindowHandle>)> = panels
            .into_iter()
            .enumerate()
            .filter_map(|(i, spec)| open_panel(&mut kernel, spec, i, cx, &mut webs))
            .collect();

        cx.spawn(async move |cx| {
            let mut bridges = HashMap::new();
            let mut warned = false;
            let mut last = vec![None; windows.len()];
            // パネルごとの最後に描いた時刻（None = 次のフレームで必ず描く）。
            let mut drawn: Vec<Option<Instant>> = vec![None; windows.len()];
            // パネルごとの、入力も変化も無いときに描き直す間隔。描いても変わらなければ IDLE_MAX まで延ばす。
            let mut idle: Vec<Duration> = vec![IDLE_REDRAW; windows.len()];
            // パネルごとの、前のフレームで見えていたか（隠れていた・視野の外だったパネルは見えた瞬間に描き直す）。
            let mut seen: Vec<bool> = vec![false; windows.len()];
            let mut render_sizes = HashMap::new();
            let limit = kernel
                .max_overlays()
                .map_or(MAX_PANELS, |m| m.min(MAX_PANELS));
            // ドラッグ中の手と、その見た目を描くパネル。
            let mut drag: Option<(Hand, PanelId)> = None;
            // VR では poll がブロックして周期を作る。待ちの前に一度 GPUI へ制御を返す。
            // 0 だとこのループがすぐ再スケジュールされ、後から積まれた GPUI のタスクが走らない。
            // 途中で開いたパネルのリサイズ（SetWindowPos はタスクで行われる）が届かず、1x1 のまま表示されなかった。
            // poll は次の vsync までの残りを眠るので、ここで 1ms 譲っても周期は変わらない。
            let wait = Duration::from_millis(1);
            loop {
                let begin = Instant::now();
                // GPUI のタイマーは Windows では粗く（1ms を頼んでも 6〜12ms 掛かり、フレームが 60fps 台に頭打ちになる）、
                // バックグラウンドのスレッドで眠ってから戻る（std の sleep は細かい）。
                cx.background_executor()
                    .spawn(async move { std::thread::sleep(wait) })
                    .await;
                if let Some(remote) = &remote {
                    while let Ok(f) = remote.try_recv() {
                        cx.update(f);
                    }
                }
                let mut inputs: Vec<Vec<PlatformInput>> =
                    (0..windows.len()).map(|_| Vec::new()).collect();
                let (gpui_inputs, web_events) =
                    pump_with(&mut kernel, &mut bridges, |p| webs.is_web(p));
                for (panel, input) in gpui_inputs {
                    inputs[panel].push(input);
                }
                // タッチの長押しの進み具合を手ごとに置き、変わればカーソルを描き直す（リングが満ちていく）。
                let mut hold: HashMap<Hand, f32> = HashMap::new();
                for (&(_, hand), b) in &bridges {
                    if let Some(p) = b.hold_progress() {
                        let e = hold.entry(hand).or_default();
                        *e = e.max(p);
                    }
                }
                let hold_changed = cx.update(|cx| {
                    let ctl = cx.default_global::<PanelControl>();
                    let changed = ctl.hold != hold;
                    ctl.hold = hold;
                    changed
                });
                // 前のフレームの配送でホバーした要素のツールチップを手に振り分ける。変わればツールチップのパネルを描き直す。
                let tooltip_changed = cx.update(|cx| {
                    let ids = windows
                        .iter()
                        .filter_map(|&(p, w)| w.map(|w| (p, w.window_id())));
                    sync_tooltips(&kernel, &bridges, ids, cx)
                });
                for &(p, w) in &windows {
                    let part = kernel.panel(p).pointer.map(|(_, part)| part);
                    if hold_changed && part == Some(Pointer::Cursor) {
                        drawn[p] = None;
                    }
                    if tooltip_changed
                        && part == Some(Pointer::Tooltip)
                        && let Some(w) = w
                    {
                        let _ = cx.update_window(w, |_, w, _| w.refresh());
                        drawn[p] = None;
                    }
                }
                // ドラッグの見た目は中心がカーソルに来るよう置かれるので、そのウィンドウのマウスを中心に置いて毎フレーム描く。
                if let Some((_, dp)) = drag {
                    let (w, h) = kernel.panel(dp).resolution;
                    let pos = Vec2::new(w as f32, h as f32) / 2.0;
                    inputs[dp].extend(
                        PointerBridge::default()
                            .translate(PointerEvent::Move { pos }, PointerMode::Mouse),
                    );
                }
                // wait = ランタイムの待ち（VR は vsync、novr はエミュレーターの周期）。オーバーレイ層と合成は別に測る。
                let timing = kernel.timing();
                let mut stages = Stages {
                    wait: begin
                        .elapsed()
                        .saturating_sub(timing.overlay + timing.compositor),
                    overlay: timing.overlay,
                    compositor: timing.compositor,
                    ..Default::default()
                };
                if kernel.quit_requested() {
                    webs.shutdown();
                    cx.update(quit);
                    return;
                }
                // WindowHandle::update はルートビューを借用したままになり、draw やクリック処理で
                // ビューを更新すると二重借用で落ちる。AnyWindowHandle 経由で借用しない。
                // 何もしていないパネルの描き直しは 1 フレームに 1 枚まで（同じフレームに重なると読み戻しの待ちが積み上がる）。
                let mut idle_slot = true;
                for ((panel, window), inputs) in windows.iter().copied().zip(inputs) {
                    let Some(window) = window else { continue }; // Web のパネルは後で web::Host が回す
                    if kernel.panel(panel).closed {
                        continue;
                    }
                    // 描画と読み戻し（GPU の完了待ちを含む）は重いので、入力・状態の変化があったとき、
                    // 前回描いてから idle[panel] 経ったとき（1 フレームに 1 枚）だけ行う。
                    // 見えていないパネル（ほかのパネルに隠れた・視野の外）は、何もしていないときに描き直さない（カリング）。
                    let visible = kernel.panel_seen(panel);
                    if visible && !seen[panel] {
                        drawn[panel] = None;
                    }
                    seen[panel] = visible;
                    let forced = !inputs.is_empty() || drawn[panel].is_none();
                    // 遠い（LOD1 以上の）パネルはフレームを間引いて描く。
                    let due = kernel.panel_frame_due(panel);
                    let idle_due = !forced
                        && visible
                        && due
                        && idle_slot
                        && drawn[panel].is_some_and(|t| t.elapsed() >= idle[panel]);
                    idle_slot &= !idle_due;
                    let mut redraw = forced || idle_due;
                    let mut changed = false;
                    let Ok(frame) = cx.update_window(window, |_, window, cx| {
                        let t = Instant::now();
                        for input in inputs {
                            window.dispatch_event(input, cx);
                        }
                        changed = sync_panel(&mut kernel, panel, cx);
                        redraw |= changed;
                        // 部品のトランジションが動いている間は、毎フレーム描き直す。
                        let transitioning = cx.has_global::<design::Motion>()
                            && cx
                                .global_mut::<design::Motion>()
                                .animating
                                .remove(&window.window_handle().window_id());
                        if visible
                            && (transitioning
                                || cx
                                    .try_global::<PanelControl>()
                                    .and_then(|c| c.panels.get(&panel))
                                    .is_some_and(|s| s.live))
                        {
                            // ビューが描くたびに変わる。GPUI が描き直すよう汚す。
                            window.refresh();
                            redraw = true;
                        }
                        let resolution = kernel.panel(panel).resolution;
                        let lod = kernel.panel_lod(panel);
                        if render_sizes.insert(panel, (resolution, lod)) != Some((resolution, lod))
                        {
                            set_render_lod(window, cx, resolution, lod);
                            drawn[panel] = None;
                            redraw = false;
                        }
                        if !cx.has_active_drag() {
                            if let Some((hand, _)) = drag.take() {
                                kernel.set_drag(hand, None);
                            }
                        } else if drag.is_none() {
                            drag = start_drag(&mut kernel, &bridges, &windows, panel);
                        }
                        stages.input += t.elapsed();
                        if kernel.panel(panel).closed {
                            window.remove_window();
                            return None;
                        }
                        if redraw && !due {
                            // 間引いたフレーム。次に描いてよいフレームで必ず描く。
                            drawn[panel] = None;
                            redraw = false;
                        }
                        redraw.then(|| {
                            let t = Instant::now();
                            window.draw(cx).clear(cx);
                            let scene = Instant::now();
                            let frame = read_back(window);
                            stages.draw += scene - t;
                            stages.render += scene.elapsed();
                            frame
                        })
                    }) else {
                        // ウィンドウが OS の X などで閉じられた。そのパネルだけ閉じる（全部閉じたら下で終了する）。
                        kernel.apply(panel, PanelCommand::Close);
                        continue;
                    };
                    if kernel.panel(panel).closed {
                        // 閉じたパネルの最後の描画（1 枚 2MB 近い）は手放す。
                        last[panel] = None;
                    }
                    if changed {
                        // 状態はどのパネルにも表示されうるので、ほかのパネルも描き直す（1 フレームに 1 枚ずつ）。
                        for (p, t) in drawn.iter_mut().enumerate() {
                            if p != panel
                                && let Some(t) = t
                            {
                                *t = t.checked_sub(IDLE_MAX).unwrap_or(*t);
                                idle[p] = Duration::ZERO;
                            }
                        }
                    }
                    match frame {
                        // 開いた直後はウィンドウの大きさの反映が間に合わず 1x1 で描かれる。送らずに次のフレームで描き直す。
                        Some(Ok(f))
                            if render_sizes.get(&panel).is_some_and(|&(res, lod)| {
                                (f.width, f.height)
                                    != ((res.0 >> lod).max(1), (res.1 >> lod).max(1))
                            }) =>
                        {
                            drawn[panel] = None;
                        }
                        // 画素が変わったときだけ送る（GPU 書き込みと完了待ちを省く）。
                        Some(Ok(f)) => {
                            drawn[panel] = Some(Instant::now());
                            let same = last[panel].as_ref() == Some(&f);
                            idle[panel] = if idle_due && same && !changed {
                                (idle[panel] * 2).min(IDLE_MAX)
                            } else {
                                IDLE_REDRAW
                            };
                            if !same {
                                let t = Instant::now();
                                kernel.submit(panel, f.width, f.height, &f.rgba);
                                stages.upload += t.elapsed();
                                last[panel] = Some(f);
                            }
                        }
                        Some(Err(e)) if !warned => {
                            logger::error!("cannot get panel render", error = e, panel = panel);
                            warned = true;
                        }
                        _ => {}
                    }
                }
                // Web のパネル: Tauri を 1 回回し、ポインタを送り、ページの操作を反映して撮り直す。
                let mut web_spawns = Vec::new();
                if let Some(host) = webs.host.as_mut() {
                    let t = Instant::now();
                    for e in web_events {
                        host.deliver(e, kernel.pointer_mode(), t);
                    }
                    // GPUI の画面が Web のパネルへ積んだ操作（閉じる等）。
                    for &(panel, window) in &windows {
                        if window.is_some() || kernel.panel(panel).closed {
                            continue;
                        }
                        let commands = cx.update(|cx| {
                            cx.default_global::<PanelControl>()
                                .panels
                                .get_mut(&panel)
                                .map(|s| std::mem::take(&mut s.commands))
                                .unwrap_or_default()
                        });
                        for c in commands {
                            host.apply(&mut kernel, panel, c);
                        }
                    }
                    web_spawns = host.step(&mut kernel, t);
                    stages.web += t.elapsed();
                }
                // 終了を頼まれたか、ウィンドウが全部（OS の X などで）閉じられたら終了する。
                if cx.update(|cx| cx.default_global::<PanelControl>().quit)
                    || !windows.is_empty() && (0..windows.len()).all(|p| kernel.panel(p).closed)
                {
                    webs.shutdown();
                    cx.update(quit);
                    return;
                }
                let mut spawns =
                    cx.update(|cx| std::mem::take(&mut cx.default_global::<PanelControl>().spawns));
                spawns.extend(
                    web_spawns
                        .into_iter()
                        .map(|(from, page)| PanelSpec::spawn_web(from, page)),
                );
                // 閉じたパネルの手ごとの状態（慣性のスクロールなど）は持ち越さない。
                bridges.retain(|&(p, _), _| !kernel.panel(p).closed);
                render_sizes.retain(|&p, _| !kernel.panel(p).closed);
                let open =
                    |kernel: &Kernel, n: usize| (0..n).filter(|&p| !kernel.panel(p).closed).count();
                for spec in spawns {
                    // ページや連打で開きっぱなしのパネルが際限なく増えないように。
                    if open(&kernel, windows.len()) >= limit {
                        logger::error!("too many open panels", max = limit);
                        break;
                    }
                    let i = windows.len();
                    let Some(opened) =
                        cx.update(|cx| open_panel(&mut kernel, spec, i, cx, &mut webs))
                    else {
                        break;
                    };
                    windows.push(opened);
                    last.push(None);
                    drawn.push(None);
                    idle.push(IDLE_REDRAW);
                    seen.push(false);
                }
                let open = open(&kernel, windows.len());
                cx.update(|cx| {
                    let c = cx.default_global::<PanelControl>();
                    (c.open, c.limit) = (open, limit);
                });
                // 計測とプロファイラパネルを `profiling` に合わせる。
                if let Some(p) = profiler_panel
                    && kernel.panel(p).closed
                {
                    // OS の X などで閉じられた。
                    profiler_panel = None;
                    cx.update(|cx| cx.default_global::<PanelControl>().profiling = false);
                }
                let profiling = cx.update(|cx| cx.default_global::<PanelControl>().profiling);
                if profiling != cx.update(|cx| cx.has_global::<Profiler>()) {
                    cx.update(|cx| {
                        logger::info!("profiler toggled", profiling = profiling);
                        if profiling {
                            // --profile 無しで始めたときは CSV に書かない。
                            let p =
                                Profiler::new(None).expect("cannot fail without an output path");
                            cx.set_global(with_budget(p));
                        } else {
                            log_summary(cx);
                            cx.remove_global::<Profiler>();
                        }
                        // 切り替えボタンの表示を変えるため、どのパネルも描き直させる。
                        cx.refresh_windows();
                    });
                    drawn.iter_mut().for_each(|t| *t = None);
                }
                match profiler_panel {
                    None if profiling && !hide_profiler => {
                        let spec = PanelSpec::new(Pose::IDENTITY, |panel, _, cx| ProfilerPanel {
                            panel,
                            view: cx.new(|_| ProfilerView),
                        })
                        .sized(Vec2::new(1.1, 0.56), (1100, 560))
                        .in_view(PROFILER_POSE);
                        let i = windows.len();
                        match cx.update(|cx| open_panel(&mut kernel, spec, i, cx, &mut webs)) {
                            Some((panel, window)) => {
                                logger::info!(
                                    "profiler panel opened",
                                    panel = panel,
                                    pose = ?kernel.panel(panel).pose
                                );
                                windows.push((panel, window));
                                last.push(None);
                                drawn.push(None);
                                idle.push(IDLE_REDRAW);
                                seen.push(false);
                                profiler_panel = Some(panel);
                            }
                            // 開けなかった。毎フレーム作り直そうとしないよう計測ごと止める。
                            None => cx
                                .update(|cx| cx.default_global::<PanelControl>().profiling = false),
                        }
                    }
                    Some(p) if !profiling => {
                        // 次のフレームの sync_panel が閉じてウィンドウを消す。
                        cx.update(|cx| {
                            cx.default_global::<PanelControl>()
                                .panels
                                .entry(p)
                                .or_default()
                                .commands
                                .push(PanelCommand::Close)
                        });
                        profiler_panel = None;
                    }
                    _ => {}
                }
                cx.update(|cx| {
                    if cx.has_global::<Profiler>() {
                        cx.global_mut::<Profiler>().record(Instant::now(), stages);
                    }
                    if let Some(p) = profiler_panel
                        && let Some((_, Some(w))) = windows.iter().find(|(id, _)| *id == p)
                        && drawn[p].is_none_or(|t| t.elapsed() >= PROFILER_REDRAW)
                    {
                        // グラフは毎フレーム変わるが、読むには PROFILER_REDRAW ごとで足りる。毎フレーム描くと、描画・読み戻し・
                        // 合成のコストで計測そのものを重くする（プロファイラの窓だけで 1 フレーム 20ms 近かった）。
                        let _ = cx.update_window(*w, |_, w, _| w.refresh());
                        drawn[p] = None;
                    }
                });
            }
        })
        .detach();
    });
}

/// 終了する。プロファイル中なら集計をログへ残す。
fn quit(cx: &mut App) {
    log_summary(cx);
    cx.quit();
}

/// プロファイル中なら集計をログへ残す。
fn log_summary(cx: &App) {
    if let Some(p) = cx.try_global::<Profiler>() {
        let s = p.summary();
        let ms = |d: Duration| format!("{:.3}", d.as_secs_f64() * 1e3);
        logger::info!(
            "profile summary (recent)",
            frames = s.frames,
            dropped = s.dropped,
            fps = format!("{:.1}", s.fps),
            frame_avg_ms = ms(s.frame_avg),
            frame_p99_ms = ms(s.frame_p99),
            cpu_avg_ms = ms(s.cpu_avg),
            gpu_avg_ms = ms(s.gpu_avg)
        );
    }
}

/// プロファイラのパネル。グラフの上に閉じるボタン（計測も止める）を出す。
/// VR ではパネルに OS の閉じるボタンが無いので要る。
struct ProfilerPanel {
    panel: PanelId,
    view: Entity<ProfilerView>,
}

impl Render for ProfilerPanel {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div()
            .size_full()
            .flex()
            .flex_col()
            .bg(rgb(0x17181c))
            .child(
                div().flex().justify_end().p_1().child(
                    div()
                        .id(("close-profiler", self.panel))
                        .debug_selector(|| "close-profiler".into())
                        .px_3()
                        .py_1()
                        .bg(rgb(0x3a3d46))
                        .hover(|s| s.bg(rgb(0x50545f)))
                        .text_color(rgb(0xd8dae0))
                        .child("Close")
                        .on_click(|_, _, cx| {
                            cx.default_global::<PanelControl>().profiling = false;
                        }),
                ),
            )
            .child(div().flex_1().child(self.view.clone()))
    }
}

/// カーネルにパネルを作り、中身（GPUI のウィンドウか Web のページ）を開く。Web のパネルのウィンドウは `None`。
/// ランタイムがオーバーレイを作れない（数の上限など）ときはエラーを出して `None`。
fn open_panel(
    kernel: &mut Kernel,
    mut spec: PanelSpec,
    index: usize,
    cx: &mut App,
    webs: &mut WebSlot,
) -> Option<(PanelId, Option<AnyWindowHandle>)> {
    let pose = spec.pose_for(kernel, index);
    let panel = match kernel.add_panel(
        &format!("nyaw.panel.{index}"),
        pose,
        spec.size,
        spec.resolution,
    ) {
        Ok(p) => p,
        Err(e) => {
            logger::error!("cannot open panel", error = ?e);
            return None;
        }
    };
    if !spec.animated {
        kernel.apply(panel, PanelCommand::Animated(false));
    }
    if let Some((device, offset)) = spec.attach.take() {
        kernel.apply(panel, PanelCommand::Attach(device, offset));
    }
    if spec.locked {
        kernel.apply(panel, PanelCommand::Lock(true));
    }
    if let Some(parent) = spec.parent {
        kernel.apply(panel, PanelCommand::Parent(Some(parent)));
    }
    if let Some((hand, part)) = spec.pointer {
        kernel.apply(panel, PanelCommand::Pointer(hand, part));
    }
    if let Content::Web(page) = &spec.content {
        match webs.host() {
            Some(host) => host.open(kernel, panel, page),
            None => kernel.apply(panel, PanelCommand::Close),
        }
        return Some((panel, None));
    }
    let drag_layer = spec.pointer.is_some_and(|(_, part)| part == Pointer::Drag);
    let window: AnyWindowHandle = open_panel_window(cx, spec.resolution, |w, cx| {
        let view = spec.build(panel, w, cx);
        cx.new(|_| PanelRoot::new(view))
    })
    .expect("open panel window")
    .into();
    if drag_layer {
        cx.default_global::<DragWindows>().0.push(window);
    }
    Some((panel, Some(window)))
}

#[cfg(test)]
mod tests {
    use super::*;
    use design::Theme;
    use gpui::{TestAppContext, div};
    use kernel::{Device, Runtime};

    /// UI 層だけで確かめるための最小の画面。
    #[derive(Default)]
    struct Probe {
        clicks: usize,
    }

    impl Render for Probe {
        fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
            div().size_full().child(
                div()
                    .id("probe")
                    .debug_selector(|| "probe".into())
                    .size_10()
                    .on_click(cx.listener(|this, _, _, _| this.clicks += 1)),
            )
        }
    }

    #[gpui::test]
    fn laser_click_reaches_gpui_view(cx: &mut TestAppContext) {
        let (testing, cx) = cx.add_window_view(|_, _| Probe::default());
        cx.run_until_parked();
        let c = cx.debug_bounds("probe").expect("button rendered").center();
        let pos = Vec2::new(c.x.into(), c.y.into());

        let mut bridge = PointerBridge::default();
        for ev in [
            PointerEvent::Move { pos },
            PointerEvent::Down { pos },
            PointerEvent::Up { pos },
            PointerEvent::Leave,
        ] {
            for input in bridge.translate(ev, PointerMode::Mouse) {
                cx.update(|window, cx| {
                    window.dispatch_event(input, cx);
                });
            }
        }
        cx.run_until_parked();
        assert_eq!(testing.read_with(cx, |h, _| h.clicks), 1);
    }

    #[gpui::test]
    fn panel_window_is_hidden_and_matches_panel_resolution(cx: &mut TestAppContext) {
        let options = panel_window_options(PANEL_PX);
        assert!(!options.show && !options.focus);
        assert!(matches!(
            options.window_background,
            gpui::WindowBackgroundAppearance::Transparent
        ));
        let window = cx.update(|cx| {
            open_panel_window(cx, PANEL_PX, |_, cx| cx.new(|_| Probe::default())).unwrap()
        });
        let vp = cx
            .update(|cx| cx.update_window(window.into(), |_, w, _| w.viewport_size()))
            .unwrap();
        assert_eq!(vp, gpui::size(px(PANEL_PX.0 as f32), px(PANEL_PX.1 as f32)));
    }

    #[gpui::test]
    fn spec_attaches_locks_and_sizes_panel(cx: &mut TestAppContext) {
        let mut kernel = Kernel::boot(Some("novr"), Default::default()).unwrap();
        let hand = Device::Controller(Hand::Left);
        let spec = PanelSpec::new(Pose::IDENTITY, |_, _, _| Probe::default())
            .sized(Vec2::new(0.1, 0.05), (10, 5))
            .attached(hand.clone(), Pose::IDENTITY)
            .locked()
            .without_animation();
        let mut webs = WebSlot::new(&[], None);
        let (panel, _) = cx
            .update(|cx| open_panel(&mut kernel, spec, 0, cx, &mut webs))
            .unwrap();
        let p = kernel.panel(panel);
        assert_eq!(p.attached, Some((hand, Pose::IDENTITY)));
        assert!(p.locked);
        assert!(!p.animated);
        assert_eq!((p.size, p.resolution), (Vec2::new(0.1, 0.05), (10, 5)));
    }

    #[gpui::test]
    fn sync_panel_applies_commands_and_reports_status(cx: &mut TestAppContext) {
        let mut kernel = Kernel::boot(Some("novr"), Default::default()).unwrap();
        let panel = kernel
            .add_panel("t", Pose::IDENTITY, Vec2::ONE, PANEL_PX)
            .unwrap();
        cx.update(|cx| {
            cx.default_global::<PanelControl>()
                .panels
                .entry(panel)
                .or_default()
                .commands
                .extend([
                    PanelCommand::Attach(Device::Hmd, Pose::IDENTITY),
                    PanelCommand::Lock(true),
                ]);
            sync_panel(&mut kernel, panel, cx);
            let ctl = &cx.global::<PanelControl>().panels[&panel];
            assert!(ctl.commands.is_empty());
            assert_eq!(
                ctl.status,
                PanelStatus {
                    runtime: Some(Runtime::NoVr),
                    attached: Some(Device::Hmd),
                    locked: true,
                    animated: true,
                    ..Default::default()
                }
            );
            assert_eq!(Theme::current(cx), Theme::DARK);
            cx.default_global::<PanelControl>()
                .panels
                .entry(panel)
                .or_default()
                .commands
                .push(PanelCommand::Theme(ThemeMode::Light));
            sync_panel(&mut kernel, panel, cx);
            assert_eq!(Theme::current(cx), Theme::LIGHT, "every screen follows");
            cx.default_global::<PanelControl>()
                .panels
                .get_mut(&panel)
                .unwrap()
                .commands
                .extend([PanelCommand::Detach, PanelCommand::Animated(false)]);
            sync_panel(&mut kernel, panel, cx);
        });
        assert_eq!(kernel.panel(panel).attached, None);
        assert!(!kernel.panel(panel).animated);
    }

    /// Web のページのパネルも GPUI のパネルと同じく置く（隣に開く・目の前に出す）。ビューにはならない。
    #[gpui::test]
    fn web_spec_opens_without_gpui_window_when_web_is_unavailable(cx: &mut TestAppContext) {
        let mut kernel = Kernel::boot(Some("novr"), Default::default()).unwrap();
        let first = PanelSpec::new(Pose::IDENTITY, |_, _, _| Probe::default());
        let mut webs = WebSlot::new(&[], None);
        // テストプラットフォームでは Tauri を起こさない（起こせなかった扱い）。パネルは作られ、すぐ閉じる。
        webs.failed = true;
        cx.update(|cx| open_panel(&mut kernel, first, 0, cx, &mut webs))
            .unwrap();
        let spec = PanelSpec::spawn_web(0, "demo");
        assert_eq!(spec.web_page(), Some("demo"));
        assert_eq!(spec.pose_for(&kernel, 1).position, SPAWN_GAP);
        let (panel, window) = cx
            .update(|cx| open_panel(&mut kernel, spec, 1, cx, &mut webs))
            .unwrap();
        assert_eq!((panel, window.is_none()), (1, true));
        assert!(kernel.panel(panel).closed, "closes if web cannot start");
        assert!(
            PanelSpec::new(Pose::IDENTITY, |_, _, _| Probe::default())
                .web_page()
                .is_none()
        );
    }

    /// 目の前に出すパネルは頭の向く水平方向に置き、うつむいていても立てて置く。
    #[test]
    fn in_view_places_level_in_front_of_head() {
        // +X を向き、30° うつむいた頭。
        let head = Pose::new(
            Vec3::new(1.0, 1.6, 0.0),
            Quat::from_rotation_y(-std::f32::consts::FRAC_PI_2) * Quat::from_rotation_x(-0.5),
        );
        let p = head.leveled() * Pose::new(Vec3::new(0.0, 0.0, -1.0), Quat::IDENTITY);
        assert!(p.position.abs_diff_eq(Vec3::new(2.0, 1.6, 0.0), 1e-5));
        assert!((p.orientation * Vec3::Y).abs_diff_eq(Vec3::Y, 1e-5));
    }

    #[test]
    fn move_to_the_same_place_is_sent_once_until_leave() {
        let mut b = PointerBridge::default();
        let pos = Vec2::new(3.0, 4.0);
        let mut send = |ev| b.translate(ev, PointerMode::Mouse).len();
        assert_eq!(send(PointerEvent::Move { pos }), 1);
        assert_eq!(send(PointerEvent::Move { pos }), 0, "not sent while still");
        assert_eq!(send(PointerEvent::Down { pos }), 1);
        assert_eq!(send(PointerEvent::Move { pos }), 0);
        assert_eq!(send(PointerEvent::Leave), 1);
        assert_eq!(send(PointerEvent::Move { pos }), 1, "sent on re-entry");
    }

    #[test]
    fn leave_releases_held_button() {
        let mut b = PointerBridge::default();
        b.translate(
            PointerEvent::Down {
                pos: Vec2::new(3.0, 4.0),
            },
            PointerMode::Mouse,
        );
        let [PlatformInput::MouseExited(e)] =
            &b.translate(PointerEvent::Leave, PointerMode::Mouse)[..]
        else {
            panic!()
        };
        assert_eq!(
            (e.position, e.pressed_button),
            (Point::new(px(3.0), px(4.0)), Some(MouseButton::Left))
        );
        assert!(!b.pressed);
    }

    /// タッチ: 動かさずに離すとタップ（押して離す）、しきい値を超えて動かすとスワイプ（指に付いてくるスクロール）だけになる。
    #[test]
    fn touch_taps_or_swipes() {
        let pos = |x, y| Vec2::new(x, y);
        let kinds = |v: Vec<PlatformInput>| {
            v.iter()
                .map(|i| match i {
                    PlatformInput::MouseMove(_) => "move",
                    PlatformInput::MouseDown(_) => "down",
                    PlatformInput::MouseUp(_) => "up",
                    PlatformInput::ScrollWheel(_) => "scroll",
                    _ => "other",
                })
                .collect::<Vec<_>>()
        };
        let touch = PointerMode::Touch;
        let mut b = PointerBridge::default();
        assert!(
            b.translate(
                PointerEvent::Down {
                    pos: pos(10.0, 10.0)
                },
                touch
            )
            .is_empty()
        );
        assert!(
            b.translate(
                PointerEvent::Move {
                    pos: pos(12.0, 12.0)
                },
                touch
            )
            .is_empty()
        );
        let tap = b.translate(
            PointerEvent::Up {
                pos: pos(12.0, 12.0),
            },
            touch,
        );
        assert_eq!(kinds(tap), ["move", "down", "up"]);

        b.translate(
            PointerEvent::Down {
                pos: pos(10.0, 10.0),
            },
            touch,
        );
        // 押している間に方式が変わっても、離すまではタッチのまま。
        let swipe = b.translate(
            PointerEvent::Move {
                pos: pos(10.0, 40.0),
            },
            PointerMode::Mouse,
        );
        let [PlatformInput::ScrollWheel(e)] = &swipe[..] else {
            panic!("{swipe:?}")
        };
        assert_eq!(e.delta.pixel_delta(px(1.0)), Point::new(px(0.0), px(30.0)));
        assert_eq!(e.touch_phase, TouchPhase::Started);
        assert_eq!(
            e.position,
            Point::new(px(10.0), px(10.0)),
            "target is where it was pressed"
        );
        assert_eq!(
            kinds(b.translate(
                PointerEvent::Up {
                    pos: pos(10.0, 40.0)
                },
                touch
            )),
            ["scroll", "move"]
        );
        assert!(!b.pressed);
        // 離した後も慣性で同じ向きに滑り、だんだん止まる。
        let Some(PlatformInput::ScrollWheel(first)) = b.momentum() else {
            panic!("no momentum")
        };
        let dy = |e: &ScrollWheelEvent| f32::from(e.delta.pixel_delta(px(1.0)).y);
        assert!(dy(&first) > 0.0);
        let mut last = dy(&first);
        let mut frames = 0;
        while let Some(PlatformInput::ScrollWheel(e)) = b.momentum() {
            assert!(dy(&e) < last);
            last = dy(&e);
            frames += 1;
        }
        assert!(frames > 10, "does not stop immediately ({frames})");
        assert!(b.momentum().is_none());
    }

    #[test]
    fn touch_long_press_becomes_mouse_press_for_drag() {
        let touch = PointerMode::Touch;
        let mut b = PointerBridge::default();
        let at = Vec2::new(10.0, 10.0);
        assert!(
            b.translate(PointerEvent::Down { pos: at }, touch)
                .is_empty()
        );
        assert!(
            b.translate(PointerEvent::Move { pos: at }, touch)
                .is_empty()
        );
        std::thread::sleep(LONG_PRESS);
        // 動かさずに長押しすると、押した所でマウスの押下になり、押したまま閾値を越えて動かして（ドラッグがすぐ始まる）元へ戻す。
        let held = b.translate(PointerEvent::Move { pos: at }, touch);
        let [
            PlatformInput::MouseMove(_),
            PlatformInput::MouseDown(_),
            PlatformInput::MouseMove(nudge),
            PlatformInput::MouseMove(back),
        ] = &held[..]
        else {
            panic!("{held:?}")
        };
        assert_eq!(nudge.pressed_button, Some(MouseButton::Left));
        assert!(
            f32::from(nudge.position.x) - 10.0 > 2.0,
            "exceeds the GPUI drag threshold"
        );
        assert_eq!(back.position, Point::new(px(10.0), px(10.0)));
        // 以降はマウスのドラッグ（押したままの Move）で、スクロールにはならない。
        let drag = b.translate(
            PointerEvent::Move {
                pos: Vec2::new(10.0, 60.0),
            },
            touch,
        );
        let [PlatformInput::MouseMove(m)] = &drag[..] else {
            panic!("{drag:?}")
        };
        assert_eq!(m.pressed_button, Some(MouseButton::Left));
        let up = b.translate(
            PointerEvent::Up {
                pos: Vec2::new(10.0, 60.0),
            },
            touch,
        );
        assert!(matches!(up[..], [PlatformInput::MouseUp(_)]), "{up:?}");
        assert!(b.momentum().is_none(), "drag has no momentum");
    }
}
