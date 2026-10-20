//! Linux と同じ wgpu 経路を、表示ウィンドウなしで実 GPU に通す。
use gpui::{
    Bounds, ContentMask, Corners, DevicePixels, PlatformHeadlessRenderer, Quad, ScaledPixels,
    Scene, point, rgba, size,
};

fn quad(x: f32, y: f32, width: f32, height: f32, color: u32, radius: f32) -> Quad {
    Quad {
        bounds: Bounds::new(
            point(ScaledPixels(x), ScaledPixels(y)),
            size(ScaledPixels(width), ScaledPixels(height)),
        ),
        content_mask: ContentMask {
            bounds: Bounds::new(
                point(ScaledPixels(0.), ScaledPixels(0.)),
                size(ScaledPixels(320.), ScaledPixels(200.)),
            ),
        },
        background: rgba(color).into(),
        corner_radii: Corners::all(ScaledPixels(radius)),
        ..Default::default()
    }
}

fn stripes() -> Scene {
    let mut scene = Scene::default();
    for x in 0..160 {
        scene.insert_primitive(quad(
            x as f32 * 2.,
            0.,
            2.,
            200.,
            if x % 2 == 0 { 0x000000ff } else { 0xffffffff },
            0.,
        ));
    }
    scene
}

#[test]
#[ignore = "実 GPU が必要"]
fn drop_shadow_stays_outside_the_glass_face() {
    let mut renderer = gpui_wgpu::WgpuHeadlessRenderer::new().unwrap();
    let mut scene = Scene::default();
    scene.insert_primitive(quad(0., 0., 320., 200., 0xffffffff, 0.));
    let face = quad(50., 40., 160., 60., 0xffffffff, 30.);
    scene.insert_primitive(gpui::Shadow {
        order: 0,
        blur_radius: ScaledPixels(3.),
        bounds: Bounds::new(
            point(ScaledPixels(50.), ScaledPixels(42.)),
            face.bounds.size,
        ),
        corner_radii: face.corner_radii,
        content_mask: face.content_mask,
        color: gpui::hsla(0., 0., 0., 0.24),
        element_bounds: face.bounds,
        element_corner_radii: face.corner_radii,
        inset: 0,
        pad: 0,
    });
    scene.finish();
    let frame = renderer
        .render_scene_to_image(&scene, size(DevicePixels(320), DevicePixels(200)))
        .unwrap();
    assert_eq!(
        frame.get_pixel(100, 70).0,
        [255; 4],
        "外側の影を半透明の面の下へ敷かない"
    );
    assert!(frame.get_pixel(100, 102).0[0] < 250, "外周には影を残す");
}

