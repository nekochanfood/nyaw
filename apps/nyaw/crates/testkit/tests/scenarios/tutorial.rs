//! The tutorial on the dashboard (it starts when setup finishes). The tour lights one part of the UI at a time and
//! dims and blocks the rest, Next moves on (doing the suggested action marks the step done, except where the action
//! leads straight into the next step), and a practice window stands in for a mirrored app. Runs the real wrist and dashboard on the novr emulator.

use gpui::TestAppContext;
use testkit::{Button, Driver, NoVrKit};
use wrist::Wrist;

/// Frames for a click to take effect (open a panel, apply its commands, hand the status back).
fn settle(kit: &mut NoVrKit<Wrist>) {
    for _ in 0..12 {
        kit.step();
    }
}

/// The left hand's X button twice (novr: the X key). Ticks only the kernel, so both presses fall within
/// `kernel::DOUBLE_PRESS` even in a slow debug build (see `dashboard.rs`).
fn press_x_twice(kit: &mut NoVrKit<Wrist>) {
    for held in [true, false, true, false] {
        kit.kernel.subsystems.backend.set_button(Button::X, held);
        kit.kernel.tick();
    }
    settle(kit);
}

/// Opens the wrist, asks for the tour (as finishing setup does) and shows the dashboard. Returns the dashboard.
fn start(cx: &mut TestAppContext) -> (NoVrKit<'_, Wrist>, usize) {
    let mut kit = NoVrKit::new(cx, |_, cx| {
        Wrist::new(
            0,
            |f, o| testing::spec(f, o),
            |f, o| settings::spec(f, o),
            cx,
        )
    });
    kit.step().step();
    kit.app(ui::tour::start);
    press_x_twice(&mut kit);
    let dashboard = kit.read(|w| w.dashboard).expect("dashboard opened");
    (kit, dashboard)
}

/// Clicks `selector` on the panel that shows it.
fn click(kit: &mut NoVrKit<Wrist>, selector: &'static str) {
    let panel = kit
        .find(selector)
        .unwrap_or_else(|| panic!("{selector:?} is not shown"));
    kit.select_panel(panel).click(selector);
    settle(kit);
}

fn at(kit: &mut NoVrKit<Wrist>, step: &'static str) -> bool {
    kit.find(step).is_some()
}

fn running(kit: &mut NoVrKit<Wrist>) -> bool {
    kit.app(|cx| ui::tour::running(cx))
}

/// Whether the coach marks the step's suggested action as done.
fn done(kit: &mut NoVrKit<Wrist>) -> bool {
    kit.find("tutorial-done").is_some()
}

