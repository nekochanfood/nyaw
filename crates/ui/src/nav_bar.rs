//! パネルの下に付くナビゲーションバー（[`NavBar`]）と、パネルの中身を包む根（[`PanelRoot`]）。

use super::*;

pub(crate) struct NavBar {
    pub(crate) id: PanelId,
    pub(crate) owner: PanelId,
    pub(crate) title: Option<Title>,
    /// 左端に OS キーボードのトグルを出す。
    pub(crate) os_keyboard: bool,
    /// 左端に、はめる場所から外す・戻すボタンを出す。
    pub(crate) dockable: bool,
    /// 右端を、閉じる代わりにしまうボタンにする。
    pub(crate) minimize: Option<Minimize>,
    pub(crate) source_minimize: Option<Minimize>,
    /// 広がり具合（0 = 線、1 = 帯）と、向かっている先・その動き。
    pub(crate) open: design::animation::Transition,
}

impl Render for NavBar {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let now = design::drive::now();
        // 指されているかは `sync_panel` が状態に書き、変わればこのバーを描き直す。
        let state = cx
            .default_global::<PanelControl>()
            .panels
            .entry(self.id)
            .or_default();
        let to = if state.status.pointed { 1.0 } else { 0.0 };
        // 広がるのも縮むのも落ち着いて動く（ease-in-out）。
        let animated = cx.has_global::<design::Motion>();
        self.open
            .go(to, design::animation::Motion::Morph, now, animated);
        let (open, moving) = self.open.at(now);
        // 動いている間だけ毎フレーム描き直す。
        cx.default_global::<PanelControl>()
            .panels
            .entry(self.id)
            .or_default()
            .live = moving;
        let owner = self.owner;
        let title = match &self.title {
            Some(Title::Key(k)) => i18n::t(cx, k),
            Some(Title::Text(t)) => t.clone(),
            None => Default::default(),
        };
        let theme = design::Theme::current(cx);
        // OS キーボードのトグル。出ている間（持ち主に向いたキーボードがある）は強調色。
        // キーボードの出入りでは `keyboard` がこのバーを描き直させるので、キーボード自身の X で閉じても色が戻る。
        let os_keyboard = self.os_keyboard.then(|| {
            let on = keyboard::keyboard_visible(owner, cx);
            design::components::icon_button(
                &theme,
                "nav-keyboard",
                &design::icons::KEYBOARD,
                i18n::t(cx, "keyboard.os_button"),
                if on {
                    design::components::Variant::Primary
                } else {
                    design::components::Variant::Default
                },
            )
            .on_click(move |_, _, cx| {
                keyboard::toggle_os_keyboard(owner, cx);
            })
        });
        // はめる場所から外す（はまっている間）・戻す（外れていて、場所が開いている間）。
        let dock = self.dockable.then(|| {
            let ctl = cx.global::<PanelControl>();
            let parent = ctl.panels.get(&owner).and_then(|p| p.status.parent);
            let slot = dock_slot(cx);
            let inside = parent.is_some() && parent == slot.map(|(o, _)| o);
            (inside || slot.is_some()).then(|| {
                let (id, glyph, tip) = if inside {
                    (
                        "nav-undock",
                        &design::icons::ARROW_SQUARE_OUT,
                        i18n::t(cx, "dock.undock"),
                    )
                } else {
                    (
                        "nav-dock",
                        &design::icons::CARET_LINE_DOWN,
                        i18n::t(cx, "dock.dock"),
                    )
                };
                design::components::icon_button(
                    &theme,
                    id,
                    glyph,
                    tip,
                    design::components::Variant::Default,
                )
                .on_click(move |_, _, cx| {
                    if inside {
                        undock(owner, cx)
                    } else {
                        dock(owner, cx)
                    }
                })
            })
        });
        let leading = dock.flatten().into_iter().chain(os_keyboard).collect();
        let minimize = self.minimize.clone();
        let beside_close = self.source_minimize.clone().map(|minimize| {
            design::components::icon_button(
                &theme,
                "nav-minimize-source",
                &design::icons::MINUS,
                i18n::t(cx, "common.minimize"),
                design::components::Variant::Default,
            )
            .on_click(move |_, _, cx| minimize(owner, cx))
        });
        div().size_full().child(design::components::nav_bar(
            &theme,
            open,
            title,
            minimize.is_some(),
            i18n::t(
                cx,
                if minimize.is_some() {
                    "common.minimize"
                } else {
                    "common.close"
                },
            ),
            move |_, _, cx| match &minimize {
                Some(minimize) => minimize(owner, cx),
                None => command(owner, PanelCommand::Close, cx),
            },
            leading,
            beside_close,
        ))
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
