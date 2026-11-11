//! Layer 5: 画面。セットアップウィザード（初回起動と、デバッグからのやり直し）。nyaw を使い始めるのに要る設定を順に決める。
//! 見た目と遷移は `design::components` のウィザード（`wizard_*` + `navigation`）。選んだものは、その場で全体の設定に反映する。

use design::components::{self, Button, Wizard};
use design::{Accent, Icon, Logo, Mode, Theme, font_size, icons, spacing};
use gpui::{
    App, ClickEvent, Context, Div, Entity, FontWeight, IntoElement, Render, SharedString, Window,
    div, prelude::*, px,
};
use kernel::vr::{Pose, Quat, Vec2, Vec3};
use kernel::{
    DominantHand, Language, LazyFollow, PanelCommand, PanelId, PersonalColor, Setting, ThemeMode,
    WristDesign, WristFace,
};
use ui::{PanelControl, PanelSpec};

/// 1:1 のパネル。
const SIZE: Vec2 = Vec2::new(0.8, 0.8);
const PX: (u32, u32) = (800, 800);
/// 一枚目のロゴの高さ。
const LOGO_HEIGHT: f32 = 72.0;
/// 選べるカードに載せるアイコンの大きさ。
const CHOICE_ICON: f32 = 48.0;
/// パーソナルカラーのスライダーの長さ（残りの幅に見本を置く）。
const PICKER_WIDTH: f32 = 340.0;
/// 文字盤の見本の直径。
const FACE_PX: f32 = 104.0;
/// 頭から水平にこの距離（m）を保って付いてくる。
const FOLLOW_DISTANCE: f32 = 1.0;
/// 目の前（頭から 1m 先）。
const IN_FRONT: Pose = Pose::new(Vec3::new(0.0, 0.0, -1.0), Quat::IDENTITY);

/// 手順。[`STEPS`] の順に進む。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Step {
    /// ようこそと、言語。
    Language,
    /// 利き手。
    Hand,
    /// テーマ（ライト / ダーク）。
    Theme,
    /// パーソナルカラー。
    Color,
    /// wrist の文字盤の種類とデザイン。
    Wrist,
    /// 完了。
    Done,
}

pub const STEPS: [Step; 6] = [
    Step::Language,
    Step::Hand,
    Step::Theme,
    Step::Color,
    Step::Wrist,
    Step::Done,
];

/// wrist の文字盤の見本（デザインと直径 px）を描く関数。画面どうしは直接依存しないので、main が `wrist::preview` を渡す。
pub type FacePreview = fn(WristDesign, f32, &App) -> Div;

/// 頭の向き（左右だけ）と歩きに付いてくる設定。本番の起動仕様とテストで同じものを使う。
pub fn follow() -> LazyFollow {
    LazyFollow::new(FOLLOW_DISTANCE)
}

/// 初回起動で、目の前に出すウィザードの起動仕様。
pub fn spec(faces: FacePreview) -> PanelSpec {
    // 開いた元の無いパネルなので、頭の前に出す。
    PanelSpec::root(IN_FRONT, move |id, _, _| SetupWizard::new(id, faces))
        .in_view(IN_FRONT)
        .sized(SIZE, PX)
        // 閉じられない（ロックダウン中は閉じる要求も無視される）ので、閉じるボタンの付いたバーは付けない。
        .without_nav_bar()
        .lockdown()
        .lazy_follow(follow())
}

/// デバッグから強制的にやり直す: パネル `from` の隣に開く。
pub fn spawn_spec(from: PanelId, faces: FacePreview) -> PanelSpec {
    PanelSpec::spawn(from, move |id, _, _| SetupWizard::new(id, faces))
        .sized(SIZE, PX)
        .without_nav_bar()
        .lockdown()
        .lazy_follow(follow())
}

pub struct SetupWizard {
    panel: PanelId,
    pub wizard: Wizard,
    faces: FacePreview,
}

impl SetupWizard {
    pub fn new(panel: PanelId, faces: FacePreview) -> Self {
        Self {
            panel,
            wizard: Wizard::new(STEPS.len()),
            faces,
        }
    }

    /// 今の手順。
    pub fn step(&self) -> Step {
        STEPS[self.wizard.step()]
    }
}

/// 終えたことを設定に残し、ロックダウンを放してからこのパネルを閉じる（同じパネルのコマンドは積んだ順に処理される）。
fn finish(panel: PanelId, cx: &mut App) {
    let control = cx.default_global::<PanelControl>();
    control.setting_changes.push(Setting::SetupDone(true));
    let commands = &mut control.panels.entry(panel).or_default().commands;
    commands.push(PanelCommand::Lockdown(false));
    commands.push(PanelCommand::Close);
}

/// 押すと全体の設定を `setting` に変える（フレームループがすぐ反映し、この画面も新しい設定で描き直る）。
fn set(setting: Setting) -> impl Fn(&ClickEvent, &mut Window, &mut App) + 'static {
    move |_, _, cx| {
        cx.default_global::<PanelControl>()
            .setting_changes
            .push(setting)
    }
}

