//! チュートリアル: セットアップを終えて開いたダッシュボードで、基本の操作を順に案内する。パネルをまたいで焦点を置く仕組み
//! （焦点の外を暗くし、強調した要素だけを押せるようにする）は `ui::tour` にあり、ここは手順と、案内のパネル
//! （[`Coach`]）と、練習用のウィンドウ（[`Practice`]）を持つ。どの手順も「次へ」で進めるので、案内した操作を
//! しなくてもよい（操作すると自動で次へ進む）。後の手順に要る状態（はめた画面・練習用のウィンドウ）は、その手順に
//! 入るときに用意する。

use design::Icon;
use gpui::ClickEvent;
use guide::{Control, Controller, Model};
use kernel::Hand;

use super::*;

/// 手順。[`STEPS`] の順に進む。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Step {
    /// 開始画面（始める・スキップ）。
    Intro,
    /// ダッシュボードのバー。
    Bar,
    /// ショートカットの二回押しで開閉する。
    Toggle,
    /// メニュー（設定・終了）。
    Menu,
    /// 時刻のボタンでクイック設定を開く。
    QuickSettings,
    /// はめた画面をつかんで外す。
    Grab,
    /// 外した画面をはめ直す。
    Dock,
    /// 画面のナビゲーションバー（閉じる・「…」）。
    NavBar,
    /// ランチャーを開く。
    Launcher,
    /// ランチャーで練習用のウィンドウを選ぶ。
    Pick,
    /// 映した画面を操作する（練習用のウィンドウ）。
    Operate,
    /// 映した画面をしまう。
    Minimize,
    /// しまった画面をバーのボタンで呼び戻す。
    Recall,
    /// 完了。
    Done,
}

pub const STEPS: [Step; 14] = [
    Step::Intro,
    Step::Bar,
    Step::Toggle,
    Step::Menu,
    Step::QuickSettings,
    Step::Grab,
    Step::Dock,
    Step::NavBar,
    Step::Launcher,
    Step::Pick,
    Step::Operate,
    Step::Minimize,
    Step::Recall,
    Step::Done,
];

impl Step {
    /// 次の手順（最後はそのまま）。
    pub fn next(self) -> Step {
        let at = STEPS.iter().position(|&s| s == self).unwrap_or(0);
        STEPS[(at + 1).min(STEPS.len() - 1)]
    }

    /// 開始画面と完了を除いた、何番目の手順か（1 から）。
    fn number(self) -> usize {
        STEPS.iter().position(|&s| s == self).unwrap_or(0)
    }

    fn icon(self) -> &'static Icon {
        match self {
            Step::Intro => &icons::GRADUATION_CAP,
            Step::Bar | Step::Toggle => &icons::SQUARES_FOUR,
            Step::Menu => &icons::LIST,
            Step::QuickSettings => &icons::BELL,
            Step::Grab | Step::Dock => &icons::HAND_GRABBING,
            Step::NavBar => &icons::DOTS_THREE,
            Step::Launcher | Step::Pick => &icons::DOTS_NINE,
            Step::Operate => &icons::CURSOR_CLICK,
            Step::Minimize | Step::Recall => &icons::APP_WINDOW,
            Step::Done => &icons::FLAG_CHECKERED,
        }
    }

    /// 案内のパネルに、利き手のコントローラの画像で示す操作。
    fn controls(self) -> &'static [Control] {
        match self {
            Step::Intro => &[
                Control::Select,
                Control::Grab,
                Control::Scroll,
                Control::Menu,
            ],
            Step::QuickSettings
            | Step::NavBar
            | Step::Launcher
            | Step::Pick
            | Step::Minimize
            | Step::Recall => &[Control::Select],
            Step::Grab | Step::Dock => &[Control::Grab],
            Step::Operate => &[Control::Select, Control::Scroll],
            Step::Bar | Step::Toggle | Step::Menu | Step::Done => &[],
        }
    }
}

