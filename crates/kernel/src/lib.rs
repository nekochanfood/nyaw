//! カーネル: 一点集中型の中枢。ブートローダー（`main`）が起こし、UI や画面はこれだけを相手にする。
//!
//! 入れ子の層: 設定（[`config`]）を内に持ち、下位の層（ランタイム `vr`・入力 `input`・オーバーレイ `overlay`）の状態を
//! 1 か所で持つ。フレームの進行（[`Kernel::tick`]）・パネルへの操作（[`Kernel::apply`]）・描画結果の受け取り
//! （[`Kernel::submit`]）をシステムコールのように提供する。上位層が下位層を直接組み立てたり、状態を別々に持ったりしない。
//! 上位層が語彙として使う `vr` / `overlay` の型は再エクスポートする。

pub mod config;

pub use config::Config;
pub use overlay::{self, Overlay, Panel, PanelId, PointerEvent, UiEvent};
pub use vr::{self, Device, Pose, Runtime, Surface, Vec2, VrBackend, VrError};

/// 上位層（ビュー）からパネルへの操作要求。
#[derive(Debug, Clone, PartialEq)]
pub enum PanelCommand {
    Attach(Device, Pose),
    Detach,
    Lock(bool),
    Close,
}

/// パネルの状態。ビューの表示用。
#[derive(Debug, Clone, Default, PartialEq)]
pub struct PanelStatus {
    pub runtime: Option<Runtime>,
    pub attached: Option<Device>,
    pub locked: bool,
    /// 見えているトラッカーの ID（取り付け先の候補）。
    pub trackers: Vec<String>,
    /// メニューボタンを押している。
    pub menu: bool,
    /// パネルを掴んでいる。
    pub grabbed: bool,
    /// 閉じられた。
    pub closed: bool,
}

/// 中枢。本番は `Kernel`（= `Kernel<Box<dyn VrBackend>>`）、テストは具象のバックエンド（モック等）を持てる。
pub struct Kernel<B: VrBackend = Box<dyn VrBackend>> {
    pub config: Config,
    /// テストがモックを直接いじるために公開している。本番コードはメソッド経由で使う。
    pub vr: B,
    overlay: Overlay,
}

impl Kernel {
    /// ランタイムを選んで起動する。`requested` は `"--novr"` / `"novr"` のような指定（コマンドライン > 設定ファイル。決めるのは呼び出し側）。
    /// 指定なしで VR ランタイムが無ければ novr で起動する（novr は常に使える）。指定したのに繋がらなければエラー。
    pub fn boot(requested: Option<&str>, config: Config) -> Result<Self, VrError> {
        let prefer = match requested {
            Some("--novr" | "novr") => Some(Runtime::NoVr),
            Some("--openxr" | "openxr") => Some(Runtime::OpenXr),
            Some("--openvr" | "openvr") => Some(Runtime::OpenVr),
            _ => None,
        };
        let vr = match vr::connect(prefer) {
            Ok(b) => b,
            Err(e) if prefer.is_none() => {
                logger::warn!("VR ランタイムに接続できないので novr で起動する", error = e);
                vr::connect(Some(Runtime::NoVr)).expect("novr は常に接続できる")
            }
            Err(e) => {
                logger::error!("VR ランタイムに接続できない", runtime = ?prefer, error = e);
                return Err(e);
            }
        };
        logger::info!("ランタイムを選択", runtime = ?vr.runtime());
        Ok(Self::with_backend(vr, config))
    }
}

impl<B: VrBackend> Kernel<B> {
    /// 接続済みのバックエンドで起動する（テスト用。本番は [`Kernel::boot`]）。
    pub fn with_backend(vr: B, config: Config) -> Self {
        Self {
            config,
            vr,
            overlay: Overlay::new(),
        }
    }

    pub fn runtime(&self) -> Runtime {
        self.vr.runtime()
    }

    pub fn surface(&self) -> Surface {
        self.vr.surface()
    }

    pub fn add_panel(
        &mut self,
        key: &str,
        pose: Pose,
        size: Vec2,
        resolution: (u32, u32),
    ) -> Result<PanelId, VrError> {
        self.overlay
            .add_panel(&mut self.vr, key, pose, size, resolution)
    }

    pub fn panel(&self, id: PanelId) -> &Panel {
        self.overlay.panel(id)
    }

    /// 1 フレーム進める。ランタイムを poll し、パネルの追従・掴み移動とレーザーのヒット判定を行って UI イベントを返す。
    pub fn tick(&mut self) -> Vec<UiEvent> {
        self.overlay.tick(&mut self.vr)
    }

    /// ランタイムが終了を求めている（SteamVR の終了など）。呼び出し側はループを抜ける。
    pub fn quit_requested(&self) -> bool {
        self.overlay.quit_requested()
    }

    /// パネルの描画結果（RGBA8）をランタイムへ送る。
    pub fn submit(&mut self, panel: PanelId, width: u32, height: u32, rgba: &[u8]) {
        self.overlay
            .submit(&mut self.vr, panel, width, height, rgba)
    }

    pub fn apply(&mut self, panel: PanelId, cmd: PanelCommand) {
        match cmd {
            PanelCommand::Attach(device, offset) => self.overlay.attach(panel, device, offset),
            PanelCommand::Detach => self.overlay.detach(panel),
            PanelCommand::Lock(locked) => self.overlay.set_locked(panel, locked),
            PanelCommand::Close => self.overlay.close(&mut self.vr, panel),
        }
    }

    pub fn status(&self, panel: PanelId) -> PanelStatus {
        let p = self.overlay.panel(panel);
        PanelStatus {
            runtime: Some(self.vr.runtime()),
            attached: p.attached.as_ref().map(|(d, _)| d.clone()),
            locked: p.locked,
            trackers: self.overlay.trackers().to_vec(),
            menu: self.overlay.menu_held(),
            grabbed: self.overlay.grabbed(panel),
            closed: p.closed,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use vr::mock::MockBackend;

    fn kernel() -> (Kernel<MockBackend>, PanelId) {
        let mut k = Kernel::with_backend(MockBackend::default(), Config::default());
        let p = k.add_panel("t", Pose::IDENTITY, Vec2::ONE, (4, 4)).unwrap();
        (k, p)
    }

    #[test]
    fn boot_novr_by_name() {
        let k = Kernel::boot(Some("--novr"), Config::default()).unwrap();
        assert_eq!(
            (k.runtime(), k.surface()),
            (Runtime::NoVr, Surface::Desktop)
        );
    }

    #[test]
    fn boot_without_request_always_succeeds() {
        // 実ランタイムの有無に依らず、指定なしなら novr にフォールバックして起動できる
        assert!(Kernel::boot(None, Config::default()).is_ok());
    }

    #[test]
    fn apply_lock_shows_in_status() {
        let (mut k, p) = kernel();
        assert!(!k.status(p).locked);
        k.apply(p, PanelCommand::Lock(true));
        assert!(k.status(p).locked);
    }

    #[test]
    fn submit_reaches_backend() {
        let (mut k, p) = kernel();
        k.submit(p, 1, 1, &[1, 2, 3, 4]);
        let tex = k.vr.overlays[0].texture.as_ref().unwrap();
        assert_eq!(tex.pixel(0, 0), [1, 2, 3, 4]);
    }
}
