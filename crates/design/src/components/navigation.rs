//! 画面の積み重ね（push 遷移）。設定の段階的開示のように、選んだ項目の詳しい画面を右から重ねて出す。
//!
//! - 積むと新しい画面が右から入り、今の画面を左へ押し出す（2 枚は隙間なく並んで一緒に動く）。戻すと逆に右へ押し戻す。
//!   積み下ろしは呼び出し側が自分の `Vec` で行い、部品は前の描画との差（増えた・減った）から動きを決める。
//! - スワイプバック: 1 枚以上積んでいるとき、押したまま右から左へ動かすと、指に付いて上の画面が左へ抜け、下の画面が右から引っ張られて入ってくる。
//!   幅の半分を越えるか、勢いよく払って離すと `on_back` を呼ぶ（呼び出し側が下ろすと、離した所から左へ抜ける）。
//!   足りなければ元に戻る。
//!   - タッチ（指のスライド = ピクセル単位のスクロール）とマウス（押したまま動かす）のどちらでも動く。
//!   - 中の要素が先に受け取って止めたもの（スライダーのハンドルなど）は奪わない。縦にスクロールする一覧は
//!     横の動きを縦に読み替えないよう `restrict_scroll_to_axis()` を付ける。
//!   - スワイプ中は中身を覆い、離したときに下にあったボタンが押されたことにしない。
//! - パネルを掴んでいる手（グリップ）のポインタは無視する。オーバーレイは掴み中の手のイベントを送らず、掴んだ瞬間に
//!   その手の `MouseExited` だけが届く。ボタンを押していない動き・離脱（別の手のもの）ではスワイプを止めない。
//! - [`crate::Motion`] が無ければ（テスト）動きは即座に終わる。

use std::{rc::Rc, time::Instant};

use gpui::{
    AnyElement, App, Bounds, DispatchPhase, ElementId, IntoElement, MouseButton, MouseDownEvent,
    MouseExitEvent, MouseMoveEvent, MouseUpEvent, Pixels, Point, RenderOnce, ScrollDelta,
    ScrollWheelEvent, TouchPhase, Window, canvas, div, prelude::*, px, rgba,
};

use super::keep_drawing;
use crate::{Motion, Theme};

/// スワイプと決めるまでのマウスの動き（px）。タッチは UI 層が同じだけ待ってから送ってくる。
const SLOP: f32 = 12.0;
/// 離したときにこれ以上進んでいれば戻る（幅に対する割合）。
const COMMIT: f32 = 0.5;
/// 離す直前の 1 イベントの左向きの動き（px）がこれ以上なら、半分に届かなくても払ったとみなして戻る。
const FLING: f32 = 8.0;

/// 押してから離すまでのジェスチャ。
#[derive(Clone, Copy, Debug, PartialEq)]
enum Gesture {
    Idle,
    /// マウスで押した（まだスワイプと決まっていない）。押した位置。
    Pressed(Point<Pixels>),
    /// スワイプ中。上の画面の左へのずれ（px）と、直近の動き（px / イベント）。
    /// `start` はマウスのとき押した位置の x（タッチは `None`。動き量を足していく）。
    Swiping {
        offset: f32,
        velocity: f32,
        start: Option<f32>,
    },
    /// タッチのスワイプを離した後。UI 層の慣性のスクロールが続くので、次の指のスライドまで飲み込む。
    Coasting,
}

/// 上の画面のずれ（幅に対する割合。0 = 定位置、1 = 抜けた。向きは `State::dir`）の動き。
type Slide = crate::animation::Transition;

struct State<P> {
    /// 前の描画での積み重ね。
    shown: Vec<P>,
    /// 下ろされて抜けていく途中の画面。
    leaving: Option<P>,
    slide: Slide,
    gesture: Gesture,
    /// 直近の描画での範囲。
    bounds: Bounds<Pixels>,
    /// 上の画面がずれる向き（1 = 右、-1 = 左）。積み下ろしは右、スワイプバックは指に付いて左へ。
    dir: f32,
}

impl<P> State<P> {
    fn width_dir(&self) -> (f32, f32) {
        (self.width(), self.dir)
    }

    fn width(&self) -> f32 {
        f32::from(self.bounds.size.width).max(1.0)
    }

    /// 今の上の画面のずれ（割合）と、動いているか。
    fn offset(&self, now: Instant) -> (f32, bool) {
        match self.gesture {
            Gesture::Swiping { offset, .. } => ((offset / self.width()).clamp(0.0, 1.0), false),
            _ => self.slide.at(now),
        }
    }

    /// スワイプを始められるか: 積んであり、積み下ろしの動きの途中でない。
    fn can_swipe(&self, now: Instant) -> bool {
        self.shown.len() > 1 && self.leaving.is_none() && !self.slide.at(now).1
    }

