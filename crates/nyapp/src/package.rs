//! `.nyapp` パッケージ: `manifest.toml` と `app.wasm`、あればアイコンの背景と前景を入れた zip
//! （jar・apk と同じ考え方）。
//! ファイル名はマニフェストの ID と同じ `<id>.nyapp`。

use std::fs::File;
use std::io::{Read, Seek, Write};
use std::path::Path;

use zip::write::SimpleFileOptions;
use zip::{CompressionMethod, ZipArchive, ZipWriter};

use crate::{Error, Manifest};

pub const EXTENSION: &str = "nyapp";
pub const MANIFEST: &str = "manifest.toml";
pub const WASM: &str = "app.wasm";
/// アプリのアイコンは背景と前景の 2 枚を重ねる（Android のアダプティブアイコンと同じ考え方）。どちらも正方形で、
/// ベクター（SVG）かラスター（PNG。192px 程度）。両方あれば SVG を使う。
/// - 背景（`icon-background.*`）: 端まで塗った地。nyaw が角丸の四角に切る。
/// - 前景（`icon-foreground.*`）: 地を透明にした絵。真ん中の 2/3 に収める（角丸で切られないように）。前景だけでも使う
///   （進行中の作業の知らせなど）。
///
/// 無ければ nyaw が代わりを描く（[`crate::icon`]。前景が無ければ名前の頭文字、背景が無ければアクセントの色）。
pub const ICON_BACKGROUND: [&str; 2] = ["icon-background.svg", "icon-background.png"];
pub const ICON_FOREGROUND: [&str; 2] = ["icon-foreground.svg", "icon-foreground.png"];
/// 展開した大きさの上限（壊れた・悪意のある zip で展開しすぎないように）。
const MAX_MANIFEST: u64 = 64 << 10;
const MAX_WASM: u64 = 32 << 20;
const MAX_ICON: u64 = 1 << 20;

/// マニフェストだけを読む（一覧に出すため。wasm は展開しない）。
pub fn read_manifest(path: &Path) -> Result<Manifest, Error> {
    let mut zip = ZipArchive::new(File::open(path)?)?;
    Manifest::parse(&read_text(&mut zip, MANIFEST, MAX_MANIFEST)?)
}

/// パッケージに入った画像 1 枚（中身はファイルのまま）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Image {
    Svg(Vec<u8>),
    Png(Vec<u8>),
}

impl Image {
    fn bytes(&self) -> &[u8] {
        match self {
            Image::Svg(b) | Image::Png(b) => b,
        }
    }

