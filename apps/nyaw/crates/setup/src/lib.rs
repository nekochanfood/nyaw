//! Layer 5 画面: セットアップウィザード。初回起動およびデバッグからの再実行時に、必要な設定を順に決定する。
//! 外観と遷移は `design::components` のウィザード（`wizard_*` + `navigation`）。パネルの地は設定画面のコンテンツ側と同じ背景色で、
//! アクセシビリティ・外観・文字盤は設定画面と同一の部品（`ui::prefs`）をそのまま配置する。選択内容は即座に全体設定へ反映する。

use design::components::{self, Button, Variant, WIZARD_CONTENT, WIZARD_MARGIN, Wizard};
use design::{Icon, Logo, Theme, font_size, icons, spacing};
use gpui::{
    App, ClickEvent, Context, Div, Entity, FontWeight, IntoElement, MouseDownEvent, Render,
    SharedString, Window, div, prelude::*, px,
};
use guide::{Control, Controller, Model};
use kernel::vr::{Pose, Quat, Vec2, Vec3};
use kernel::{DominantHand, Hand, LazyFollow, PanelCommand, PanelId, Setting, Settings};
use ui::{PanelControl, PanelSpec};

/// パネルサイズ（1px = 1mm）。オーバーレイの既定と同じ 4:3 で、最も高さのある文字盤ステップ（見出し・設定画面と同一の
/// 部品・フッター）が収まるよう、既定（0.8m × 0.6m）より拡大する。
const SIZE: Vec2 = Vec2::new(1.152, 0.864);
const PX: (u32, u32) = (1152, 864);
/// 一枚目のロゴの高さ。
const LOGO_HEIGHT: f32 = 176.0;
/// 選択カードに配置するアイコンのサイズ。
const CHOICE_ICON: f32 = 48.0;
/// 頭部から水平にこの距離（m）を保って追従する。パネルが大きいため、近すぎると視界が覆われ、UI スケール（面サイズ最大
/// 1.5 倍）を適用すると視野から逸脱する。1.5m なら等倍で横幅が視野の 42°、1.5 倍で 60° 程度。
const FOLLOW_DISTANCE: f32 = 1.5;
/// 正面（頭部から [`FOLLOW_DISTANCE`] 先）。
const IN_FRONT: Pose = Pose::new(Vec3::new(0.0, 0.0, -FOLLOW_DISTANCE), Quat::IDENTITY);
/// 完了画面で、下に寄って映すコントローラの枠の高さ（画面の高さに対する割合）と、その上端で地へ溶かす高さ（枠に対する割合）。
/// 文字が大きく見出しが下へ伸びても、溶けた部分に重なるだけで読める。
const ZOOM_H: f32 = 0.68;
const ZOOM_FADE: f32 = 0.3;
/// 完了画面の見出しを置く上の帯（画面の高さに対する割合）。
const DONE_HEAD_H: f32 = 0.34;
/// 開発ビルドだけ、完了画面の右上にコントローラを切り替えるドロップダウンを出す（機種ごとの映り方を確かめる）。
const CONTROLLER_PICKER: bool = cfg!(debug_assertions);

/// ステップ。[`STEPS`] の順に遷移する。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Step {
    /// ウェルカム画面（言語もここで変更可能）。
    Welcome,
    /// 利き手。
    Hand,
    /// アクセシビリティ（文字サイズ・UI スケール・太字）。視認性を先に決定してから外観を選択する。
    Accessibility,
    /// 外観（テーマとパーソナルカラー）。
    Appearance,
    /// wrist の文字盤。
    Wrist,
    /// SteamVR との自動起動設定。OpenVR の場合のみ表示する（[`shown`]）。
    AutoLaunch,
    /// 完了。
    Done,
}

pub const STEPS: [Step; 7] = [
    Step::Welcome,
    Step::Hand,
    Step::Accessibility,
    Step::Appearance,
    Step::Wrist,
    Step::AutoLaunch,
    Step::Done,
];

