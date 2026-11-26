//! レーザーイベントを GPUI のマウス入力に変換する（[`PointerBridge`]）。タッチモードではタップ・スワイプ・長押しを識別する。

use super::*;

/// 片手のレーザーイベントを GPUI のプラットフォーム入力に変換する。GPUI のイベントは
/// 位置とボタン状態を保持するが、オーバーレイのイベントにはそれらがないため、直前の値を内部に保持する。
///
/// [`PointerMode::Touch`] では、押したまま移動するとスワイプ（ピクセル単位のスクロール。離すと慣性で滑る）となり、
/// 移動せず離すとタップ（押下→解放）、移動せず [`LONG_PRESS`] 保持するとマウス押下（そのまま移動すれば
/// ドラッグ＆ドロップ）となる。GPUI にはタッチ入力がないため、方式が確定するまで押下イベントの送出を保留する。
/// 方式は押下時点で決定し、解放まで変更しない。
#[derive(Debug, Default)]
pub struct PointerBridge {
    pos: Point<Pixels>,
    pub(crate) pressed: bool,
    button: Option<MouseButton>,
    pub(crate) touch: Option<Touch>,
    /// スワイプ解放後の慣性（宛先位置と 1 フレームあたりの移動量）。
    momentum: Option<(Point<Pixels>, Point<Pixels>)>,
    /// この手が最後に送出した位置（パネルから外れたら `None`）。同一位置への Move は送出しない
    /// （オーバーレイは指している間に毎 tick Move を発行するため、静止時でも再描画と読み戻しが毎フレーム発生する）。
    sent: Option<Point<Pixels>>,
    pub(crate) rehover: bool,
    /// スワイプと判定する移動量（論理 px）。[`pump`] がパネルの距離と密度から毎フレーム算出する（[`SWIPE_SLOP`] を下回らない）。
    pub slop: f32,
}

/// タッチ押下中の内部状態。
#[derive(Debug)]
pub(crate) struct Touch {
    start: Point<Pixels>,
    /// 押下時刻（長押し判定に使用）。
    at: Instant,
    /// スワイプとして確定済み。
    swiping: bool,
    /// 長押しによりマウス押下に遷移済み（以降はマウスと同様に動作する）。
    held: bool,
    /// スワイプの 1 イベントあたりの移動量（慣性の初速度。直近の値に重みを置いて平滑化する）。
    velocity: Point<Pixels>,
}

/// この時間以上移動せず押下を継続すると、タッチモードでもマウス押下に遷移する（ドラッグ＆ドロップが可能になる）。
pub const LONG_PRESS: Duration = Duration::from_millis(400);

/// 長押しでドラッグを開始する際に押下状態のまま移動させる量（論理 px）。GPUI のドラッグ閾値（2px）を超過する値。
const DRAG_NUDGE: f32 = 3.0;

/// 慣性の 1 フレームあたりの減衰係数（約 90Hz で 1 秒後に約 1% まで減衰）。
const FRICTION: f32 = 0.95;

/// 慣性がこの速度を下回ったら停止する（論理 px / フレーム）。
const MOMENTUM_STOP: f32 = 0.5;

/// この移動量を超えるとタップではなくスワイプと判定する（論理 px）。距離・密度に基づく閾値（[`SWIPE_SLOP_DEG`]）の下限値。
pub const SWIPE_SLOP: f32 = 12.0;

/// この角度（頭基準）を超える移動でスワイプと判定する。遠距離・高密度パネル（キーボード等）でも
/// 手ブレによるタップの誤判定を防ぐため、角度基準で決定し毎フレーム px に変換する（[`PointerBridge::slop`]）。
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
                        // 長押し: 押下位置でマウス押下に遷移する。以降はマウスと同様に動作する（移動すればドラッグ）。
                        // GPUI のドラッグ閾値（2px）を超える移動を挿入し、カーソルを動かさなくても即座にドラッグ開始の
                        // 見た目を表示する。最後に現在位置へ復帰する。
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
                    // コンテンツが指に追従する方向（下方移動で上方スクロール）。スクロール対象位置は押下位置に固定する。
                    return vec![PlatformInput::ScrollWheel(ScrollWheelEvent {
                        position: t.start,
                        delta: ScrollDelta::Pixels(pos - last),
                        modifiers: Default::default(),
                        // スクロールバー等がスライドの開始・終了を検知できるよう、タッチフェーズを設定する。
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
                        // 解放後も慣性で滑り続ける（フレームごとの momentum）。GPUI のマウスカーソルは解放位置に移動する。
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
                    // タップ: 押下→解放のシーケンスを送出する。
                    let mut out = self.mouse(PointerEvent::Move { pos });
                    out.extend(self.mouse(PointerEvent::Down { pos }));
                    out.extend(self.mouse(PointerEvent::Up { pos }));
                    return out;
                }
                PointerEvent::Leave => {
                    // スワイプ中にパネルから外れた場合、スライド終了を通知する。
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
                // スティックスクロールはマウスと同様に処理する。タッチ中の再押下は発生しない。
                PointerEvent::Scroll { .. }
                | PointerEvent::Down { .. }
                | PointerEvent::DownButton { .. }
                | PointerEvent::UpButton { .. } => {}
            }
        } else if let (PointerMode::Touch, PointerEvent::Down { pos }) = (mode, ev) {
            // タッチ開始で慣性スクロールを停止する。
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

    /// タッチ押下中かつ未移動時の、長押し（[`LONG_PRESS`]）到達までの進捗（0〜1）。非押下・スワイプ中・
    /// 長押し完了後は `None` を返す。ポインタのリング表示に使用する。
    pub fn hold_progress(&self) -> Option<f32> {
        let t = self.touch.as_ref().filter(|t| !t.swiping && !t.held)?;
        // 通常のタップでリングがちらつかないよう、押下直後は非表示とし、残りの時間で 0〜1 まで進行させる。
        const RING_DELAY: Duration = Duration::from_millis(150);
        let e = t.at.elapsed().checked_sub(RING_DELAY)?;
        Some((e.as_secs_f32() / (LONG_PRESS - RING_DELAY).as_secs_f32()).min(1.0))
    }

    /// この手のレーザーがパネル上を指しているかどうか。
    pub fn on_panel(&self) -> bool {
        self.sent.is_some()
    }

    /// タッチの長押しによりマウス押下状態に遷移済み（ドラッグ操作が可能）。
    pub fn held(&self) -> bool {
        self.touch.as_ref().is_some_and(|t| t.held)
    }

    /// スワイプ解放後の慣性スクロールの 1 フレーム分を返す。慣性が停止していれば `None`。フレームごとに 1 回呼び出す。
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
            // 指を離した後の惰性スクロール（指のスライド操作そのものではない）。
            touch_phase: TouchPhase::Ended,
        }))
    }

    /// マウスイベントとして変換する。
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
            // スクロールによりポインタ直下の要素が移動するため、次の Move でホバー対象を再判定する。
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

/// 入力配送前にポインタのコンテキストを更新する。本番環境と各 testkit で同一の消去条件を適用する。
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
