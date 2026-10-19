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
    for color in [0x282b2eff, 0xfefefeff] {
        let mut scene = stripes();
        scene.insert_primitive(quad(30., 30., 260., 140., color, 24.));
        scene.insert_primitive(quad(
            180.,
            60.,
            90.,
            54.,
            if color == 0x282b2eff {
                0x42464aff
            } else {
                0xe0e1e5ff
            },
            27.,
        ));
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
        let render = |renderer: &mut gpui_wgpu::WgpuHeadlessRenderer, translucent, light| {
            paint::set(
                99,
                paint::Material {
                    pointers: [[200., 85.], [80., 85.]],
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
        let baseline = render(&mut renderer, 0, 0);
        let frosted = render(&mut renderer, 1, 0);
        let lit = render(&mut renderer, 1, 1);
        let contrast = |a, b| {
            (frosted.get_pixel(a, 140).0[0] as i32 - frosted.get_pixel(b, 140).0[0] as i32).abs()
        };
        // 単なる 12% の透過なら白黒の差は約 30。ぼかしにより細部を落とす。
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
        if color == 0x282b2eff {
            assert!(b[0] > a[0], "暗い部品の輪郭は反射で明るくなる");
        }
        assert_ne!(
            frosted.get_pixel(50, 85),
            lit.get_pixel(50, 85),
            "任意の輪郭を持つキーのパスも照らす"
        );
        if color == 0xfefefeff {
            assert!(
                (frosted.get_pixel(230, 85).0[0] as i32 - frosted.get_pixel(230, 130).0[0] as i32)
                    .abs()
                    > 10,
                "透過中も明るい地とボタンを識別できる"
            );
        }
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
