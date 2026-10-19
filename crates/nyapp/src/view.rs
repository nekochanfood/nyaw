//! ニャプリのパネル: サンドボックスが返す画面の木（[`Node`]）を `design` の部品で描き、押された・指されたなどを
//! 出来事としてニャプリへ返す。ニャプリが落ちたら（trap・燃料切れ・壊れた画面）パネルを白黒にして残し、
//! その子として強制終了のダイアログ（再起動・閉じる）を出す。

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::rc::Rc;
use std::sync::OnceLock;
#[cfg(not(test))]
use std::sync::{Arc, Mutex, mpsc};
use std::time::Duration;

use design::components::{self, Variant as ButtonVariant};
use design::{Theme, font_size, icons, radius, spacing};
use gpui::{
    AnyElement, App, Bounds, Context, Div, FontWeight, MouseMoveEvent, Pixels, Point, Task, Window,
    div, prelude::*, px, rgba,
};
use i18n::Lang;
use kernel::PanelId;
use kernel::audio::Sound;
use kernel::vr::Vec2;
use nyapp_sdk::{Align, Color, Controller, Event, Gap, Node, Shape, Size, Stack, Variant};
use ui::{PanelControl, PanelSpec};

use crate::{Error, Installed, worker};
#[cfg(test)]
use crate::{Sandbox, installed, package};

/// パネルの大きさと解像度（ダッシュボードにはめる画面と同じ）。
const SIZE_M: Vec2 = Vec2::new(1.024, 0.768);
const RES_PX: (u32, u32) = (1024, 768);
/// 周期の下限（ms）。これより短く頼まれても、メインスレッドを回しすぎないよう延ばす。
const MIN_TICK_MS: u32 = 16;
/// [`Size::Xl`] と [`Size::Display`] の文字の大きさ（px）。
const XL_PX: f32 = 32.0;
const DISPLAY_PX: f32 = 72.0;

/// ニャプリ `app` を開く起動仕様。`opened` は作られたパネルの ID を受け取る（ダッシュボードにはめるなど）。
/// タイトル（ナビゲーションバー）は `lang` での名前。
pub fn spec(
    from: PanelId,
    app: Installed,
    lang: Lang,
    opened: impl FnOnce(PanelId, &mut App) + 'static,
) -> PanelSpec {
    let title = app.manifest.name_in(lang.code()).to_owned();
    PanelSpec::spawn(from, move |panel, _, cx: &mut Context<NyappView>| {
        crate::opened(&app.manifest.id, panel, cx);
        opened(panel, cx);
        NyappView::new(panel, app, cx)
    })
    .sized(SIZE_M, RES_PX)
    .titled_text(title)
}

enum State {
    Loading,
    #[cfg(not(test))]
    Running(worker::RemoteSandbox),
    #[cfg(test)]
    TestRunning(Box<Sandbox>),
    /// 落ちた。最後の画面を白黒で残し、子のダイアログ（[`CrashDialog`]）で知らせる。`asked` はダイアログを出したか
    /// （高さを文に合わせて測るので、ウィンドウのある描画のときに出す）。
    Failed {
        reason: String,
        asked: bool,
    },
}

pub struct NyappView {
    panel: PanelId,
    app: Installed,
    state: State,
    tree: Node,
    /// 今の周期と、それを回すタスク（落とすと止まる）。
    ticker: Option<(u32, Task<()>)>,
    /// 画面にある面（[`Node::Canvas`]）の ID と、描いた場所。ポインタの座標を面の左上からに直す。
    canvases: RefCell<HashMap<String, Rc<Cell<Bounds<Pixels>>>>>,
    /// 最後に知らせたポインタの位置（同じ所なら送らない）。
    pointer: Option<Point<Pixels>>,
    generation: u64,
}

impl NyappView {
    fn new(panel: PanelId, app: Installed, cx: &mut Context<Self>) -> Self {
        let mut this = Self {
            panel,
            app,
            state: State::Loading,
            tree: Node::Spacer,
            ticker: None,
            canvases: RefCell::default(),
            pointer: None,
            generation: 0,
        };
        this.load(cx);
        this
    }

