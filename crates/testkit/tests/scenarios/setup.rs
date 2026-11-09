use gpui::TestAppContext;
use setup::SetupWizard;
use testing::{SetupWizardSpawn, Testing};
use testkit::{Driver, NoVrKit};

#[gpui::test]
fn setup_walks_through_three_screens_and_marks_done(cx: &mut TestAppContext) {
    let mut kit = NoVrKit::new(cx, |_, _| SetupWizard::new(0));
    assert!(!kit.kernel.settings().setup_done);
    // 起動仕様（`setup::spec`）と同じく、開いた時点からロックダウンを取る。開いている間は閉じられない。
    kit.kernel.apply(0, kernel::PanelCommand::Lockdown(true));
    kit.step();
    assert!(kit.kernel.settings().lockdown);
    kit.kernel.apply(0, kernel::PanelCommand::Close);
    assert!(!kit.kernel.panel(0).closed);

    // 一枚目: ようこそ → 二枚目: 仮置き → 戻れる → 三枚目: 完了。
    kit.click("setup-next");
    assert_eq!(kit.read(|v| v.wizard.step()), 1);
    kit.bounds("setup-placeholder"); // 仮置きの画面が描かれている
    kit.click("setup-back");
    assert_eq!(kit.read(|v| v.wizard.step()), 0);
    kit.click("setup-next").click("setup-next");
    assert!(kit.read(|v| v.wizard.is_last()));
    // 完了を押すまでは終えたことにならない。
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
        cx.set_global(SetupWizardSpawn(setup::spawn_spec));
        Testing::default()
    });
    kit.click("setup-wizard");
    assert!(!kit.kernel.panel(1).closed);
}