/// 案内の手順のうち、区切りの見出しに出す名前（開始画面と完了は無し）。
fn section(step: Step, cx: &App) -> Option<SharedString> {
    let name = match step {
        Step::Intro | Step::Done => return None,
        Step::Bar | Step::Toggle | Step::Menu | Step::QuickSettings => {
            i18n::t(cx, "tutorial.section_dashboard")
        }
        Step::Grab | Step::Dock | Step::NavBar => i18n::t(cx, "tutorial.section_panel"),
        _ => i18n::t(cx, "tutorial.section_mirror"),
    };
    Some(name)
}

/// 手順の見出しと説明。
fn texts(step: Step, shortcut: Option<&str>, cx: &App) -> (SharedString, SharedString) {
    match step {
        Step::Intro => (
            i18n::t(cx, "tutorial.intro_title"),
            i18n::t(cx, "tutorial.intro_body"),
        ),
        Step::Bar => (
            i18n::t(cx, "tutorial.bar_title"),
            i18n::t(cx, "tutorial.bar_body"),
        ),
        Step::Toggle => (
            i18n::t(cx, "tutorial.toggle_title"),
            match shortcut {
                Some(button) => i18n::tf(cx, "tutorial.toggle_body", &[("button", &button)]),
                None => i18n::t(cx, "tutorial.toggle_body_any"),
            },
        ),
        Step::Menu => (
            i18n::t(cx, "tutorial.menu_title"),
            i18n::t(cx, "tutorial.menu_body"),
        ),
        Step::QuickSettings => (
            i18n::t(cx, "tutorial.quick_title"),
            i18n::t(cx, "tutorial.quick_body"),
        ),
        Step::Grab => (
            i18n::t(cx, "tutorial.grab_title"),
            i18n::t(cx, "tutorial.grab_body"),
        ),
        Step::Dock => (
            i18n::t(cx, "tutorial.dock_title"),
            i18n::t(cx, "tutorial.dock_body"),
        ),
        Step::NavBar => (
            i18n::t(cx, "tutorial.nav_title"),
            i18n::t(cx, "tutorial.nav_body"),
        ),
        Step::Launcher => (
            i18n::t(cx, "tutorial.launcher_title"),
            i18n::t(cx, "tutorial.launcher_body"),
        ),
        Step::Pick => (
            i18n::t(cx, "tutorial.pick_title"),
            i18n::t(cx, "tutorial.pick_body"),
        ),
        Step::Operate => (
            i18n::t(cx, "tutorial.operate_title"),
            i18n::t(cx, "tutorial.operate_body"),
        ),
        Step::Minimize => (
            i18n::t(cx, "tutorial.minimize_title"),
            i18n::t(cx, "tutorial.minimize_body"),
        ),
        Step::Recall => (
            i18n::t(cx, "tutorial.recall_title"),
            i18n::t(cx, "tutorial.recall_body"),
        ),
        Step::Done => (
            i18n::t(cx, "tutorial.done_title"),
            i18n::t(cx, "tutorial.done_body"),
        ),
    }
}

/// 案内のパネルの大きさ（1px = 1mm）。
const COACH_M: Vec2 = Vec2::new(0.6, 0.76);
pub const COACH_PX: (u32, u32) = (600, 760);
/// 案内のパネルを、はめた画面の右端からどれだけ離し（m）、どれだけ内へ向けるか（rad）。
const COACH_GAP: f32 = 0.04;
const COACH_TURN: f32 = 30.0 * std::f32::consts::PI / 180.0;
/// 案内のパネルに出すコントローラの画像の一辺（px。説明の列が左右に半分ずつ付く）と、寄って映す枠の高さ。
const GUIDE_PX: f32 = 240.0;
const ZOOM_PX: f32 = 300.0;
/// 練習用のウィンドウのリストの行数と、行の高さ（px）。下までスクロールしないと最後のボタンが見えない。
const PRACTICE_ROWS: usize = 12;
const PRACTICE_ROW_PX: f32 = 56.0;

