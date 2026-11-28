//! nyaw のフルロゴのアニメーション（assets/animations/logo-full）を描けること。

use lottie::Animation;

const LIGHT: &str = include_str!("../../../../../assets/animations/logo-full/logo-full-light.json");
const DARK: &str = include_str!("../../../../../assets/animations/logo-full/logo-full-dark.json");

fn load(json: &str) -> Animation {
    Animation::from_json(json).expect("logo-full を読めること")
}

#[test]
fn reads_markers() {
    let anim = load(LIGHT);
    let intro = anim.marker("intro").unwrap();
    assert_eq!((intro.start, intro.end()), (0., 180.));
    for part in ["earL", "earR", "tail", "n", "y", "a", "w", "ni", "ya", "u"] {
        let m = anim
            .marker(&format!("twitch-{part}"))
            .unwrap_or_else(|| panic!("twitch-{part}"));
        assert!(
            m.start >= intro.end() && m.end() <= anim.out_point,
            "{part}"
        );
    }
}

#[test]
fn first_frame_is_empty() {
    let anim = load(LIGHT);
    let pixmap = anim.render(0., 401, 199).unwrap();
    assert!(pixmap.pixels().iter().all(|p| p.alpha() == 0));
}

/// 静止画の SVG を、合成と同じ余白（build.mjs の MARGIN）を付けて描く。
fn still(svg: &[u8], width: u32, height: u32) -> resvg::tiny_skia::Pixmap {
    let tree = resvg::usvg::Tree::from_data(svg, &Default::default()).unwrap();
    let mut pixmap = resvg::tiny_skia::Pixmap::new(width, height).unwrap();
    let k = width as f32 / 401.;
    let ts = resvg::tiny_skia::Transform::from_scale(k, k).pre_translate(24., 24.);
    resvg::render(&tree, ts, &mut pixmap.as_mut());
    pixmap
}

/// イントロの最後のフレームは、静止画のロゴとほぼ同じ見た目になる。
#[test]
fn intro_ends_on_the_still_logo() {
    for (json, svg) in [
        (
            LIGHT,
            &include_bytes!("../../../../../assets/logos/Logo_Full_Light.svg")[..],
        ),
        (
            DARK,
            &include_bytes!("../../../../../assets/logos/Logo_Full_Dark.svg")[..],
        ),
    ] {
        let anim = load(json);
        let end = anim.marker("intro").unwrap().end();
        let (w, h) = (802, 398);
        let got = anim.render(end, w, h).unwrap();
        let want = still(svg, w, h);
        let (mut total, mut far) = (0u64, 0usize);
        for (a, b) in got.pixels().iter().zip(want.pixels()) {
            let d = [
                a.red().abs_diff(b.red()),
                a.green().abs_diff(b.green()),
                a.blue().abs_diff(b.blue()),
                a.alpha().abs_diff(b.alpha()),
            ];
            let m = *d.iter().max().unwrap();
            total += m as u64;
            if m > 64 {
                far += 1;
            }
        }
        let n = (w * h) as usize;
        let mean = total as f64 / n as f64;
        eprintln!(
            "mean {mean:.2} far {far} ({:.3}%)",
            far as f64 / n as f64 * 100.
        );
        assert!(mean < 3., "平均の差 {mean}");
        assert!((far as f64) < n as f64 * 0.01, "大きく違う画素 {far}");
    }
}

/// しっぽが出る前は、文字と耳が左右の中央にある。イントロの最後は、しっぽを含むロゴ全体が中央にある。
#[test]
fn stays_centered_before_and_after_the_tail() {
    let anim = load(LIGHT);
    let end = anim.marker("intro").unwrap().end();
    for frame in [56., end] {
        let pixmap = anim.render(frame, 401, 199).unwrap();
        let columns = || {
            pixmap
                .pixels()
                .iter()
                .enumerate()
                .filter(|(_, p)| p.alpha() > 128)
                .map(|(i, _)| i % 401)
        };
        let left = columns().min().expect("何か描かれていること");
        let right = columns().max().unwrap();
        let center = (left + right + 1) as f64 / 2.;
        assert!(
            (center - anim.width / 2.).abs() < 8.,
            "フレーム {frame}: 中心 {center}"
        );
    }
}

#[test]
fn hit_finds_parts_after_the_intro() {
    let anim = load(LIGHT);
    let end = anim.marker("intro").unwrap().end();
    // 合成の座標（SVG の座標に余白 24 を足したもの）
    let cases = [
        ((86., 50.), Some("intro-hit earL")),
        ((182., 50.), Some("intro-hit earR")),
        ((44., 104.), Some("intro-hit n")),
        ((10., 10.), None),
    ];
    for ((x, y), want) in cases {
        assert_eq!(anim.hit(end, x, y).as_deref(), want, "({x}, {y})");
    }
    // しっぽは芯線の上のどこか
    assert!((300..340).any(|x| anim.hit(end, x as f64, 146.).as_deref() == Some("intro-hit tail")));
    // イントロの最初は、どの部品もまだない
    assert_eq!(anim.hit(0., 86., 50.), None);
}

#[test]
fn rejects_unsupported_features() {
    let json = r#"{"w":10,"h":10,"fr":60,"ip":0,"op":1,"layers":[{"ty":4,"parent":1,"ks":{},"shapes":[]}]}"#;
    assert!(matches!(
        Animation::from_json(json),
        Err(lottie::Error::Unsupported(_))
    ));
}
