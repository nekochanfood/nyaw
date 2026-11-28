//! Lottie のアニメーションを再生する部品。`intro_logo("id").height(px(144.))` は、フルロゴのイントロ
//! （リポジトリルートの `assets/animations/logo-full`）を再生し、再生し終えたら、文字や耳やしっぽを押すとその部品が反応する。
//! 読み込みと描画はバックグラウンドスレッドで行う。進め方（[`Playback`]）は GPUI に依存しない。詳しくは AGENT.md。

use std::{sync::Arc, time::Instant};

use gpui::{
    App, Bounds, ElementId, IntoElement, MouseButton, MouseDownEvent, Pixels, RenderImage,
    RenderOnce, Window, canvas, div, img, prelude::*, px,
};
use lottie::Animation;

use crate::{Logo, Mode, Motion, drive};

/// フルロゴのイントロ。合成は、ロゴの外形（[`Logo::Full`] の viewBox）の四辺に [`MARGIN`] を足した大きさ。
fn source(mode: Mode) -> &'static str {
    match mode {
        Mode::Light => {
            include_str!("../../../../../assets/animations/logo-full/logo-full-light.json")
        }
        Mode::Dark => {
            include_str!("../../../../../assets/animations/logo-full/logo-full-dark.json")
        }
    }
}

/// 合成の四辺の余白（`assets/animations/logo-full/build.mjs` の MARGIN）と、ロゴの外形の幅。
const MARGIN: f32 = 24.0;
const VIEW_BOX_WIDTH: f32 = 352.49;

/// 再生の進め方。イントロの区間を再生し、再生し終えたら（`ready`）、押された部品の反応の区間を再生する。
#[derive(Debug, Default)]
pub struct Playback {
    segment: Option<Segment>,
    started: bool,
    ready: bool,
    /// 表示するフレーム。
    frame: f64,
}

#[derive(Clone, Copy, Debug)]
struct Segment {
    from: f64,
    to: f64,
    since: Instant,
    intro: bool,
}

impl Playback {
    /// イントロを始める。`animated` でなければ（動きのないテストなど）、再生し終えた状態から始める。
    pub fn start(&mut self, anim: &Animation, animated: bool, now: Instant) {
        if self.started {
            return;
        }
        self.started = true;
        let Some(intro) = anim.marker("intro") else {
            self.ready = true;
            self.frame = anim.in_point;
            return;
        };
        if animated {
            self.segment = Some(Segment {
                from: intro.start,
                to: intro.end(),
                since: now,
                intro: true,
            });
            self.frame = intro.start;
        } else {
            self.ready = true;
            self.frame = intro.end();
        }
    }

    /// 時計を `now` まで進める。区間を再生中なら `true`。
    pub fn advance(&mut self, frame_rate: f64, now: Instant) -> bool {
        let Some(seg) = self.segment else {
            return false;
        };
        let frame = seg.from + now.saturating_duration_since(seg.since).as_secs_f64() * frame_rate;
        if frame >= seg.to {
            self.frame = seg.to;
            self.segment = None;
            self.ready |= seg.intro;
            return false;
        }
        self.frame = frame;
        true
    }

    /// 合成の座標 `(x, y)` が押された。イントロを再生し終えていて、部品（`cl` が `intro-hit <部品>`）に当たれば、
    /// その部品の反応の区間（`twitch-<部品>`）を始めて `true` を返す。
    pub fn press(&mut self, anim: &Animation, x: f64, y: f64, now: Instant) -> bool {
        if !self.ready {
            return false;
        }
        let Some(class) = anim.hit(self.frame, x, y) else {
            return false;
        };
        let Some(marker) = class
            .strip_prefix("intro-hit ")
            .and_then(|part| anim.marker(&format!("twitch-{part}")))
        else {
            return false;
        };
        self.segment = Some(Segment {
            from: marker.start,
            to: marker.end(),
            since: now,
            intro: false,
        });
        self.frame = marker.start;
        true
    }

    pub fn ready(&self) -> bool {
        self.ready
    }

    pub fn frame(&self) -> f64 {
        self.frame
    }
}

/// 描いた画像の内容（同じなら描き直さない）。
#[derive(Clone, Copy, Debug, PartialEq)]
struct Key {
    frame: i64,
    mode: Mode,
    width: u32,
    height: u32,
}

/// 部品の状態。アニメーションはテーマごとに一度だけ読む。
#[derive(Default)]
struct Player {
    light: Load,
    dark: Load,
    playback: Playback,
    image: Option<(Arc<RenderImage>, Key)>,
    rendering: Option<Key>,
    /// 合成の画像を置いた範囲（ウィンドウの座標）。押した位置を合成の座標にするのに使う。
    stage: Bounds<Pixels>,
}