/// 案内のパネルの、バーから見た姿勢（距離・高さの設定が `distance`・`height` のとき）。はめた画面の右に、内へ向けて立てる。
pub fn coach_offset(distance: f32, height: f32) -> Pose {
    let half = COACH_M.x / 2.0;
    let (sin, cos) = COACH_TURN.sin_cos();
    let center = Vec3::new(
        SIZE_M.x / 2.0 + COACH_GAP + half * cos,
        HEIGHT_ORIGIN + height,
        -distance + half * sin,
    );
    pose(distance, height).inverse() * Pose::new(center, Quat::from_rotation_y(-COACH_TURN))
}

/// 進めている案内。
pub(crate) struct Tutorial {
    pub(crate) step: Step,
    /// 手順に入った時刻（コントローラが現れる動きの起点。`design::drive::now` の時計）。
    entered: Instant,
    /// 案内のパネルと、練習用のウィンドウ（開いたら）。
    pub(crate) coach: Option<PanelId>,
    pub(crate) practice: Option<PanelId>,
    /// 前に見たとき、手順の操作が済んでいたか。済んでいない状態から済んだ状態に変わったら次へ進む。
    done: bool,
    /// 開閉の手順で、ダッシュボードが隠れたのを見た。
    hid: bool,
}

impl Dashboard {
    /// 進めている案内の手順（案内していなければ `None`）。
    pub(crate) fn tutorial_step(&self) -> Option<Step> {
        self.tutorial.as_ref().map(|t| t.step)
    }

    /// 案内の頼み（`ui::tour::start`）に応え、案内を進める。頼まれたときに隠れていれば、出てから始める。
    /// フレームループが状態を書き戻すたびに呼ばれるので、変わっていなければ何もしない。
    pub(crate) fn follow_tutorial(&mut self, cx: &mut Context<Self>) {
        let asked = cx.try_global::<ui::tour::Tour>().map_or(0, |t| t.asked);
        if asked != self.tour_asked {
            self.tour_asked = asked;
            self.tour_waiting = self.tutorial.is_none();
        }
        let hidden = self.hidden(cx);
        if self.tour_waiting && !hidden && !ui::settings(cx).lockdown {
            self.tour_waiting = false;
            self.start_tutorial(cx);
        }
        let Some(step) = self.tutorial_step() else {
            return;
        };
        if step == Step::Toggle && hidden {
            self.tutorial.as_mut().expect("running").hid = true;
        }
        let done = self.step_done(step, cx);
        let t = self.tutorial.as_mut().expect("running");
        let advance = done && !t.done;
        t.done = done;
        if advance {
            self.tutorial_go(step.next(), cx);
        }
        let focus = self.tour_focus(cx);
        ui::tour::set(Some(focus), cx);
    }

    /// 案内を始める: はめている画面を外し、開いているポップオーバーを閉じて、案内のパネルを画面の右に開く。
    fn start_tutorial(&mut self, cx: &mut Context<Self>) {
        if let Some(id) = ui::docked(cx) {
            if self.streams.iter().any(|s| s.panel == id) {
                self.stash(id, cx);
            } else {
                close(id, cx);
                self.pages.retain(|&(_, p)| p != id);
            }
        }
        for menu in [self.open_menu(cx), open_popover(self.overflow_menu, cx)]
            .into_iter()
            .flatten()
        {
            close(menu, cx);
        }
        self.tutorial = Some(Tutorial {
            step: Step::Intro,
            entered: design::drive::now(),
            coach: None,
            practice: None,
            done: false,
            hid: false,
        });
        let s = ui::settings(cx);
        let dashboard = cx.weak_entity();
        let spec = PanelSpec::spawn(self.panel, move |id, _, cx: &mut Context<Coach>| {
            let weak = dashboard.clone();
            let _ = dashboard.update(cx, |d, _| {
                if let Some(t) = &mut d.tutorial {
                    t.coach = Some(id);
                }
            });
            Coach { dashboard: weak }
        })
        .docked_to(self.panel)
        .offset(coach_offset(s.dashboard_distance, s.dashboard_height))
        .sized(COACH_M, COACH_PX)
        // 案内のパネルは閉じたり動かしたりする対象ではない（終えるときは「終了」ボタン）。
        .without_nav_bar()
        .without_resize()
        .locked();
        cx.default_global::<PanelControl>().spawns.push(spec);
        ui::notify(self.panel, cx);
    }