    /// 指を離した。戻るなら `true`（その場で止め、呼び出し側が下ろすのを待つ）。足りなければ元へ戻り始める。
    fn release(&mut self, now: Instant, animated: bool) -> bool {
        let Gesture::Swiping {
            offset, velocity, ..
        } = self.gesture
        else {
            return false;
        };
        self.gesture = Gesture::Idle;
        let t = (offset / self.width()).clamp(0.0, 1.0);
        self.slide = Slide::rest(t);
        let back = t >= COMMIT || (velocity >= FLING && t > 0.0);
        if !back {
            self.slide
                .go(0.0, crate::animation::Motion::Return, now, animated);
        }
        back
    }
}

type Render<P> = Rc<dyn Fn(&P, &mut Window, &mut App) -> AnyElement>;
type OnBack = Rc<dyn Fn(&mut Window, &mut App)>;

/// [`navigation`] で作る。
#[derive(IntoElement)]
pub struct Navigation<P: Clone + PartialEq + 'static> {
    id: &'static str,
    stack: Vec<P>,
    render: Render<P>,
    on_back: OnBack,
    /// 画面の地（上の画面が下の画面を隠す）。
    background: u32,
}

/// `stack`（先頭が一番下の画面。空にしない）の一番上を見せる。`render` が画面 1 枚の中身を作る。
/// スワイプバックで戻るときに `on_back` が呼ばれるので、呼び出し側は `stack.pop()` する。
/// 全体は `{id}`、一番上の画面は `{id}-top`、動いている間に隣に並ぶ下の画面は `{id}-below` で指せる。
pub fn navigation<P: Clone + PartialEq + 'static>(
    theme: &Theme,
    id: &'static str,
    stack: &[P],
    render: impl Fn(&P, &mut Window, &mut App) -> AnyElement + 'static,
    on_back: impl Fn(&mut Window, &mut App) + 'static,
) -> Navigation<P> {
    assert!(!stack.is_empty(), "navigation needs a root page");
    Navigation {
        id,
        stack: stack.to_vec(),
        render: Rc::new(render),
        on_back: Rc::new(on_back),
        background: theme.background,
    }
}