/// 現在のランタイムでステップ `step` を表示するか。自動起動を切り替え可能なのは OpenVR のみ（他は `auto_launch` が `None`）。
pub fn shown(step: Step, now: &Settings) -> bool {
    step != Step::AutoLaunch || now.auto_launch.is_some()
}

/// 頭部の向き（水平のみ）と歩行に追従する設定。本番の起動仕様とテストで共通使用する。
pub fn follow() -> LazyFollow {
    LazyFollow::new(FOLLOW_DISTANCE)
}

/// 初回起動で正面に表示するウィザードの起動仕様。
pub fn spec() -> PanelSpec {
    // 親パネルのないルートパネルのため、頭部の正面に表示する。
    PanelSpec::root(IN_FRONT, |id, _, cx| SetupWizard::build(id, cx))
        .in_view(IN_FRONT)
        .sized(SIZE, PX)
        // 閉じ不可（ロックダウン中は閉じる要求も無視される）のため、ナビゲーションバーを付与しない。
        .without_nav_bar()
        .lockdown()
        .lazy_follow(follow())
}

/// デバッグからの強制再実行: パネル `from` の隣に開く。
pub fn spawn_spec(from: PanelId) -> PanelSpec {
    PanelSpec::spawn(from, |id, _, cx| SetupWizard::build(id, cx))
        .sized(SIZE, PX)
        .without_nav_bar()
        .lockdown()
        .lazy_follow(follow())
}

pub struct SetupWizard {
    panel: PanelId,
    pub wizard: Wizard,
    /// 最初の画面の言語ドロップダウンを展開中。
    pub language_open: bool,
    /// 外観・文字盤の部品（`ui::prefs`）の状態。
    pub appearance: ui::prefs::Appearance,
    pub watch: ui::prefs::WatchFaces,
    /// 完了画面に映すコントローラ（開発ビルドのドロップダウンで選ぶ）。`None` は左手で使用中の機種。
    pub preview: Option<Model>,
    pub preview_open: bool,
    /// 反映したショートカットの二回押しの回数（[`kernel::PanelStatus::double_press`]）。
    presses: u32,
    /// 完了画面でロックダウンを解いている（ダッシュボードを開けるように）。
    unlocked: bool,
    /// 完了画面に入った時刻（コントローラが下から現れる動きの起点。`design::drive::now` の時計）。
    pub done_at: Option<std::time::Instant>,
}

impl SetupWizard {
    pub fn new(panel: PanelId) -> Self {
        Self {
            panel,
            wizard: Wizard::new(STEPS.len()),
            language_open: false,
            appearance: Default::default(),
            watch: Default::default(),
            preview: None,
            preview_open: false,
            presses: 0,
            unlocked: false,
            done_at: None,
        }
    }

    /// パネルとして開くウィザード: ショートカットの二回押し（[`kernel::PanelStatus::double_press`]）を見る。
    pub fn build(panel: PanelId, cx: &mut Context<Self>) -> Self {
        cx.observe_global::<PanelControl>(Self::sync).detach();
        let mut v = Self::new(panel);
        v.presses = presses(panel, cx);
        v
    }

    /// 完了画面ではロックダウンを解き、ショートカットの二回押しでダッシュボードを開けるようにする（開くのは wrist）。
    /// その二回押しで完了する。ほかの画面ではロックダウンを保つ。押した回数が変わるたび・画面を移るたびに呼ぶ。
    fn sync(&mut self, cx: &mut Context<Self>) {
        let n = presses(self.panel, cx);
        let pressed = n != self.presses;
        self.presses = n;
        let done = self.step() == Step::Done;
        if done != self.unlocked {
            self.unlocked = done;
            self.done_at = done.then(design::drive::now);
            ui::command(self.panel, PanelCommand::Lockdown(!done), cx);
        }
        if done && pressed {
            finish(self.panel, cx);
        }
    }

    /// 現在のステップ。
    pub fn step(&self) -> Step {
        STEPS[self.wizard.step()]
    }