    /// パッケージを読み、サンドボックスを起こす（別スレッド）。再起動もこれ。
    fn load(&mut self, cx: &mut Context<Self>) {
        self.state = State::Loading;
        self.ticker = None;
        self.generation += 1;
        let generation = self.generation;
        let path = self.app.path.clone();
        #[cfg(not(test))]
        let id = self.app.manifest.id.clone();
        let language = Lang::current(cx).code();
        cx.spawn(async move |this, cx| {
            #[cfg(test)]
            let loaded = cx
                .background_executor()
                .spawn(async move {
                    let (manifest, wasm) = package::read(&path)?;
                    Sandbox::load(&manifest, &wasm, installed::appdata(&manifest.id), language)
                })
                .await;
            #[cfg(not(test))]
            let loaded = cx
                .background_executor()
                .spawn(async move { worker::RemoteSandbox::start(&path, &id, language) })
                .await;
            let _ = this.update(cx, |v, cx| {
                if v.generation != generation {
                    return;
                }
                match loaded {
                    #[cfg(test)]
                    Ok((sandbox, tree)) => {
                        let mut sandbox = Box::new(sandbox);
                        let step = worker::step(&mut sandbox, tree);
                        v.state = State::TestRunning(sandbox);
                        v.after_step(step, cx);
                    }
                    #[cfg(not(test))]
                    Ok((remote, step, rx)) => {
                        v.state = State::Running(remote);
                        v.after_step(step, cx);
                        v.listen(rx, generation, cx);
                    }
                    Err(e) => v.fail(e, cx),
                }
            });
        })
        .detach();
        self.redraw(cx);
    }

    /// 出来事をニャプリへ渡し、返ってきた画面に替える。
    fn send(&mut self, event: Event, cx: &mut Context<Self>) {
        let now_playing = cx.default_global::<PanelControl>().media.as_ref().map(|m| {
            if m.artist.is_empty() {
                m.title.clone()
            } else {
                format!("{} - {}", m.title, m.artist)
            }
        });
        let controllers = pointed_controllers(cx.default_global::<PanelControl>(), self.panel);
        #[cfg(test)]
        let mut local_result = None;
        match &mut self.state {
            #[cfg(not(test))]
            State::Running(remote) => {
                if let Err(e) =
                    remote.event(event, Lang::current(cx).code(), now_playing, controllers)
                {
                    self.fail(e, cx);
                }
            }
            #[cfg(test)]
            State::TestRunning(sandbox) => {
                let host = sandbox.host();
                host.language = Lang::current(cx).code();
                host.now_playing = now_playing;
                host.controllers = controllers;
                local_result = Some(
                    sandbox
                        .event(&event)
                        .map(|tree| worker::step(sandbox, tree)),
                );
            }
            _ => {}
        }
        #[cfg(test)]
        if let Some(result) = local_result {
            match result {
                Ok(step) => self.after_step(step, cx),
                Err(e) => self.fail(e, cx),
            }
        }
    }

    #[cfg(not(test))]
    fn listen(
        &self,
        rx: mpsc::Receiver<Result<worker::Step, Error>>,
        generation: u64,
        cx: &mut Context<Self>,
    ) {
        let rx = Arc::new(Mutex::new(rx));
        cx.spawn(async move |this, cx| {
            loop {
                let input = rx.clone();
                let result = cx
                    .background_executor()
                    .spawn(async move { input.lock().unwrap().recv() })
                    .await;
                let Ok(keep_listening) = this.update(cx, |v, cx| {
                    if v.generation != generation {
                        return false;
                    }
                    match result {
                        Ok(Ok(step)) => v.after_step(step, cx),
                        Ok(Err(e)) => v.fail(e, cx),
                        Err(_) => v.fail(Error::new("the nyapp worker stopped"), cx),
                    }
                    matches!(v.state, State::Running(_))
                }) else {
                    return;
                };
                if !keep_listening {
                    return;
                }
            }
        })
        .detach();
    }