    /// 手順 `step` へ進む。その手順に要る状態（はめた画面・練習用のウィンドウ）が無ければ用意する。
    pub(crate) fn tutorial_go(&mut self, step: Step, cx: &mut Context<Self>) {
        let Some(t) = &mut self.tutorial else {
            return;
        };
        t.step = step;
        t.entered = design::drive::now();
        t.hid = false;
        let coach = t.coach;
        match step {
            Step::Grab | Step::Dock | Step::NavBar if self.lesson_screen(cx).is_none() => {
                self.toggle(Page::QuickSettings, cx);
            }
            Step::Pick if self.current(cx) != Some(Page::Launcher) => {
                self.toggle(Page::Launcher, cx);
            }
            Step::Operate | Step::Minimize if !self.practice_shown(cx) => self.open_practice(cx),
            Step::Recall if self.practice_shown(cx) => {
                if let Some(p) = self.practice() {
                    self.minimize(p, cx);
                }
            }
            _ => {}
        }
        // 入った時点で済んでいる操作では進まない（変わったときだけ進む）。
        let done = self.step_done(step, cx);
        if let Some(t) = &mut self.tutorial {
            t.done = done;
        }
        let focus = self.tour_focus(cx);
        ui::tour::set(Some(focus), cx);
        if let Some(coach) = coach {
            ui::redraw(coach, cx);
        }
        ui::notify(self.panel, cx);
    }

    /// 案内を終える（スキップ・終了・完了）。練習用のウィンドウと案内のパネルを閉じる。
    pub(crate) fn end_tutorial(&mut self, cx: &mut Context<Self>) {
        let Some(t) = self.tutorial.take() else {
            return;
        };
        for id in t.practice.into_iter().chain(t.coach) {
            close(id, cx);
        }
        ui::tour::set(None, cx);
        ui::notify(self.panel, cx);
    }

    /// 案内で扱う画面（ダッシュボードのボタンで開いた画面のうち、はめているもの、無ければ最後に開いたもの）。
    fn lesson_screen(&self, cx: &App) -> Option<PanelId> {
        let open = |id: &PanelId| !closed(*id, cx);
        let docked = ui::docked(cx).filter(|d| self.pages.iter().any(|(_, p)| p == d));
        docked.or_else(|| self.pages.iter().rev().map(|&(_, p)| p).find(open))
    }

    /// 開いた練習用のウィンドウ（閉じたものも返す。閉じたかは呼び出し側が見る）。
    fn practice(&self) -> Option<PanelId> {
        self.tutorial.as_ref().and_then(|t| t.practice)
    }

    /// 練習用のウィンドウがダッシュボードにはまって見えている。
    fn practice_shown(&self, cx: &App) -> bool {
        self.practice()
            .is_some_and(|p| ui::docked(cx) == Some(p) && !closed(p, cx) && !self.stashed(p))
    }

    /// 配信 `id` をしまってある（バーのボタンだけが残っている）。
    fn stashed(&self, id: PanelId) -> bool {
        self.streams.iter().any(|s| s.panel == id && s.stashed)
    }

    /// 手順 `step` の操作が済んでいるか（済んだときに自動で次へ進む）。
    fn step_done(&self, step: Step, cx: &App) -> bool {
        match step {
            Step::Toggle => self.tutorial.as_ref().is_some_and(|t| t.hid) && !self.hidden(cx),
            Step::QuickSettings => self.current(cx) == Some(Page::QuickSettings),
            Step::Grab => self
                .lesson_screen(cx)
                .is_some_and(|s| ui::docked(cx) != Some(s)),
            Step::Dock => self
                .lesson_screen(cx)
                .is_some_and(|s| ui::docked(cx) == Some(s)),
            Step::NavBar => self.lesson_screen(cx).is_none(),
            Step::Launcher => self.current(cx) == Some(Page::Launcher),
            Step::Pick | Step::Recall => self.practice_shown(cx),
            Step::Minimize => self.practice().is_some_and(|p| self.stashed(p)),
            // 練習用のウィンドウの課題は、そのボタンを押したときに進める（[`Self::practice_done`]）。
            Step::Intro | Step::Bar | Step::Menu | Step::Operate | Step::Done => false,
        }
    }