    /// 次（`forward`）または前方の、表示対象ステップへ移動する。非表示ステップはスキップする（最初と最後は常に表示するため、端では移動しない）。
    pub fn go(&mut self, forward: bool, now: &Settings) {
        let mut w = self.wizard;
        loop {
            let moved = if forward { w.next() } else { w.back() };
            if !moved {
                return;
            }
            if shown(STEPS[w.step()], now) {
                self.wizard = w;
                return;
            }
        }
    }
}

/// パネル `panel` から見たショートカットの二回押しの回数。
fn presses(panel: PanelId, cx: &App) -> u32 {
    cx.try_global::<PanelControl>()
        .and_then(|c| c.panels.get(&panel))
        .map_or(0, |p| p.status.double_press)
}

/// 左手で使用中のコントローラ（無ければ Quest Touch Plus）。ショートカットは左手のボタン。
fn left_controller(cx: &App) -> Controller {
    cx.try_global::<PanelControl>()
        .and_then(|c| c.controllers.iter().find(|c| c.hand == Hand::Left))
        .and_then(|c| c.controller)
        .unwrap_or(Controller::of(Model::QuestPlus))
}

/// 完了を設定に記録し、ロックダウンを解除してからこのパネルを閉じる（同一パネルのコマンドはキュー順に処理される）。
fn finish(panel: PanelId, cx: &mut App) {
    let control = cx.default_global::<PanelControl>();
    control.setting_changes.push(Setting::SetupDone(true));
    let commands = &mut control.panels.entry(panel).or_default().commands;
    commands.push(PanelCommand::Lockdown(false));
    commands.push(PanelCommand::Close);
}

/// 全体設定を `setting` に変更する（フレームループが即座に反映し、この画面も新しい設定で再描画される）。
fn push(setting: Setting, cx: &mut App) {
    let control = cx.default_global::<PanelControl>();
    control.setting_changes.push(setting);
}

/// ウィザードの状態を更新して再描画するハンドラを生成する（部品の `set` やクリックハンドラに渡す）。
fn update<T: 'static>(
    wizard: &Entity<SetupWizard>,
    apply: fn(&mut SetupWizard, T),
) -> impl Fn(T, &mut Window, &mut App) + Clone + 'static {
    let wizard = wizard.clone();
    move |value, _, cx| {
        wizard.update(cx, |v, cx| {
            apply(v, value);
            cx.notify();
        })
    }
}

/// 選択カードを横並びに配置する（幅は均等分割）。
fn row(choices: impl IntoIterator<Item = Button>) -> Div {
    div()
        .flex()
        .gap(px(spacing::MD))
        .children(choices.into_iter().map(|c| c.flex_1().min_w_0()))
}

/// テキスト主体の選択カード（大きめの太字テキストの上にアイコン）。
fn text_choice(
    theme: &Theme,
    id: &'static str,
    selected: bool,
    glyph: &'static Icon,
    label: SharedString,
) -> Button {
    let content = div()
        .w_full()
        .flex()
        .flex_col()
        .items_center()
        .gap(px(spacing::SM))
        .child(design::icon(glyph).size(px(CHOICE_ICON)))
        .child(
            div()
                .min_w_0()
                .truncate()
                .text_size(px(font_size::LG))
                .font_weight(FontWeight::BOLD)
                .child(label),
        );
    components::choice(theme, id, selected, content).py(px(spacing::XL))
}

