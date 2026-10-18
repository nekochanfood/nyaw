//! Windows: GSMTC（`GlobalSystemMediaTransportControlsSessionManager`）。切り替えて選んだセッションか、無ければ
//! OS がいちばん関係があると見たセッション（`GetCurrentSession`）を出す。

use std::time::{Duration, SystemTime, UNIX_EPOCH};

use windows::{
    ApplicationModel::AppInfo,
    Foundation::Size,
    Media::Control::{
        GlobalSystemMediaTransportControlsSession as MediaSession,
        GlobalSystemMediaTransportControlsSessionManager as Manager,
        GlobalSystemMediaTransportControlsSessionMediaProperties as Properties,
        GlobalSystemMediaTransportControlsSessionPlaybackStatus as Status,
    },
    Storage::Streams::{DataReader, IRandomAccessStreamReference},
    Win32::{
        Storage::EnhancedStorage::PKEY_Link_TargetParsingPath,
        System::{
            Com::CoTaskMemFree,
            WinRT::{RO_INIT_MULTITHREADED, RoInitialize},
        },
        UI::Shell::{IShellItem2, SHCreateItemFromParsingName, SIGDN_NORMALDISPLAY},
    },
    core::{HSTRING, PWSTR},
};

use super::{Backend, Command, Controls, Pick, Raw, fallback_name, picked};

/// Windows の時刻（1601 年からの 100ns）と UNIX 時刻の差（100ns）。
const EPOCH_DIFF: i64 = 116_444_736_000_000_000;

pub(crate) struct Session {
    manager: Manager,
    /// 最後に読んだセッション（操作はこれに送る）と、その曲（ジャケットを読むため）。
    session: Option<MediaSession>,
    properties: Option<Properties>,
}

impl Session {
    pub(crate) fn new() -> Option<Self> {
        // WinRT はスレッドごとに初期化が要る（このワーカーのスレッドだけで使う）。
        let _ = unsafe { RoInitialize(RO_INIT_MULTITHREADED) };
        let manager = Manager::RequestAsync().ok()?.get().ok()?;
        Some(Self {
            manager,
            session: None,
            properties: None,
        })
    }

    /// 再生中・一時停止中のセッション（OS の並び順）と、そのアプリの ID。
    fn active(&self) -> Vec<(String, MediaSession)> {
        let Ok(list) = self.manager.GetSessions() else {
            return Vec::new();
        };
        (0..list.Size().unwrap_or(0))
            .filter_map(|i| list.GetAt(i).ok())
            .filter(|s| {
                s.GetPlaybackInfo()
                    .and_then(|i| i.PlaybackStatus())
                    .is_ok_and(|st| st == Status::Playing || st == Status::Paused)
            })
            .map(|s| (app_id(&s), s))
            .collect()
    }
}

fn app_id(session: &MediaSession) -> String {
    session
        .SourceAppUserModelId()
        .map(|s| s.to_string())
        .unwrap_or_default()
}

impl Backend for Session {
    fn read(&mut self, pick: Option<&Pick>) -> Option<Raw> {
        let active = self.active();
        let sessions: Vec<String> = active.iter().map(|(id, _)| id.clone()).collect();
        // 選んだものが無ければ、OS がいちばん関係があると見たもの（無ければ最初）。
        let current = self.manager.GetCurrentSession().ok();
        let index = picked(&sessions, pick)
            .or_else(|| {
                let current = current.as_ref()?;
                active
                    .iter()
                    .position(|(_, s)| s == current)
                    .or_else(|| sessions.iter().position(|id| *id == app_id(current)))
            })
            .or((!active.is_empty()).then_some(0))?;
        let session = active[index].1.clone();
        let info = session.GetPlaybackInfo().ok()?;
        let playing = info.PlaybackStatus().ok()? == Status::Playing;
        let properties = session.TryGetMediaPropertiesAsync().ok()?.get().ok()?;
        let text = |s: windows::core::Result<HSTRING>| s.map(|s| s.to_string()).unwrap_or_default();
        let (title, artist, album) = (
            text(properties.Title()),
            text(properties.Artist()),
            text(properties.AlbumTitle()),
        );
        let ticks = |t: i64| Duration::from_nanos(t.max(0) as u64 * 100);
        let timeline = session.GetTimelineProperties().ok();
        let (mut position, duration) = timeline
            .as_ref()
            .map(|t| {
                let start = t.StartTime().map_or(0, |d| d.Duration);
                let end = t.EndTime().map_or(0, |d| d.Duration);
                let position = t.Position().map_or(0, |d| d.Duration) - start;
                (ticks(position), (end > start).then(|| ticks(end - start)))
            })
            .unwrap_or_default();
        // 位置はアプリが最後に知らせた時点のもの。再生中なら、そこからの経過を足す。
        if playing
            && let Some(updated) = timeline.and_then(|t| t.LastUpdatedTime().ok())
            && let Ok(now) = SystemTime::now().duration_since(UNIX_EPOCH)
        {
            let now = (now.as_nanos() / 100) as i64 + EPOCH_DIFF;
            position += ticks(now - updated.UniversalTime);
        }
        let controls = info.Controls().ok();
        let can = |f: fn(&windows::Media::Control::GlobalSystemMediaTransportControlsSessionPlaybackControls) -> windows::core::Result<bool>| {
            controls.as_ref().is_some_and(|c| f(c).unwrap_or(false))
        };
        let controls = Controls {
            play_pause: can(|c| c.IsPlayPauseToggleEnabled()),
            next: can(|c| c.IsNextEnabled()),
            previous: can(|c| c.IsPreviousEnabled()),
            seek: can(|c| c.IsPlaybackPositionEnabled()),
            raise: false,
        };
        let app_id = sessions[index].clone();
        self.properties = Some(properties);
        self.session = Some(session);
        Some(Raw {
            art_key: format!("{app_id}\n{title}\n{artist}\n{album}"),
            title,
            artist,
            album,
            playing,
            position,
            duration,
            app_id,
            controls,
            sessions,
            index,
        })
    }