/// Doing what a step suggests marks it done on the coach but stays on the step: only Next moves on.
#[gpui::test]
fn the_tour_walks_through_the_dashboard_screens_and_a_practice_mirror(cx: &mut TestAppContext) {
    let (mut kit, dashboard) = start(cx);
    assert!(at(&mut kit, "tutorial-step-Intro"));
    // Outside the focus, the bar and the wrist are dimmed and take no press: the menu doesn't open.
    assert!(kit.kernel.panel(dashboard).dimmed);
    assert!(kit.kernel.panel(0).dimmed);
    let count = kit.panel_count();
    kit.select_panel(dashboard).click("dashboard-menu");
    settle(&mut kit);
    assert_eq!(kit.panel_count(), count, "the bar took the press");
    // The shortcut doesn't close the dashboard while the tour doesn't allow it.
    press_x_twice(&mut kit);
    assert!(!kit.kernel.status(dashboard).hidden);

    click(&mut kit, "tutorial-next");
    assert!(at(&mut kit, "tutorial-step-Bar"));
    assert!(!kit.kernel.panel(dashboard).dimmed);
    // A step that only explains has no prompt.
    assert!(kit.find("tutorial-try").is_none() && !done(&mut kit));
    click(&mut kit, "tutorial-next");
    assert!(at(&mut kit, "tutorial-step-Toggle"));
    // Closing and opening again with the shortcut is marked done (the prompt to try it turns into it); the step stays.
    assert!(!done(&mut kit));
    assert!(kit.find("tutorial-try").is_some());
    let coach = kit.find("tutorial-step-Toggle").expect("coach");
    press_x_twice(&mut kit);
    assert!(kit.kernel.status(dashboard).hidden);
    // While the dashboard is closed, the coach comes off it, follows the head like the setup wizard, and shows only
    // how to open it again (no buttons).
    assert!(!kit.kernel.status(coach).hidden);
    assert!(kit.kernel.panel(coach).parent.is_none());
    assert!(kit.kernel.panel(coach).lazy_follow.is_some());
    assert!(kit.find("tutorial-hint").is_some());
    assert!(kit.find("tutorial-next").is_none());
    press_x_twice(&mut kit);
    assert!(!kit.kernel.status(dashboard).hidden);
    // Opened again, it goes back on the dashboard as before.
    assert!(kit.kernel.panel(coach).lazy_follow.is_none());
    assert_eq!(
        kit.kernel.panel(coach).parent.map(|p| p.panel),
        Some(dashboard)
    );
    assert!(kit.find("tutorial-hint").is_none());
    assert!(at(&mut kit, "tutorial-step-Toggle") && done(&mut kit));
    assert!(kit.find("tutorial-try").is_none());
    click(&mut kit, "tutorial-next");
    assert!(at(&mut kit, "tutorial-step-Menu"));
    assert!(!done(&mut kit), "a new step starts undone");
    click(&mut kit, "tutorial-next");

    // Only the highlighted button takes the press: the launcher stays shut, the clock opens quick settings.
    assert!(at(&mut kit, "tutorial-step-QuickSettings"));
    kit.select_panel(dashboard).click("dashboard-launcher");
    settle(&mut kit);
    assert!(
        kit.find("dashboard-search").is_none(),
        "the launcher opened"
    );
    click(&mut kit, "dashboard-quick-settings");
    let screen = kit.find("quick-time").expect("quick settings opened");
    assert!(at(&mut kit, "tutorial-step-QuickSettings") && done(&mut kit));
    click(&mut kit, "tutorial-next");
    // Taking the screen off the dashboard and putting it back is one try across two steps: taking it off leads on to
    // putting it back, and putting it back is marked done. novr can't grab, so the dock is driven directly here.
    assert!(at(&mut kit, "tutorial-step-Grab"));
    assert!(kit.find("tutorial-try").is_some());
    kit.app(|cx| ui::undock(screen, cx));
    settle(&mut kit);
    assert!(at(&mut kit, "tutorial-step-Dock"));
    assert!(kit.find("tutorial-try").is_some() && !done(&mut kit));
    kit.app(|cx| ui::dock(screen, cx));
    settle(&mut kit);
    assert!(at(&mut kit, "tutorial-step-Dock") && done(&mut kit));
    click(&mut kit, "tutorial-next");

    // The screen's bar: × closes the screen.
    assert!(at(&mut kit, "tutorial-step-NavBar"));
    let nav = kit.kernel.nav_bar(screen).expect("the screen has a bar");
    // The bar being explained stays shown even when nobody points at the screen.
    assert!(kit.kernel.panel(nav).stay_awake);
    kit.select_panel(nav).click("nav-close");
    settle(&mut kit);
    assert!(kit.kernel.panel(screen).closed);
    assert!(at(&mut kit, "tutorial-step-NavBar") && done(&mut kit));
    // With the screen gone, the coach docks in the middle of the dashboard.
    let coach = kit.find("tutorial-step-NavBar").expect("coach");
    let parent = kit.kernel.panel(coach).parent.expect("on the dashboard");
    assert!(parent.slot.is_some() && parent.panel == dashboard);
    click(&mut kit, "tutorial-next");

    // The launcher lists the practice window while the tour runs; picking it docks it like a mirrored window.
    // Opening the launcher and picking the practice window lead straight on to the next step.
    assert!(at(&mut kit, "tutorial-step-Launcher"));
    click(&mut kit, "dashboard-launcher");
    assert!(at(&mut kit, "tutorial-step-Pick"));
    click(&mut kit, "dashboard-practice");
    let practice = kit.find("practice-list").expect("practice window opened");
    assert!(at(&mut kit, "tutorial-step-Operate"));
    assert!(!kit.kernel.panel(practice).dimmed);
    // Only the practice window, standing in for a mirrored app, takes right and middle clicks.
    assert!(kit.kernel.panel_mouse_buttons(practice));
    assert!(!kit.kernel.panel_mouse_buttons(dashboard));

    // The practice window takes any input (novr's controller left-clicks only: right and middle clicks need a
    // controller that reports a touched button).
    kit.select_panel(practice).click("practice-left");
    kit.select_panel(practice).scroll("practice-list", -3.0);
    settle(&mut kit);
    assert!(at(&mut kit, "tutorial-step-Operate") && done(&mut kit));
    click(&mut kit, "tutorial-next");

    // On the window's bar the keyboard button can't be pressed (it would type into the OS); − puts the window away.
    assert!(at(&mut kit, "tutorial-step-Minimize"));
    let nav = kit
        .kernel
        .nav_bar(practice)
        .expect("the practice window has a bar");
    assert!(kit.kernel.panel(nav).stay_awake);
    let count = kit.panel_count();
    kit.select_panel(nav).click("nav-keyboard");
    settle(&mut kit);
    assert_eq!(kit.panel_count(), count, "the keyboard opened");
    kit.select_panel(nav).click("nav-minimize-source");
    settle(&mut kit);
    assert!(kit.kernel.status(practice).hidden);

    // Putting it away leads on, too: its button on the bar brings it back. The bar no longer has to stay shown.
    assert!(!kit.kernel.panel(nav).stay_awake);
    assert!(at(&mut kit, "tutorial-step-Recall"));
    let button: &'static str = Box::leak(format!("dashboard-stream-{practice}").into_boxed_str());
    kit.select_panel(dashboard).click(button);
    settle(&mut kit);
    assert!(!kit.kernel.status(practice).hidden);
    assert!(at(&mut kit, "tutorial-step-Recall") && done(&mut kit));
    click(&mut kit, "tutorial-next");

    // The last page puts the practice window away; Done closes it and the coach, and lifts the dimming.
    assert!(at(&mut kit, "tutorial-step-Done"));
    assert!(kit.kernel.status(practice).hidden);
    click(&mut kit, "tutorial-finish");
    assert!(kit.kernel.panel(practice).closed);
    assert!(!running(&mut kit));
    assert!(kit.find("tutorial-step-Done").is_none());
    assert!(!kit.kernel.panel(dashboard).dimmed);
    assert!(!kit.kernel.panel(0).dimmed);
}

