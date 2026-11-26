//! The dashboard's menu popover stands up from the bar, over the screen docked above it. It must be drawn over that
//! screen, even though the bar it belongs to is farther from the head than the screen. Runs the real wrist and
//! dashboard on the novr emulator.

use gpui::TestAppContext;
use testkit::{Button, Driver, NoVrKit};
use wrist::Wrist;

fn settle(kit: &mut NoVrKit<Wrist>) {
    for _ in 0..40 {
        kit.step();
    }
}

/// Clicks `button` on `panel` and returns the panel that opened.
fn open(kit: &mut NoVrKit<Wrist>, panel: usize, button: &'static str) -> usize {
    let next = kit.panel_count();
    kit.select_panel(panel).click(button);
    settle(kit);
    assert!(kit.panel_count() > next, "{button} opened nothing");
    next
}

#[gpui::test]
fn the_dashboard_menu_is_drawn_over_the_docked_screen(cx: &mut TestAppContext) {
    let mut kit = NoVrKit::new(cx, |_, cx| {
        Wrist::new(
            0,
            |f, o| testing::spec(f, o),
            |f, o| settings::spec(f, o),
            cx,
        )
    });
    kit.step().step();
    // The left hand's X button twice shows the dashboard (see `dashboard.rs`).
    for held in [true, false, true, false] {
        kit.kernel.subsystems.backend.set_button(Button::X, held);
        kit.kernel.tick();
    }
    settle(&mut kit);
    let dashboard = kit.read(|w| w.dashboard).expect("dashboard opened");

    // Dock the settings screen from the menu, then open the menu again over it.
    let menu = open(&mut kit, dashboard, "dashboard-menu");
    let screen = open(&mut kit, menu, "menu-settings");
    let slot = kit.kernel.panel(screen).parent.expect("docked");
    assert!(slot.panel == dashboard && slot.slot.is_some());
    let menu = open(&mut kit, dashboard, "dashboard-menu");

    let bar = kit.kernel.nav_bar(screen).expect("the screen has a bar");
    let eye = kit.kernel.hmd().position;
    let far = |id: usize| kit.kernel.panel(id).pose.position.distance(eye);
    // The situation that hid the menu: the screen is nearer to the head than the menu's owner.
    assert!(far(screen) < far(dashboard));
    let order = |id: usize| kit.kernel.panel(id).draw_order();
    assert!(order(menu) > order(screen), "menu under the screen");
    assert!(order(menu) > order(bar), "menu under the screen's bar");

    // An item over the screen takes the press: "settings" again closes the docked settings screen.
    kit.select_panel(menu).click("menu-settings");
    settle(&mut kit);
    assert!(kit.kernel.panel(screen).closed);
}
