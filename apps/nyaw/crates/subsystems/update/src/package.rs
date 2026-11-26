//! 配布形式。更新では、実行中のビルドと同じ形式のアセットを取得する。

/// Linux の tar.gz、deb、rpm、pkg.tar.zst が実行ファイルの隣に置き、自身の形式を記録するファイル（`installer/linux.sh`）。
#[cfg(target_os = "linux")]
const MARKER: &str = "package-format";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Package {
    /// Windows のインストーラー（Inno Setup）。
    WindowsSetup,
    AppImage,
    /// 利用者が展開して配置する tar.gz。
    Tar,
    Deb,
    Rpm,
    /// Arch Linux の pkg.tar.zst。
    Pacman,
}

impl Package {
    pub const ALL: [Self; 6] = [
        Self::WindowsSetup,
        Self::AppImage,
        Self::Tar,
        Self::Deb,
        Self::Rpm,
        Self::Pacman,
    ];

    /// アセット名のうち、バージョンより後の部分。
    const fn suffix(self) -> &'static str {
        match self {
            Self::WindowsSetup => "-setup.exe",
            Self::AppImage => "-x86_64.AppImage",
            Self::Tar => "-x86_64.tar.gz",
            Self::Deb => "-amd64.deb",
            Self::Rpm => "-x86_64.rpm",
            Self::Pacman => "-x86_64.pkg.tar.zst",
        }
    }

    /// `name` がこの形式のアセット（`nyaw-<バージョン><suffix>`）か判定する。
    /// パス区切りを含む名前は、ダウンロード先の外を指すため除外する。
    pub fn matches(self, name: &str) -> bool {
        name.strip_prefix("nyaw-")
            .and_then(|rest| rest.strip_suffix(self.suffix()))
            .is_some_and(|version| !version.is_empty() && !version.contains(['/', '\\']))
    }

    /// アセット名が示す形式。
    pub fn of(name: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|p| p.matches(name))
    }

    /// 実行中のビルドの配布形式。Windows は常にインストーラーとする。Linux では、配布物から起動していないビルド
    /// （`cargo build` の成果物など）は `None` であり、更新できない。
    #[cfg(windows)]
    pub fn current() -> Option<Self> {
        Some(Self::WindowsSetup)
    }

    /// 実行中のビルドの配布形式。配布物から起動していないビルド（`cargo build` の成果物など）は `None` であり、更新できない。
    #[cfg(target_os = "linux")]
    pub fn current() -> Option<Self> {
        if os::running_appimage().is_some() {
            return Some(Self::AppImage);
        }
        let exe = std::env::current_exe().ok()?;
        let marker = std::fs::read_to_string(exe.parent()?.join(MARKER)).ok()?;
        Self::from_marker(&marker)
    }

    #[cfg(target_os = "linux")]
    pub(crate) fn from_marker(marker: &str) -> Option<Self> {
        match marker.trim() {
            "tar" => Some(Self::Tar),
            "deb" => Some(Self::Deb),
            "rpm" => Some(Self::Rpm),
            "pacman" => Some(Self::Pacman),
            _ => None,
        }
    }
}