    /// 呼び出しの間にニャプリが頼んだこと（周期・音）を反映して描き直す。
    fn after_step(&mut self, step: worker::Step, cx: &mut Context<Self>) {
        self.tree = step.view;
        let (ticker, beeps, activity) = (step.ticker, step.beeps, step.activity);
        if let Some(activity) = activity {
            crate::activity::set(&self.app, activity, cx);
        }
        match ticker {
            Some(0) => self.ticker = None,
            Some(ms) if self.ticker.as_ref().is_none_or(|(now, _)| *now != ms) => {
                let period = Duration::from_millis(ms.max(MIN_TICK_MS).into());
                let task = cx.spawn(async move |this, cx| {
                    loop {
                        cx.background_executor().timer(period).await;
                        if this.update(cx, |v, cx| v.send(Event::Tick, cx)).is_err() {
                            return;
                        }
                    }
                });
                self.ticker = Some((ms, task));
            }
            _ => {}
        }
        if beeps > 0 {
            command(
                self.panel,
                kernel::PanelCommand::Play(Sound::Wav(beep()), false),
                cx,
            );
        }
        self.redraw(cx);
    }

    /// ニャプリが落ちた（trap・燃料切れ・壊れた画面・起動できない）。パネルは閉じずに最後の画面を白黒で残し、次の描画で
    /// 子として強制終了のダイアログ（再起動・閉じる）を出す。本体とほかのパネルはそのまま動き続ける。
    fn fail(&mut self, error: Error, cx: &mut Context<Self>) {
        logger::warn!("nyapp stopped", app = self.app.manifest.id, error = error);
        let mut reason: String = error.to_string().chars().take(CRASH_REASON_CHARS).collect();
        if reason.chars().count() == CRASH_REASON_CHARS {
            reason.push('…');
        }
        self.state = State::Failed {
            reason,
            asked: false,
        };
        self.ticker = None;
        self.redraw(cx);
    }

    /// 強制終了のダイアログを、このパネルの子として出す（外側を押しても閉じない。開いている間このパネルは暗くなり押せない）。
    fn ask_after_crash(&mut self, window: &Window, cx: &mut Context<Self>) {
        let State::Failed { reason, asked } = &mut self.state else {
            return;
        };
        if std::mem::replace(asked, true) {
            return;
        }
        let name = self
            .app
            .manifest
            .name_in(Lang::current(cx).code())
            .to_owned();
        let title = i18n::tf(cx, "nyapp.crashed_title", &[("name", &name)]);
        let body = i18n::tf(cx, "nyapp.crashed_body", &[("reason", reason)]);
        let height = components::dialog_height(window, CRASH_DIALOG_WIDTH, &title, &body, 2);
        let (nyapp, view) = (self.panel, cx.weak_entity());
        let spec = PanelSpec::child(self.panel, true, move |panel, _, _| CrashDialog {
            panel,
            nyapp,
            view,
            title,
            body,
        })
        .without_dismiss()
        .sized(
            Vec2::new(CRASH_DIALOG_WIDTH as f32 / 1000.0, height as f32 / 1000.0),
            (CRASH_DIALOG_WIDTH, height),
        )
        // ダイアログは答えて閉じるもの。ナビゲーションバーは付けない。
        .without_nav_bar();
        cx.default_global::<PanelControl>().spawns.push(spec);
    }

    /// ポインタが動いた。画面にある面ごとに、その左上からの座標で知らせる。
    fn pointer_moved(&mut self, at: Point<Pixels>, cx: &mut Context<Self>) {
        if self
            .pointer
            .is_some_and(|p| (p.x - at.x).abs() < px(1.0) && (p.y - at.y).abs() < px(1.0))
        {
            return;
        }
        self.pointer = Some(at);
        let canvases: Vec<_> = self
            .canvases
            .borrow()
            .iter()
            .map(|(id, b)| (id.clone(), b.get().origin))
            .collect();
        for (id, origin) in canvases {
            let (x, y) = (f32::from(at.x - origin.x), f32::from(at.y - origin.y));
            self.send(Event::Pointer { id, x, y }, cx);
        }
    }

    fn redraw(&self, cx: &mut Context<Self>) {
        cx.notify();
        cx.default_global::<PanelControl>()
            .panels
            .entry(self.panel)
            .or_default()
            .redraw = true;
    }

