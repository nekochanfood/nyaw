//! E2E for the shared settings parts (`ui::prefs`) on the Settings screen, with the VR laser: appearance (preview, theme,
//! the personal color history and the dialog that makes a custom color), the watch faces (the carousel on an arc),
//! accessibility and the keyboard (a preview of the keyboard, its type, the key sound and its volume).

use gpui::{Context, TestAppContext, px};
use kernel::subsystems::keyboard::{Database, KeyboardType, Layout};
use kernel::{PointerMode, Setting, ThemeMode, WristDesign, WristFace};
use testing::Settings;
use testkit::{Action, Driver, Vec2, VrKit};

/// How dark the runtime shows the Settings panel (1 = as drawn; less while a child dialog is open).
fn tint(kit: &VrKit<'_, Settings>) -> f32 {
    kit.kernel.subsystems.backend.overlays[0].tint
}

#[gpui::test]
fn appearance_is_preview_then_theme_then_centered_colors(cx: &mut TestAppContext) {
    let mut kit = VrKit::new(cx, |_, _| Settings::new(0));
    kit.click("settings-appearance");
    assert_eq!(kit.read(|v| v.category), 1);
    let [preview, theme, first, last, custom] = [
        "theme-preview",
        "setting-theme",
        "accent-blue",
        "accent-teal",
        "accent-custom",
    ]
    .map(|s| kit.bounds(s));
    assert!(preview.bottom() <= theme.top(), "preview on top");
    assert!(theme.bottom() <= first.top(), "then theme, then colors");
    // The swatches and the custom button make one row, centered, with the custom button at the right end.
    let content = kit.bounds("settings-content");
    let middle = (first.left() + custom.right()) / 2.0;
    assert!((middle - content.center().x).abs() < px(2.0), "{middle:?}");
    assert!(custom.left() >= last.right());
    assert_eq!(custom.center().y, first.center().y);

    kit.click("setting-theme");
    assert!(kit.read(|v| v.appearance.theme_open));
    kit.click("setting-theme-light");
    assert_eq!(kit.kernel.settings().theme, ThemeMode::Light);
    assert!(!kit.read(|v| v.appearance.theme_open), "choosing closes");

    // Picking a swatch selects it and leaves the row as it is (the history only takes colors made by hand).
    assert_eq!(kit.kernel.settings().personal_color, None);
    kit.click("accent-pink");
    let pink = kit.kernel.settings().personal_color.expect("picked");
    assert_eq!(pink.hue, 350.0);
    assert_eq!(kit.kernel.settings().color_history[0], None);
    assert_eq!(kit.bounds("accent-blue"), first);
}

#[gpui::test]
fn custom_color_dialog_dims_the_screen_and_adds_to_the_history(cx: &mut TestAppContext) {
    let mut kit = VrKit::new(cx, |_, _| Settings::new(0));
    kit.click("settings-appearance");
    let (blue, teal) = (kit.bounds("accent-blue"), kit.bounds("accent-teal"));
    kit.click("accent-custom").step().step();
    let dialog = 1;
    assert_eq!(kit.kernel.status(dialog).parent, Some(0));
    assert!(tint(&kit) < 1.0, "the screen behind is dimmed");

    // Cancel changes nothing.
    kit.panel(dialog).click("color-cancel").step().step();
    assert!(kit.kernel.panel(dialog).closed);
    assert_eq!(tint(&kit), 1.0);
    assert_eq!(kit.kernel.settings().personal_color, None);

    // Make a color: slide the hue to the left. Nothing changes until OK.
    kit.panel(0).click("accent-custom").step().step();
    let dialog = 2;
    kit.panel(dialog);
    kit.bounds("theme-preview");
    let handle = kit.bounds("accent-hue-handle").center();
    let from = Vec2::new(f32::from(handle.x), f32::from(handle.y));
    kit.point_at(from).press(Action::Select);
    for i in 1..=10 {
        kit.point_at(from - Vec2::new(8.0 * i as f32, 0.0));
    }
    kit.release(Action::Select).step();
    assert_eq!(kit.kernel.settings().personal_color, None);
    kit.click("color-ok").step().step();
    assert!(kit.kernel.panel(dialog).closed);
    let made = kit.kernel.settings().personal_color.expect("made");
    assert!(
        made.hue < 240.0,
        "hue moved from the default blue: {made:?}"
    );
    assert_eq!(kit.kernel.settings().color_history[0], Some(made));

    // The made color takes the first place in the row: the presets shift right and the last one drops off.
    kit.panel(0).hover("setting-theme").step();
    let width = blue.size.width;
    assert_eq!(kit.bounds("accent-blue").left(), blue.left() + width);
    assert_eq!(kit.bounds("accent-green").left(), teal.left());
}