    /// 今の手順の焦点: 案内のパネルはいつも触れる。手順ごとに、説明する要素のあるパネルを明るくする。
    fn tour_focus(&self, cx: &App) -> ui::tour::Focus {
        let Some(t) = &self.tutorial else {
            return Default::default();
        };
        let bar = self.panel;
        let nav = |owner: PanelId| cx.global::<PanelControl>().nav_bars.get(&owner).copied();
        let ids = |ids: &[&str]| {
            ids.iter()
                .map(|&s| SharedString::from(s))
                .collect::<Vec<_>>()
        };
        let mut f = ui::tour::Focus {
            free: t.coach.into_iter().collect(),
            ..Default::default()
        };
        let screen = self.lesson_screen(cx);
        let practice = self.practice().filter(|&p| !closed(p, cx));
        match t.step {
            Step::Intro | Step::Done => {}
            Step::Bar => (f.lit, f.marks) = (vec![bar], ids(&["dashboard-bar"])),
            Step::Toggle => (f.lit, f.toggle) = (vec![bar], true),
            Step::Menu => (f.lit, f.marks) = (vec![bar], ids(&["dashboard-menu"])),
            Step::QuickSettings => {
                (f.lit, f.spots) = (vec![bar], ids(&["dashboard-quick-settings"]))
            }
            Step::Grab | Step::Dock => f.lit = [bar].into_iter().chain(screen).collect(),
            Step::NavBar => {
                f.lit = screen.into_iter().chain(screen.and_then(nav)).collect();
                (f.spots, f.marks) = (ids(&["nav-close"]), ids(&["nav-more"]));
            }
            Step::Launcher => (f.lit, f.spots) = (vec![bar], ids(&["dashboard-launcher"])),
            Step::Pick => {
                let launcher = self.page_panel(Page::Launcher, cx);
                (f.lit, f.spots) = (launcher.into_iter().collect(), ids(&["dashboard-practice"]));
            }
            Step::Operate => f.free.extend(practice),
            Step::Minimize => {
                f.lit = practice.into_iter().chain(practice.and_then(nav)).collect();
                (f.spots, f.marks) = (ids(&["nav-minimize-source"]), ids(&["nav-keyboard"]));
            }
            Step::Recall => {
                f.lit = vec![bar];
                f.spots = practice.map(stream_key).into_iter().collect();
            }
        }
        f
    }

    /// 練習用のウィンドウを開く（開いていれば、はめて見せる）。ランチャーのタイルと、手順に入るときに使う。
    pub(crate) fn open_practice(&mut self, cx: &mut Context<Self>) {
        if self.tutorial.is_none() {
            return;
        }
        if let Some(p) = self.practice().filter(|&p| !closed(p, cx)) {
            self.show_stream(p, cx);
            return;
        }
        self.stash_docked(cx);
        let (dashboard, stash) = (cx.weak_entity(), cx.weak_entity());
        let spec = PanelSpec::spawn(self.panel, move |id, _, cx: &mut Context<Practice>| {
            ui::dock(id, cx);
            let weak = dashboard.clone();
            let _ = dashboard.update(cx, |d, cx| d.practice_opened(id, cx));
            Practice {
                dashboard: weak,
                scroll: ScrollHandle::new(),
                pressed: false,
            }
        })
        // 映した画面と同じく、ドッキングスロットに合わせて正面に置く。
        .in_view(SCREEN_POSE)
        .sized(SIZE_M, RES_PX)
        .titled("tutorial.practice_title")
        .dockable()
        .with_os_keyboard()
        .source_minimizable(move |id, cx| {
            let _ = stash.update(cx, |d, cx| d.minimize(id, cx));
        });
        cx.default_global::<PanelControl>().spawns.push(spec);
    }

    /// 練習用のウィンドウが開いた: 映したウィンドウと同じく、バーにボタンを足す。
    fn practice_opened(&mut self, id: PanelId, cx: &mut Context<Self>) {
        let Some(t) = &mut self.tutorial else {
            close(id, cx);
            return;
        };
        t.practice = Some(id);
        let name = i18n::t(cx, "tutorial.practice_title");
        self.add_stream(id, SourceKind::Window, name, None, false, cx);
    }

