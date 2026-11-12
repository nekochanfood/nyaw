use gpui::{TestAppContext, px};
use kernel::{DominantHand, Language, PanelCommand, ThemeMode, WristDesign};
use setup::{SetupWizard, Step};
use testing::{BACKGROUND, PANEL_BG, SetupWizardSpawn, Testing};
use testkit::{Driver, NoVrKit, Vec2, VrKit};

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

    // 完了の画面からも戻れる（「完了」の下の「戻る」）。完了を押すまでは終えたことにならない。
    assert!(kit.read(|v| v.wizard.is_last()));
    let (finish, back) = (kit.bounds("setup-finish"), kit.bounds("setup-back"));
    assert!(finish.bottom() <= back.top());
    kit.click("setup-back");
    assert_eq!(kit.read(|v| v.step()), Step::Wrist);
    kit.click("setup-next");
    assert!(kit.read(|v| v.wizard.is_last()));
    kit.step();
    assert!(!kit.kernel.settings().setup_done);
    kit.click("setup-finish").step();
    assert!(kit.kernel.settings().setup_done);
    assert!(!kit.kernel.settings().lockdown, "終えたら外れる");
    assert!(kit.kernel.panel(0).closed);
}

/// The first screen: the logo, Start and the language dropdown under it make one group in the middle of the
/// screen (no text: the logo says the name). The dropdown opens below itself.
#[gpui::test]
fn welcome_centers_the_logo_with_start_and_the_language_under_it(cx: &mut TestAppContext) {
    let mut kit = VrKit::new(cx, |_, _| SetupWizard::new(0));
    let panel = kit.bounds("setup-nav");
    let [hero, start, language] =
        ["setup-hero", "setup-next", "setup-language"].map(|s| kit.bounds(s));
    let near = |a: gpui::Pixels, b: gpui::Pixels| (a - b).abs() < px(2.0);
    // 上からロゴ・「はじめる」・言語のドロップダウンが 1 つのまとまりで、画面の上下左右中央。ロゴは大きい。
    for part in [hero, start, language] {
        assert!(near(part.center().x, panel.center().x), "{part:?}");
    }
    assert!(near(
        (hero.top() + language.bottom()) / 2.0,
        panel.center().y
    ));
    assert!(hero.size.height >= px(160.0), "{:?}", hero.size);
    assert!(hero.bottom() < start.top() && start.bottom() <= language.top());
    // ドロップダウンは下へ開く（「はじめる」を隠さない）。
    kit.click("setup-language").step();
    let options = ["setup-lang-en", "setup-lang-ja"].map(|s| kit.bounds(s));
    let language = kit.bounds("setup-language");
    assert!(options.iter().all(|o| o.top() >= language.bottom()));
    // 外を押すと、選ばずに閉じる。
    kit.click("setup-hero");
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

/// The wizard's panel has the backdrop color of the Settings content side, so the shared parts sit on it in their
/// own colors (cards on the backdrop) with no box around them.
#[gpui::test]
fn wizard_panel_is_the_backdrop_with_the_parts_in_their_own_colors(cx: &mut TestAppContext) {
    let mut kit = VrKit::new(cx, |_, _| SetupWizard::new(0));
    kit.click("setup-next").click("setup-next");
    assert_eq!(kit.read(|v| v.step()), Step::Appearance);
    let row = kit.bounds("setting-theme");
    let x = f32::from(row.left()) + 4.0;
    let inside = kit.vr_color_at_px(Vec2::new(x, f32::from(row.center().y)));
    let between = kit.vr_color_at_px(Vec2::new(x, f32::from(row.bottom()) + 8.0));
    assert_eq!(
        inside, PANEL_BG,
        "the row is a card, as on the Settings screen"
    );
    assert_eq!(between, BACKGROUND, "the panel itself is the backdrop");
    // The watch faces too: around them is the panel's backdrop.
    kit.click("setup-next");
    let faces = kit.bounds("watch-faces");
    let corner = Vec2::new(f32::from(faces.left()) + 2.0, f32::from(faces.top()) + 2.0);
    assert_eq!(kit.vr_color_at_px(corner), BACKGROUND);
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
