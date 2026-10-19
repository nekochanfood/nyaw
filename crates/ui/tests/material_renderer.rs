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
        path.color = rgba(color).into();
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
                        padding: 0,
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
            (a[0] as i32 - b[0] as i32).abs() > 8,
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
        let control = frosted.get_pixel(250, 87).0;
        let card = frosted.get_pixel(250, 140).0;
        assert!(
            control[0] > card[0] + 10,
            "両テーマでカードよりボタンが明るい: {control:?} {card:?}"
        );
        let top = frosted.get_pixel(225, 61).0;
        let bottom = frosted.get_pixel(225, 112).0;
        assert!(
            top[0] > bottom[0],
            "上の反射と下の陰影で凸面を示す: {top:?} {bottom:?}"
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
    let mut scene = stripes();
    scene.insert_primitive(quad(30., 30., 260., 140., 0xffffff01, 24.));
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
    paint::remove(99);
}