#[derive(Default)]
enum Load {
    #[default]
    Idle,
    Loading,
    Ready(Arc<Animation>),
    Failed,
}

impl Player {
    fn load(&mut self, mode: Mode) -> &mut Load {
        match mode {
            Mode::Light => &mut self.light,
            Mode::Dark => &mut self.dark,
        }
    }

    /// 毎回の描画で呼ぶ。読み込みと描画を必要に応じて始める。次のフレームも描くべきなら `true`。
    fn tick(
        &mut self,
        mode: Mode,
        size: (u32, u32),
        animated: bool,
        cx: &mut gpui::Context<Self>,
    ) -> bool {
        let anim = match self.load(mode) {
            Load::Ready(anim) => anim.clone(),
            Load::Loading | Load::Failed => return false,
            load @ Load::Idle => {
                *load = Load::Loading;
                cx.spawn(async move |this, cx| {
                    let parsed = cx
                        .background_executor()
                        .spawn(async move { Animation::from_json(source(mode)) })
                        .await;
                    let _ = this.update(cx, |p, cx| {
                        *p.load(mode) = match parsed {
                            Ok(anim) => Load::Ready(Arc::new(anim)),
                            Err(e) => {
                                logger::warn!("cannot load the logo animation", error = e);
                                Load::Failed
                            }
                        };
                        cx.notify();
                    });
                })
                .detach();
                return false;
            }
        };
        let now = drive::now();
        self.playback.start(&anim, animated, now);
        let playing = self.playback.advance(anim.frame_rate, now);
        let key = Key {
            frame: self.playback.frame().floor() as i64,
            mode,
            width: size.0,
            height: size.1,
        };
        let shown = self.image.as_ref().map(|(_, k)| *k);
        if shown != Some(key) && self.rendering.is_none() && size.0 > 0 && size.1 > 0 {
            self.rendering = Some(key);
            cx.spawn(async move |this, cx| {
                let image = cx
                    .background_executor()
                    .spawn(async move {
                        anim.render(key.frame as f64, key.width, key.height)
                            .and_then(to_image)
                    })
                    .await;
                let _ = this.update(cx, |p, cx| {
                    p.rendering = None;
                    if let Some(image) = image {
                        p.image = Some((image, key));
                    }
                    cx.notify();
                });
            })
            .detach();
        }
        playing || self.rendering.is_some()
    }

    /// ウィンドウの座標 `position` が押された。反応を始めたら `true`。
    fn press(&mut self, mode: Mode, position: gpui::Point<Pixels>) -> bool {
        let Load::Ready(anim) = self.load(mode) else {
            return false;
        };
        let anim = anim.clone();
        let (w, h) = (
            f32::from(self.stage.size.width),
            f32::from(self.stage.size.height),
        );
        if w <= 0. || h <= 0. {
            return false;
        }
        let x = f32::from(position.x - self.stage.origin.x) / w * anim.width as f32;
        let y = f32::from(position.y - self.stage.origin.y) / h * anim.height as f32;
        self.playback.press(&anim, x as f64, y as f64, drive::now())
    }
}

/// 乗算済みの RGBA を、GPUI の画像（乗算していない BGRA）にする。
fn to_image(pixmap: lottie::Pixmap) -> Option<Arc<RenderImage>> {
    let (w, h) = (pixmap.width(), pixmap.height());
    let mut bgra = Vec::with_capacity((w * h * 4) as usize);
    for p in pixmap.pixels() {
        let c = p.demultiply();
        bgra.extend_from_slice(&[c.blue(), c.green(), c.red(), c.alpha()]);
    }
    let buffer = image::RgbaImage::from_raw(w, h, bgra)?;
    Some(crate::images::track(Arc::new(RenderImage::new(vec![
        image::Frame::new(buffer),
    ]))))
}

/// フルロゴのイントロの部品。高さで大きさを決め、幅は [`Logo::Full`] の縦横比から決める（静止画の [`crate::logo`] と同じ大きさ）。
/// 合成の余白の分は、外側へはみ出して描く。
#[derive(IntoElement)]
pub struct IntroLogo {
    id: ElementId,
    height: Pixels,
}

pub fn intro_logo(id: impl Into<ElementId>) -> IntroLogo {
    IntroLogo {
        id: id.into(),
        height: px(crate::font_size::LG),
    }
}

