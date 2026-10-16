//! キー位置の期待値は生成コードと共有せず、標準の行配列をフィクスチャで保持する。
use keyboard::{Database, KeyboardType, Layout};

#[test]
fn ansi_iso_jis_rows_sizes_and_clusters_match_the_fixtures() {
    let db = Database::load().unwrap();
    for (layout, fixture, counts) in [
        ("us", include_str!("fixtures/ansi.txt"), [104, 87, 61]),
        ("de", include_str!("fixtures/iso.txt"), [105, 88, 62]),
        ("jp", include_str!("fixtures/jis.txt"), [109, 92, 66]),
    ] {
        let keyboard = db.keyboard(&Layout::new(layout, "")).unwrap();
        for (kind, count) in KeyboardType::ALL.into_iter().zip(counts) {
            let caps = keyboard.keycaps(kind);
            assert_eq!(caps.len(), count, "{layout}: {kind:?}");
            let offset = if kind == KeyboardType::Compact {
                0.
            } else {
                1.5
            };
            for (row, line) in fixture.lines().enumerate() {
                let mut x = 0.;
                for token in line.split_whitespace() {
                    let (name, dimensions) = token.split_once(':').unwrap();
                    let (width, height) = dimensions.split_once('x').unwrap_or((dimensions, "1"));
                    let (width, height) = (
                        width.parse::<f32>().unwrap(),
                        height.parse::<f32>().unwrap(),
                    );
                    if name != "_" {
                        let key = caps
                            .iter()
                            .find(|k| k.name == name)
                            .unwrap_or_else(|| panic!("{layout}: missing {name}"));
                        assert_eq!(
                            (key.x, key.y, key.width, key.height),
                            (x, offset + row as f32, width, height),
                            "{layout}: {name} / {kind:?}"
                        );
                    }
                    x += width;
                }
            }
            if kind != KeyboardType::Compact {
                let escape = caps.iter().find(|k| k.name == "ESC").unwrap();
                let f1 = caps.iter().find(|k| k.name == "FK01").unwrap();
                let up = caps.iter().find(|k| k.name == "UP").unwrap();
                assert_eq!((escape.x, escape.y), (0., 0.));
                assert_eq!((f1.x, f1.y), (2., 0.));
                assert_eq!((up.x, up.y), (16.5, 4.5));
            }
            if kind == KeyboardType::Full {
                for (name, expected) in [
                    ("KP0", (19., 5.5, 2., 1.)),
                    ("KPAD", (22., 2.5, 1., 2.)),
                    ("KPEN", (22., 4.5, 1., 2.)),
                ] {
                    let k = caps.iter().find(|k| k.name == name).unwrap();
                    assert_eq!((k.x, k.y, k.width, k.height), expected);
                }
            }
        }
    }
}

#[test]
fn displayed_keys_produce_the_expected_text_for_every_type() {
    let db = Database::load().unwrap();
    for kind in KeyboardType::ALL {
        for (layout, variant, names, expected) in [
            (
                "us",
                "",
                vec!["AD01", "AD02", "AD03", "AD04", "AD05"],
                "qwert",
            ),
            (
                "fr",
                "",
                vec!["AD01", "AD02", "AD03", "AD04", "AD05"],
                "azert",
            ),
            (
                "us",
                "dvorak",
                vec!["AC01", "AC02", "AC03", "AC04", "AC05"],
                "aoeui",
            ),
            ("de", "", vec!["AD06", "AB01"], "zy"),
            ("ru", "", vec!["AC01", "AC02", "AC03"], "фыв"),
            ("jp", "kana", vec!["AD01", "AD02", "AD03"], "タテイ"),
        ] {
            let mut keyboard = db.keyboard(&Layout::new(layout, variant)).unwrap();
            let keys = keyboard.keycaps(kind);
            let mut text = String::new();
            for name in names {
                let code = keys.iter().find(|k| k.name == name).unwrap().code;
                let label = keyboard.label(code);
                let stroke = keyboard.click(code).unwrap();
                assert_eq!(label, stroke.text);
                text.push_str(&stroke.text);
            }
            assert_eq!(text, expected, "{layout}/{variant}: {kind:?}");
        }
        let mut us = db.keyboard(&Layout::new("us", "")).unwrap();
        us.click(us.code("LFSH").unwrap());
        assert_eq!(us.label(us.code("AE01").unwrap()), "!");
        assert_eq!(us.click(us.code("AE01").unwrap()).unwrap().text, "!");
        let mut de = db.keyboard(&Layout::new("de", "")).unwrap();
        de.click(de.code("RALT").unwrap());
        assert_eq!(de.click(de.code("AD01").unwrap()).unwrap().text, "@");
    }
}
