//! E2E for the Pointer category of the Settings screen, with the VR laser: the simple / detailed switch of the movement
//! sliders, the cursor rows, and the switches that silence the pointer's tap and hover sounds.

use gpui::TestAppContext;
use kernel::PointerTuning;
use testing::Settings;
use testkit::{Action, Driver, VrKit};

const DETAILED: [&str; 3] = [
    "setting-smoothing",
    "setting-release",
    "setting-laser-length",
];

/// Simple (the default) shows one slider, detailed the three it stands for. Under it are the cursor and sound rows.
#[gpui::test]
fn simple_is_one_slider_and_detailed_is_three(cx: &mut TestAppContext) {
    let mut kit = VrKit::new(cx, |_, _| Settings::new(0));
    kit.click("settings-pointer");
    assert_eq!(kit.kernel.settings().pointer_tuning, PointerTuning::Simple);
    let shown = |kit: &mut VrKit<'_, Settings>, id: &'static str| kit.try_bounds(id).is_some();
    assert!(shown(&mut kit, "setting-steadiness"));
    assert!(DETAILED.iter().all(|&id| !shown(&mut kit, id)));
    let steadiness = kit.bounds("setting-steadiness");
    for id in [
        "setting-cursor-size",
        "setting-tooltips",
        "setting-tap-sound",
        "setting-hover-sound",
    ] {
        assert!(kit.bounds(id).top() > steadiness.bottom(), "{id}");
    }

    kit.click("setting-pointer-tuning")
        .click("setting-tuning-detailed");
    assert_eq!(
        kit.kernel.settings().pointer_tuning,
        PointerTuning::Detailed
    );
    assert!(!shown(&mut kit, "setting-steadiness"));
    assert!(DETAILED.iter().all(|&id| shown(&mut kit, id)));

    kit.click("setting-pointer-tuning")
        .click("setting-tuning-simple");
    assert_eq!(kit.kernel.settings().pointer_tuning, PointerTuning::Simple);
    assert!(shown(&mut kit, "setting-steadiness"));
}

/// Sounds started by pointing at the selected category (coming from another one) and by pressing it.
fn cues(kit: &mut VrKit<'_, Settings>) -> (usize, usize) {
    let playing = |kit: &VrKit<'_, Settings>| kit.kernel.status(0).audio.playing;
    kit.hover("settings-about");
    let before = playing(kit);
    kit.hover("settings-pointer");
    let hover = playing(kit) - before;
    let before = playing(kit);
    kit.press(Action::Select).release(Action::Select);
    (hover, playing(kit) - before)
}

/// Tap and hover sounds are on by default, and each switch silences only its own sound. Pressing still works.
#[gpui::test]
fn sound_switches_silence_the_tap_and_hover_sounds(cx: &mut TestAppContext) {
    let mut kit = VrKit::new(cx, |_, _| Settings::new(0));
    kit.click("settings-pointer");
    assert_eq!(cues(&mut kit), (1, 1));

    kit.click("setting-tap-sound");
    assert!(!kit.kernel.settings().tap_sound);
    assert_eq!(cues(&mut kit), (1, 0));

    kit.click("setting-hover-sound");
    let s = kit.kernel.settings();
    assert!(!s.hover_sound && !s.tap_sound);
    assert_eq!(cues(&mut kit), (0, 0));
    assert_eq!(kit.read(|v| v.category), 4, "still on the Pointer category");

    kit.click("setting-tap-sound").click("setting-hover-sound");
    assert_eq!(cues(&mut kit), (1, 1));
}
