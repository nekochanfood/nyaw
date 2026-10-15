use design::{Weight, icon, icons};
use gpui::{point, px};

#[test]
fn every_weight_of_a_used_icon_builds() {
    for w in Weight::ALL {
        let paths = icon(&icons::HEART).weight(w).size(px(32.)).paths(point(px(0.), px(0.)));
        assert!(!paths.is_empty(), "{w:?}");
        assert!(paths.iter().all(|(p, _)| !p.vertices.is_empty()), "{w:?}");
    }
}

#[test]
fn duotone_has_a_faint_layer() {
    let p = icon(&icons::CARET_DOWN).weight(Weight::Duotone).paths(point(px(0.), px(0.)));
    assert_eq!(p.iter().map(|(_, o)| *o).collect::<Vec<_>>(), [0.2, 1.0]);
}

#[test]
fn only_used_icons_are_built_in() {
    let names: Vec<_> = icons::ALL.iter().map(|i| i.name).collect();
    assert!(names.contains(&"heart") && names.contains(&"caret-down"));
    assert!(!names.contains(&"acorn"), "{names:?}");
}

#[test]
fn transform_keeps_the_glyph_inside_its_box() {
    let v = icon(&icons::HEART).size(px(100.)).rotate(90.).flip_x();
    for (p, _) in v.paths(point(px(10.), px(10.))) {
        let b = p.bounds;
        let (x, w) = (f32::from(b.origin.x), f32::from(b.size.width));
        assert!(x >= 9.0 && x + w <= 111.0, "{b:?}");
    }
}