    /// 画面の木の要素 1 つを描く。`path` は要素の場所（スクロールの状態を要素ごとに分ける ID）、`in_row` は横の並びの中か
    /// （文字を折り返さず 1 行で省略する）。どの要素も中身より狭く縮められ、長い文字・ラベルは省略してはみ出さない。
    fn node(
        &self,
        node: &Node,
        path: &str,
        in_row: bool,
        theme: &Theme,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        // 落ちた画面は白黒（テーマの色は白黒のテーマで、`Rgb` の色はここで灰色にする）。
        let gray = matches!(self.state, State::Failed { .. });
        let color = |c: Color| {
            let rgba = color(c, theme);
            if gray { design::grayscale(rgba) } else { rgba }
        };
        match node {
            Node::Column(s) => self.stack(div().flex().flex_col(), s, path, false, theme, cx),
            Node::Row(s) => self.stack(div().flex().flex_row(), s, path, true, theme, cx),
            Node::Card(s) => self.stack(
                components::card(theme).flex().flex_col().p(px(spacing::LG)),
                s,
                path,
                false,
                theme,
                cx,
            ),
            Node::Grid {
                columns,
                gap: g,
                children,
            } => div()
                .grid()
                .grid_cols(*columns)
                .gap(px(gap(*g)))
                .children(self.children(children, path, false, theme, cx))
                .into_any_element(),
            Node::Text(t) => div()
                .min_w_0()
                .when(in_row, |d| d.truncate())
                .text_size(px(match t.size {
                    Size::Sm => font_size::SM,
                    Size::Md => font_size::MD,
                    Size::Lg => font_size::LG,
                    Size::Xl => XL_PX,
                    Size::Display => DISPLAY_PX,
                }))
                .when(t.bold, |d| d.font_weight(FontWeight::BOLD))
                .text_color(rgba(color(t.color)))
                .child(t.text.clone())
                .into_any_element(),
            Node::Button(b) => {
                let id = b.id.clone();
                let variant = match b.variant {
                    Variant::Default => ButtonVariant::Default,
                    Variant::Primary => ButtonVariant::Primary,
                    Variant::Danger => ButtonVariant::Danger,
                    Variant::Ghost => ButtonVariant::Ghost,
                };
                components::button(theme, b.id.clone(), b.label.clone(), variant)
                    .selected(b.selected)
                    // ラベルが長くても押せる範囲を広げない（`grow` どうしは同じ幅に分け、はみ出す分は省略する）。
                    .min_w_0()
                    .truncate()
                    .when(b.grow, |b| b.flex_1())
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.send(Event::Click { id: id.clone() }, cx)
                    }))
                    .into_any_element()
            }
            Node::Switch { id, on } => {
                let (id, on) = (id.clone(), *on);
                components::switch(theme, id.clone(), on)
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.send(
                            Event::Toggle {
                                id: id.clone(),
                                on: !on,
                            },
                            cx,
                        )
                    }))
                    .into_any_element()
            }
            Node::Canvas(c) => {
                let bounds = self
                    .canvases
                    .borrow_mut()
                    .entry(c.id.clone())
                    .or_default()
                    .clone();
                let id = c.id.clone();
                div()
                    .debug_selector(move || id)
                    .relative()
                    .flex_none()
                    .w(px(c.width))
                    .h(px(c.height))
                    .overflow_hidden()
                    // 描いた場所を覚える（ポインタの座標を面の左上からに直すため）。
                    .child(
                        gpui::canvas(move |b, _, _| bounds.set(b), |_, _, _, _| {})
                            .absolute()
                            .size_full(),
                    )
                    .children(c.shapes.iter().map(|s| shape(s, &color)))
                    .into_any_element()
            }
            Node::Spacer => div().flex_1().into_any_element(),
        }
    }

    fn stack(
        &self,
        base: impl gpui::Element + Styled + ParentElement + InteractiveElement + IntoElement,
        s: &Stack,
        path: &str,
        row: bool,
        theme: &Theme,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let d = base
            .min_w_0()
            .gap(px(gap(s.gap)))
            .when(s.grow, |d| d.flex_1().min_h_0().min_w_0());
        let d = match s.align {
            Align::Stretch => d,
            Align::Start => d.items_start(),
            Align::Center => d.items_center(),
            Align::End => d.items_end(),
        };
        let d = d.children(self.children(&s.children, path, row, theme, cx));
        if s.scroll {
            d.id(gpui::SharedString::from(format!("nyapp-scroll-{path}")))
                .overflow_y_scroll()
                .into_any_element()
        } else {
            d.into_any_element()
        }
    }

    fn children(
        &self,
        children: &[Node],
        path: &str,
        in_row: bool,
        theme: &Theme,
        cx: &mut Context<Self>,
    ) -> Vec<AnyElement> {
        children
            .iter()
            .enumerate()
            .map(|(i, n)| self.node(n, &format!("{path}.{i}"), in_row, theme, cx))
            .collect()
    }
}