    /// `names`（`[svg, png]`）のうちこの形式の名前。
    fn name(&self, names: [&'static str; 2]) -> &'static str {
        match self {
            Image::Svg(_) => names[0],
            Image::Png(_) => names[1],
        }
    }
}

/// パッケージに入ったアイコンの背景と前景（どちらも無くてよい）。
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Icon {
    pub background: Option<Image>,
    pub foreground: Option<Image>,
}

/// マニフェストとアイコンを読む。一覧に出すため。
pub fn read_listing(path: &Path) -> Result<(Manifest, Icon), Error> {
    let mut zip = ZipArchive::new(File::open(path)?)?;
    let manifest = Manifest::parse(&read_text(&mut zip, MANIFEST, MAX_MANIFEST)?)?;
    let mut layer = |[svg, png]: [&str; 2]| -> Result<Option<Image>, Error> {
        Ok(if zip.index_for_name(svg).is_some() {
            Some(Image::Svg(read_entry(&mut zip, svg, MAX_ICON)?))
        } else if zip.index_for_name(png).is_some() {
            Some(Image::Png(read_entry(&mut zip, png, MAX_ICON)?))
        } else {
            None
        })
    };
    let icon = Icon {
        background: layer(ICON_BACKGROUND)?,
        foreground: layer(ICON_FOREGROUND)?,
    };
    Ok((manifest, icon))
}

/// マニフェストと wasm を読む。
pub fn read(path: &Path) -> Result<(Manifest, Vec<u8>), Error> {
    let mut zip = ZipArchive::new(File::open(path)?)?;
    read_archive(&mut zip)
}

/// ワーカーが親から受け取ったパッケージを、ファイルを開かずに読む。
pub fn read_bytes(bytes: &[u8]) -> Result<(Manifest, Vec<u8>), Error> {
    let mut zip = ZipArchive::new(std::io::Cursor::new(bytes))?;
    read_archive(&mut zip)
}

fn read_archive<R: Read + Seek>(zip: &mut ZipArchive<R>) -> Result<(Manifest, Vec<u8>), Error> {
    let manifest = Manifest::parse(&read_text(zip, MANIFEST, MAX_MANIFEST)?)?;
    let wasm = read_entry(zip, WASM, MAX_WASM)?;
    Ok((manifest, wasm))
}

/// パッケージを作る。マニフェストは書く前に確かめる。
pub fn write(path: &Path, manifest: &str, wasm: &[u8], icon: &Icon) -> Result<Manifest, Error> {
    let parsed = Manifest::parse(manifest)?;
    let mut zip = ZipWriter::new(File::create(path)?);
    let options = SimpleFileOptions::default().compression_method(CompressionMethod::Deflated);
    zip.start_file(MANIFEST, options)?;
    zip.write_all(manifest.as_bytes())?;
    zip.start_file(WASM, options)?;
    zip.write_all(wasm)?;
    for (image, names) in [
        (&icon.background, ICON_BACKGROUND),
        (&icon.foreground, ICON_FOREGROUND),
    ] {
        if let Some(image) = image {
            zip.start_file(image.name(names), options)?;
            zip.write_all(image.bytes())?;
        }
    }
    zip.finish()?;
    Ok(parsed)
}

fn read_text<R: Read + Seek>(
    zip: &mut ZipArchive<R>,
    name: &str,
    max: u64,
) -> Result<String, Error> {
    String::from_utf8(read_entry(zip, name, max)?)
        .map_err(|_| Error::new(format!("{name} is not UTF-8")))
}

fn read_entry<R: Read + Seek>(
    zip: &mut ZipArchive<R>,
    name: &str,
    max: u64,
) -> Result<Vec<u8>, Error> {
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

    const WASM_BYTES: &[u8] = b"\0asm";
    const MANIFEST_TOML: &str =
        "id = \"net.bettaworx.test\"\nname = \"Test\"\nversion = \"1\"\napi = 1\n";

    #[test]
    fn writes_and_reads_back_a_package() {
        let dir = std::env::temp_dir().join(format!("nyapp-package-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("net.bettaworx.test.nyapp");
        let icon = Icon {
            background: Some(Image::Png(b"png".to_vec())),
            foreground: Some(Image::Svg(b"<svg/>".to_vec())),
        };
        write(&path, MANIFEST_TOML, WASM_BYTES, &icon).unwrap();
        assert_eq!(read_manifest(&path).unwrap().id, "net.bettaworx.test");
        assert_eq!(
            read_listing(&path).unwrap().1,
            icon,
            "背景と前景を分けて読む"
        );
        let (manifest, wasm) = read(&path).unwrap();
        assert_eq!(manifest.name, "Test");
        assert_eq!(wasm, WASM_BYTES);
        let foreground_only = Icon {
            background: None,
            foreground: Some(Image::Png(b"png".to_vec())),
        };
        write(&path, MANIFEST_TOML, b"", &foreground_only).unwrap();
        assert_eq!(read_listing(&path).unwrap().1, foreground_only);
        write(&path, MANIFEST_TOML, b"", &Icon::default()).unwrap();
        assert_eq!(
            read_listing(&path).unwrap().1,
            Icon::default(),
            "アイコンは無くてよい"
        );
        assert!(
            write(&path, "id = \"bad\"", b"", &Icon::default()).is_err(),
            "壊れたマニフェストでは作らない"
        );
        std::fs::remove_dir_all(dir).unwrap();
    }
}