impl IntroLogo {
    pub fn height(mut self, height: Pixels) -> Self {
        self.height = height;
        self
    }
}

impl RenderOnce for IntroLogo {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let mode = cx.try_global::<Mode>().copied().unwrap_or_default();
        let animated = cx.has_global::<Motion>();
        let width = self.height * Logo::Full.aspect();
        // 合成の大きさ（ロゴの外形に余白を足したもの）と、外形からのはみ出し
        let k = f32::from(width) / VIEW_BOX_WIDTH;
        let margin = px(MARGIN * k);
        let (stage_w, stage_h) = (width + margin * 2., self.height + margin * 2.);
        let scale = window.scale_factor();
        let size = (
            (f32::from(stage_w) * scale).ceil() as u32,
            (f32::from(stage_h) * scale).ceil() as u32,
        );

        let state = window.use_keyed_state(self.id.clone(), cx, |_, _| Player::default());
        let keep = state.update(cx, |p, cx| p.tick(mode, size, animated, cx));
        if keep {
            drive::keep_drawing(window, cx);
        }
        let (failed, image) = state.read_with(cx, |p, _| {
            let load = match mode {
                Mode::Light => &p.light,
                Mode::Dark => &p.dark,
            };
            // テーマを切り替えた直後は、新しいテーマの画像を描き終えるまで前の画像を出す（消えるよりちらつかない）
            (
                matches!(load, Load::Failed),
                p.image.as_ref().map(|(i, _)| i.clone()),
            )
        });
        if failed {
            // 読めなければ静止画のロゴを出す
            return crate::logo(Logo::Full)
                .height(self.height)
                .into_any_element();
        }

        let watch = {
            let state = state.clone();
            canvas(
                move |bounds, _, cx| state.update(cx, |p, _| p.stage = bounds),
                |_, _, _, _| {},
            )
            .absolute()
            .size_full()
        };
        let stage = div()
            .id(self.id)
            .absolute()
            .left(-margin)
            .top(-margin)
            .w(stage_w)
            .h(stage_h)
            .child(watch)
            .children(image.map(|i| img(i).size_full()))
            .on_mouse_down(MouseButton::Left, move |e: &MouseDownEvent, window, cx| {
                if state.update(cx, |p, _| p.press(mode, e.position)) {
                    window.refresh();
                }
            });
        div()
            .relative()
            .flex_none()
            .w(width)
            .h(self.height)
            .child(stage)
            .into_any_element()
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::*;

    fn anim() -> Animation {
        Animation::from_json(source(Mode::Light)).unwrap()
    }

    /// 合成の座標で、左の耳の上の点。
    const EAR: (f64, f64) = (86., 50.);

    #[test]
    fn presses_do_nothing_until_the_intro_ends() {
        let anim = anim();
        let t0 = Instant::now();
        let mut p = Playback::default();
        p.start(&anim, true, t0);
        assert!(!p.ready());
        // イントロの途中（2 秒）では耳がもう見えているが、押しても反応しない
        let mid = t0 + Duration::from_secs(2);
        assert!(p.advance(anim.frame_rate, mid));
        assert!(!p.press(&anim, EAR.0, EAR.1, mid));
        // イントロ（3 秒）を過ぎたら反応する
        let end = t0 + Duration::from_millis(3100);
        assert!(!p.advance(anim.frame_rate, end));
        assert!(p.ready());
        assert_eq!(p.frame(), anim.marker("intro").unwrap().end());
        assert!(p.press(&anim, EAR.0, EAR.1, end));
        let twitch = anim.marker("twitch-earL").unwrap();
        assert_eq!(p.frame(), twitch.start);
        assert!(p.advance(anim.frame_rate, end + Duration::from_millis(100)));
        // 反応の区間を再生し終えると、そこで止まる
        assert!(!p.advance(anim.frame_rate, end + Duration::from_secs(1)));
        assert_eq!(p.frame(), twitch.end());
    }

    #[test]
    fn presses_outside_the_parts_do_nothing() {
        let anim = anim();
        let mut p = Playback::default();
        p.start(&anim, false, Instant::now());
        assert!(p.ready());
        assert!(!p.press(&anim, 5., 5., Instant::now()));
    }

    #[test]
    fn without_motion_it_starts_at_the_end_of_the_intro() {
        let anim = anim();
        let mut p = Playback::default();
        p.start(&anim, false, Instant::now());
        assert!(!p.advance(anim.frame_rate, Instant::now()));
        assert_eq!(p.frame(), anim.marker("intro").unwrap().end());
    }
}