impl Render for NyappView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        self.ask_after_crash(window, cx);
        let theme = match self.state {
            State::Failed { .. } => Theme::current(cx).grayscale(),
            _ => Theme::current(cx),
        };
        let message =
            |text: gpui::SharedString| div().text_color(rgba(theme.neutral_text_low)).child(text);
        match &self.state {
            State::Loading => components::card_panel(&theme)
                .flex()
                .items_center()
                .justify_center()
                .child(message(i18n::t(cx, "nyapp.loading"))),
            // 起動できずに落ちたものは、出す画面が無い。
            State::Failed { .. } if self.tree == Node::Spacer => components::card_panel(&theme),
            _ => {
                // 今の画面にある面だけを数え直す。
                self.canvases.borrow_mut().clear();
                let tree = self.node(&self.tree, "0", false, &theme, cx);
                components::panel(&theme)
                    .flex()
                    .flex_col()
                    .on_mouse_move(cx.listener(|this, e: &MouseMoveEvent, _, cx| {
                        this.pointer_moved(e.position, cx)
                    }))
                    .child(div().size_full().flex().flex_col().child(tree))
            }
        }
    }
}

/// `panel` をレーザーで指している手のコントローラの入力だけ。指していなければ空（パネルの外の入力はニャプリに見せない）。
fn pointed_controllers(ctl: &PanelControl, panel: PanelId) -> Vec<Controller> {
    let Some(state) = ctl.panels.get(&panel) else {
        return Vec::new();
    };
    ctl.controllers
        .iter()
        .filter(|c| state.pointing.contains(&c.hand))
        .map(|c| Controller {
            hand: match c.hand {
                kernel::vr::Hand::Left => nyapp_sdk::Hand::Left,
                kernel::vr::Hand::Right => nyapp_sdk::Hand::Right,
            },
            select: c.select,
            trigger: c.trigger,
            grab: c.grab,
            menu: c.menu,
            shortcut: c.shortcut,
            scroll: (c.scroll.x, c.scroll.y),
        })
        .collect()
}

fn gap(gap: Gap) -> f32 {
    match gap {
        Gap::None => 0.0,
        Gap::Xs => spacing::XS,
        Gap::Sm => spacing::SM,
        Gap::Md => spacing::MD,
        Gap::Lg => spacing::LG,
        Gap::Xl => spacing::XL,
    }
}

/// 色の役割をテーマの色（0xRRGGBBAA）にする。
fn color(color: Color, theme: &Theme) -> u32 {
    match color {
        Color::Text => theme.neutral_text,
        Color::Low => theme.neutral_text_low,
        Color::Accent => theme.accent_solid,
        Color::Danger => theme.danger_solid,
        Color::OnAccent => theme.on_accent,
        Color::Card => theme.card,
        Color::Background => theme.background,
        Color::Rgb(rgb) => (rgb << 8) | 0xff,
    }
}

fn shape(shape: &Shape, color: &dyn Fn(Color) -> u32) -> Div {
    match *shape {
        Shape::Circle { x, y, r, color: c } => div()
            .absolute()
            .left(px(x - r))
            .top(px(y - r))
            .size(px(2.0 * r))
            .rounded(px(radius::FULL))
            .bg(rgba(color(c))),
        Shape::Rect {
            x,
            y,
            w,
            h,
            radius: r,
            color: c,
        } => div()
            .absolute()
            .left(px(x))
            .top(px(y))
            .w(px(w))
            .h(px(h))
            .rounded(px(r))
            .bg(rgba(color(c))),
    }
}