/// Holds the trigger on the middle of the carousel, moves sideways by `dx` px, and releases.
fn drag(kit: &mut VrKit<'_, Settings>, dx: f32) {
    let c = kit.bounds("watch-faces").center();
    let from = Vec2::new(f32::from(c.x), f32::from(c.y));
    kit.point_at(from).press(Action::Select);
    for i in 1..=10 {
        kit.point_at(from + Vec2::new(dx * i as f32 / 10.0, 0.0));
    }
    kit.release(Action::Select).step().step();
}

fn shown(kit: &mut VrKit<'_, Settings>) -> WristDesign {
    let used = kit.kernel.settings().wrist_design;
    kit.read(|v| v.watch.shown(used))
}

#[gpui::test]
fn watch_faces_turn_loop_filter_and_set(cx: &mut TestAppContext) {
    let mut kit = VrKit::new(cx, |_, _| Settings::new(0));
    kit.click("settings-watch");
    assert_eq!(kit.read(|v| v.category), 2);
    // The face in use is in the middle, between the category above and its name and the button below.
    let [kind, faces, name, set] =
        ["watch-kind", "watch-faces", "watch-name", "watch-set"].map(|s| kit.bounds(s));
    assert!(kind.bottom() <= faces.top() && faces.bottom() <= name.top());
    assert!(name.bottom() <= set.top());
    let dots = kit.bounds("watch-face-dots");
    assert!((dots.center().x - faces.center().x).abs() < px(1.0));
    // Its neighbors sit lower along the arc, at the same size, and the ends are joined (the last design is on the left).
    let (left, right) = (
        kit.bounds("watch-face-railway"),
        kit.bounds("watch-face-ring"),
    );
    assert!(left.center().x < dots.left() && right.center().x > dots.right());
    assert!(right.center().y > dots.center().y);
    assert!((right.size.width - dots.size.width).abs() < px(1.0));
    // The carets sit on both sides of the name, below the faces.
    let (prev, next) = (kit.bounds("watch-prev"), kit.bounds("watch-next"));
    assert!(prev.right() < name.left() && next.left() > name.right());
    assert!(prev.top() >= faces.bottom() && (prev.center().y - next.center().y).abs() < px(1.0));
    // The space above the faces (to the category) and below them (to the row of the name) is the same.
    let above = faces.top() - kind.bottom();
    let below = prev.top().min(name.top()) - faces.bottom();
    assert!(
        above > px(0.0) && (above - below).abs() < px(1.0),
        "{above:?} / {below:?}"
    );

    // The button of the face in use cannot be pressed.
    kit.click("watch-set");
    assert_eq!(kit.kernel.settings().wrist_design, WristDesign::Dots);
    // Turning only browses; the button sets what is in the middle.
    kit.click("watch-next");
    assert_eq!(shown(&mut kit), WristDesign::Ring);
    assert_eq!(kit.kernel.settings().wrist_design, WristDesign::Dots);
    kit.click("watch-set");
    assert_eq!(kit.kernel.settings().wrist_design, WristDesign::Ring);
    // Going back past the first design loops to the last.
    kit.click("watch-prev").click("watch-prev");
    assert_eq!(shown(&mut kit), WristDesign::Railway);

    // The category narrows the designs. What was shown is not digital, nor is the one in use: the first digital one shows.
    kit.click("watch-kind");
    assert!(kit.read(|v| v.watch.open));
    kit.click("watch-kind-digital");
    assert_eq!(
        kit.read(|v| (v.watch.face, v.watch.open)),
        (Some(WristFace::Digital), false)
    );
    assert_eq!(shown(&mut kit), WristDesign::Lcd);
    kit.click("watch-set");
    assert_eq!(kit.kernel.settings().wrist_design, WristDesign::Lcd);

    // Holding and moving left brings the next design to the middle; pressing a neighbor does too.
    drag(&mut kit, -150.0);
    assert_eq!(shown(&mut kit), WristDesign::Flip);
    kit.click("watch-face-terminal");
    assert_eq!(shown(&mut kit), WristDesign::Terminal);
    assert_eq!(kit.kernel.settings().wrist_design, WristDesign::Lcd);

    // Leaving the category and coming back puts the design in use in the middle again, among all designs.
    kit.click("settings-general").click("settings-watch");
    assert_eq!(kit.read(|v| v.watch.face), None);
    assert_eq!(shown(&mut kit), WristDesign::Lcd);
}