/// ステップ `step` の画面 1 枚。現在の設定（[`PanelControl::settings`]）から選択中の項目を示す。
fn page(step: Step, wizard: &Entity<SetupWizard>, window: &mut Window, cx: &mut App) -> Div {
    let theme = Theme::current(cx);
    let now = ui::settings(cx);
    let (panel, language_open, appearance, watch, preview, preview_open, done_at) = {
        let v = wizard.read(cx);
        (
            v.panel,
            v.language_open,
            v.appearance,
            v.watch,
            v.preview,
            v.preview_open,
            v.done_at,
        )
    };
    let go = |forward: bool| {
        let wizard = wizard.clone();
        move |_: &ClickEvent, _: &mut Window, cx: &mut App| {
            let now = ui::settings(cx);
            wizard.update(cx, |v, cx| {
                v.go(forward, &now);
                v.sync(cx);
                cx.notify();
            })
        }
    };
    // 2 枚目以降の画面: アイコン・見出し・説明・選択肢・「戻る」「次へ」。
    let choose = |glyph: &'static Icon, title: &str, description: &str, body: Div, cx: &App| {
        components::wizard_step(
            &theme,
            "setup",
            components::wizard_icon(&theme, glyph),
            i18n::t(cx, title),
            i18n::t(cx, description),
            body,
            Some(
                components::wizard_back(&theme, "setup-back", i18n::t(cx, "setup.back"))
                    .on_click(go(false)),
            ),
            components::wizard_next(&theme, "setup-next", i18n::t(cx, "setup.next"))
                .on_click(go(true)),
        )
    };
    match step {
        Step::Welcome => {
            // 言語は左下のドロップダウン（上方向に展開）で変更する。名前はその言語自体で表示する。
            let open = update(wizard, |v: &mut SetupWizard, open| v.language_open = open);
            let options = i18n::Lang::ALL.map(|l| {
                let close = open.clone();
                let id = format!("setup-lang-{}", l.code());
                let selected = ui::language(l) == now.language;
                components::select_option(&theme, id, l.native_name(), selected).on_click(
                    move |_, window, cx| {
                        push(Setting::Language(ui::language(l)), cx);
                        close(false, window, cx)
                    },
                )
            });
            let (show, dismiss) = (open.clone(), open);
            let popover = components::dropdown_popover(
                &theme,
                window,
                cx,
                "setup-language",
                language_open,
                true,
                options,
                move |_: &MouseDownEvent, window, cx| dismiss(false, window, cx),
            );
            let mut langs = i18n::Lang::ALL.into_iter();
            let current = langs.find(|&l| ui::language(l) == now.language);
            let languages = components::dropdown(
                &theme,
                "setup-language",
                Variant::OnBackground,
                Some(&icons::GLOBE),
                current.unwrap_or_default().native_name(),
            )
            .on_click(move |_, window, cx| show(true, window, cx))
            .child(popover);
            // 言語ドロップダウンの左にアプリ終了ボタン（確認ダイアログをウィザードの子として表示）。ロックダウン中はダッシュボードの
            // メニューを表示できないため、ここから終了可能にする。
            let quit = components::icon_button(
                &theme,
                "setup-quit",
                &icons::POWER,
                i18n::t(cx, "quit.label"),
                Variant::OnBackground,
            )
            .on_click(move |_, window, cx| {
                let spec =
                    ui::quit_dialog(window, cx, |build| PanelSpec::child(panel, true, build));
                cx.default_global::<PanelControl>().spawns.push(spec);
            });
            let others = div()
                .flex()
                .gap(px(spacing::SM))
                .child(quit)
                .child(languages);
            let start = components::wizard_next(&theme, "setup-next", i18n::t(cx, "setup.next"))
                .on_click(go(true));
            components::wizard_welcome(
                &theme,
                "setup",
                design::logo(Logo::Full).height(px(LOGO_HEIGHT)),
                i18n::t(cx, "setup.welcome_title"),
                i18n::t(cx, "setup.welcome_description"),
                Some(others),
                start,
            )
        }
        Step::Hand => {
            let hands = [
                (DominantHand::Left, "setup-hand-left", "settings.left"),
                (DominantHand::Right, "setup-hand-right", "settings.right"),
            ];
            let choices = hands.map(|(hand, id, label)| {
                let selected = now.dominant_hand == hand;
                text_choice(&theme, id, selected, &icons::HAND, i18n::t(cx, label))
                    .on_click(move |_, _, cx| push(Setting::DominantHand(hand), cx))
            });
            choose(
                &icons::HAND,
                "setup.hand_title",
                "setup.hand_description",
                row(choices),
                cx,
            )
        }
        Step::Accessibility => {
            // 操作中の値は部品が保持するため、ここに状態は不要。
            let body = ui::prefs::accessibility(window, cx);
            choose(
                &icons::PERSON_ARMS_SPREAD,
                "setup.accessibility_title",
                "setup.accessibility_description",
                body,
                cx,
            )
        }
        Step::Appearance => {
            let set = update(wizard, |v: &mut SetupWizard, state| v.appearance = state);
            let body = ui::prefs::appearance(panel, appearance, set, window, cx);
            choose(
                &icons::PALETTE,
                "setup.appearance_title",
                "setup.appearance_description",
                body,
                cx,
            )
        }
        Step::Wrist => {
            let set = update(wizard, |v: &mut SetupWizard, state| v.watch = state);
            let body = ui::prefs::watch_faces(watch, set, window, cx);
            choose(
                &icons::WATCH,
                "setup.wrist_title",
                "setup.wrist_description",
                body,
                cx,
            )
        }
        Step::AutoLaunch => {
            // 選択するまで無効のまま（SteamVR には何も登録しない）。同一項目を再選択しても SteamVR を操作しない。
            let on = now.auto_launch == Some(true);
            let options = [
                (
                    true,
                    "setup-auto-launch-on",
                    &icons::ROCKET_LAUNCH,
                    "settings.auto_launch",
                ),
                (
                    false,
                    "setup-auto-launch-off",
                    &icons::HAND_TAP,
                    "setup.auto_launch_off",
                ),
            ];
            let choices = options.map(|(value, id, glyph, label)| {
                text_choice(&theme, id, on == value, glyph, i18n::t(cx, label)).on_click(
                    move |_, _, cx| {
                        if on != value {
                            push(Setting::AutoLaunch(value), cx)
                        }
                    },
                )
            });
            choose(
                &icons::ROCKET_LAUNCH,
                "setup.auto_launch_title",
                "setup.auto_launch_description",
                row(choices),
                cx,
            )
        }
        Step::Done => {
            // 下に左手のコントローラを寄って映し、ショートカットのボタンをダブルタップの波紋で示す。それを押してダッシュボードを
            // 開くと完了する（[`SetupWizard::sync`]）。「完了」ボタンは置かない。
            let hand = Hand::Left;
            let controller = preview.map_or_else(|| left_controller(cx), Controller::of);
            let model = controller.model;
            let shortcut: Vec<&'static str> = controller
                .parts(Control::Shortcut, hand)
                .into_iter()
                .filter(|p| guide::parts(model, hand, p).iter().any(|q| q.visible))
                .collect();
            let description = match shortcut.first() {
                Some(p) => i18n::tf(
                    cx,
                    "setup.done_description",
                    &[("button", &guide::part_name(cx, p))],
                ),
                None => i18n::t(cx, "setup.done_description_any"),
            };
            let view = window.viewport_size();
            let (w, h) = (view.width, view.height * ZOOM_H);
            let zoom = guide::zoom(model, hand)
                .size(w, h)
                .parts(shortcut)
                .fade(h * ZOOM_FADE)
                .enter(done_at);
            let back = components::wizard_back(&theme, "setup-back", i18n::t(cx, "setup.back"))
                .on_click(go(false));
            let head = components::heading(
                &theme,
                "setup",
                components::wizard_icon(&theme, &icons::CHECK_CIRCLE),
                i18n::t(cx, "setup.done_title"),
                Some(description),
            )
            .max_w(px(WIZARD_CONTENT))
            .items_center()
            .text_center();
            div()
                .relative()
                .size_full()
                .child(div().absolute().left_0().bottom_0().child(zoom))
                .child(
                    div()
                        .absolute()
                        .top_0()
                        .left_0()
                        .w_full()
                        .h(view.height * DONE_HEAD_H)
                        .px(px(WIZARD_MARGIN))
                        .flex()
                        .items_end()
                        .justify_center()
                        .child(head),
                )
                .child(
                    div()
                        .absolute()
                        .left(px(WIZARD_MARGIN))
                        .bottom(px(WIZARD_MARGIN))
                        .child(back),
                )
                .when(CONTROLLER_PICKER, |d| {
                    d.child(
                        div()
                            .absolute()
                            .top(px(WIZARD_MARGIN))
                            .right(px(WIZARD_MARGIN))
                            .child(controller_picker(wizard, preview, preview_open, window, cx)),
                    )
                })
        }
    }
}