impl<P: Clone + PartialEq + 'static> RenderOnce for Navigation<P> {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let Navigation {
            id,
            stack,
            render,
            on_back,
            background,
        } = self;
        let animated = cx.has_global::<Motion>();
        let now = crate::drive::now();
        let state = window.use_keyed_state(id, cx, {
            let stack = stack.clone();
            move |_, _| State {
                shown: stack,
                leaving: None,
                slide: Slide::rest(0.0),
                gesture: Gesture::Idle,
                bounds: Bounds::default(),
                dir: 1.0,
            }
        });

        // 前の描画からの積み下ろしで動きを決める。
        let (t, moving, top, below, swiping) = state.update(cx, |s, _| {
            use crate::animation::Motion::{Enter, Return};
            if stack.len() > s.shown.len() {
                // 積んだ: 右から入る。
                s.leaving = None;
                s.dir = 1.0;
                s.gesture = Gesture::Idle;
                s.slide = Slide::rest(1.0);
                s.slide.go(0.0, Enter, now, animated);
            } else if stack.len() < s.shown.len() {
                // 下ろした: 今の位置（スワイプで離した所）から抜ける（ボタンなら右、スワイプなら左）。
                let (cur, _) = s.offset(now);
                s.leaving = s.shown.last().cloned();
                s.gesture = Gesture::Idle;
                s.slide = Slide::rest(cur);
                s.slide.go(1.0, Return, now, animated);
            }
            s.shown = stack.clone();
            let swiping = matches!(s.gesture, Gesture::Swiping { .. });
            let (t, moving) = s.offset(now);
            if !moving && !swiping {
                if s.leaving.take().is_some() || t == 0.0 {
                    // 抜けきった・定位置にいる。次の積み下ろしは右へ。
                    s.dir = 1.0;
                    s.slide = Slide::rest(0.0);
                } else if t != 0.0 {
                    // 戻ると離したのに下ろされなかった。元へ戻る。
                    s.slide.go(0.0, Return, now, animated);
                }
            }
            let (t, moving) = s.offset(now);
            // (画面, 積み重ねの段)。段は要素の状態（スクロール位置など）の鍵にする。
            let n = stack.len();
            let (top, below) = match s.leaving.clone() {
                Some(p) => ((p, n), Some((stack[n - 1].clone(), n - 1))),
                None => (
                    (stack[n - 1].clone(), n - 1),
                    (t > 0.0 && n > 1).then(|| (stack[n - 2].clone(), n - 2)),
                ),
            };
            (t, moving, top, below, swiping)
        });
        if moving {
            keep_drawing(window, cx);
        }
        let (width, dir) = state.read(cx).width_dir();

        // 範囲を覚え、ウィンドウ全体で入力を見る。最初の子なので、捕捉の段階では中身より先、
        // 浮き上がりの段階では中身より後に呼ばれる（中の要素が止めたものは届かない）。
        let watch = canvas(
            {
                let state = state.clone();
                move |bounds, _, cx| state.update(cx, |s, _| s.bounds = bounds)
            },
            move |_, _, window, _| {
                let s = state.clone();
                let back = on_back.clone();
                window.on_mouse_event(move |e: &ScrollWheelEvent, phase, window, cx| {
                    let ScrollDelta::Pixels(d) = e.delta else {
                        return;
                    };
                    let (dx, dy) = (f32::from(d.x), f32::from(d.y));
                    let now = crate::drive::now();
                    let gesture = s.read(cx).gesture;
                    match (phase, gesture, e.touch_phase) {
                        // スワイプ中・離した後の慣性は中身へ流さない。
                        (DispatchPhase::Capture, Gesture::Swiping { .. }, TouchPhase::Ended) => {
                            cx.stop_propagation();
                            let animated = cx.has_global::<Motion>();
                            let go = s.update(cx, |s, _| {
                                let go = s.release(now, animated);
                                s.gesture = Gesture::Coasting;
                                go
                            });
                            if go {
                                back(window, cx);
                            }
                            window.refresh();
                        }
                        (DispatchPhase::Capture, Gesture::Swiping { .. }, _) => {
                            cx.stop_propagation();
                            s.update(cx, |s, _| {
                                if let Gesture::Swiping {
                                    offset, velocity, ..
                                } = &mut s.gesture
                                {
                                    *offset = (*offset - dx).max(0.0);
                                    *velocity = *velocity * 0.5 + -dx * 0.5;
                                }
                            });
                            window.refresh();
                        }
                        (DispatchPhase::Capture, Gesture::Coasting, TouchPhase::Ended) => {
                            cx.stop_propagation();
                        }
                        (DispatchPhase::Capture, Gesture::Coasting, _) => {
                            s.update(cx, |s, _| s.gesture = Gesture::Idle);
                        }
                        // 指のスライドの始まりが左向き（右から左）なら奪う（中の要素が止めていなければ）。
                        (DispatchPhase::Bubble, Gesture::Idle, TouchPhase::Started)
                            if -dx > dy.abs()
                                && s.read(cx).bounds.contains(&e.position)
                                && s.read(cx).can_swipe(now) =>
                        {
                            cx.stop_propagation();
                            s.update(cx, |s, _| {
                                s.dir = -1.0;
                                s.gesture = Gesture::Swiping {
                                    offset: -dx,
                                    velocity: 0.0,
                                    start: None,
                                }
                            });
                            window.refresh();
                        }
                        _ => {}
                    }
                });
                let st = state.clone();
                window.on_mouse_event(move |e: &MouseDownEvent, phase, _, cx| {
                    let now = crate::drive::now();
                    let s = st.read(cx);
                    if phase == DispatchPhase::Bubble
                        && e.button == MouseButton::Left
                        && matches!(s.gesture, Gesture::Idle | Gesture::Coasting)
                        && s.bounds.contains(&e.position)
                        && s.can_swipe(now)
                    {
                        st.update(cx, |s, _| s.gesture = Gesture::Pressed(e.position));
                    }
                });
                let st = state.clone();
                window.on_mouse_event(move |e: &MouseMoveEvent, phase, window, cx| {
                    // ボタンを押していない動きは別の手のもの（GPUI のマウスは 1 つ）。押しているときだけ見る。
                    if phase != DispatchPhase::Capture
                        || e.pressed_button != Some(MouseButton::Left)
                    {
                        return;
                    }
                    let x = f32::from(e.position.x);
                    match st.read(cx).gesture {
                        Gesture::Pressed(at) => {
                            let d = e.position - at;
                            let (dx, dy) = (f32::from(d.x), f32::from(d.y));
                            if dx.hypot(dy) <= SLOP {
                                return;
                            }
                            st.update(cx, |s, _| {
                                s.dir = -1.0;
                                s.gesture = if -dx > dy.abs() {
                                    Gesture::Swiping {
                                        offset: -dx,
                                        velocity: 0.0,
                                        start: Some(f32::from(at.x)),
                                    }
                                } else {
                                    // 左向きでない動きでは諦めず、ここを起点に置き直して左への動きを待つ。長押しの間の
                                    // 手ブレや、手ブレ抑えで遅れたレーザーが手に追いつく分で、最初の動きが左とは限らない。
                                    Gesture::Pressed(e.position)
                                }
                            });
                            window.refresh();
                        }
                        Gesture::Swiping {
                            offset,
                            start: Some(start),
                            ..
                        } => {
                            cx.stop_propagation();
                            let next = (start - x).max(0.0);
                            st.update(cx, |s, _| {
                                if let Gesture::Swiping {
                                    offset: o,
                                    velocity,
                                    ..
                                } = &mut s.gesture
                                {
                                    *velocity = *velocity * 0.5 + (next - offset) * 0.5;
                                    *o = next;
                                }
                            });
                            window.refresh();
                        }
                        _ => {}
                    }
                });
                // 離した・押したまま外れた（その手でパネルを掴んだ）。外れたのが押していない手なら別の手なので続ける。
                let release = {
                    let (st, back) = (state.clone(), on_back.clone());
                    move |window: &mut Window, cx: &mut App| {
                        let s = st.read(cx);
                        match s.gesture {
                            Gesture::Pressed(_) => st.update(cx, |s, _| s.gesture = Gesture::Idle),
                            Gesture::Swiping { start: Some(_), .. } => {
                                let animated = cx.has_global::<Motion>();
                                let now = crate::drive::now();
                                if st.update(cx, |s, _| s.release(now, animated)) {
                                    back(window, cx);
                                }
                                window.refresh();
                            }
                            _ => {}
                        }
                    }
                };
                let up = release.clone();
                window.on_mouse_event(move |e: &MouseUpEvent, phase, window, cx| {
                    if phase == DispatchPhase::Capture && e.button == MouseButton::Left {
                        up(window, cx);
                    }
                });
                window.on_mouse_event(move |e: &MouseExitEvent, phase, window, cx| {
                    if phase == DispatchPhase::Capture
                        && e.pressed_button == Some(MouseButton::Left)
                    {
                        release(window, cx);
                    }
                });
            },
        )
        .absolute()
        .size_full();

        let page = |p: &P, level: usize, x: f32, window: &mut Window, cx: &mut App| {
            div()
                .id(ElementId::NamedInteger("nav-page".into(), level as u64))
                .absolute()
                .top_0()
                .bottom_0()
                .w_full()
                .left(px(x))
                .bg(rgba(background))
                .child(render(p, window, cx))
        };
        let mut root = div()
            .id(id)
            .debug_selector(move || id.into())
            .relative()
            .size_full()
            .overflow_hidden()
            .child(watch);
        if let Some((b, level)) = &below {
            // 下の画面は上の画面の隣に並ぶ（押し出す・引っ張る）。上の画面が抜けきると定位置。
            root = root.child(
                page(b, *level, -dir * (1.0 - t) * width, window, cx)
                    .debug_selector(move || format!("{id}-below")),
            );
        }
        let (top, level) = top;
        root = root.child(
            page(&top, level, dir * t * width, window, cx)
                .debug_selector(move || format!("{id}-top")),
        );
        if swiping {
            // スワイプ中は中身を覆う（離したときにボタンの上でもクリックにしない・ホバーを消す）。
            root = root.child(div().id("nav-shield").absolute().size_full().occlude());
        }
        root
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn state(width: f32, offset: f32, velocity: f32) -> State<u8> {
        State {
            shown: vec![0, 1],
            leaving: None,
            slide: Slide::rest(0.0),
            gesture: Gesture::Swiping {
                offset,
                velocity,
                start: None,
            },
            bounds: Bounds::new(Point::default(), gpui::size(px(width), px(100.0))),
            dir: -1.0,
        }
    }

    #[test]
    fn release_goes_back_past_half_or_on_a_fling() {
        let now = Instant::now();
        assert!(state(400.0, 210.0, 0.0).release(now, false));
        assert!(state(400.0, 40.0, FLING).release(now, false), "fling");
        let mut s = state(400.0, 150.0, 1.0);
        assert!(!s.release(now, false), "too short");
        assert_eq!(s.slide.at(now), (0.0, false), "snaps back");
        assert_eq!(s.gesture, Gesture::Idle);
    }

    #[test]
    fn root_page_cannot_be_swiped() {
        let now = Instant::now();
        let mut s = state(400.0, 0.0, 0.0);
        assert!(s.can_swipe(now));
        s.shown = vec![0];
        assert!(!s.can_swipe(now));
    }

    #[test]
    fn slide_eases_toward_its_target() {
        let now = Instant::now();
        let mut s = Slide::rest(1.0);
        s.go(0.0, crate::animation::Motion::Enter, now, true);
        let (t, moving) = s.at(now + std::time::Duration::from_millis(50));
        assert!(moving && t > 0.0 && t < 1.0, "{t}");
        assert_eq!(s.at(now + std::time::Duration::from_secs(1)), (0.0, false));
        s.go(1.0, crate::animation::Motion::Return, now, false);
        assert_eq!(s.at(now), (1.0, false), "no motion: instant");
    }
}