/// 強制終了のダイアログの幅（px。1000px/m）。高さは文に合わせて測る。理由は [`CRASH_REASON_CHARS`] 文字で切る。
const CRASH_DIALOG_WIDTH: u32 = 440;
const CRASH_REASON_CHARS: usize = 120;

/// ニャプリが強制終了したことを知らせるダイアログ（落ちたニャプリのパネルの子）。再起動（`nyapp-crash-restart`。同じパネルで
/// 起こし直す）か閉じる（`nyapp-crash-close`。ニャプリのパネルごと閉じる）。
pub struct CrashDialog {
    panel: PanelId,
    /// 落ちたニャプリのパネルと、そのビュー。
    nyapp: PanelId,
    view: gpui::WeakEntity<NyappView>,
    title: gpui::SharedString,
    body: gpui::SharedString,
}

impl Render for CrashDialog {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = Theme::current(cx);
        let (panel, nyapp) = (self.panel, self.nyapp);
        components::dialog(
            &theme,
            "nyapp-crash-dialog",
            &icons::WARNING,
            self.title.clone(),
            self.body.clone(),
            [
                components::button(
                    &theme,
                    "nyapp-crash-restart",
                    i18n::t(cx, "nyapp.restart"),
                    ButtonVariant::Primary,
                )
                .on_click(cx.listener(move |this, _, _, cx| {
                    command(panel, kernel::PanelCommand::Close, cx);
                    let _ = this.view.update(cx, |v, cx| v.load(cx));
                })),
                // 子は親と一緒に閉じる。
                components::button(
                    &theme,
                    "nyapp-crash-close",
                    i18n::t(cx, "nyapp.close"),
                    ButtonVariant::Default,
                )
                .on_click(move |_, _, cx| command(nyapp, kernel::PanelCommand::Close, cx)),
            ],
        )
    }
}

fn command(panel: PanelId, command: kernel::PanelCommand, cx: &mut App) {
    cx.default_global::<PanelControl>()
        .panels
        .entry(panel)
        .or_default()
        .commands
        .push(command);
}