/// 完了画面に映すコントローラを選ぶドロップダウン（`setup-controller`。開発ビルドのみ）。先頭は使用中の機種、続いて全機種。
fn controller_picker(
    wizard: &Entity<SetupWizard>,
    preview: Option<Model>,
    open: bool,
    window: &mut Window,
    cx: &mut App,
) -> impl IntoElement {
    let theme = Theme::current(cx);
    let set_open = update(wizard, |v: &mut SetupWizard, open| v.preview_open = open);
    let pick = update(wizard, |v: &mut SetupWizard, model| {
        v.preview = model;
        v.preview_open = false;
    });
    let in_use = i18n::t(cx, "setup.controller_in_use");
    let choices = std::iter::once((None, "in-use".to_owned(), in_use.clone()))
        .chain(Model::ALL.map(|m| (Some(m), m.render_model(Hand::Left), m.name().into())));
    let options: Vec<_> = choices
        .map(|(model, id, label)| {
            let pick = pick.clone();
            components::select_option(
                &theme,
                format!("setup-controller-{id}"),
                label,
                preview == model,
            )
            .on_click(move |_, window, cx| pick(model, window, cx))
        })
        .collect();
    let dismiss = set_open.clone();
    let popover = components::dropdown_popover(
        &theme,
        window,
        cx,
        "setup-controller",
        open,
        false,
        options,
        move |_: &MouseDownEvent, window, cx| dismiss(false, window, cx),
    );
    let label: SharedString = preview.map_or(in_use, |m| m.name().into());
    components::dropdown(
        &theme,
        "setup-controller",
        Variant::OnBackground,
        None,
        label,
    )
    .on_click(move |_, window, cx| set_open(true, window, cx))
    .child(popover)
}