#[gpui::test]
fn watch_faces_follow_a_touch_slide(cx: &mut TestAppContext) {
    let mut kit = VrKit::new(cx, |_, _| Settings::new(0));
    kit.kernel.set(Setting::PointerMode(PointerMode::Touch));
    kit.step();
    kit.click("settings-watch");
    // With touch, holding and moving is a finger slide: left for the next design, right back again (and on past the first).
    drag(&mut kit, -150.0);
    assert_eq!(shown(&mut kit), WristDesign::Ring);
    drag(&mut kit, 150.0);
    assert_eq!(shown(&mut kit), WristDesign::Dots);
    drag(&mut kit, 150.0);
    assert_eq!(shown(&mut kit), WristDesign::Railway);
    // A tap on the button still presses it.
    kit.click("watch-set");
    assert_eq!(kit.kernel.settings().wrist_design, WristDesign::Railway);
}

/// Grabs the handle of slider `id` and moves it to `at` along its travel (0 = start, 1 = end), still holding.
fn grab_to(kit: &mut VrKit<'_, Settings>, id: &'static str, handle: &'static str, at: f32) {
    let (h, s) = (kit.bounds(handle), kit.bounds(id));
    let from = Vec2::new(f32::from(h.center().x), f32::from(h.center().y));
    let travel = f32::from(s.size.width - h.size.width);
    let to = f32::from(s.left() + h.size.width / 2.0) + at * travel;
    kit.point_at(from).press(Action::Select);
    for i in 1..=5 {
        kit.point_at(from + Vec2::new((to - from.x) * i as f32 / 5.0, 0.0));
    }
    kit.step();
}

const TEXT_SLIDER: (&str, &str) = (
    "setting-text-size-slider",
    "setting-text-size-slider-handle",
);
const UI_SLIDER: (&str, &str) = ("setting-ui-scale-slider", "setting-ui-scale-slider-handle");

