//! Layer 5 画面: セットアップウィザード。初回起動およびデバッグからの再実行時に、必要な設定を順に決定する。
//! 外観と遷移は `design::components` のウィザード（`wizard_*` + `navigation`）。パネルの地は設定画面のコンテンツ側と同じ背景色で、
//! アクセシビリティ・外観・文字盤は設定画面と同一の部品（`ui::prefs`）をそのまま配置する。選択内容は即座に全体設定へ反映する。

use design::components::{self, Button, Variant, Wizard};
use design::{Icon, Logo, Theme, font_size, icons, spacing};
use gpui::{
    App, ClickEvent, Context, Div, Entity, FontWeight, IntoElement, MouseDownEvent, Render,
    SharedString, Window, div, prelude::*, px,
};
use kernel::vr::{Pose, Quat, Vec2, Vec3};
use kernel::{DominantHand, LazyFollow, PanelCommand, PanelId, Setting, Settings};
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
    PanelSpec::root(IN_FRONT, |id, _, _| SetupWizard::new(id))
        .in_view(IN_FRONT)
        .sized(SIZE, PX)
        // 閉じ不可（ロックダウン中は閉じる要求も無視される）のため、ナビゲーションバーを付与しない。
        .without_nav_bar()
        .lockdown()
        .lazy_follow(follow())
}

/// デバッグからの強制再実行: パネル `from` の隣に開く。
pub fn spawn_spec(from: PanelId) -> PanelSpec {
    PanelSpec::spawn(from, |id, _, _| SetupWizard::new(id))
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
}

impl SetupWizard {
    pub fn new(panel: PanelId) -> Self {
        Self {
            panel,
            wizard: Wizard::new(STEPS.len()),
            language_open: false,
            appearance: Default::default(),
            watch: Default::default(),
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
    let (panel, language_open, appearance, watch) = {
        let v = wizard.read(cx);
        (v.panel, v.language_open, v.appearance, v.watch)
    };
    let go = |forward: bool| {
        let wizard = wizard.clone();
        move |_: &ClickEvent, _: &mut Window, cx: &mut App| {
            let now = ui::settings(cx);
            wizard.update(cx, |v, cx| {
                v.go(forward, &now);
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
            let done = components::wizard_next(&theme, "setup-finish", i18n::t(cx, "setup.finish"))
                .on_click(move |_, _, cx| finish(panel, cx));
            // 完了前に確認できるよう、左下に「戻る」を配置。
            let back = components::wizard_back(&theme, "setup-back", i18n::t(cx, "setup.back"))
                .on_click(go(false));
            components::wizard_welcome(
                &theme,
                "setup",
                components::wizard_icon(&theme, &icons::CHECK_CIRCLE),
                i18n::t(cx, "setup.done_title"),
                i18n::t(cx, "setup.done_description"),
                Some(back),
                done,
            )
        }
    }
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
