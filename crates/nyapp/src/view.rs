//! ニャプリのパネル: サンドボックスが返す画面の木（[`Node`]）を `design` の部品で描き、押された・指されたなどを
//! 出来事としてニャプリへ返す。ニャプリが落ちたら（trap・燃料切れ・壊れた画面）理由と再起動のボタンを出す。

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::rc::Rc;
use std::sync::OnceLock;
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
use kernel::vr::{Pose, Quat, Vec2, Vec3};
use nyapp_sdk::{Align, Color, Event, Gap, Node, Shape, Size, Stack, Variant};
use ui::{PanelControl, PanelSpec};

use crate::{Error, Installed, Sandbox, installed, package};

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
    Running(Box<Sandbox>),
    /// 落ちた（パネルは閉じていく。知らせるのはダイアログ）。
    Failed,
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
        };
        this.load(cx);
        this
    }

    /// パッケージを読み、サンドボックスを起こす（別スレッド）。再起動もこれ。
    fn load(&mut self, cx: &mut Context<Self>) {
        self.state = State::Loading;
        self.ticker = None;
        let path = self.app.path.clone();
        let language = Lang::current(cx).code();
        cx.spawn(async move |this, cx| {
            let loaded = cx
                .background_executor()
                .spawn(async move {
                    let (manifest, wasm) = package::read(&path)?;
                    Sandbox::load(&manifest, &wasm, installed::appdata(&manifest.id), language)
                })
                .await;
            let _ = this.update(cx, |v, cx| match loaded {
                Ok((sandbox, tree)) => {
                    v.state = State::Running(Box::new(sandbox));
                    v.tree = tree;
                    v.after_call(cx);
                }
                Err(e) => v.fail(e, cx),
            });
        })
        .detach();
        self.redraw(cx);
    }

    /// 出来事をニャプリへ渡し、返ってきた画面に替える。
    fn send(&mut self, event: Event, cx: &mut Context<Self>) {
        let State::Running(sandbox) = &mut self.state else {
            return;
        };
        let host = sandbox.host();
        host.language = Lang::current(cx).code();
        host.now_playing = cx.default_global::<PanelControl>().media.as_ref().map(|m| {
            if m.artist.is_empty() {
                m.title.clone()
            } else {
                format!("{} - {}", m.title, m.artist)
            }
        });
        match sandbox.event(&event) {
            Ok(tree) => {
                self.tree = tree;
                self.after_call(cx);
            }
            Err(e) => self.fail(e, cx),
        }
    }

    /// 呼び出しの間にニャプリが頼んだこと（周期・音）を反映して描き直す。
    fn after_call(&mut self, cx: &mut Context<Self>) {
        let State::Running(sandbox) = &mut self.state else {
            return;
        };
        let host = sandbox.host();
        let (ticker, beeps) = (host.ticker.take(), std::mem::take(&mut host.beeps));
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

    /// ニャプリが落ちた（trap・燃料切れ・壊れた画面・起動できない）。パネルを閉じ、視界の中央に強制終了を知らせる
    /// ダイアログ（再起動・閉じる）を出す。本体とほかのパネルはそのまま動き続ける。
    fn fail(&mut self, error: Error, cx: &mut Context<Self>) {
        logger::warn!("nyapp stopped", app = self.app.manifest.id, error = error);
        self.state = State::Failed;
        self.ticker = None;
        let docked = ui::docked(cx) == Some(self.panel);
        command(self.panel, kernel::PanelCommand::Close, cx);
        let spec = crash_dialog_spec(self.panel, self.app.clone(), error.to_string(), docked, cx);
        cx.default_global::<PanelControl>().spawns.push(spec);
        self.redraw(cx);
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

    /// 画面の木の要素 1 つを描く。`path` は要素の場所（スクロールの状態を要素ごとに分ける ID）。
    fn node(&self, node: &Node, path: &str, theme: &Theme, cx: &mut Context<Self>) -> AnyElement {
        match node {
            Node::Column(s) => self.stack(div().flex().flex_col(), s, path, theme, cx),
            Node::Row(s) => self.stack(div().flex().flex_row(), s, path, theme, cx),
            Node::Card(s) => self.stack(
                components::card(theme).flex().flex_col().p(px(spacing::LG)),
                s,
                path,
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
                .children(self.children(children, path, theme, cx))
                .into_any_element(),
            Node::Text(t) => div()
                .text_size(px(match t.size {
                    Size::Sm => font_size::SM,
                    Size::Md => font_size::MD,
                    Size::Lg => font_size::LG,
                    Size::Xl => XL_PX,
                    Size::Display => DISPLAY_PX,
                }))
                .when(t.bold, |d| d.font_weight(FontWeight::BOLD))
                .text_color(rgba(color(t.color, theme)))
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
                    .children(c.shapes.iter().map(|s| shape(s, theme)))
                    .into_any_element()
            }
            Node::Spacer => div().flex_1().into_any_element(),
        }
    }

    fn stack(
        &self,
        base: Div,
        s: &Stack,
        path: &str,
        theme: &Theme,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let d = base
            .gap(px(gap(s.gap)))
            .when(s.grow, |d| d.flex_1().min_h_0().min_w_0());
        let d = match s.align {
            Align::Stretch => d,
            Align::Start => d.items_start(),
            Align::Center => d.items_center(),
            Align::End => d.items_end(),
        };
        let d = d.children(self.children(&s.children, path, theme, cx));
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
        theme: &Theme,
        cx: &mut Context<Self>,
    ) -> Vec<AnyElement> {
        children
            .iter()
            .enumerate()
            .map(|(i, n)| self.node(n, &format!("{path}.{i}"), theme, cx))
            .collect()
    }
}

impl Render for NyappView {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = Theme::current(cx);
        let message =
            |text: gpui::SharedString| div().text_color(rgba(theme.neutral_text_low)).child(text);
        match &self.state {
            State::Loading => components::card_panel(&theme)
                .flex()
                .items_center()
                .justify_center()
                .child(message(i18n::t(cx, "nyapp.loading"))),
            State::Failed => components::card_panel(&theme),
            State::Running(_) => {
                // 今の画面にある面だけを数え直す。
                self.canvases.borrow_mut().clear();
                let tree = self.node(&self.tree, "0", &theme, cx);
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

fn shape(shape: &Shape, theme: &Theme) -> Div {
    match *shape {
        Shape::Circle { x, y, r, color: c } => div()
            .absolute()
            .left(px(x - r))
            .top(px(y - r))
            .size(px(2.0 * r))
            .rounded(px(radius::FULL))
            .bg(rgba(color(c, theme))),
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
            .bg(rgba(color(c, theme))),
    }
}

/// 強制終了のダイアログの幅と高さ（px。1000px/m）。理由は長くても [`CRASH_REASON_CHARS`] 文字で切るので、高さは決め打ち。
const CRASH_DIALOG_PX: (u32, u32) = (440, 380);
const CRASH_REASON_CHARS: usize = 120;
/// 強制終了のダイアログ・再起動したパネルを出す所（開いた瞬間の頭から）。
const CRASH_DIALOG_POSE: Pose = Pose::new(Vec3::new(0.0, 0.0, -1.0), Quat::IDENTITY);
const RESTART_POSE: Pose = Pose::new(Vec3::new(0.0, 0.0, -1.1), Quat::IDENTITY);

/// ニャプリ `app` が強制終了したことを知らせるダイアログの起動仕様。`docked` なら、再起動したものもダッシュボードにはめる。
fn crash_dialog_spec(
    from: PanelId,
    app: Installed,
    reason: String,
    docked: bool,
    cx: &App,
) -> PanelSpec {
    let lang = Lang::current(cx);
    let mut reason: String = reason.chars().take(CRASH_REASON_CHARS).collect();
    if reason.chars().count() == CRASH_REASON_CHARS {
        reason.push('…');
    }
    PanelSpec::spawn(from, move |panel, _, _| CrashDialog {
        panel,
        app,
        reason,
        lang,
        docked,
    })
    .sized(
        Vec2::new(
            CRASH_DIALOG_PX.0 as f32 / 1000.0,
            CRASH_DIALOG_PX.1 as f32 / 1000.0,
        ),
        CRASH_DIALOG_PX,
    )
    // 開いた元のパネルは閉じていくので、その手前ではなく視界の中央に出す。
    .in_view(CRASH_DIALOG_POSE)
    // ダイアログは答えて閉じるもの。ナビゲーションバーは付けない。
    .without_nav_bar()
}

/// ニャプリが強制終了したことを知らせるダイアログ。再起動（`nyapp-crash-restart`）か閉じる（`nyapp-crash-dismiss`）。
pub struct CrashDialog {
    panel: PanelId,
    app: Installed,
    reason: String,
    lang: Lang,
    docked: bool,
}

impl CrashDialog {
    /// 閉じて、ニャプリを新しいパネルで開き直す。
    fn restart(&mut self, cx: &mut App) {
        command(self.panel, kernel::PanelCommand::Close, cx);
        let docked = self.docked;
        let spec = spec(self.panel, self.app.clone(), self.lang, move |id, cx| {
            if docked {
                ui::dock(id, cx);
            }
        })
        // 開いた元のダイアログは閉じていくので、目の前に出す。はめるならダッシュボードの所へ動く。
        .in_view(RESTART_POSE)
        .dockable();
        cx.default_global::<PanelControl>().spawns.push(spec);
    }
}

impl Render for CrashDialog {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = Theme::current(cx);
        let name = self.app.manifest.name_in(self.lang.code()).to_owned();
        let panel = self.panel;
        components::dialog(
            &theme,
            "nyapp-crash-dialog",
            &icons::WARNING,
            i18n::tf(cx, "nyapp.crashed_title", &[("name", &name)]),
            i18n::tf(cx, "nyapp.crashed_body", &[("reason", &self.reason)]),
            [
                components::button(
                    &theme,
                    "nyapp-crash-restart",
                    i18n::t(cx, "nyapp.restart"),
                    ButtonVariant::Primary,
                )
                .on_click(cx.listener(|this, _, _, cx| this.restart(cx))),
                components::button(
                    &theme,
                    "nyapp-crash-dismiss",
                    i18n::t(cx, "nyapp.dismiss"),
                    ButtonVariant::Default,
                )
                .on_click(move |_, _, cx| command(panel, kernel::PanelCommand::Close, cx)),
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
fn beep() -> &'static [u8] {
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
        let view = r#"{"type":"button","id":"boom","label":"Boom"}"#;
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
        let manifest =
            package::write(&path, manifest, wasm.unwrap_or(&wat).as_bytes(), None).unwrap();
        Installed {
            manifest,
            path,
            icon: None,
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

    /// ニャプリが trap すると、本体は落ちずにパネルを閉じ、強制終了のダイアログを 1 枚出す。
    #[gpui::test]
    fn a_crashing_nyapp_closes_its_panel_and_shows_the_crash_dialog(cx: &mut TestAppContext) {
        let app = crashing_app("trap", None);
        let (view, cx) = cx.add_window_view(|_, cx| NyappView::new(5, app, cx));
        cx.run_until_parked();
        assert!(view.read_with(cx, |v, _| matches!(v.state, State::Running(_))));
        click(cx, "boom");
        cx.run_until_parked();
        assert!(view.read_with(cx, |v, _| matches!(v.state, State::Failed)));
        assert!(
            commands(cx, 5).contains(&PanelCommand::Close),
            "パネルを閉じる"
        );
        let spawns = cx.update(|_, cx| cx.default_global::<PanelControl>().spawns.len());
        assert_eq!(spawns, 1, "強制終了のダイアログ");
    }

    /// 起動できない（wasm として読めない）ものも同じく閉じて知らせる。
    #[gpui::test]
    fn a_broken_package_is_reported_like_a_crash(cx: &mut TestAppContext) {
        let app = crashing_app("broken", Some("not wasm"));
        let (view, cx) = cx.add_window_view(|_, cx| NyappView::new(5, app, cx));
        cx.run_until_parked();
        assert!(view.read_with(cx, |v, _| matches!(v.state, State::Failed)));
        assert!(commands(cx, 5).contains(&PanelCommand::Close));
        let spawns = cx.update(|_, cx| cx.default_global::<PanelControl>().spawns.len());
        assert_eq!(spawns, 1);
    }

    /// 強制終了のダイアログ（PanelId 9）を開く。
    fn crash_dialog(cx: &mut TestAppContext) -> &mut VisualTestContext {
        let app = crashing_app("dialog", None);
        cx.add_window_view(|_, _| CrashDialog {
            panel: 9,
            app,
            reason: "wasm trap: unreachable".into(),
            lang: Lang::Ja,
            docked: false,
        })
        .1
    }

    /// ダイアログの再起動はダイアログを閉じてニャプリを開き直し、閉じるはダイアログを閉じるだけ。
    #[gpui::test]
    fn the_crash_dialog_restarts_or_dismisses(cx: &mut TestAppContext) {
        let cx1 = crash_dialog(cx);
        click(cx1, "nyapp-crash-restart");
        assert!(commands(cx1, 9).contains(&PanelCommand::Close));
        let spawns = cx1.update(|_, cx| cx.default_global::<PanelControl>().spawns.len());
        assert_eq!(spawns, 1, "ニャプリを開き直す");

        let cx2 = crash_dialog(cx);
        cx2.update(|_, cx| cx.default_global::<PanelControl>().spawns.clear());
        click(cx2, "nyapp-crash-dismiss");
        assert!(commands(cx2, 9).contains(&PanelCommand::Close));
        let spawns = cx2.update(|_, cx| cx.default_global::<PanelControl>().spawns.len());
        assert_eq!(spawns, 0, "閉じるだけ");
    }

    /// 知らせの音は RIFF の WAV として長さが合っている。
    #[test]
    fn beep_is_a_well_formed_wav() {
        let wav = beep();
        assert_eq!(&wav[..4], b"RIFF");
        assert_eq!(
            u32::from_le_bytes(wav[4..8].try_into().unwrap()) as usize,
            wav.len() - 8
        );
        assert_eq!(&wav[36..40], b"data");
    }
}