impl Render for SetupWizard {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = Theme::current(cx);
        let this = cx.entity();
        let now = ui::settings(cx);
        let stack: Vec<usize> = (0..=self.wizard.step())
            .filter(|&i| shown(STEPS[i], &now))
            .collect();
        let draw = move |&index: &usize, window: &mut Window, cx: &mut App| {
            page(STEPS[index], &this, window, cx).into_any_element()
        };
        // 戻る操作は「戻る」ボタンのみ。ページ自体はスワイプで移動しない（色や文字盤の選択中に横スワイプで戻るのを防止する）。
        components::wizard_panel(&theme).child(
            components::navigation(&theme, "setup-nav", &stack, draw, |_, _| {})
                .without_swipe_back(),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn steps_start_with_the_welcome_and_end_with_done() {
        assert_eq!(STEPS.first(), Some(&Step::Welcome));
        assert_eq!(STEPS.last(), Some(&Step::Done));
        assert_eq!(SetupWizard::new(0).step(), Step::Welcome);
    }

    /// アクセシビリティは、外観選択ステップの直前に位置する。
    #[test]
    fn accessibility_comes_right_before_appearance() {
        let at = |step| STEPS.iter().position(|&s| s == step).expect("a step");
        assert_eq!(at(Step::Accessibility) + 1, at(Step::Appearance));
        assert!(at(Step::Hand) < at(Step::Accessibility));
    }

    /// SteamVR の自動起動ステップは、切り替え可能なランタイム（OpenVR）の場合のみ通過する。他では前後ともスキップする。
    #[test]
    fn auto_launch_step_is_only_shown_on_openvr() {
        let openvr = Settings {
            auto_launch: Some(false),
            ..Default::default()
        };
        let other = Settings::default();
        let mut v = SetupWizard::new(0);
        for _ in 0..4 {
            v.go(true, &other);
        }
        assert_eq!(v.step(), Step::Wrist);
        v.go(true, &other);
        assert_eq!(v.step(), Step::Done);
        v.go(false, &other);
        assert_eq!(v.step(), Step::Wrist);
        v.go(true, &openvr);
        assert_eq!(v.step(), Step::AutoLaunch);
        v.go(true, &openvr);
        assert_eq!(v.step(), Step::Done);
        // 末端では移動しない。
        v.go(true, &openvr);
        assert_eq!(v.step(), Step::Done);
    }
}