#[test]
#[ignore = "実 GPU が必要"]
fn wgpu_material_blurs_backdrop_and_lights_controls_in_both_themes() {
    let mut renderer = gpui_wgpu::WgpuHeadlessRenderer::new().unwrap();
    for theme in [design::Theme::DARK, design::Theme::LIGHT] {
        let glass = theme.translucent();
        let color = glass.card;
        let mut scene = stripes();
        scene.insert_primitive(quad(30., 30., 260., 140., color, 24.));
        scene.insert_primitive(quad(180., 60., 90., 54., glass.control, 27.));
        let mut path = gpui::PathBuilder::fill();
        path.move_to(point(gpui::px(50.), gpui::px(60.)));
        for (x, y) in [
            (135., 60.),
            (135., 85.),
            (115., 85.),
            (115., 114.),
            (50., 114.),
        ] {
            path.line_to(point(gpui::px(x), gpui::px(y)));
        }
        path.close();
        let mut path = path.build().unwrap().scale(1.);
        path.color = rgba(glass.control).into();
        path.content_mask = quad(0., 0., 320., 200., color, 0.).content_mask;
        scene.insert_primitive(path);
        scene.finish();
        let render =
            |renderer: &mut gpui_wgpu::WgpuHeadlessRenderer, translucent, light, pointers| {
                paint::set(
                    99,
                    paint::Material {
                        pointers,
                        translucent,
                        light,
                        scale: 1.,
                        edge_strength: design::Effects::DEFAULT_EDGE_STRENGTH,
                    },
                );
                paint::with_window(99, || {
                    renderer
                        .render_scene_to_image(&scene, size(DevicePixels(320), DevicePixels(200)))
                        .unwrap()
                })
            };
        let pointers = [[200., 85.], [80., 85.]];
        let baseline = render(&mut renderer, 0, 0, pointers);
        let frosted = render(&mut renderer, 1, 0, pointers);
        let lit = render(&mut renderer, 1, 1, pointers);
        let distant = render(&mut renderer, 1, 1, [[80., 85.], [-1e6; 2]]);
        let contrast = |a, b| {
            (frosted.get_pixel(a, 140).0[0] as i32 - frosted.get_pixel(b, 140).0[0] as i32).abs()
        };
        // 8% の透過だけなら白黒の差は約 20。ぼかしで細部を落とす。
        assert!(
            (baseline.get_pixel(100, 140).0[0] as i32 - baseline.get_pixel(102, 140).0[0] as i32)
                .abs()
                > 10,
            "ぼかし前は半透明な面を通して背面の縞が見える"
        );
        assert!(
            contrast(100, 102) < 10,
            "すりガラスの背後の細かい縞が拡散する: {} {:?} {:?}",
            contrast(100, 102),
            frosted.get_pixel(100, 140),
            frosted.get_pixel(102, 140)
        );
        assert_eq!(
            baseline.get_pixel(30, 30),
            frosted.get_pixel(30, 30),
            "角丸の外の背面はぼかさない"
        );
        let a = frosted.get_pixel(181, 87).0;
        let b = lit.get_pixel(181, 87).0;
        assert!(
            (a[0] as i32 - b[0] as i32).abs() >= if theme == design::Theme::LIGHT { 3 } else { 8 },
            "明暗の両テーマで操作部品の輪郭が照らされる: {a:?} -> {b:?}"
        );
        if theme == design::Theme::DARK {
            assert!(b[0] > a[0], "暗い部品の輪郭は反射で明るくなる");
        }
        assert_ne!(
            frosted.get_pixel(50, 85),
            lit.get_pixel(50, 85),
            "任意の輪郭を持つキーのパスも照らす"
        );
        let top = frosted.get_pixel(225, 61).0;
        let bottom = frosted.get_pixel(225, 112).0;
        for (x, y) in [(225, 112), (80, 112)] {
            let flat = baseline.get_pixel(x, y).0;
            let rim = frosted.get_pixel(x, y).0;
            assert!(
                (0..3).all(|c| rim[c] >= flat[c]),
                "ボタンと任意形状のキーの縁を面色より暗くしない: {flat:?} -> {rim:?}"
            );
        }
        assert!(
            top[0] >= bottom[0],
            "上下の縁を白い反射で示す: {top:?} {bottom:?}"
        );
        assert_eq!(
            distant.get_pixel(280, 155),
            frosted.get_pixel(280, 155),
            "180px より遠くの面はポインタで照らさない"
        );
    }
    let mut surfaces = Scene::default();
    surfaces.insert_primitive(quad(
        20.,
        20.,
        130.,
        130.,
        design::Theme::LIGHT.translucent().card,
        12.,
    ));
    surfaces.insert_primitive(quad(
        180.,
        20.,
        100.,
        54.,
        design::Theme::LIGHT.translucent().card,
        27.,
    ));
    surfaces.finish();
    let alpha = paint::with_window(99, || {
        renderer
            .render_to_image_deferred(&surfaces, size(DevicePixels(320), DevicePixels(200)), true)
            .unwrap()
    });
    for (x, y) in [(80, 80), (230, 47)] {
        assert_eq!(
            alpha.get_pixel(x, y).0[3],
            (design::Theme::LIGHT.translucent().card & 255) as u8,
            "同じ役割の不透明度をサイズや照明で変えない"
        );
    }
    assert!(
        alpha.get_pixel(80, 21).0[3] > alpha.get_pixel(80, 80).0[3],
        "白い面も縁の反射だけ不透明にして、中央を暗くせず光を見せる"
    );
    for gradient in [false, true] {
        let mut scene = stripes();
        let mut ghost = quad(30., 30., 260., 140., 0xffffff01, 24.);
        if gradient {
            ghost.background = gpui::linear_gradient(
                180.,
                gpui::linear_color_stop(rgba(0xffffff01), 0.),
                gpui::linear_color_stop(rgba(0xffffff01), 1.),
            );
        }
        scene.insert_primitive(ghost);
        scene.finish();
        let ghost = paint::with_window(99, || {
            renderer
                .render_scene_to_image(&scene, size(DevicePixels(320), DevicePixels(200)))
                .unwrap()
        });
        assert!(
            ghost.get_pixel(100, 140).0[0] < 3,
            "ghost のフェード末尾に、ぼかしだけを残さない"
        );
    }
    paint::remove(99);
}

