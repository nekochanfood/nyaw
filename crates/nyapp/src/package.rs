//! `.nyapp` パッケージ: `manifest.toml` と `app.wasm`、あればアイコン（`icon.svg` か `icon.png`）を入れた zip
//! （jar・apk と同じ考え方）。
//! ファイル名はマニフェストの ID と同じ `<id>.nyapp`。

use std::fs::File;
use std::io::{Read, Write};
use std::path::Path;

use zip::write::SimpleFileOptions;
use zip::{CompressionMethod, ZipArchive, ZipWriter};

use crate::{Error, Manifest};

pub const EXTENSION: &str = "nyapp";
pub const MANIFEST: &str = "manifest.toml";
pub const WASM: &str = "app.wasm";
/// アプリのアイコン。ベクター（SVG）かラスター（PNG。正方形で 192px 程度）。両方あれば SVG を使う。
/// 無ければ nyaw が名前の頭文字で代わりを描く（[`crate::icon`]）。
pub const ICON_SVG: &str = "icon.svg";
pub const ICON_PNG: &str = "icon.png";
/// 展開した大きさの上限（壊れた・悪意のある zip で展開しすぎないように）。
const MAX_MANIFEST: u64 = 64 << 10;
const MAX_WASM: u64 = 32 << 20;
const MAX_ICON: u64 = 1 << 20;

/// マニフェストだけを読む（一覧に出すため。wasm は展開しない）。
pub fn read_manifest(path: &Path) -> Result<Manifest, Error> {
    let mut zip = ZipArchive::new(File::open(path)?)?;
    Manifest::parse(&read_text(&mut zip, MANIFEST, MAX_MANIFEST)?)
}

/// パッケージに入ったアイコン（中身はファイルのまま）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Icon {
    Svg(Vec<u8>),
    Png(Vec<u8>),
}

impl Icon {
    fn name(&self) -> &'static str {
        match self {
            Icon::Svg(_) => ICON_SVG,
            Icon::Png(_) => ICON_PNG,
        }
    }

    fn bytes(&self) -> &[u8] {
        match self {
            Icon::Svg(b) | Icon::Png(b) => b,
        }
    }
}

/// マニフェストとアイコン（あれば）を読む。一覧に出すため。
pub fn read_listing(path: &Path) -> Result<(Manifest, Option<Icon>), Error> {
    let mut zip = ZipArchive::new(File::open(path)?)?;
    let manifest = Manifest::parse(&read_text(&mut zip, MANIFEST, MAX_MANIFEST)?)?;
    let icon = if zip.index_for_name(ICON_SVG).is_some() {
        Some(Icon::Svg(read_entry(&mut zip, ICON_SVG, MAX_ICON)?))
    } else if zip.index_for_name(ICON_PNG).is_some() {
        Some(Icon::Png(read_entry(&mut zip, ICON_PNG, MAX_ICON)?))
    } else {
        None
    };
    Ok((manifest, icon))
}

/// マニフェストと wasm を読む。
pub fn read(path: &Path) -> Result<(Manifest, Vec<u8>), Error> {
    let mut zip = ZipArchive::new(File::open(path)?)?;
    let manifest = Manifest::parse(&read_text(&mut zip, MANIFEST, MAX_MANIFEST)?)?;
    let wasm = read_entry(&mut zip, WASM, MAX_WASM)?;
    Ok((manifest, wasm))
}

/// パッケージを作る。マニフェストは書く前に確かめる。
pub fn write(
    path: &Path,
    manifest: &str,
    wasm: &[u8],
    icon: Option<&Icon>,
) -> Result<Manifest, Error> {
    let parsed = Manifest::parse(manifest)?;
    let mut zip = ZipWriter::new(File::create(path)?);
    let options = SimpleFileOptions::default().compression_method(CompressionMethod::Deflated);
    zip.start_file(MANIFEST, options)?;
    zip.write_all(manifest.as_bytes())?;
    zip.start_file(WASM, options)?;
    zip.write_all(wasm)?;
    if let Some(icon) = icon {
        zip.start_file(icon.name(), options)?;
        zip.write_all(icon.bytes())?;
    }
    zip.finish()?;
    Ok(parsed)
}

fn read_text(zip: &mut ZipArchive<File>, name: &str, max: u64) -> Result<String, Error> {
    String::from_utf8(read_entry(zip, name, max)?)
        .map_err(|_| Error::new(format!("{name} is not UTF-8")))
}

fn read_entry(zip: &mut ZipArchive<File>, name: &str, max: u64) -> Result<Vec<u8>, Error> {
    let entry = zip
        .by_name(name)
        .map_err(|e| Error::new(format!("{name}: {e}")))?;
    if entry.size() > max {
        return Err(Error::new(format!("{name} is larger than {max} bytes")));
    }
    // 申告された大きさを信じず、上限で切って読む。
    let mut out = Vec::new();
    entry.take(max + 1).read_to_end(&mut out)?;
    if out.len() as u64 > max {
        return Err(Error::new(format!("{name} is larger than {max} bytes")));
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    const MANIFEST_TOML: &str =
        "id = \"net.bettaworx.test\"\nname = \"Test\"\nversion = \"1\"\napi = 1\n";

    #[test]
    fn writes_and_reads_back_a_package() {
        let dir = std::env::temp_dir().join(format!("nyapp-package-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("net.bettaworx.test.nyapp");
        let svg = Icon::Svg(b"<svg/>".to_vec());
        write(&path, MANIFEST_TOML, b"\0asm", Some(&svg)).unwrap();
        assert_eq!(read_manifest(&path).unwrap().id, "net.bettaworx.test");
        assert_eq!(read_listing(&path).unwrap().1, Some(svg));
        let (manifest, wasm) = read(&path).unwrap();
        assert_eq!(manifest.name, "Test");
        assert_eq!(wasm, b"\0asm");
        write(&path, MANIFEST_TOML, b"", None).unwrap();
        assert_eq!(read_listing(&path).unwrap().1, None, "アイコンは無くてよい");
        assert!(
            write(&path, "id = \"bad\"", b"", None).is_err(),
            "壊れたマニフェストでは作らない"
        );
        std::fs::remove_dir_all(dir).unwrap();
    }
}
