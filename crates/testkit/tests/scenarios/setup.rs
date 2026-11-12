use gpui::{TestAppContext, px};
use kernel::{DominantHand, Language, PanelCommand, ThemeMode, WristDesign};
use setup::{SetupWizard, Step};
use testing::{SetupWizardSpawn, Testing};
use testkit::{Driver, NoVrKit, VrKit};

/// The wizard walks through every decision. Each choice applies at once, and only Finish marks the setup done.
#[gpui::test]
fn setup_decides_each_setting_and_marks_done(cx: &mut TestAppContext) {
    let mut kit = NoVrKit::new(cx, |_, _| SetupWizard::new(0));
    assert!(!kit.kernel.settings().setup_done);
    // 起動仕様（`setup::spec`）と同じく、開いた時点からロックダウンを取る。開いている間は閉じられない。
    kit.kernel.apply(0, PanelCommand::Lockdown(true));
    kit.step();
    assert!(kit.kernel.settings().lockdown);
    kit.kernel.apply(0, PanelCommand::Close);
    assert!(!kit.kernel.panel(0).closed);

    // 一枚目: 言語は「はじめる」の下のドロップダウンで、選ぶとその場で切り替わる（「はじめる」を待たない）。
    assert_eq!(kit.read(|v| v.step()), Step::Welcome);
    kit.click("setup-language");
    assert!(kit.read(|v| v.language_open));
    kit.click("setup-lang-ja");
    assert_eq!(kit.kernel.settings().language, Language::Ja);
    assert!(!kit.read(|v| v.language_open), "選ぶと閉じる");
    kit.click("setup-next");

    // 利き手。戻っても選んだものは残る。
    assert_eq!(kit.read(|v| v.step()), Step::Hand);
    kit.click("setup-hand-left");
    assert_eq!(kit.kernel.settings().dominant_hand, DominantHand::Left);
    kit.click("setup-back");
    assert_eq!(kit.read(|v| v.step()), Step::Welcome);
    kit.click("setup-next").click("setup-next");
    assert_eq!(kit.kernel.settings().dominant_hand, DominantHand::Left);

    // 外観: テーマとパーソナルカラーを 1 枚で決める（設定画面と同じ部品。上から見本・テーマ・色）。
    assert_eq!(kit.read(|v| v.step()), Step::Appearance);
    let [preview, theme, colors] =
        ["theme-preview", "setting-theme", "accent-custom"].map(|s| kit.bounds(s));
    assert!(preview.bottom() <= theme.top() && theme.bottom() <= colors.top());
    kit.click("setting-theme").click("setting-theme-light");
    assert_eq!(kit.kernel.settings().theme, ThemeMode::Light);
    assert_eq!(kit.kernel.settings().personal_color, None);
    kit.click("accent-pink");
    let pink = kit.kernel.settings().personal_color.expect("picked");
    assert_eq!(pink.hue, 350.0);
    kit.click("setup-next");

    // 文字盤: 回すだけでは変わらず、「設定する」で決まる。種類で絞り込める。
    assert_eq!(kit.read(|v| v.step()), Step::Wrist);
    kit.click("watch-next");
    assert_eq!(kit.kernel.settings().wrist_design, WristDesign::Dots);
    kit.click("watch-set");
    assert_eq!(kit.kernel.settings().wrist_design, WristDesign::Ring);
    kit.click("watch-kind")
        .click("watch-kind-analog")
        .click("watch-set");
    assert_eq!(kit.kernel.settings().wrist_design, WristDesign::Minimal);
    kit.click("setup-next");

    // 完了を押すまでは終えたことにならない。
    assert!(kit.read(|v| v.wizard.is_last()));
    kit.step();
    assert!(!kit.kernel.settings().setup_done);
    kit.click("setup-finish").step();
    assert!(kit.kernel.settings().setup_done);
    assert!(!kit.kernel.settings().lockdown, "終えたら外れる");
    assert!(kit.kernel.panel(0).closed);
}

/// The first screen: the logo and one line sit in the middle of the screen; Start is at the bottom with the language
/// dropdown under it, which opens upward.
#[gpui::test]
fn welcome_centers_the_logo_and_puts_the_language_under_start(cx: &mut TestAppContext) {
    let mut kit = VrKit::new(cx, |_, _| SetupWizard::new(0));
    let panel = kit.bounds("setup-nav");
    let [hero, title, start, language] =
        ["setup-hero", "setup-title", "setup-next", "setup-language"].map(|s| kit.bounds(s));
    let near = |a: gpui::Pixels, b: gpui::Pixels| (a - b).abs() < px(2.0);
    // ロゴと一言のかたまりが、画面の上下左右中央。ロゴは大きい。
    assert!(near(hero.center().x, panel.center().x) && near(title.center().x, panel.center().x));
    assert!(near((hero.top() + title.bottom()) / 2.0, panel.center().y));
    assert!(hero.size.height >= px(160.0), "{:?}", hero.size);
    // 「はじめる」は下の帯の左右中央。その下に言語のドロップダウン。
    assert!(title.bottom() < start.top() && start.bottom() <= language.top());
    assert!(
        near(start.center().x, panel.center().x) && near(language.center().x, panel.center().x)
    );
    // ドロップダウンは上へ開く（下は画面の縁）。
    kit.click("setup-language").step();
    let options = ["setup-lang-en", "setup-lang-ja"].map(|s| kit.bounds(s));
    let language = kit.bounds("setup-language");
    assert!(options.iter().all(|o| o.bottom() <= language.top()));
    // 外を押すと、選ばずに閉じる。
    kit.click("setup-title");
    assert!(!kit.read(|v| v.language_open));
    assert_eq!(kit.read(|v| v.step()), Step::Welcome);
}

/// On the appearance step, the dialog that makes a custom color opens as the wizard's child, even in lockdown.
#[gpui::test]
fn setup_makes_a_custom_color_in_a_child_dialog(cx: &mut TestAppContext) {
    let mut kit = VrKit::new(cx, |_, _| SetupWizard::new(0));
    kit.kernel.apply(0, PanelCommand::Lockdown(true));
    kit.click("setup-next").click("setup-next");
    assert_eq!(kit.read(|v| v.step()), Step::Appearance);
    kit.click("accent-custom").step().step();
    let dialog = 1;
    assert_eq!(kit.kernel.status(dialog).parent, Some(0));
    let tint = kit.kernel.subsystems.backend.overlays[0].tint;
    assert!(tint < 1.0, "the wizard behind is dimmed");
    kit.panel(dialog).click("color-ok").step().step();
    assert!(kit.kernel.panel(dialog).closed);
    let made = kit.kernel.settings().personal_color;
    assert!(made.is_some());
    assert_eq!(kit.kernel.settings().color_history[0], made);
    assert!(
        kit.kernel.settings().lockdown,
        "the wizard keeps the lockdown"
    );
    kit.panel(0).click("setup-next");
    assert_eq!(kit.read(|v| v.step()), Step::Wrist);
}

#[gpui::test]
fn testing_can_retry_the_setup_wizard(cx: &mut TestAppContext) {
    let mut kit = NoVrKit::new(cx, |_, cx| {
        cx.set_global(SetupWizardSpawn(setup::spawn_spec));
        Testing::default()
    });
    kit.click("setup-wizard");
    assert!(!kit.kernel.panel(1).closed);
}
