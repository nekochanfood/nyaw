//! チュートリアル: セットアップを終えて開いたダッシュボードで、基本の操作を順に案内する。パネルをまたいで焦点を置く仕組み
//! （焦点の外を暗くし、強調した要素だけを押せるようにする）は `ui::tour` にあり、ここは手順と、案内のパネル
//! （[`Coach`]）と、練習用のウィンドウ（[`Practice`]）を持つ。どの手順も「次へ」で進めるので、案内した操作を
//! しなくてもよい（操作すると自動で次へ進む）。後の手順に要る状態（はめた画面・練習用のウィンドウ）は、その手順に
//! 入るときに用意する。

use design::Icon;
use design::components::WIZARD_MARGIN;
use gpui::{ClickEvent, MouseButton};
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

    /// 前の手順（最初はそのまま）。
    pub fn back(self) -> Step {
        let at = STEPS.iter().position(|&s| s == self).unwrap_or(0);
        STEPS[at.saturating_sub(1)]
    }

    /// 案内のパネルを置く所。`free` はダッシュボードに画面がはまっていないか。
    fn spot(self, free: bool) -> Spot {
        match self {
            Step::Intro => Spot::Docked,
            Step::NavBar | Step::Minimize => Spot::Below,
            Step::Grab | Step::Dock | Step::Pick | Step::Operate => Spot::Beside,
            _ if free => Spot::Bar,
            _ => Spot::Beside,
        }
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
            Step::Operate => &[
                Control::Select,
                Control::Scroll,
                Control::RightClick,
                Control::MiddleClick,
            ],
            Step::Bar | Step::Toggle | Step::Menu | Step::Done => &[],
        }
    }
}

/// 手順の見出しと説明。`shortcut` はショートカットのボタンの名前、`clicks` は右クリック・中クリックで親指を触れておく
/// ボタンの名前（無い機種は `None`）。
fn texts(
    step: Step,
    shortcut: Option<&str>,
    clicks: Option<(&str, &str)>,
    cx: &App,
) -> (SharedString, SharedString) {
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
            match clicks {
                Some((right, middle)) => i18n::tf(
                    cx,
                    "tutorial.operate_body",
                    &[("right", &right), ("middle", &middle)],
                ),
                None => i18n::t(cx, "tutorial.operate_body_left"),
            },
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

/// 案内のパネルの大きさ（m）と解像度（px）。セットアップウィザードと同じ 4:3 で、どの手順でも変えない。1px = 0.8mm に
/// 詰め、頭から 1.3m 前後で、1.5m 先のウィザード（1px = 1mm）と文字が同じくらいの大きさに見えるようにする。
/// 画面の右に置いても、右端が頭から 50° ほどに収まる。
pub const COACH_M: Vec2 = Vec2::new(0.64, 0.48);
pub const COACH_PX: (u32, u32) = (800, 600);
/// 案内のパネルを画面の右に置くとき、はめた画面の右端から空ける角度（rad。頭から見て）。
const COACH_GAP: f32 = 2.0 * std::f32::consts::PI / 180.0;
/// 画面の下のバーを説明するとき、画面の右に置く案内のパネルを下げる量（m）。
const COACH_DROP: f32 = 0.2;
/// 案内のパネルの中身の幅と、コントローラの画像を出す枠の高さ（px）。枠は下の帯の上に固定し、説明の長さで動かさない。
const CONTENT_PX: f32 = COACH_PX.0 as f32 - 2.0 * (components::PANEL_MARGIN + WIZARD_MARGIN);
const VISUAL_PX: f32 = 260.0;
/// コントローラの画像の一辺（px。説明の列が左右に半分ずつ付く）と、寄って映す枠の縁を地へ溶かす幅。
const GUIDE_PX: f32 = 240.0;
const ZOOM_EDGE_PX: f32 = 56.0;
/// 見出しの横のアイコンと、進み具合の今の番号の大きさ（px）。
const TITLE_ICON_PX: f32 = 40.0;
const PROGRESS_PX: f32 = 44.0;

/// 案内のパネルを置く所。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Spot {
    /// ダッシュボードにはめる（開始画面）。
    Docked,
    /// バーの上に浮かべる（画面がはまっていないとき、バーのボタンを説明する。はめた所と同じ位置）。
    Bar,
    /// はめた画面の右。
    Beside,
    /// はめた画面の右の、下寄り（画面の下のバーを説明する）。
    Below,
}

