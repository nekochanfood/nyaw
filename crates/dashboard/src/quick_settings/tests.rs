use super::*;
use crate::test_util::*;

#[test]
fn helpers_format_and_pick_glyphs() {
    assert!(std::ptr::eq(
        battery_glyph(&battery(1.0, false)),
        &icons::BATTERY_FULL
    ));
    assert!(std::ptr::eq(
        battery_glyph(&battery(0.5, false)),
        &icons::BATTERY_MEDIUM
    ));
    assert!(std::ptr::eq(
        battery_glyph(&battery(0.05, false)),
        &icons::BATTERY_EMPTY
    ));
    assert!(std::ptr::eq(
        battery_glyph(&battery(0.05, true)),
        &icons::BATTERY_CHARGING
    ));
    assert_eq!(percent(0.826), "83%");
}

#[gpui::test]
fn quick_settings_lists_batteries_in_device_order(cx: &mut TestAppContext) {
    let (_screen, cx) = cx.add_window_view(|_, cx| {
        cx.default_global::<PanelControl>().batteries = vec![
            Battery {
                device: Device::Tracker("LHR-2".into()),
                level: 0.4,
                charging: false,
            },
            Battery {
                device: Device::Controller(Hand::Right),
                level: 0.7,
                charging: true,
            },
            battery(0.9, false),
        ];
        Screen::new(2, Page::QuickSettings, WeakEntity::new_invalid(), cx)
    });
    cx.run_until_parked();
    // HMD・右コントローラ・トラッカーの順で左から並ぶ。
    let x = ["battery-hmd", "battery-right", "battery-tracker-1"]
        .map(|s| cx.debug_bounds(s).unwrap().origin.x);
    assert!(x[0] < x[1] && x[1] < x[2]);
}
