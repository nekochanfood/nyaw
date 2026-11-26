use gpui::TestAppContext;
use testing::{Testing, WizardTest};
use testkit::{Driver, NoVrKit, on_every_runtime};

fn walk(kit: &mut impl Driver<WizardTest>) {
    assert_eq!(kit.read(|v| v.wizard.step()), 0);
    kit.click("wizard-next").click("wizard-next");
    assert_eq!(kit.read(|v| v.wizard.step()), 2);
    kit.click("wizard-back");
    assert_eq!(kit.read(|v| v.wizard.step()), 1);
    kit.click("wizard-next").click("wizard-next");
    assert!(kit.read(|v| v.wizard.is_last()));
    kit.click("wizard-restart");
    assert!(kit.read(|v| v.wizard.is_first()));
}
on_every_runtime!(walk, |_, _| WizardTest::new(0));

#[gpui::test]
fn testing_opens_wizard_screen(cx: &mut TestAppContext) {
    let mut kit = NoVrKit::new(cx, |_, _| Testing::default());
    kit.click("wizard-test");
    assert!(!kit.kernel.panel(1).closed);
}