/// 頭から見た案内のパネルの姿勢（ダッシュボードの距離・高さの設定が `distance`・`height` のとき。頭は水平に前を向く）。
/// どの置き場所でも頭からの距離をはめた所と同じにして頭のほうへ向け、見かけの大きさを変えない。
pub fn coach_pose(spot: Spot, distance: f32, height: f32) -> Pose {
    let margin = |size: Vec2, res: (u32, u32)| components::PANEL_MARGIN * size.y / res.1 as f32;
    // バーの上端（[`pose`] と同じ計算）と、案内のパネルの面の高さの半分。
    let face = SIZE_M.y / 2.0 - margin(SIZE_M, RES_PX);
    let top = HEIGHT_ORIGIN + height - (face + ui::NAV_BAR_HEIGHT_M + kernel::overlay::SLOT_GAP);
    let half = COACH_M.y / 2.0 - margin(COACH_M, COACH_PX);
    // はめた所: ナビゲーションバーを持たないので、バーの上端から隙間だけ上に下端が来る。
    let docked = Vec3::new(0.0, top + kernel::overlay::SLOT_GAP + half, -distance);
    let r = docked.length();
    // 画面の右: 頭から見て、はめた画面の右端から `COACH_GAP` 空けて左端が来る向き。
    let beside = |y: f32| {
        let flat = (r * r - y * y).max(0.0).sqrt();
        let yaw = (SIZE_M.x / 2.0).atan2(distance) + COACH_GAP + (COACH_M.x / 2.0).atan2(flat);
        Pose::new(
            Vec3::new(flat * yaw.sin(), y, -flat * yaw.cos()),
            Quat::from_rotation_y(-yaw),
        )
    };
    match spot {
        // バーの上は、はめた所と同じ位置（外れて浮かぶだけ）。
        Spot::Docked | Spot::Bar => Pose::new(docked, Quat::IDENTITY),
        Spot::Beside => beside(HEIGHT_ORIGIN + height),
        Spot::Below => beside(HEIGHT_ORIGIN + height - COACH_DROP),
    }
}

/// 案内のパネルの、バーから見た姿勢（[`coach_pose`]）。
fn coach_offset(spot: Spot, distance: f32, height: f32) -> Pose {
    pose(distance, height).inverse() * coach_pose(spot, distance, height)
}

/// 進めている案内。
pub(crate) struct Tutorial {
    pub(crate) step: Step,
    /// 手順に入った時刻（コントローラが現れる動きの起点。`design::drive::now` の時計）。
    entered: Instant,
    /// 案内のパネルと、練習用のウィンドウ（開いたら）。
    pub(crate) coach: Option<PanelId>,
    pub(crate) practice: Option<PanelId>,
    /// 前に見たとき、手順の操作が済んでいたか（済んでいない状態から済んだ状態に変わったら `done`）。
    was: bool,
    /// この手順で、案内した操作をした（案内のパネルに「できました」を出す。次へは進まない）。
    pub(crate) done: bool,
    /// 開閉の手順で、ダッシュボードが隠れたのを見た。
    hid: bool,
    /// 案内のパネルを置いた所（[`Dashboard::place_coach`]）。
    spot: Option<Spot>,
}

impl Dashboard {
    /// 進めている案内の手順（案内していなければ `None`）。
    pub(crate) fn tutorial_step(&self) -> Option<Step> {
        self.tutorial.as_ref().map(|t| t.step)
    }

    /// 手順の操作をした: 案内のパネルに「できました」を出す（その手順の間は出したまま）。
    fn mark_done(&mut self, cx: &mut Context<Self>) {
        let Some(t) = &mut self.tutorial else {
            return;
        };
        t.done = true;
        if let Some(coach) = t.coach {
            ui::redraw(coach, cx);
        }
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
        // 案内した操作をしても次へは進めない（進むのは「次へ」だけ）。案内のパネルに「できました」を出す。
        let now = self.step_done(step, cx);
        let t = self.tutorial.as_mut().expect("running");
        let did = now && !t.was && !t.done;
        t.was = now;
        if did {
            self.mark_done(cx);
        }
        self.place_coach(cx);
        let focus = self.tour_focus(cx);
        ui::tour::set(Some(focus), cx);
    }