/// Nothing has to be done to move on: Next alone goes through every step, and each step that needs something
/// (a docked screen, the launcher, the practice window, or the window put away) gets it on the way in.
#[gpui::test]
fn every_step_can_be_passed_with_next(cx: &mut TestAppContext) {
    let (mut kit, dashboard) = start(cx);
    let steps = [
        "tutorial-step-Intro",
        "tutorial-step-Bar",
        "tutorial-step-Toggle",
        "tutorial-step-Menu",
        "tutorial-step-QuickSettings",
        "tutorial-step-Grab",
        "tutorial-step-Dock",
        "tutorial-step-NavBar",
        "tutorial-step-Launcher",
        "tutorial-step-Pick",
        "tutorial-step-Operate",
        "tutorial-step-Minimize",
        "tutorial-step-Recall",
    ];
    for step in steps {
        assert!(at(&mut kit, step), "not at {step}");
        match step {
            "tutorial-step-Grab" => assert!(kit.find("quick-time").is_some()),
            "tutorial-step-Pick" => assert!(kit.find("dashboard-practice").is_some()),
            "tutorial-step-Operate" => assert!(kit.find("practice-list").is_some()),
            _ => {}
        }
        if step == "tutorial-step-Recall" {
            let practice = kit.find("practice-list").expect("practice window");
            assert!(kit.kernel.status(practice).hidden, "not put away");
        }
        click(&mut kit, "tutorial-next");
    }
    assert!(at(&mut kit, "tutorial-step-Done"));
    let practice = kit.find("practice-list").expect("practice window");
    click(&mut kit, "tutorial-finish");
    assert!(kit.kernel.panel(practice).closed);
    assert!(!running(&mut kit));
    assert!(!kit.kernel.status(dashboard).hidden);
}