/// The accessibility category: a text sample on top, then three equal rows. Text size applies while dragging, UI scale
/// only previews while dragging and applies on release, bold is a switch. None of them changes another.
#[gpui::test]
fn accessibility_is_a_sample_then_text_size_ui_scale_and_bold(cx: &mut TestAppContext) {
    use kernel::{TEXT_SCALE_MAX, TEXT_SCALE_MIN, UI_SCALE_MAX, UI_SCALE_MIN};
    let mut kit = VrKit::new(cx, |_, _| Settings::new(0));
    kit.click("settings-accessibility");
    assert_eq!(kit.read(|v| v.category), 3);
    let [preview, sample, text, ui, bold] = [
        "a11y-preview",
        "a11y-sample",
        "setting-text-size",
        "setting-ui-scale",
        "setting-bold-text",
    ]
    .map(|s| kit.bounds(s));
    assert!(preview.bottom() <= text.top(), "the sample is on top");
    assert!(sample.top() >= preview.top() && sample.bottom() <= preview.bottom());
    assert!(text.bottom() <= ui.top() && ui.bottom() <= bold.top());
    assert_eq!((text.size, ui.size), (ui.size, bold.size), "equal rows");

    // Text size: applies while the handle is held, snapped to the steps. The sample grows inside its box, and
    // neither the box nor the slider under the laser moves.
    let slider = kit.bounds(TEXT_SLIDER.0);
    grab_to(&mut kit, TEXT_SLIDER.0, TEXT_SLIDER.1, 1.0);
    assert_eq!(kit.kernel.settings().text_scale, TEXT_SCALE_MAX);
    let grown = kit.bounds("a11y-sample");
    assert!(grown.size.height > sample.size.height && grown.size.width > sample.size.width);
    assert!(grown.top() >= preview.top() && grown.bottom() <= preview.bottom());
    assert_eq!(kit.bounds("a11y-preview"), preview);
    assert_eq!(kit.bounds(TEXT_SLIDER.0), slider);
    assert_eq!(kit.bounds("setting-ui-scale").size, ui.size);
    kit.release(Action::Select).step();
    grab_to(&mut kit, TEXT_SLIDER.0, TEXT_SLIDER.1, 0.0);
    kit.release(Action::Select).step();
    assert_eq!(kit.kernel.settings().text_scale, TEXT_SCALE_MIN);
    let small = kit.bounds("a11y-sample");
    assert!(small.size.height < sample.size.height);
    let s = kit.kernel.settings();
    assert_eq!((s.ui_scale, s.bold_text), (1.0, false), "the others stay");

    // Bold: the row is a switch.
    kit.click("setting-bold-text");
    let s = kit.kernel.settings();
    assert_eq!(
        (s.bold_text, s.text_scale, s.ui_scale),
        (true, TEXT_SCALE_MIN, 1.0)
    );
    kit.click("setting-bold-text");
    assert!(!kit.kernel.settings().bold_text);

    // UI scale: the panel would move under the laser, so while the handle is held only the sample is zoomed to the
    // size it will have. Letting go applies it: the panel itself grows, and the sample is back at its own size.
    let (size, resolution) = (kit.kernel.panel(0).size, kit.kernel.panel(0).resolution);
    let before = kit.bounds("a11y-sample");
    grab_to(&mut kit, UI_SLIDER.0, UI_SLIDER.1, 1.0);
    assert_eq!(kit.kernel.settings().ui_scale, 1.0, "not while dragging");
    assert_eq!(kit.kernel.panel(0).size, size);
    let zoomed = kit.bounds("a11y-sample");
    let ratio = f32::from(zoomed.size.width) / f32::from(before.size.width);
    assert!((ratio - UI_SCALE_MAX).abs() < 0.1, "{ratio}");
    kit.release(Action::Select).step().step();
    assert_eq!(kit.kernel.settings().ui_scale, UI_SCALE_MAX);
    assert!(
        kit.kernel
            .panel(0)
            .size
            .abs_diff_eq(size * UI_SCALE_MAX, 1e-5)
    );
    assert_eq!(kit.kernel.panel(0).resolution, resolution, "same layout");
    assert_eq!(kit.bounds("a11y-sample").size, before.size);
    assert_eq!(kit.kernel.settings().text_scale, TEXT_SCALE_MIN);

    // Both at their far ends, in every combination, the screen still works: the rows stay inside the content and
    // every control still answers the laser.
    for (text_at, ui_at) in [(1.0, 0.0), (0.0, 0.0), (1.0, 1.0), (0.0, 1.0)] {
        grab_to(&mut kit, TEXT_SLIDER.0, TEXT_SLIDER.1, text_at);
        kit.release(Action::Select).step();
        grab_to(&mut kit, UI_SLIDER.0, UI_SLIDER.1, ui_at);
        kit.release(Action::Select).step().step();
        let s = kit.kernel.settings();
        let want_text = TEXT_SCALE_MIN + text_at * (TEXT_SCALE_MAX - TEXT_SCALE_MIN);
        let want_ui = UI_SCALE_MIN + ui_at * (UI_SCALE_MAX - UI_SCALE_MIN);
        assert!((s.text_scale - want_text).abs() < 1e-5, "{text_at} {ui_at}");
        assert!((s.ui_scale - want_ui).abs() < 1e-5, "{text_at} {ui_at}");
        let content = kit.bounds("settings-content");
        for row in ["setting-text-size", "setting-ui-scale", "setting-bold-text"] {
            let b = kit.bounds(row);
            assert!(b.left() >= content.left() && b.right() <= content.right());
            assert_eq!(b.size, text.size, "{row}: {text_at} {ui_at}");
        }
        let on = kit.kernel.settings().bold_text;
        kit.click("setting-bold-text");
        assert_eq!(kit.kernel.settings().bold_text, !on, "{text_at} {ui_at}");
    }
}

