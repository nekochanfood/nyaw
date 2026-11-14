//! レーザーのイベントを GPUI のマウス入力に変換する（[`PointerBridge`]。タッチの方式ではタップ・スワイプ・長押し）。

use super::*;

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
    pub(crate) pressed: bool,
    button: Option<MouseButton>,
    pub(crate) touch: Option<Touch>,
    /// スワイプを離した後の慣性（宛先の位置と、1 フレームあたりの移動量）。
    momentum: Option<(Point<Pixels>, Point<Pixels>)>,
    /// この手が最後に送った位置（パネルを離れたら `None`）。同じ位置への Move は送らない
    /// （オーバーレイは指している間 毎 tick Move を出すので、止まっていても描き直しと読み戻しが毎フレーム走ってしまう）。
    sent: Option<Point<Pixels>>,
    pub(crate) rehover: bool,
    /// スワイプとみなす動き（論理 px）。[`pump`] がパネルの距離と密度から毎フレーム決める（[`SWIPE_SLOP`] を下回らない）。
    pub slop: f32,
}

/// タッチで押している間の状態。
#[derive(Debug)]
pub(crate) struct Touch {
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

/// これより動いたらタップではなくスワイプ（論理 px）。指す距離・パネルの密度で決める閾値（[`SWIPE_SLOP_DEG`]）の下限。
pub const SWIPE_SLOP: f32 = 12.0;

/// これより（頭から見た角度で）動いたらタップではなくスワイプ。遠い・画素の細かいパネル（キーボード）でも、
/// 手ブレでタップがスワイプにならないよう、角度で決めて毎フレーム px に直す（[`PointerBridge::slop`]）。
pub const SWIPE_SLOP_DEG: f32 = 1.0;

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
                    t.swiping |= f32::from(d.x).hypot(f32::from(d.y)) > self.slop.max(SWIPE_SLOP);
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
                PointerEvent::Scroll { .. }
                | PointerEvent::Down { .. }
                | PointerEvent::DownButton { .. }
                | PointerEvent::UpButton { .. } => {}
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

    /// この手のレーザーがパネルの上にある。
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
        let held = |pressed: bool| pressed.then_some(self.button.unwrap_or(MouseButton::Left));
        match ev {
            PointerEvent::Move { pos } if self.sent == Some(at(pos)) && !self.rehover => {
                return Vec::new();
            }
            PointerEvent::Leave => {
                self.sent = None;
                self.rehover = false;
            }
            PointerEvent::Move { pos }
            | PointerEvent::Down { pos }
            | PointerEvent::Up { pos }
            | PointerEvent::DownButton { pos, .. }
            | PointerEvent::UpButton { pos, .. } => self.sent = Some(at(pos)),
            // スクロールでキーがポインタの下から動く。次の Move でホバーを判定し直す。
            PointerEvent::Scroll { pos, .. } => {
                self.sent = Some(at(pos));
                self.rehover = true;
            }
        }
        vec![match ev {
            PointerEvent::Move { pos } => {
                self.pos = at(pos);
                self.rehover = false;
                PlatformInput::MouseMove(MouseMoveEvent {
                    position: self.pos,
                    pressed_button: held(self.pressed),
                    modifiers: Default::default(),
                })
            }
            PointerEvent::Down { pos } | PointerEvent::DownButton { pos, .. } => {
                (self.pos, self.pressed) = (at(pos), true);
                let button = match ev {
                    PointerEvent::DownButton {
                        button: ClickButton::Middle,
                        ..
                    } => MouseButton::Middle,
                    PointerEvent::DownButton {
                        button: ClickButton::Right,
                        ..
                    } => MouseButton::Right,
                    _ => MouseButton::Left,
                };
                self.button = Some(button);
                PlatformInput::MouseDown(MouseDownEvent {
                    button,
                    position: self.pos,
                    modifiers: Default::default(),
                    click_count: 1,
                    first_mouse: false,
                })
            }
            PointerEvent::Up { pos } | PointerEvent::UpButton { pos, .. } => {
                (self.pos, self.pressed) = (at(pos), false);
                self.rehover = true;
                PlatformInput::MouseUp(MouseUpEvent {
                    button: self.button.take().unwrap_or(MouseButton::Left),
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
                self.button = None;
                PlatformInput::MouseExited(MouseExitEvent {
                    position: self.pos,
                    pressed_button,
                    modifiers: Default::default(),
                })
            }
        }]
    }
}

/// 配送前にポインタのコンテキストを更新する。本番と各 testkit で同じ消滅条件を使う。
pub fn dispatch_input(
    panel: PanelId,
    hand: Hand,
    input: PlatformInput,
    window: &mut Window,
    cx: &mut App,
) {
    if matches!(input, PlatformInput::MouseDown(_)) {
        keyboard::pointer_selected(Some(panel), cx);
    }
    let source = match hand {
        Hand::Left => 0,
        Hand::Right => 1,
    };
    if matches!(
        input,
        PlatformInput::MouseMove(_) | PlatformInput::MouseExited(_)
    ) {
        let id = window.window_handle().window_id();
        cx.default_global::<design::components::Tooltips>()
            .0
            .retain(|&(window, hand), _| window != id || hand != source);
        design::cue::begin_hover(window, source, cx);
    }
    let context = cx.default_global::<design::components::TooltipSource>();
    context.0 = Some(source);
    context.1.insert(window.window_handle().window_id());
    window.dispatch_event(input, cx);
    cx.default_global::<design::components::TooltipSource>().0 = None;
    cue::flush(panel, window, cx);
    link::flush(panel, window, cx);
}

#[cfg(test)]
mod tests;