/// Skip on the first screen ends the tour at once. The door ends it midway after asking, and closes the practice window.
#[gpui::test]
fn the_tour_can_be_skipped_or_ended_midway(cx: &mut TestAppContext) {
    let (mut kit, dashboard) = start(cx);
    click(&mut kit, "tutorial-skip");
    assert!(!running(&mut kit));
    assert!(kit.find("tutorial-step-Intro").is_none());
    assert!(!kit.kernel.panel(dashboard).dimmed);
    // With the tour over, the bar takes presses again.
    let count = kit.panel_count();
    kit.select_panel(dashboard).click("dashboard-menu");
    settle(&mut kit);
    assert!(kit.panel_count() > count, "the menu didn't open");
    // Close the menu again.
    kit.select_panel(dashboard).click("dashboard-menu");
    settle(&mut kit);

    // Asked again (from the debug screen), it starts over on the shown dashboard.
    kit.app(ui::tour::start);
    settle(&mut kit);
    assert!(at(&mut kit, "tutorial-step-Intro"));
    for _ in 0..10 {
        click(&mut kit, "tutorial-next");
    }
    assert!(at(&mut kit, "tutorial-step-Operate"));
    let practice = kit.find("practice-list").expect("practice window");
    // The door asks first: the dialog takes presses during the tour, and Cancel keeps the tour going.
    click(&mut kit, "tutorial-quit");
    let dialog = kit
        .find("tutorial-quit-dialog-title")
        .expect("asks before ending");
    assert!(!kit.kernel.panel(dialog).dimmed);
    click(&mut kit, "tutorial-quit-cancel");
    assert!(kit.kernel.panel(dialog).closed);
    assert!(running(&mut kit));
    assert!(at(&mut kit, "tutorial-step-Operate"));
    click(&mut kit, "tutorial-quit");
    click(&mut kit, "tutorial-quit-confirm");
    assert!(!running(&mut kit));
    assert!(kit.kernel.panel(practice).closed);
    assert!(kit.find("tutorial-quit-dialog-title").is_none());
}

/// The start screen opens docked on the dashboard. Start lifts it off, and it floats near what each step explains:
/// above the bar, beside the docked screen, lower down by the screen's bar. Back goes to the previous step (down to
/// the start screen, docked again) without moving on by itself.
#[gpui::test]
fn the_coach_starts_docked_then_floats_near_each_step_and_goes_back(cx: &mut TestAppContext) {
    use dashboard::CoachSpot;
    let (mut kit, dashboard) = start(cx);
    let coach = kit.find("tutorial-step-Intro").expect("start screen");
    let parent = |kit: &NoVrKit<Wrist>| kit.kernel.panel(coach).parent.expect("on the dashboard");
    let near = |kit: &NoVrKit<Wrist>, spot| {
        let want = dashboard::coach_pose(spot, kernel::DASHBOARD_DISTANCE, 0.0).position;
        let got = kit.kernel.panel(coach).pose.position;
        assert!(got.distance(want) < 0.02, "{spot:?}: {got} != {want}");
    };
    assert!(parent(&kit).slot.is_some() && parent(&kit).panel == dashboard);
    near(&kit, CoachSpot::Docked);

    click(&mut kit, "tutorial-next");
    assert!(at(&mut kit, "tutorial-step-Bar"));
    assert!(parent(&kit).slot.is_none() && parent(&kit).panel == dashboard);
    assert!(kit.app(|cx| ui::docked(cx)).is_none(), "the slot is free");
    near(&kit, CoachSpot::Bar);
    click(&mut kit, "tutorial-back");
    assert!(at(&mut kit, "tutorial-step-Intro"));
    assert!(parent(&kit).slot.is_some());
    near(&kit, CoachSpot::Docked);

    for _ in 0..5 {
        click(&mut kit, "tutorial-next");
    }
    assert!(at(&mut kit, "tutorial-step-Grab"));
    let screen = kit
        .find("quick-time")
        .expect("quick settings docked on the way in");
    near(&kit, CoachSpot::Beside);
    click(&mut kit, "tutorial-next");
    click(&mut kit, "tutorial-next");
    assert!(at(&mut kit, "tutorial-step-NavBar"));
    near(&kit, CoachSpot::Below);
    // Back to grabbing: the screen stays docked, and that doesn't count as done.
    click(&mut kit, "tutorial-back");
    click(&mut kit, "tutorial-back");
    assert!(at(&mut kit, "tutorial-step-Grab"));
    assert!(!done(&mut kit));
    assert_eq!(kit.app(|cx| ui::docked(cx)), Some(screen));
    near(&kit, CoachSpot::Beside);
}