    /// 練習用のウィンドウの課題（いちばん下のボタン）を終えた。操作の手順なら次へ進む。
    fn practice_done(&mut self, cx: &mut Context<Self>) {
        if self.tutorial_step() == Some(Step::Operate) {
            self.tutorial_go(Step::Operate.next(), cx);
        }
    }
}

/// パネル `id` が閉じている。
fn closed(id: PanelId, cx: &App) -> bool {
    cx.try_global::<PanelControl>()
        .and_then(|c| c.panels.get(&id))
        .is_some_and(|p| p.status.closed)
}

/// `hand` で使っているコントローラ（無ければ Quest Touch Plus）。
fn controller(hand: Hand, cx: &App) -> Controller {
    cx.try_global::<PanelControl>()
        .and_then(|c| c.controllers.iter().find(|c| c.hand == hand))
        .and_then(|c| c.controller)
        .unwrap_or(Controller::of(Model::QuestPlus))
}

/// 案内のパネル: 区切りと進み具合、見出しと説明、コントローラの画像、下に「終了」と「次へ」。
pub(crate) struct Coach {
    dashboard: WeakEntity<Dashboard>,
}

impl Render for Coach {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = Theme::current(cx);
        let step = self
            .dashboard
            .upgrade()
            .and_then(|d| d.read(cx).tutorial.as_ref().map(|t| (t.step, t.entered)));
        let Some((step, entered)) = step else {
            return div();
        };
        let act = |f: fn(&mut Dashboard, &mut Context<Dashboard>)| {
            let d = self.dashboard.clone();
            move |_: &ClickEvent, _: &mut Window, cx: &mut App| {
                let _ = d.update(cx, f);
            }
        };
        let next = |d: &mut Dashboard, cx: &mut Context<Dashboard>| {
            if let Some(step) = d.tutorial_step() {
                d.tutorial_go(step.next(), cx)
            }
        };
        let end = |d: &mut Dashboard, cx: &mut Context<Dashboard>| d.end_tutorial(cx);
        // ショートカットは左手、ほかの操作は利き手のコントローラで示す。
        let left = controller(Hand::Left, cx);
        let shortcut: Vec<&'static str> = left
            .parts(Control::Shortcut, Hand::Left)
            .into_iter()
            .filter(|p| {
                guide::parts(left.model, Hand::Left, p)
                    .iter()
                    .any(|q| q.visible)
            })
            .collect();
        let name = shortcut.first().map(|p| guide::part_name(cx, p));
        let (title, body) = texts(step, name.as_deref(), cx);
        let hand = ui::settings(cx).dominant_hand.hand();
        let own = controller(hand, cx);
        let visual = if step == Step::Toggle {
            Some(
                guide::zoom(left.model, Hand::Left)
                    .size(px(GUIDE_PX * 2.0), px(ZOOM_PX))
                    .parts(shortcut)
                    .enter(Some(entered))
                    .into_any_element(),
            )
        } else {
            let callouts: Vec<_> = guide::bindings(cx, own, hand)
                .into_iter()
                .filter(|(c, _)| step.controls().contains(c))
                .map(|(_, c)| c)
                .collect();
            (!callouts.is_empty()).then(|| {
                guide::controller(own.model, hand)
                    .size(px(GUIDE_PX))
                    .callouts(callouts)
                    .into_any_element()
            })
        };
        let progress = section(step, cx).map(|s| {
            let n = step.number().to_string();
            let total = (STEPS.len() - 2).to_string();
            i18n::tf(
                cx,
                "tutorial.progress",
                &[("section", &s), ("n", &n), ("total", &total)],
            )
        });
        let (other, forward) = match step {
            Step::Intro => (
                Some(
                    components::button(
                        &theme,
                        "tutorial-skip",
                        i18n::t(cx, "tutorial.skip"),
                        Variant::Default,
                    )
                    .on_click(act(end)),
                ),
                components::wizard_next(&theme, "tutorial-next", i18n::t(cx, "tutorial.start"))
                    .on_click(act(next)),
            ),
            Step::Done => (
                None,
                components::wizard_next(&theme, "tutorial-finish", i18n::t(cx, "tutorial.finish"))
                    .on_click(act(end)),
            ),
            _ => (
                Some(
                    components::button(
                        &theme,
                        "tutorial-quit",
                        i18n::t(cx, "tutorial.quit"),
                        Variant::Default,
                    )
                    .on_click(act(end)),
                ),
                components::wizard_next(&theme, "tutorial-next", i18n::t(cx, "tutorial.next"))
                    .on_click(act(next)),
            ),
        };
        let head = components::heading(
            &theme,
            "tutorial",
            components::wizard_icon(&theme, step.icon()),
            title,
            Some(body),
        );
        components::card_panel(&theme)
            .debug_selector(move || format!("tutorial-step-{step:?}"))
            .p(px(spacing::XL))
            .flex()
            .flex_col()
            .gap(px(spacing::LG))
            .children(progress.map(|p| {
                div()
                    .debug_selector(|| "tutorial-progress".into())
                    .text_size(px(font_size::SM))
                    .font_weight(FontWeight::BOLD)
                    .text_color(rgba(theme.accent_text))
                    .child(p)
            }))
            .child(head)
            .child(
                div()
                    .flex_1()
                    .min_h_0()
                    .flex()
                    .items_center()
                    .justify_center()
                    .children(visual),
            )
            .child(
                div()
                    .w_full()
                    .flex()
                    .justify_between()
                    .child(other.map_or_else(|| div().into_any_element(), |b| b.into_any_element()))
                    .child(forward),
            )
    }
}