fn language(lang: i18n::Lang) -> Language {
    match lang {
        i18n::Lang::En => Language::En,
        i18n::Lang::Ja => Language::Ja,
    }
}

/// 選べるカードを横に並べる（幅は等分）。
fn row(choices: impl IntoIterator<Item = Button>) -> Div {
    div()
        .flex()
        .gap(px(spacing::MD))
        .children(choices.into_iter().map(|c| c.flex_1().min_w_0()))
}

/// 選べるカードの中身: 見本やアイコンと名前を、縦に中央揃えで積む。
fn stack() -> Div {
    div()
        .w_full()
        .flex()
        .flex_col()
        .items_center()
        .gap(px(spacing::SM))
}

/// 選べるカードの下に出す名前。
fn name(text: impl Into<SharedString>) -> Div {
    div().min_w_0().truncate().child(text.into())
}

/// 文字が主役の選べるカード（大きめの太字。`glyph` があれば上にアイコン）。
fn text_choice(
    theme: &Theme,
    id: impl Into<SharedString>,
    selected: bool,
    glyph: Option<&'static Icon>,
    label: impl Into<SharedString>,
) -> Button {
    let content = stack()
        .children(glyph.map(|g| design::icon(g).size(px(CHOICE_ICON))))
        .child(
            name(label)
                .text_size(px(font_size::LG))
                .font_weight(FontWeight::BOLD),
        );
    components::choice(theme, id, selected, content).py(px(spacing::XL))
}