/// Going back sets the dashboard up as it was when that step began, so its action can be done again: what later
/// steps opened (the screen, the launcher, the practice window) closes.
#[gpui::test]
fn going_back_undoes_what_later_steps_opened(cx: &mut TestAppContext) {
    let (mut kit, _) = start(cx);
    for _ in 0..5 {
        click(&mut kit, "tutorial-next");
    }
    assert!(at(&mut kit, "tutorial-step-Grab"));
    assert!(kit.find("quick-time").is_some(), "opened on the way in");
    click(&mut kit, "tutorial-back");
    assert!(at(&mut kit, "tutorial-step-QuickSettings"));
    assert!(kit.find("quick-time").is_none(), "closed again");
    click(&mut kit, "dashboard-quick-settings");
    assert!(kit.find("quick-time").is_some() && done(&mut kit));

    // Forward to using the practice window, then back to picking it and to opening the launcher.
    for _ in 0..6 {
        click(&mut kit, "tutorial-next");
    }
    assert!(at(&mut kit, "tutorial-step-Operate"));
    let practice = kit.find("practice-list").expect("practice window");
    click(&mut kit, "tutorial-back");
    assert!(at(&mut kit, "tutorial-step-Pick"));
    assert!(kit.kernel.panel(practice).closed);
    assert!(
        kit.find("dashboard-practice").is_some(),
        "the launcher is open"
    );
    click(&mut kit, "tutorial-back");
    assert!(at(&mut kit, "tutorial-step-Launcher"));
    assert!(
        kit.find("dashboard-search").is_none(),
        "the launcher closed"
    );
    // Opening it again leads on to picking, as the first time.
    click(&mut kit, "dashboard-launcher");
    assert!(kit.find("dashboard-search").is_some());
    assert!(at(&mut kit, "tutorial-step-Pick"));
}

/// Every step in order, as its `debug_selector` on the coach.
const ALL_STEPS: [&str; 14] = [
    "tutorial-step-Intro",
    "tutorial-step-Bar",
    "tutorial-step-Toggle",
    "tutorial-step-Menu",
    "tutorial-step-QuickSettings",
    "tutorial-step-Grab",
    "tutorial-step-Dock",
    "tutorial-step-NavBar",
    "tutorial-step-Launcher",
    "tutorial-step-Pick",
    "tutorial-step-Operate",
    "tutorial-step-Minimize",
    "tutorial-step-Recall",
    "tutorial-step-Done",
];

/// The coach shows `step`, and the dashboard is as that step begins: which screen is open, and whether the practice
/// window is shown, put away, or closed. On the first page the coach is docked.
fn expect_step(kit: &mut NoVrKit<Wrist>, step: &'static str) {
    assert!(at(kit, step), "not at {step}");
    let screen = kit.find("quick-time").is_some();
    let launcher = kit.find("dashboard-search").is_some();
    let practice = kit.find("practice-list");
    let hidden = practice.map(|p| kit.kernel.status(p).hidden);
    let (want_screen, want_launcher, want_practice) = match step {
        "tutorial-step-Grab" | "tutorial-step-Dock" | "tutorial-step-NavBar" => (true, false, None),
        "tutorial-step-Pick" => (false, true, None),
        "tutorial-step-Operate" | "tutorial-step-Minimize" => (false, false, Some(false)),
        "tutorial-step-Recall" | "tutorial-step-Done" => (false, false, Some(true)),
        _ => (false, false, None),
    };
    assert_eq!(screen, want_screen, "{step}: quick settings open");
    assert_eq!(launcher, want_launcher, "{step}: launcher open");
    assert_eq!(hidden, want_practice, "{step}: practice window hidden");
    if step == "tutorial-step-Intro" {
        let coach = kit.find(step).expect("coach");
        let slot = kit.kernel.panel(coach).parent.and_then(|p| p.slot);
        assert!(slot.is_some(), "the first page is docked");
    }
}

/// Next to the last page, Back all the way to the first, then Next to the last again: every step is reached in both
/// directions and starts from its own state each time, and the tour still finishes.
#[gpui::test]
fn the_tour_goes_to_the_end_back_to_the_start_and_to_the_end_again(cx: &mut TestAppContext) {
    let (mut kit, dashboard) = start(cx);
    expect_step(&mut kit, ALL_STEPS[0]);
    for step in &ALL_STEPS[1..] {
        click(&mut kit, "tutorial-next");
        expect_step(&mut kit, step);
    }
    for step in ALL_STEPS[..ALL_STEPS.len() - 1].iter().rev() {
        click(&mut kit, "tutorial-back");
        expect_step(&mut kit, step);
    }
    for step in &ALL_STEPS[1..] {
        click(&mut kit, "tutorial-next");
        expect_step(&mut kit, step);
    }
    let practice = kit.find("practice-list").expect("practice window");
    click(&mut kit, "tutorial-finish");
    assert!(!running(&mut kit));
    assert!(kit.kernel.panel(practice).closed);
    assert!(!kit.kernel.status(dashboard).hidden);
    assert!(!kit.kernel.panel(dashboard).dimmed);
}