    fn art(&mut self) -> Option<Vec<u8>> {
        read_stream(&self.properties.as_ref()?.Thumbnail().ok()?)
    }

    fn app(&mut self, id: &str) -> (String, Option<Vec<u8>>, Option<String>) {
        // ストアのアプリ（と登録されたアプリ）は AppInfo から名前とロゴが取れる。ブラウザのような ID を持つ
        // デスクトップのアプリ（"Chrome"、Firefox の "308046B0AF4A39CB" など）はスタートメニューから引く。
        // "Spotify.exe" のように ID が実行ファイル名のものはどちらでも引けないので、ID から名前を作る
        // （アイコンは画面側がウィンドウのものを使う）。
        let info = AppInfo::GetFromAppUserModelId(&HSTRING::from(id))
            .and_then(|a| a.DisplayInfo())
            .ok();
        let shell = shell_app(id);
        let name = info
            .as_ref()
            .and_then(|d| d.DisplayName().ok())
            .map(|n| n.to_string())
            .or_else(|| shell.as_ref().map(|(n, _)| n.clone()))
            .filter(|n| !n.is_empty())
            .unwrap_or_else(|| fallback_name(id));
        let logo = info
            .and_then(|d| d.GetLogo(Size { Width: 64.0, Height: 64.0 }).ok())
            .and_then(|r| read_stream(&r.into()));
        (name, logo, shell.and_then(|(_, exe)| exe))
    }

    fn command(&mut self, command: Command) {
        let Some(session) = &self.session else {
            return;
        };
        let done = match command {
            Command::PlayPause => session.TryTogglePlayPauseAsync().and_then(|a| a.get()),
            Command::Next => session.TrySkipNextAsync().and_then(|a| a.get()),
            Command::Previous => session.TrySkipPreviousAsync().and_then(|a| a.get()),
            Command::Seek(at) => session
                .TryChangePlaybackPositionAsync((at.as_nanos() / 100) as i64)
                .and_then(|a| a.get()),
            Command::Raise | Command::Switch(_) => return,
        };
        if let Err(e) = done {
            logger::warn!("media command failed", command = ?command, error = e);
        }
    }
}

/// スタートメニューのアプリ（`shell:AppsFolder`）を AppUserModelId で引き、表示名と、ショートカットの指す
/// 実行ファイル名（"chrome.exe" など）を返す。
fn shell_app(id: &str) -> Option<(String, Option<String>)> {
    let path = HSTRING::from(format!("shell:AppsFolder\\{id}"));
    let item: IShellItem2 = unsafe { SHCreateItemFromParsingName(&path, None) }.ok()?;
    let take = |s: windows::core::Result<PWSTR>| {
        let s = s.ok()?;
        let text = unsafe { s.to_string() }.ok();
        unsafe { CoTaskMemFree(Some(s.0 as *const _)) };
        text
    };
    let name = take(unsafe { item.GetDisplayName(SIGDN_NORMALDISPLAY) })?;
    let exe = take(unsafe { item.GetString(&PKEY_Link_TargetParsingPath) })
        .and_then(|p| p.rsplit('\\').next().map(str::to_owned));
    Some((name, exe))
}

/// ストリームの中身を全部読む。
fn read_stream(reference: &IRandomAccessStreamReference) -> Option<Vec<u8>> {
    let stream = reference.OpenReadAsync().ok()?.get().ok()?;
    let size = u32::try_from(stream.Size().ok()?).ok()?;
    let reader = DataReader::CreateDataReader(&stream).ok()?;
    reader.LoadAsync(size).ok()?.get().ok()?;
    let mut bytes = vec![0; size as usize];
    reader.ReadBytes(&mut bytes).ok()?;
    Some(bytes)
}