/// 練習用のウィンドウ: 映したアプリの代わりに操作を試す。下までスクロールして、いちばん下のボタンを押す。
pub(crate) struct Practice {
    dashboard: WeakEntity<Dashboard>,
    scroll: ScrollHandle,
    /// いちばん下のボタンを押した。
    pub(crate) pressed: bool,
}

impl Render for Practice {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = Theme::current(cx);
        let rows = (1..=PRACTICE_ROWS).map(|n| {
            div()
                .flex_none()
                .h(px(PRACTICE_ROW_PX))
                .px(px(spacing::LG))
                .flex()
                .items_center()
                .border_b_1()
                .border_color(rgba(theme.neutral_border_subtle))
                .child(i18n::tf(
                    cx,
                    "tutorial.practice_row",
                    &[("n", &n.to_string())],
                ))
        });
        let goal = if self.pressed {
            components::disabled_button(
                &theme,
                "practice-goal",
                i18n::t(cx, "tutorial.practice_done"),
            )
        } else {
            components::button(
                &theme,
                "practice-goal",
                i18n::t(cx, "tutorial.practice_press"),
                Variant::Primary,
            )
            .on_click(cx.listener(|this, _, _, cx| {
                this.pressed = true;
                cx.notify();
                let _ = this.dashboard.update(cx, |d, cx| d.practice_done(cx));
            }))
        };
        components::card_panel(&theme)
            .p(px(spacing::XL))
            .flex()
            .flex_col()
            .gap(px(spacing::MD))
            .child(
                div()
                    .text_size(px(font_size::LG))
                    .font_weight(FontWeight::BOLD)
                    .child(i18n::t(cx, "tutorial.practice_title")),
            )
            .child(
                div()
                    .text_color(rgba(theme.neutral_text_low))
                    .child(i18n::t(cx, "tutorial.practice_hint")),
            )
            .child(
                div()
                    .id("practice-list")
                    .debug_selector(|| "practice-list".into())
                    .flex_1()
                    .min_h_0()
                    .overflow_y_scroll()
                    .track_scroll(&self.scroll)
                    .flex()
                    .flex_col()
                    .children(rows)
                    .child(
                        div()
                            .flex_none()
                            .py(px(spacing::LG))
                            .flex()
                            .justify_center()
                            .child(goal),
                    ),
            )
    }
}

#[cfg(test)]
mod tests;
