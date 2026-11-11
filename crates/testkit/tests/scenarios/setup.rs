use gpui::TestAppContext;
use kernel::{DominantHand, Language, ThemeMode, WristDesign};
use setup::{SetupWizard, Step};
use testing::{SetupWizardSpawn, Testing};
use testkit::{Driver, NoVrKit};

/// The wizard walks through every decision. Each choice applies at once, and only Finish marks the setup done.
#[gpui::test]
fn setup_decides_each_setting_and_marks_done(cx: &mut TestAppContext) {
    let mut kit = NoVrKit::new(cx, |_, _| SetupWizard::new(0, wrist::preview));
    assert!(!kit.kernel.settings().setup_done);
    // 起動仕様（`setup::spec`）と同じく、開いた時点からロックダウンを取る。開いている間は閉じられない。
    kit.kernel.apply(0, kernel::PanelCommand::Lockdown(true));
    kit.step();
    assert!(kit.kernel.settings().lockdown);
    kit.kernel.apply(0, kernel::PanelCommand::Close);
    assert!(!kit.kernel.panel(0).closed);

    // 一枚目: 言語。選ぶとその場で切り替わる（「次へ」を待たない）。
    assert_eq!(kit.read(|v| v.step()), Step::Language);
    kit.click("setup-lang-ja");
    assert_eq!(kit.kernel.settings().language, Language::Ja);
    kit.click("setup-next");

    // 利き手。戻っても選んだものは残る。
    assert_eq!(kit.read(|v| v.step()), Step::Hand);
    kit.click("setup-hand-left");
    assert_eq!(kit.kernel.settings().dominant_hand, DominantHand::Left);
    kit.click("setup-back");
    assert_eq!(kit.read(|v| v.step()), Step::Language);
    kit.click("setup-next").click("setup-next");
    assert_eq!(kit.kernel.settings().dominant_hand, DominantHand::Left);

    // テーマ。
    assert_eq!(kit.read(|v| v.step()), Step::Theme);
    kit.click("setup-theme-light");
    assert_eq!(kit.kernel.settings().theme, ThemeMode::Light);
    kit.click("setup-next");

    // パーソナルカラー: 用意した色の見本を押す。
    assert_eq!(kit.read(|v| v.step()), Step::Color);
    assert_eq!(kit.kernel.settings().personal_color, None);
    kit.click("accent-pink");
    let pink = kit.kernel.settings().personal_color.expect("picked");
    assert_eq!(pink.hue, 350.0);
    kit.bounds("setup-preview"); // 見本が出ている
    kit.click("setup-next");

    // 腕時計: 種類を替えるとその種類の先頭のデザインになり、下の段がその種類のデザインに替わる。
    assert_eq!(kit.read(|v| v.step()), Step::Wrist);
    kit.click("setup-face-analog");
    assert_eq!(kit.kernel.settings().wrist_design, WristDesign::Minimal);
    kit.click("setup-design-railway");
    assert_eq!(kit.kernel.settings().wrist_design, WristDesign::Railway);
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

#[gpui::test]
fn testing_can_retry_the_setup_wizard(cx: &mut TestAppContext) {
    let mut kit = NoVrKit::new(cx, |_, cx| {
        cx.set_global(SetupWizardSpawn(|from| {
            setup::spawn_spec(from, wrist::preview)
        }));
        Testing::default()
    });
    kit.click("setup-wizard");
    assert!(!kit.kernel.panel(1).closed);
}