/// 手順 `step` の画面 1 枚。今の設定（[`PanelControl::settings`]）から選んでいるものを示す。
fn page(
    step: Step,
    panel: PanelId,
    faces: FacePreview,
    wizard: &Entity<SetupWizard>,
    cx: &App,
) -> Div {
    let theme = Theme::current(cx);
    let now = cx.try_global::<PanelControl>().map(|c| c.settings);
    let now = now.unwrap_or_default();
    let go = |forward: bool| {
        let wizard = wizard.clone();
        move |_: &ClickEvent, _: &mut Window, cx: &mut App| {
            wizard.update(cx, |v, cx| {
                if forward {
                    v.wizard.next()
                } else {
                    v.wizard.back()
                };
                cx.notify();
            })
        }
    };
    let next = |label: SharedString| {
        components::wizard_next(&theme, "setup-next", label).on_click(go(true))
    };
    // 二枚目からの画面: アイコン・見出し・説明・選ぶもの・「戻る」「次へ」。
    let choose =
        |glyph: &'static Icon, title: SharedString, description: SharedString, body: Div| {
            components::wizard_step(
                &theme,
                "setup",
                components::wizard_icon(&theme, glyph),
                title,
                description,
                body,
                Some(
                    components::wizard_back(&theme, "setup-back", i18n::t(cx, "setup.back"))
                        .on_click(go(false)),
                ),
                next(i18n::t(cx, "setup.next")),
            )
        };
    match step {
        Step::Language => {
            // どの言語で表示していても読めるよう、案内は対応している言語すべてで出す（今の言語が先頭）。
            let mut langs = i18n::Lang::ALL;
            langs.sort_by_key(|&l| language(l) != now.language);
            let prompt = langs.map(|l| i18n::translate(l, "setup.language_prompt").to_string());
            let choices = i18n::Lang::ALL.map(|l| {
                let id = format!("setup-lang-{}", l.code());
                // 言語の名前は、その言語自身で出す。
                text_choice(
                    &theme,
                    id,
                    language(l) == now.language,
                    None,
                    l.native_name(),
                )
                .on_click(set(Setting::Language(language(l))))
            });
            components::wizard_step(
                &theme,
                "setup",
                design::logo(Logo::Full).height(px(LOGO_HEIGHT)),
                i18n::t(cx, "setup.welcome_title"),
                prompt.join("\n"),
                row(choices),
                None,
                next(i18n::t(cx, "setup.start")),
            )
        }
        Step::Hand => {
            let (left, right) = (i18n::t(cx, "settings.left"), i18n::t(cx, "settings.right"));
            let hands = [
                (DominantHand::Left, "setup-hand-left", left),
                (DominantHand::Right, "setup-hand-right", right),
            ];
            let choices = hands.map(|(hand, id, label)| {
                let selected = now.dominant_hand == hand;
                text_choice(&theme, id, selected, Some(&icons::HAND), label)
                    .on_click(set(Setting::DominantHand(hand)))
            });
            choose(
                &icons::HAND,
                i18n::t(cx, "setup.hand_title"),
                i18n::t(cx, "setup.hand_description"),
                row(choices),
            )
        }
        Step::Theme => {
            let accent = accent(now.personal_color);
            let (dark, light) = (i18n::t(cx, "theme.dark"), i18n::t(cx, "theme.light"));
            let modes = [
                (ThemeMode::Dark, Mode::Dark, "setup-theme-dark", dark),
                (ThemeMode::Light, Mode::Light, "setup-theme-light", light),
            ];
            let choices = modes.map(|(mode, look, id, label)| {
                // 見本は、そのテーマと今のパーソナルカラーで描く。
                let content = stack()
                    .child(components::theme_thumbnail(&Theme::with(look, accent)))
                    .child(name(label));
                components::choice(&theme, id, now.theme == mode, content)
                    .on_click(set(Setting::Theme(mode)))
            });
            choose(
                &icons::PALETTE,
                i18n::t(cx, "setup.theme_title"),
                i18n::t(cx, "setup.theme_description"),
                row(choices),
            )
        }
        Step::Color => {
            let picker = components::accent_picker(
                &theme,
                accent(now.personal_color),
                PICKER_WIDTH,
                |name| i18n::t(cx, &format!("color.{name}")),
                |a, _, cx| {
                    let color = PersonalColor {
                        lightness: a.lightness,
                        chroma: a.chroma,
                        hue: a.hue,
                    };
                    let control = cx.default_global::<PanelControl>();
                    control.setting_changes.push(Setting::PersonalColor(color));
                },
            );
            // 選んだ色が実際の部品でどう見えるかの見本（上に載る文字の色も付いてくる）。
            let preview = components::theme_preview(
                &theme,
                "setup-preview",
                i18n::t(cx, "settings.personal_color"),
                i18n::t(cx, "settings.preview_body"),
                i18n::t(cx, "common.ok"),
            );
            let body = div()
                .flex()
                .items_center()
                .gap(px(spacing::LG))
                .child(picker.w(px(PICKER_WIDTH)).flex_none())
                .child(preview.flex_1().min_w_0());
            choose(
                &icons::PAINT_BRUSH,
                i18n::t(cx, "setup.color_title"),
                i18n::t(cx, "setup.color_description"),
                body,
            )
        }
        Step::Wrist => {
            let face = now.wrist_design.face();
            // 見本つきの選べるカード。押すと `design` にする。
            let choice = |id: String, design: WristDesign, selected: bool, label: String| {
                let content = stack()
                    .child(faces(design, FACE_PX, cx))
                    .child(name(i18n::t(cx, &label)));
                components::choice(&theme, id, selected, content)
                    .on_click(set(Setting::WristDesign(design)))
            };
            // 上の段は種類（選んでいる種類は今のデザイン、ほかはその種類の先頭のデザインを見本にする）。
            let kinds = WristFace::ALL.iter().map(|&f| {
                let design = if f == face {
                    now.wrist_design
                } else {
                    f.designs()[0]
                };
                let (id, label) = (
                    format!("setup-face-{}", f.key()),
                    format!("wrist.face.{}", f.key()),
                );
                choice(id, design, f == face, label)
            });
            // 下の段は、選んでいる種類のデザイン。
            let designs = face.designs().iter().map(|&d| {
                let (id, label) = (
                    format!("setup-design-{}", d.key()),
                    format!("wrist.design.{}", d.key()),
                );
                choice(id, d, d == now.wrist_design, label)
            });
            let body = div()
                .flex()
                .flex_col()
                .gap(px(spacing::MD))
                .child(row(kinds.collect::<Vec<_>>()))
                .child(row(designs.collect::<Vec<_>>()));
            choose(
                &icons::WATCH,
                i18n::t(cx, "setup.wrist_title"),
                i18n::t(cx, "setup.wrist_description"),
                body,
            )
        }
        Step::Done => {
            let done = components::wizard_next(&theme, "setup-finish", i18n::t(cx, "setup.finish"))
                .on_click(move |_, _, cx| finish(panel, cx));
            components::wizard_welcome(
                &theme,
                "setup",
                components::wizard_icon(&theme, &icons::CHECK_CIRCLE),
                i18n::t(cx, "setup.done_title"),
                i18n::t(cx, "setup.done_description"),
                [done],
            )
        }
    }
}

/// 設定のパーソナルカラー（選んでいなければ既定）を、見た目の色にする。
fn accent(color: Option<PersonalColor>) -> Accent {
    color.map_or(Accent::DEFAULT, |c| {
        Accent::new(c.lightness, c.chroma, c.hue)
    })
}

impl Render for SetupWizard {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = Theme::current(cx);
        let this = cx.entity();
        let (panel, faces) = (self.panel, self.faces);
        let stack: Vec<usize> = (0..=self.wizard.step()).collect();
        let draw = {
            let this = this.clone();
            move |&index: &usize, _: &mut Window, cx: &mut App| {
                page(STEPS[index], panel, faces, &this, cx).into_any_element()
            }
        };
        let swipe_back = move |_: &mut Window, cx: &mut App| {
            this.update(cx, |v, cx| {
                v.wizard.back();
                cx.notify();
            })
        };
        components::wizard_panel(&theme).child(
            components::navigation(&theme, "setup-nav", &stack, draw, swipe_back)
                .background(theme.card),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn steps_start_with_the_language_and_end_with_done() {
        assert_eq!(STEPS.first(), Some(&Step::Language));
        assert_eq!(STEPS.last(), Some(&Step::Done));
        let wizard = SetupWizard::new(0, |_, _, _| div());
        assert_eq!(wizard.step(), Step::Language);
    }
}