/// With touch, holding and moving on the handle is a finger slide. The UI scale still applies only on release.
#[gpui::test]
fn ui_scale_applies_when_a_touch_slide_ends(cx: &mut TestAppContext) {
    let mut kit = VrKit::new(cx, |_, _| Settings::new(0));
    kit.kernel.set(Setting::PointerMode(PointerMode::Touch));
    kit.step();
    kit.click("settings-accessibility");
    let size = kit.kernel.panel(0).size;
    grab_to(&mut kit, UI_SLIDER.0, UI_SLIDER.1, 0.0);
    assert_eq!(kit.kernel.settings().ui_scale, 1.0);
    assert_eq!(kit.kernel.panel(0).size, size);
    kit.release(Action::Select).step().step();
    assert_eq!(kit.kernel.settings().ui_scale, kernel::UI_SCALE_MIN);
    assert!(
        kit.kernel
            .panel(0)
            .size
            .abs_diff_eq(size * kernel::UI_SCALE_MIN, 1e-5)
    );
}

/// The Settings screen with its keyboard preview already showing `layout` (neither the thread that loads the layouts
/// nor the layout of the OS is waited for).
fn with_keyboard(layout: &str, cx: &mut Context<Settings>) -> Settings {
    let layout = Layout::new(layout, "");
    let keyboard = Database::load().unwrap().keyboard(&layout).unwrap();
    let settings = Settings::new(0);
    let keys = ui::prefs::new_keyboard_preview(0, cx);
    keys.update(cx, |view, _| {
        view.automatic = false;
        view.select_layout(layout);
        view.keyboard = Some(keyboard);
        view.loading = false;
    });
    assert!(settings.keyboard_preview.set(keys).is_ok());
    settings
}

/// How many sounds pressing the preview key `key` starts. The laser rests on the key first, so its hover sound is not
/// counted. Nothing renders the audio here, so every sound that started is still counted as playing.
fn key_sounds(kit: &mut VrKit<'_, Settings>, key: &'static str) -> usize {
    kit.hover(key);
    let before = kit.kernel.status(0).audio.playing;
    kit.press(Action::Select).release(Action::Select);
    kit.kernel.status(0).audio.playing - before
}

const VOLUME_SLIDER: (&str, &str) = (
    "setting-key-volume-slider",
    "setting-key-volume-slider-handle",
);

