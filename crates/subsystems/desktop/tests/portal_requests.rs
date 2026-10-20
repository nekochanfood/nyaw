//! 実デスクトップを使わず、独立した D-Bus 上で Portal の要求と応答を検証する。
#![cfg(target_os = "linux")]

use ashpd::desktop::screencast::{Screencast, SourceType};
use futures_lite::{StreamExt, future::block_on};
use std::{
    collections::HashMap,
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
    time::Duration,
};
use zbus::{Connection, message::Header, zvariant::OwnedObjectPath, zvariant::OwnedValue};

struct ScreenCast;

struct Session;

#[zbus::interface(name = "org.freedesktop.portal.Session")]
impl Session {
    #[zbus(property, name = "version")]
    fn version(&self) -> u32 {
        1
    }
}

#[zbus::interface(name = "org.freedesktop.portal.ScreenCast")]
impl ScreenCast {
    #[zbus(property, name = "version")]
    fn version(&self) -> u32 {
        5
    }

    #[zbus(property)]
    fn available_source_types(&self) -> u32 {
        3
    }

    async fn create_session(
        &self,
        options: HashMap<String, OwnedValue>,
        #[zbus(connection)] connection: &Connection,
        #[zbus(header)] header: Header<'_>,
    ) -> zbus::fdo::Result<OwnedObjectPath> {
        let sender = header
            .sender()
            .unwrap()
            .trim_start_matches(':')
            .replace('.', "_");
        let token = options["handle_token"].downcast_ref::<&str>().unwrap();
        let session_token = options["session_handle_token"]
            .downcast_ref::<&str>()
            .unwrap();
        let request = OwnedObjectPath::try_from(format!(
            "/org/freedesktop/portal/desktop/request/{sender}/{token}"
        ))
        .unwrap();
        let session = OwnedObjectPath::try_from(format!(
            "/org/freedesktop/portal/desktop/session/{sender}/{session_token}"
        ))
        .unwrap();
        connection.object_server().at(&session, Session).await?;
        connection
            .emit_signal(
                header.sender().cloned(),
                &request,
                "org.freedesktop.portal.Request",
                "Response",
                &(
                    0u32,
                    HashMap::from([(
                        "session_handle",
                        OwnedValue::from(zbus::zvariant::Str::from(session.as_str())),
                    )]),
                ),
            )
            .await?;
        Ok(request)
    }
}

#[test]
#[ignore = "run on an isolated bus: dbus-run-session -- cargo test -p desktop --test portal_requests -- --ignored"]
fn request_does_not_query_nonexistent_properties() {
    block_on(async {
        // 実 Portal の名前を奪わず、既に存在していれば接続時に失敗する。
        let service = zbus::connection::Builder::session()
            .unwrap()
            .name("org.freedesktop.portal.Desktop")
            .unwrap()
            .serve_at("/org/freedesktop/portal/desktop", ScreenCast)
            .unwrap()
            .build()
            .await
            .unwrap();
        let property_calls = Arc::new(AtomicUsize::new(0));
        let observed = property_calls.clone();
        let mut messages = zbus::MessageStream::from(&service);
        let _monitor = service.executor().spawn(
            async move {
                while let Some(Ok(message)) = messages.next().await {
                    let header = message.header();
                    if header
                        .interface()
                        .is_some_and(|i| i == "org.freedesktop.DBus.Properties")
                        && header.path().is_some_and(|p| {
                            p.starts_with("/org/freedesktop/portal/desktop/request/")
                        })
                    {
                        observed.fetch_add(1, Ordering::SeqCst);
                    }
                }
            },
            "portal request property monitor",
        );

        // 監視自体が機能していることを、既知の不要な問い合わせで確かめる。
        let probe: zbus::Proxy = zbus::proxy::Builder::new(&service)
            .destination("org.freedesktop.portal.Desktop")
            .unwrap()
            .path("/org/freedesktop/portal/desktop/request/probe")
            .unwrap()
            .interface("org.freedesktop.portal.Request")
            .unwrap()
            .cache_properties(zbus::proxy::CacheProperties::No)
            .build()
            .await
            .unwrap();
        assert!(probe.get_property::<u32>("version").await.is_err());
        async_io::Timer::after(Duration::from_millis(50)).await;
        assert!(property_calls.swap(0, Ordering::SeqCst) > 0);

        let cast = Screencast::new().await.unwrap();
        assert!(
            cast.available_source_types()
                .await
                .unwrap()
                .contains(SourceType::Monitor)
        );
        for _ in 0..3 {
            cast.create_session(Default::default()).await.unwrap();
        }
        // キャッシュのバックグラウンド処理から遅れて送られる問い合わせも観測する。
        async_io::Timer::after(Duration::from_millis(50)).await;
        assert_eq!(property_calls.load(Ordering::SeqCst), 0);
    });
}