    /// 案内のパネルを、今の手順で説明する所の近くへ動かす（置き場所が変わったときだけ）。開始画面ではダッシュボードに
    /// はめ、始めると外れて浮かぶ。浮かんでいる間もバーの子なので、ダッシュボードと一緒に動き、一緒に隠れる。
    fn place_coach(&mut self, cx: &mut Context<Self>) {
        let Some(t) = &self.tutorial else {
            return;
        };
        let Some(coach) = t.coach else {
            return;
        };
        let free = ui::docked(cx).is_none_or(|d| d == coach);
        let (was, spot) = (t.spot, t.step.spot(free));
        if was == Some(spot) {
            return;
        }
        if let Some(t) = &mut self.tutorial {
            t.spot = Some(spot);
        }
        if spot == Spot::Docked {
            if free {
                ui::dock(coach, cx);
            }
            return;
        }
        if was == Some(Spot::Docked) {
            // はめた所から外し、バーの子（はめない）にする。外した所から次の置き場所へ滑らかに動く。
            ui::undock(coach, cx);
            let float = kernel::Parent {
                panel: self.panel,
                carry: true,
                docked: true,
                ..Default::default()
            };
            ui::command(coach, PanelCommand::Parent(Some(float)), cx);
        }
        let s = ui::settings(cx);
        let offset = coach_offset(spot, s.dashboard_distance, s.dashboard_height);
        ui::command(coach, PanelCommand::Offset(offset), cx);
    }