/// The keyboard category: a preview of the keyboard on top, then its type, the key sound and the volume of that sound.
/// The preview is a smaller copy that follows the settings below it: it takes the shape of the chosen type, and its
/// keys sound (or stay silent) as the key sound is set. It types nowhere.
#[gpui::test]
fn keyboard_is_a_preview_then_type_key_sound_and_volume(cx: &mut TestAppContext) {
    let mut kit = VrKit::new(cx, |_, cx| with_keyboard("us", cx));
    kit.click("settings-keyboard");
    assert_eq!(kit.read(|v| v.category), 6);
    let [preview, keys, kind, sound, volume] = [
        "keyboard-preview",
        "virtual-keyboard",
        "setting-keyboard",
        "setting-key-sound",
        "setting-key-volume",
    ]
    .map(|s| kit.bounds(s));
    assert!(preview.bottom() <= kind.top(), "the preview is on top");
    assert!(kind.bottom() <= sound.top() && sound.bottom() <= volume.top());
    assert_eq!((kind.size, sound.size), (sound.size, volume.size));
    // On by default, at full volume.
    let s = kit.kernel.settings();
    assert_eq!((s.key_sound, s.key_sound_volume), (true, 1.0));

    // The preview is not life-size: the full-size keyboard fits the content, in the middle of its box.
    let unit = ui::prefs::KEYBOARD_PREVIEW_UNIT;
    let content = kit.bounds("settings-content");
    let fits = |keys: gpui::Bounds<gpui::Pixels>, kind: KeyboardType| {
        let size = (f32::from(keys.size.width), f32::from(keys.size.height));
        assert_eq!(
            size,
            (kind.width() * unit, kind.height() * unit),
            "{kind:?}"
        );
        assert!(keys.left() >= content.left() && keys.right() <= content.right());
        assert!(keys.top() >= preview.top() && keys.bottom() <= preview.bottom());
        let off = keys.center() - preview.center();
        assert!(off.x.abs() < px(1.0) && off.y.abs() < px(1.0), "{kind:?}");
    };
    fits(keys, KeyboardType::Full);
    assert!(kit.try_bounds("key-KP7").is_some() && kit.try_bounds("key-FK01").is_some());
    // Only the keys: the controls of the keyboard test screen and the close button of the real one are not here.
    assert!(kit.try_bounds("keyboard-auto").is_none());
    assert!(kit.try_bounds("close-keyboard").is_none());

    // Choosing a type reshapes the preview at the same scale, and neither its box nor the rows under the laser move.
    for (option, chosen) in [
        ("setting-keyboard-tkl", KeyboardType::Tenkeyless),
        ("setting-keyboard-compact", KeyboardType::Compact),
    ] {
        kit.click("setting-keyboard").click(option);
        assert_eq!(kit.kernel.settings().keyboard, chosen);
        fits(kit.bounds("virtual-keyboard"), chosen);
        assert_eq!(kit.bounds("keyboard-preview"), preview);
        assert_eq!(kit.bounds("setting-keyboard"), kind);
    }
    assert!(kit.try_bounds("key-KP7").is_none() && kit.try_bounds("key-FK01").is_none());
    let key = kit.bounds("key-AC01");
    assert!(key.size.width < px(unit), "a key is smaller than 1u");

    // A key of the preview can be pressed and sounds. It is a dummy: no keyboard is summoned and nothing is typed.
    let overlays = kit.kernel.subsystems.backend.overlays.len();
    assert_eq!(key_sounds(&mut kit, "key-AC01"), 1);
    kit.step();
    assert_eq!(kit.kernel.subsystems.backend.overlays.len(), overlays);
    assert_eq!(kit.read(|v| v.category), 6);

    // Key sound off: the same key is silent, and the volume row goes away without moving the rows above.
    kit.click("setting-key-sound");
    assert!(!kit.kernel.settings().key_sound);
    assert_eq!(key_sounds(&mut kit, "key-AC01"), 0);
    assert!(kit.try_bounds("setting-key-volume").is_none());
    assert_eq!(kit.bounds("setting-key-sound"), sound);
    kit.click("setting-key-sound");
    assert!(kit.kernel.settings().key_sound);
    assert_eq!(kit.bounds("setting-key-volume"), volume);
    assert_eq!(key_sounds(&mut kit, "key-AC01"), 1);

    // Volume: applies while the handle is held, snapped to steps of 10%. The others stay. (Each drag is longer than
    // the slop that keeps a press where it started.)
    grab_to(&mut kit, VOLUME_SLIDER.0, VOLUME_SLIDER.1, 0.32);
    assert!((kit.kernel.settings().key_sound_volume - 0.3).abs() < 1e-5);
    kit.release(Action::Select).step();
    assert!((kit.kernel.settings().key_sound_volume - 0.3).abs() < 1e-5);
    assert_eq!(kit.bounds("setting-key-volume"), volume);
    grab_to(&mut kit, VOLUME_SLIDER.0, VOLUME_SLIDER.1, 1.0);
    kit.release(Action::Select).step();
    assert_eq!(kit.kernel.settings().key_sound_volume, 1.0);
    grab_to(&mut kit, VOLUME_SLIDER.0, VOLUME_SLIDER.1, 0.0);
    kit.release(Action::Select).step();
    let s = kit.kernel.settings();
    assert_eq!((s.key_sound_volume, s.key_sound), (0.0, true));
    assert_eq!(s.keyboard, KeyboardType::Compact);
}

/// With touch (the default on the Settings screen), a tap on a key of the preview presses it, and it sounds.
#[gpui::test]
fn keyboard_preview_keys_sound_on_a_touch_tap(cx: &mut TestAppContext) {
    let mut kit = VrKit::new(cx, |_, cx| with_keyboard("jp", cx));
    kit.kernel.set(Setting::PointerMode(PointerMode::Touch));
    kit.step();
    kit.click("settings-keyboard");
    assert_eq!(key_sounds(&mut kit, "key-AC01"), 1);
    assert_eq!(
        key_sounds(&mut kit, "key-HENK"),
        1,
        "a key of the JIS layout"
    );
    kit.click("setting-key-sound");
    assert_eq!(key_sounds(&mut kit, "key-AC01"), 0);
}