/// 知らせの音（880Hz・0.2 秒、終わりを弱めた正弦波の WAV）。最初に使ったときに作る。
pub(crate) fn beep() -> &'static [u8] {
    static WAV: OnceLock<Vec<u8>> = OnceLock::new();
    WAV.get_or_init(|| {
        const RATE: u32 = 48_000;
        let n = RATE / 5;
        let samples: Vec<i16> = (0..n)
            .map(|i| {
                let t = i as f32 / RATE as f32;
                let fade = 1.0 - i as f32 / n as f32;
                ((t * 880.0 * std::f32::consts::TAU).sin() * fade * 0.4 * i16::MAX as f32) as i16
            })
            .collect();
        let data = (samples.len() * 2) as u32;
        let mut wav = Vec::with_capacity(44 + data as usize);
        wav.extend_from_slice(b"RIFF");
        wav.extend_from_slice(&(36 + data).to_le_bytes());
        wav.extend_from_slice(b"WAVEfmt ");
        // fmt: PCM・モノラル・RATE Hz・16 ビット
        for v in [16u32, 1 | (1 << 16), RATE, RATE * 2, 2 | (16 << 16)] {
            wav.extend_from_slice(&v.to_le_bytes());
        }
        wav.extend_from_slice(b"data");
        wav.extend_from_slice(&data.to_le_bytes());
        for s in samples {
            wav.extend_from_slice(&s.to_le_bytes());
        }
        wav
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use gpui::{Modifiers, TestAppContext, VisualTestContext};
    use kernel::PanelCommand;

    /// 押すと trap する（`unreachable`）ボタン `boom` だけのニャプリ。WebAssembly のテキスト形式（テストでだけ wasmi の
    /// `wat` が有効）をパッケージにして入れたものとして返す。
    fn crashing_app(name: &str, wasm: Option<&str>) -> Installed {
        app_with_view(
            name,
            r#"{"type":"button","id":"boom","label":"Boom"}"#,
            wasm,
        )
    }

    /// 画面 `view`（JSON）を返し、出来事を受けると trap するニャプリ（`wasm` を渡せばそれ）。
    fn app_with_view(name: &str, view: &str, wasm: Option<&str>) -> Installed {
        let wat = format!(
            r#"(module
                (memory (export "memory") 1)
                (data (i32.const 16) "{view}")
                (func (export "nyapp_alloc") (param i32) (result i32) (i32.const 1024))
                (func (export "nyapp_init"))
                (func (export "nyapp_view") (result i64)
                    (i64.or (i64.shl (i64.const 16) (i64.const 32)) (i64.const {len})))
                (func (export "nyapp_event") (param i32 i32) unreachable))"#,
            view = view.replace('"', "\\\""),
            len = view.len(),
        );
        let dir = std::env::temp_dir().join(format!("nyapp-view-{name}-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("net.bettaworx.crash.nyapp");
        let manifest = "id = \"net.bettaworx.crash\"
name = \"Crash\"
version = \"1\"
api = 1
";
        let manifest = package::write(
            &path,
            manifest,
            wasm.unwrap_or(&wat).as_bytes(),
            &package::Icon::default(),
        )
        .unwrap();
        Installed {
            manifest,
            path,
            icon: Default::default(),
        }
    }

    fn click(cx: &mut VisualTestContext, selector: &'static str) {
        let at = cx.debug_bounds(selector).unwrap().center();
        cx.simulate_click(at, Modifiers::none());
    }

    fn commands(cx: &mut VisualTestContext, panel: PanelId) -> Vec<PanelCommand> {
        cx.update(|_, cx| {
            std::mem::take(
                &mut cx
                    .default_global::<PanelControl>()
                    .panels
                    .entry(panel)
                    .or_default()
                    .commands,
            )
        })
    }

    /// 文字・ボタンのラベルが長くても、並びからはみ出さず（省略する）、`grow` のボタンはラベルの長さによらず同じ幅
    /// （押せる範囲がラベルで変わらない）。
    #[gpui::test]
    fn long_texts_are_truncated_and_grown_buttons_share_the_width(cx: &mut TestAppContext) {
        let long = "とても長いラベル".repeat(20);
        let view = format!(
            r#"{{"type":"column","children":[
                {{"type":"row","children":[
                    {{"type":"button","id":"tab-a","label":"A","grow":true}},
                    {{"type":"button","id":"tab-b","label":"{long}","grow":true}}]}},
                {{"type":"row","children":[
                    {{"type":"text","text":"{long}"}},
                    {{"type":"button","id":"after","label":"OK"}}]}}]}}"#
        );
        let app = app_with_view("truncate", &view.replace('\n', ""), None);
        let (_, cx) = cx.add_window_view(|_, cx| NyappView::new(5, app, cx));
        cx.run_until_parked();
        let [a, b, after] = ["tab-a", "tab-b", "after"].map(|s| cx.debug_bounds(s).unwrap());
        let width = cx.update(|window, _| window.viewport_size().width);
        assert!(
            (a.size.width - b.size.width).abs() < px(1.0),
            "{a:?} / {b:?}"
        );
        assert!(
            b.right() <= width,
            "長いラベルのボタンもパネルに収まる: {b:?}"
        );
        assert!(
            after.right() <= width,
            "長い文字の後ろのボタンも押せる所にある: {after:?}"
        );
    }

    /// コントローラの入力が見えるのは、レーザーでそのパネルを指している手だけ（指していなければ何も見えない）。
    #[test]
    fn controllers_are_visible_only_while_pointing_at_the_panel() {
        let state = |hand, select| kernel::ControllerState {
            hand,
            aim: kernel::Pose::IDENTITY,
            select,
            click_button: Default::default(),
            trigger: 1.0,
            grab: false,
            menu: false,
            shortcut: false,
            scroll: Vec2::new(0.5, -0.5),
            resize: 0.0,
            facing: None,
            metrics: None,
        };
        let mut ctl = PanelControl::default();
        ctl.controllers = vec![
            state(kernel::vr::Hand::Left, false),
            state(kernel::vr::Hand::Right, true),
        ];
        assert!(
            pointed_controllers(&ctl, 5).is_empty(),
            "パネルの状態が無い"
        );
        ctl.panels.entry(5).or_default();
        assert!(pointed_controllers(&ctl, 5).is_empty(), "指していない");
        ctl.panels.entry(5).or_default().pointing = vec![kernel::vr::Hand::Right];
        let seen = pointed_controllers(&ctl, 5);
        assert_eq!(seen.len(), 1);
        assert_eq!(seen[0].hand, nyapp_sdk::Hand::Right);
        assert!(seen[0].select && seen[0].scroll == (0.5, -0.5));
        assert!(
            pointed_controllers(&ctl, 6).is_empty(),
            "別のパネルを指している手の入力は見えない"
        );
    }

    fn spawned(cx: &mut VisualTestContext) -> usize {
        cx.update(|_, cx| cx.default_global::<PanelControl>().spawns.len())
    }

    /// ニャプリが trap すると、本体は落ちず、パネルも閉じずに白黒で残り、その子として強制終了のダイアログを 1 枚出す。
    #[gpui::test]
    fn a_crashing_nyapp_stays_gray_and_asks_with_a_child_dialog(cx: &mut TestAppContext) {
        let app = crashing_app("trap", None);
        let (view, cx) = cx.add_window_view(|_, cx| NyappView::new(5, app, cx));
        cx.run_until_parked();
        assert!(view.read_with(cx, |v, _| matches!(v.state, State::TestRunning(_))));
        click(cx, "boom");
        cx.run_until_parked();
        assert!(view.read_with(cx, |v, _| matches!(
            v.state,
            State::Failed { asked: true, .. }
        )));
        assert!(
            !commands(cx, 5).contains(&PanelCommand::Close),
            "パネルは閉じない"
        );
        assert!(cx.debug_bounds("boom").is_some(), "最後の画面を残す");
        let parent = cx.update(|_, cx| cx.default_global::<PanelControl>().spawns[0].parent());
        assert_eq!(
            parent.map(|p| (p.panel, p.dismiss)),
            Some((5, false)),
            "外を押しても閉じない子"
        );
        assert_eq!(spawned(cx), 1);
        // 描き直してもダイアログは増えない。
        cx.update(|window, _| window.refresh());
        cx.run_until_parked();
        assert_eq!(spawned(cx), 1);
    }

    /// 起動できない（wasm として読めない）ものも同じく残して知らせる。
    #[gpui::test]
    fn a_broken_package_is_reported_like_a_crash(cx: &mut TestAppContext) {
        let app = crashing_app("broken", Some("not wasm"));
        let (view, cx) = cx.add_window_view(|_, cx| NyappView::new(5, app, cx));
        cx.run_until_parked();
        assert!(view.read_with(cx, |v, _| matches!(v.state, State::Failed { .. })));
        assert!(!commands(cx, 5).contains(&PanelCommand::Close));
        assert_eq!(spawned(cx), 1);
    }

    /// 再起動はダイアログを閉じて同じパネルでニャプリを起こし直し、閉じるはニャプリのパネルを閉じる（子のダイアログも一緒に）。
    #[gpui::test]
    fn the_crash_dialog_restarts_in_place_or_closes_the_nyapp(cx: &mut TestAppContext) {
        let app = crashing_app("dialog", None);
        let (view, nyapp) = cx.add_window_view(|_, cx| NyappView::new(5, app, cx));
        nyapp.run_until_parked();
        click(nyapp, "boom");
        nyapp.run_until_parked();
        let weak = view.downgrade();
        let (_, dialog) = cx.add_window_view(move |_, _| CrashDialog {
            panel: 9,
            nyapp: 5,
            view: weak,
            title: "t".into(),
            body: "b".into(),
        });
        click(dialog, "nyapp-crash-restart");
        dialog.run_until_parked();
        assert!(
            commands(dialog, 9).contains(&PanelCommand::Close),
            "ダイアログは閉じる"
        );
        assert!(
            !commands(dialog, 5).contains(&PanelCommand::Close),
            "ニャプリは閉じない"
        );
        assert!(
            view.read_with(dialog, |v, _| matches!(v.state, State::TestRunning(_))),
            "同じパネルで起こし直す"
        );
        click(dialog, "nyapp-crash-close");
        assert!(
            commands(dialog, 5).contains(&PanelCommand::Close),
            "ニャプリのパネルを閉じる"
        );
    }
}
