//! Windows: GSMTC（`GlobalSystemMediaTransportControlsSessionManager`）。OS がいちばん関係があると見たセッション
//! （`GetCurrentSession`）を出す。

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
    Win32::System::WinRT::{RO_INIT_MULTITHREADED, RoInitialize},
    core::HSTRING,
};

use super::{Backend, Command, Controls, Raw, fallback_name};

/// Windows の時刻（1601 年からの 100ns）と UNIX 時刻の差（100ns）。
const EPOCH_DIFF: i64 = 116_444_736_000_000_000;

pub(crate) struct Session {
    manager: Manager,
    /// 最後に読んだ曲（ジャケットを読むため）。
    properties: Option<Properties>,
}

impl Session {
    pub(crate) fn new() -> Option<Self> {
        // WinRT はスレッドごとに初期化が要る（このワーカーのスレッドだけで使う）。
        let _ = unsafe { RoInitialize(RO_INIT_MULTITHREADED) };
        let manager = Manager::RequestAsync().ok()?.get().ok()?;
        Some(Self {
            manager,
            properties: None,
        })
    }

    fn current(&self) -> Option<MediaSession> {
        self.manager.GetCurrentSession().ok()
    }
}

impl Backend for Session {
    fn read(&mut self) -> Option<Raw> {
        let session = self.current()?;
        let info = session.GetPlaybackInfo().ok()?;
        let status = info.PlaybackStatus().ok()?;
        let playing = status == Status::Playing;
        if !playing && status != Status::Paused {
            return None;
        }
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
        };
        let app_id = text(session.SourceAppUserModelId());
        self.properties = Some(properties);
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
        })
    }

    fn art(&mut self) -> Option<Vec<u8>> {
        read_stream(&self.properties.as_ref()?.Thumbnail().ok()?)
    }

    fn app(&mut self, id: &str) -> (String, Option<Vec<u8>>) {
        // ストアのアプリ（と登録されたアプリ）は AppInfo から名前とロゴが取れる。"Spotify.exe" のような
        // デスクトップのアプリは取れないので、ID から名前を作る（アイコンは画面側がウィンドウのものを使う）。
        let info = AppInfo::GetFromAppUserModelId(&HSTRING::from(id))
            .and_then(|a| a.DisplayInfo())
            .ok();
        let name = info
            .as_ref()
            .and_then(|d| d.DisplayName().ok())
            .map(|n| n.to_string())
            .filter(|n| !n.is_empty())
            .unwrap_or_else(|| fallback_name(id));
        let logo = info
            .and_then(|d| d.GetLogo(Size { Width: 64.0, Height: 64.0 }).ok())
            .and_then(|r| read_stream(&r.into()));
        (name, logo)
    }

    fn command(&mut self, command: Command) {
        let Some(session) = self.current() else {
            return;
        };
        let done = match command {
            Command::PlayPause => session.TryTogglePlayPauseAsync().and_then(|a| a.get()),
            Command::Next => session.TrySkipNextAsync().and_then(|a| a.get()),
            Command::Previous => session.TrySkipPreviousAsync().and_then(|a| a.get()),
            Command::Seek(at) => session
                .TryChangePlaybackPositionAsync((at.as_nanos() / 100) as i64)
                .and_then(|a| a.get()),
            Command::Raise => return,
        };
        if let Err(e) = done {
            logger::warn!("media command failed", command = ?command, error = e);
        }
    }
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