#[test]
#[ignore = "実 GPU が必要"]
fn wgpu_glass_refracts_a_backdrop_boundary_and_leaves_the_center_unchanged() {
    let mut renderer = gpui_wgpu::WgpuHeadlessRenderer::new().unwrap();
    for theme in [design::Theme::DARK, design::Theme::LIGHT] {
        for gradient in [false, true] {
            let mut scene = Scene::default();
            for x in 0..160 {
                scene.insert_primitive(quad(
                    x as f32 * 2.,
                    0.,
                    2.,
                    200.,
                    if x < 19 { 0x000000ff } else { 0xffffffff },
                    0.,
                ));
            }
            let mut surface = quad(30., 30., 260., 140., theme.translucent().card, 24.);
            if gradient {
                let glass = theme.translucent();
                surface.background = gpui::linear_gradient(
                    180.,
                    gpui::linear_color_stop(rgba(glass.card_top), 0.),
                    gpui::linear_color_stop(rgba(glass.card_bottom), 1.),
                );
            }
            scene.insert_primitive(surface);
            scene.finish();
            let mut render = |translucent| {
                paint::set(
                    100,
                    paint::Material {
                        translucent,
                        light: 0,
                        scale: 1.,
                        ..Default::default()
                    },
                );
                paint::with_window(100, || {
                    renderer
                        .render_scene_to_image(&scene, size(DevicePixels(320), DevicePixels(200)))
                        .unwrap()
                })
            };
            let flat = render(0);
            let glass = render(1);
            let edge = glass.get_pixel(34, 100).0;
            let center = glass.get_pixel(80, 100).0;
            assert!(
                glass.get_pixel(200, 168).0[0] >= flat.get_pixel(200, 168).0[0],
                "白い背面の上でもガラスの下縁を暗くしない"
            );
            // 黒白の境界は x=38。縁 x=34 では、屈折しなければ白を拾わない。
            if theme == design::Theme::DARK {
                assert!(
                    edge[0] > center[0] + 15,
                    "屈折で内側の白が縁に映る: {edge:?} {center:?}"
                );
            } else {
                assert!(
                    edge[0] > 240,
                    "白い面の縁も黒い元の背面ではなく内側の白を映す: {edge:?}"
                );
            }
            let center_flat = flat.get_pixel(80, 100).0;
            // グラデーションの固定機能合成とシェーダー合成には 8bit 丸めの差がある。
            assert!(
                (0..3).all(|c| center[c].abs_diff(center_flat[c]) <= u8::from(gradient)),
                "中央の塗りは保つ: {center:?} {center_flat:?}"
            );
            assert_eq!(center[3], center_flat[3], "中央の不透明度は保つ");
            assert_eq!(
                glass.get_pixel(20, 100),
                flat.get_pixel(20, 100),
                "面の外は歪ませない"
            );
        }
    }
    paint::remove(100);
}
