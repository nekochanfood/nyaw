//! パネルの下に付くナビゲーションバー（[`NavBar`]）と、パネルの中身を包む根（[`PanelRoot`]）。

use super::*;

pub(crate) struct NavBar {
    pub(crate) id: PanelId,
    pub(crate) owner: PanelId,
    pub(crate) title: Option<Title>,
    /// 「…」の左に OS キーボードのトグルを出す。
    pub(crate) os_keyboard: bool,
    /// 「…」のポップオーバーに、はめる場所から外す・戻す行を出す。
    pub(crate) dockable: bool,
    /// 右端を、閉じる代わりにしまうボタンにする。
    pub(crate) minimize: Option<Minimize>,
    pub(crate) source_minimize: Option<Minimize>,
    /// 広がり具合（0 = 線、1 = 帯）と、向かっている先・その動き。
    pub(crate) open: design::animation::Transition,
    /// 手に持っている持ち主を、ダッシュボードが出ていないのでロックしてある（[`attach::hold`]）。
    pub(crate) held: bool,
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
        // 「…」: 手に持つ・はめる場所から外す/戻すを選ぶポップオーバーを開く。どのパネルにも出す。
        let dockable = self.dockable;
        let more = design::components::icon_button(
            &theme,
            "nav-more",
            &design::icons::DOTS_THREE,
            i18n::t(cx, "common.more"),
            design::components::Variant::Default,
        )
        .on_click(move |_, _, cx| {
            let spec = attach::menu_spec(owner, dockable, cx);
            cx.default_global::<PanelControl>().spawns.push(spec);
        });
        let minimize = self.minimize.clone();
        // ボタンはすべて右に寄せ、閉じるボタンの左に左から「…」・OS キーボードのトグル・配信元をしまうボタン。
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
            std::iter::once(more)
                .chain(os_keyboard)
                .chain(beside_close)
                .collect(),
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
