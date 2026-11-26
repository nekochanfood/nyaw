//! パネル下部のナビゲーションバー（[`NavBar`]）と、パネル内容を包むルートビュー（[`PanelRoot`]）。

use super::*;

pub(crate) struct NavBar {
    pub(crate) id: PanelId,
    pub(crate) owner: PanelId,
    pub(crate) title: Option<Title>,
    /// 「…」の左に OS キーボードのトグルボタンを表示する。
    pub(crate) os_keyboard: bool,
    /// 「…」ポップオーバーにドック着脱行を表示する。
    pub(crate) dockable: bool,
    /// 右端の閉じるボタンを最小化ボタンに置き換える。
    pub(crate) minimize: Option<Minimize>,
    pub(crate) source_minimize: Option<Minimize>,
    /// 展開度（0 = 線、1 = 帯）と、目標値・アニメーション状態。
    pub(crate) open: design::animation::Transition,
    /// 手に保持中の持ち主をダッシュボード非表示時にグラブロックしている状態（[`attach::hold`]）。
    pub(crate) held: bool,
}

impl Render for NavBar {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let now = design::drive::now();
        // ポイント状態は `sync_panel` が状態に書き込み、変更時にこのバーを再描画する。
        let state = cx
            .default_global::<PanelControl>()
            .panels
            .entry(self.id)
            .or_default();
        let to = if state.status.pointed { 1.0 } else { 0.0 };
        // 展開・収縮ともに ease-in-out で遷移する。
        let animated = cx.has_global::<design::Motion>();
        self.open
            .go(to, design::animation::Motion::Morph, now, animated);
        let (open, moving) = self.open.at(now);
        // アニメーション中のみ毎フレーム再描画する。
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
        // OS キーボードトグル。表示中（持ち主向けキーボードが存在する間）は強調色。
        // キーボードの表示・非表示時に `keyboard` がこのバーを再描画するため、キーボードの X ボタンで閉じても色が復帰する。
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
        // 「…」: 手への装着・ドック着脱を選択するポップオーバーを起動する。全パネル共通。
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
        // ボタンはすべて右寄せ。閉じるボタンの左に、左から「…」・OS キーボードトグル・配信元最小化ボタンの順に配置。
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
        // 全パネル共通のフォント設定（子要素が継承する）。
        div()
            .size_full()
            .font(design::font::font())
            .child(self.0.clone())
    }
}