    /// 案内を始める: はめている画面を外し、開いているポップオーバーを閉じて、案内のパネル（開始画面）をはめて開く。
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
            was: false,
            done: false,
            hid: false,
            spot: None,
        });
        let dashboard = cx.weak_entity();
        let spec = PanelSpec::spawn(self.panel, move |id, _, cx: &mut Context<Coach>| {
            // 開始画面はダッシュボードにはめて開く（画面のボタンと同じく、はまる所に合わせて正面に置いてから）。
            ui::dock(id, cx);
            let weak = dashboard.clone();
            let _ = dashboard.update(cx, |d, _| {
                if let Some(t) = &mut d.tutorial {
                    (t.coach, t.spot) = (Some(id), Some(Spot::Docked));
                }
            });
            Coach {
                panel: id,
                dashboard: weak,
            }
        })
        .in_view(SCREEN_POSE)
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
        // 先に案内のパネルを動かす（はめた所から外してから、手順に要る画面をはめる）。
        self.place_coach(cx);
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
        // 入った時点で済んでいる操作は、した扱いにしない（済んでいない状態から変わったときだけ）。
        let was = self.step_done(step, cx);
        if let Some(t) = &mut self.tutorial {
            (t.was, t.done) = (was, false);
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

    /// 手順 `step` の操作が済んでいるか（済んでいない状態から済んだら「できました」を出す）。
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
            // 練習用のウィンドウの課題は、終えたときに印を付ける（[`Self::practice_done`]）。
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
                f.spots = ids(&["nav-close"]);
            }
            Step::Launcher => (f.lit, f.spots) = (vec![bar], ids(&["dashboard-launcher"])),
            Step::Pick => {
                let launcher = self.page_panel(Page::Launcher, cx);
                (f.lit, f.spots) = (launcher.into_iter().collect(), ids(&["dashboard-practice"]));
            }
            Step::Operate => f.free.extend(practice),
            Step::Minimize => {
                f.lit = practice.into_iter().chain(practice.and_then(nav)).collect();
                f.spots = ids(&["nav-minimize-source"]);
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
                clicked: [false; 3],
                scrolled: false,
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

    /// 練習用のウィンドウの課題（どのクリックとスクロールも試した）を終えた。操作の手順なら「できました」を出す。
    fn practice_done(&mut self, cx: &mut Context<Self>) {
        if self.tutorial_step() == Some(Step::Operate) {
            self.mark_done(cx);
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
    panel: PanelId,
    dashboard: WeakEntity<Dashboard>,
}

impl Render for Coach {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = Theme::current(cx);
        let state = self.dashboard.upgrade().and_then(|d| {
            let t = d.read(cx).tutorial.as_ref()?;
            Some((t.step, t.entered, t.done))
        });
        let Some((step, entered, done)) = state else {
            return div();
        };
        let act = |f: fn(&mut Dashboard, &mut Context<Dashboard>)| {
            let d = self.dashboard.clone();
            move |_: &ClickEvent, _: &mut Window, cx: &mut App| {
                let _ = d.update(cx, f);
            }
        };
        let go = |back: bool| -> fn(&mut Dashboard, &mut Context<Dashboard>) {
            if back {
                |d, cx| {
                    if let Some(step) = d.tutorial_step() {
                        d.tutorial_go(step.back(), cx)
                    }
                }
            } else {
                |d, cx| {
                    if let Some(step) = d.tutorial_step() {
                        d.tutorial_go(step.next(), cx)
                    }
                }
            }
        };
        let end = |d: &mut Dashboard, cx: &mut Context<Dashboard>| d.end_tutorial(cx);
        // ショートカットは左手、ほかの操作は利き手のコントローラで示す。
        let left = controller(Hand::Left, cx);
        let hand = ui::settings(cx).dominant_hand.hand();
        let own = controller(hand, cx);
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
        let click = |control| {
            own.parts(control, hand)
                .first()
                .map(|p| guide::part_name(cx, p))
        };
        let clicks = click(Control::RightClick).zip(click(Control::MiddleClick));
        let (title, body) = texts(
            step,
            name.as_deref(),
            clicks.as_ref().map(|(r, m)| (r.as_ref(), m.as_ref())),
            cx,
        );
        let back = components::wizard_back(&theme, "tutorial-back", i18n::t(cx, "tutorial.back"))
            .on_click(act(go(true)));
        // 完了は、ウィザードの完了画面と同じ見た目（アイコン・見出し・説明を中央に）。
        if step == Step::Done {
            let finish =
                components::wizard_next(&theme, "tutorial-finish", i18n::t(cx, "tutorial.finish"))
                    .on_click(act(end));
            return components::wizard_panel(&theme)
                .debug_selector(move || format!("tutorial-step-{step:?}"))
                .child(components::wizard_welcome(
                    &theme,
                    "tutorial",
                    components::wizard_icon(&theme, step.icon()),
                    title,
                    body,
                    Some(back),
                    finish,
                ));
        }
        let visual = if step == Step::Toggle {
            Some(
                guide::zoom(left.model, Hand::Left)
                    .size(px(CONTENT_PX), px(VISUAL_PX))
                    .parts(shortcut)
                    .edges(px(ZOOM_EDGE_PX))
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
        // 右上の進み具合: 今の番号を大きく、全体の数を小さく上端にそろえる（開始画面には無い）。
        let progress = (step != Step::Intro).then(|| {
            div()
                .debug_selector(|| "tutorial-progress".into())
                .flex_none()
                .flex()
                .items_start()
                .gap(px(spacing::XS))
                .child(
                    div()
                        .text_size(px(PROGRESS_PX))
                        .line_height(px(PROGRESS_PX))
                        .font_weight(FontWeight::BOLD)
                        .child(step.number().to_string()),
                )
                .child(
                    div()
                        .text_size(px(font_size::MD))
                        .text_color(rgba(theme.neutral_text_low))
                        .child(format!("/{}", STEPS.len() - 2)),
                )
        });
        // 案内した操作をしたら、「次へ」の左に印を出す（次へは進めない）。
        let did = done.then(|| {
            div()
                .debug_selector(|| "tutorial-done".into())
                .flex()
                .items_center()
                .gap(px(spacing::XS))
                .text_color(rgba(theme.accent_text))
                .font_weight(FontWeight::BOLD)
                .child(design::icon(&icons::CHECK_CIRCLE).size(px(font_size::LG)))
                .child(i18n::t(cx, "tutorial.did"))
        });
        // 下の帯: 開始画面は左に「スキップ」。ほかは左に終了（開いたドア。確かめてから終える）と「戻る」。右に「次へ」。
        let (start, forward) = match step {
            Step::Intro => (
                vec![
                    components::button(
                        &theme,
                        "tutorial-skip",
                        i18n::t(cx, "tutorial.skip"),
                        Variant::OnBackground,
                    )
                    .on_click(act(end)),
                ],
                components::wizard_next(&theme, "tutorial-next", i18n::t(cx, "tutorial.start"))
                    .on_click(act(go(false))),
            ),
            _ => {
                let coach = self.panel;
                let dashboard = self.dashboard.clone();
                let quit = components::icon_button(
                    &theme,
                    "tutorial-quit",
                    &icons::DOOR_OPEN,
                    i18n::t(cx, "tutorial.quit"),
                    Variant::OnBackground,
                )
                .on_click(move |_, window, cx| {
                    let spec = quit_dialog(coach, dashboard.clone(), window, cx);
                    cx.default_global::<PanelControl>().spawns.push(spec);
                });
                (
                    vec![quit, back],
                    components::wizard_next(&theme, "tutorial-next", i18n::t(cx, "tutorial.next"))
                        .on_click(act(go(false))),
                )
            }
        };
        // 上の段: アイコン・見出しと説明・進み具合を横に並べる。コントローラの枠は下の帯の上に高さを固定し、
        // 説明の長さで位置を動かさない。
        let head = div()
            .flex_1()
            .min_h_0()
            .flex()
            .items_start()
            .gap(px(spacing::MD))
            .child(
                design::icon(step.icon())
                    .size(px(TITLE_ICON_PX))
                    .color(rgba(theme.accent_solid)),
            )
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .flex()
                    .flex_col()
                    .gap(px(spacing::SM))
                    .child(
                        div()
                            .text_size(px(font_size::LG))
                            .font_weight(FontWeight::BOLD)
                            .child(title),
                    )
                    .child(
                        div()
                            .text_size(px(font_size::MD))
                            .text_color(rgba(theme.neutral_text_low))
                            .child(body),
                    ),
            )
            .children(progress);
        components::wizard_panel(&theme)
            .debug_selector(move || format!("tutorial-step-{step:?}"))
            .child(
                div()
                    .size_full()
                    .p(px(WIZARD_MARGIN))
                    .flex()
                    .flex_col()
                    .gap(px(spacing::MD))
                    .child(head)
                    .child(
                        div()
                            .flex_none()
                            .h(px(VISUAL_PX))
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
                            .items_center()
                            .child(div().flex().gap(px(spacing::SM)).children(start))
                            .child(
                                div()
                                    .flex()
                                    .items_center()
                                    .gap(px(spacing::LG))
                                    .children(did)
                                    .child(forward),
                            ),
                    ),
            )
    }
}

/// チュートリアルを途中で終えるか確かめるダイアログ（案内のパネル `coach` の子）の起動仕様。
fn quit_dialog(
    coach: PanelId,
    dashboard: WeakEntity<Dashboard>,
    window: &Window,
    cx: &App,
) -> PanelSpec {
    let width = components::DIALOG_WIDTH;
    let height = components::dialog_height(
        window,
        &i18n::t(cx, "tutorial.quit_title"),
        &i18n::t(cx, "tutorial.quit_body"),
    );
    PanelSpec::child(coach, true, move |panel, _, _| QuitTutorial {
        panel,
        dashboard,
    })
    .sized(
        Vec2::new(width as f32 / 1000.0, height as f32 / 1000.0),
        (width, height),
    )
    .without_nav_bar()
}

/// チュートリアルを途中で終えるか確かめるダイアログ。`tutorial-quit-confirm` で終え、`tutorial-quit-cancel` なら
/// ダイアログだけ閉じて案内を続ける。
pub(crate) struct QuitTutorial {
    panel: PanelId,
    dashboard: WeakEntity<Dashboard>,
}

impl Render for QuitTutorial {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = Theme::current(cx);
        let (panel, dashboard) = (self.panel, self.dashboard.clone());
        components::dialog(
            &theme,
            "tutorial-quit-dialog",
            &icons::DOOR_OPEN,
            i18n::t(cx, "tutorial.quit_title"),
            i18n::t(cx, "tutorial.quit_body"),
            [
                components::button(
                    &theme,
                    "tutorial-quit-cancel",
                    i18n::t(cx, "common.cancel"),
                    Variant::Default,
                )
                .on_click(move |_, _, cx| ui::command(panel, PanelCommand::Close, cx)),
                components::button(
                    &theme,
                    "tutorial-quit-confirm",
                    i18n::t(cx, "tutorial.quit_confirm"),
                    Variant::Danger,
                )
                .on_click(move |_, _, cx| {
                    ui::command(panel, PanelCommand::Close, cx);
                    let _ = dashboard.update(cx, |d, cx| d.end_tutorial(cx));
                }),
            ],
        )
        // ドアの形が塗りでは潰れて判別しにくいため、線描画にする。
        .weight(design::Weight::Regular)
    }
}

/// 練習用のウィンドウ: 映したアプリの代わりに、マウスの操作を試す。左・右・中クリックの的と、スクロールするリスト。
pub(crate) struct Practice {
    dashboard: WeakEntity<Dashboard>,
    /// 左・右・中クリックで的を押したか。
    pub(crate) clicked: [bool; 3],
    /// リストをスクロールしたか。
    pub(crate) scrolled: bool,
}

/// 練習用のウィンドウのクリックの的: (ボタン, ID, アイコン, 名前のキー（`guide.control`）)。
const TARGETS: [(MouseButton, &str, &Icon, Control); 3] = [
    (
        MouseButton::Left,
        "practice-left",
        &icons::MOUSE_LEFT_CLICK,
        Control::Select,
    ),
    (
        MouseButton::Right,
        "practice-right",
        &icons::MOUSE_RIGHT_CLICK,
        Control::RightClick,
    ),
    (
        MouseButton::Middle,
        "practice-middle",
        &icons::MOUSE_MIDDLE_CLICK,
        Control::MiddleClick,
    ),
];
/// クリックの的の高さと、スクロールするリストの行数・行の高さ（px）。
const TARGET_PX: f32 = 150.0;
const PRACTICE_ROWS: usize = 12;
const PRACTICE_ROW_PX: f32 = 48.0;

impl Practice {
    /// 試せる操作をすべて試したら、案内に知らせる。右・中クリックは、利き手のコントローラにある場合だけ数える。
    fn check(&self, cx: &mut Context<Self>) {
        let hand = ui::settings(cx).dominant_hand.hand();
        let own = controller(hand, cx);
        let all = TARGETS
            .iter()
            .zip(self.clicked)
            .all(|((_, _, _, c), done)| {
                done || (*c != Control::Select && own.parts(*c, hand).is_empty())
            });
        if all && self.scrolled {
            let _ = self.dashboard.update(cx, |d, cx| d.practice_done(cx));
        }
    }
}

impl Render for Practice {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = Theme::current(cx);
        let hand = ui::settings(cx).dominant_hand.hand();
        let own = controller(hand, cx);
        let accent = rgba(theme.accent_solid);
        let targets = TARGETS
            .iter()
            .enumerate()
            .map(|(i, &(button, id, glyph, control))| {
                let done = self.clicked[i];
                // 右・中クリックは、親指を触れておくボタンのある機種だけ（無ければそう書く）。
                let unavailable = control != Control::Select && own.parts(control, hand).is_empty();
                let label = match control {
                    Control::Select => i18n::t(cx, "tutorial.practice_left"),
                    other => guide::control_name(cx, other),
                };
                let note = if done {
                    i18n::t(cx, "tutorial.did")
                } else if unavailable {
                    i18n::t(cx, "tutorial.practice_unavailable")
                } else {
                    i18n::t(cx, "tutorial.practice_try")
                };
                div()
                    .id(id)
                    .debug_selector(move || id.into())
                    .flex_1()
                    .h(px(TARGET_PX))
                    .flex()
                    .flex_col()
                    .items_center()
                    .justify_center()
                    .gap(px(spacing::SM))
                    .rounded(px(design::radius::LG))
                    .bg(rgba(theme.control))
                    .border_2()
                    .border_color(if done {
                        accent
                    } else {
                        rgba(theme.neutral_border_subtle)
                    })
                    .when(unavailable && !done, |d| d.opacity(0.5))
                    .child(
                        design::icon(if done { &icons::CHECK_CIRCLE } else { glyph })
                            .size(px(TITLE_ICON_PX))
                            .color(if done {
                                accent
                            } else {
                                rgba(theme.neutral_text)
                            }),
                    )
                    .child(div().font_weight(FontWeight::BOLD).child(label))
                    .child(
                        div()
                            .text_size(px(font_size::SM))
                            .text_color(rgba(theme.neutral_text_low))
                            .child(note),
                    )
                    .on_mouse_down(
                        button,
                        cx.listener(move |this, _, _, cx| {
                            if !this.clicked[i] {
                                this.clicked[i] = true;
                                this.check(cx);
                                cx.notify();
                            }
                        }),
                    )
            });
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
            .child(div().flex().gap(px(spacing::MD)).children(targets))
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(spacing::SM))
                    .font_weight(FontWeight::BOLD)
                    .when(self.scrolled, |d| d.text_color(accent))
                    .child(
                        design::icon(if self.scrolled {
                            &icons::CHECK_CIRCLE
                        } else {
                            &icons::MOUSE_SCROLL
                        })
                        .size(px(font_size::LG)),
                    )
                    .child(i18n::t(cx, "tutorial.practice_scroll")),
            )
            .child(
                div()
                    .id("practice-list")
                    .debug_selector(|| "practice-list".into())
                    .flex_1()
                    .min_h_0()
                    .overflow_y_scroll()
                    .rounded(px(design::radius::LG))
                    .bg(rgba(theme.control))
                    .flex()
                    .flex_col()
                    .children(rows)
                    .on_scroll_wheel(cx.listener(|this, _, _, cx| {
                        if !this.scrolled {
                            this.scrolled = true;
                            this.check(cx);
                            cx.notify();
                        }
                    })),
            )
    }
}

#[cfg(test)]
mod tests;
