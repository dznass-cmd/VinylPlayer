use crate::audio::AudioEngine;
use std::sync::Arc;
use zbus::{fdo, interface, Connection};
use zvariant::{ObjectPath, OwnedValue, Str, Value};

pub struct MprisRoot;

#[interface(name = "org.mpris.MediaPlayer2")]
impl MprisRoot {
    #[zbus(property)]
    async fn identity(&self) -> &str {
        "VinylPlayer"
    }

    #[zbus(property)]
    async fn can_quit(&self) -> bool {
        false
    }

    #[zbus(property)]
    async fn can_raise(&self) -> bool {
        true
    }

    #[zbus(property)]
    async fn has_track_list(&self) -> bool {
        false
    }

    #[zbus(property)]
    async fn supported_uri_schemes(&self) -> Vec<&str> {
        vec!["file"]
    }

    #[zbus(property)]
    async fn supported_mime_types(&self) -> Vec<&str> {
        vec!["audio/flac", "audio/mpeg", "audio/ogg", "audio/x-wav"]
    }
}

pub struct MprisPlayer {
    pub audio: Arc<AudioEngine>,
}

#[interface(name = "org.mpris.MediaPlayer2.Player")]
impl MprisPlayer {
    async fn play(&self) -> fdo::Result<()> {
        self.audio
            .play()
            .map_err(|e| fdo::Error::Failed(e.to_string()))
    }

    async fn pause(&self) -> fdo::Result<()> {
        self.audio
            .pause()
            .map_err(|e| fdo::Error::Failed(e.to_string()))
    }

    async fn play_pause(&self) -> fdo::Result<()> {
        if self.audio.state() == crate::audio::PlaybackState::Playing {
            self.pause().await
        } else {
            self.play().await
        }
    }

    async fn stop(&self) -> fdo::Result<()> {
        self.audio
            .stop()
            .map_err(|e| fdo::Error::Failed(e.to_string()))
    }

    async fn next(&self) -> fdo::Result<()> {
        self.audio
            .next()
            .map_err(|e| fdo::Error::Failed(e.to_string()))
    }

    async fn previous(&self) -> fdo::Result<()> {
        self.audio
            .prev()
            .map_err(|e| fdo::Error::Failed(e.to_string()))
    }

    #[zbus(property)]
    async fn playback_status(&self) -> &str {
        match self.audio.state() {
            crate::audio::PlaybackState::Playing => "Playing",
            crate::audio::PlaybackState::Paused => "Paused",
            _ => "Stopped",
        }
    }

    #[zbus(property)]
    async fn position(&self) -> i64 {
        self.audio.position_us()
    }

    #[zbus(property)]
    async fn loop_status(&self) -> &str {
        self.audio.repeat_mode().to_mpris_str()
    }

    #[zbus(property)]
    async fn set_loop_status(&self, loop_status: &str) -> zbus::Result<()> {
        let mode = crate::audio::RepeatMode::from_mpris_str(loop_status);
        self.audio.set_repeat_mode(mode);
        Ok(())
    }

    #[zbus(property)]
    async fn shuffle(&self) -> bool {
        self.audio.shuffle()
    }

    #[zbus(property)]
    async fn set_shuffle(&self, shuffle: bool) -> zbus::Result<()> {
        self.audio.set_shuffle(shuffle);
        Ok(())
    }

    #[zbus(property)]
    async fn metadata(&self) -> std::collections::HashMap<String, OwnedValue> {
        let m = self.audio.metadata();
        let mut o = std::collections::HashMap::new();
        o.insert(
            "mpris:trackid".into(),
            OwnedValue::from(ObjectPath::try_from("/org/mpris/MediaPlayer2/track/0").unwrap()),
        );
        if !m.title.is_empty() {
            o.insert("xesam:title".into(), OwnedValue::from(Str::from(m.title)));
        }
        if !m.artist.is_empty() {
            if let Ok(val) = Value::from(vec![m.artist]).try_to_owned() {
                o.insert("xesam:artist".into(), val);
            }
        }
        if !m.album.is_empty() {
            o.insert("xesam:album".into(), OwnedValue::from(Str::from(m.album)));
        }
        if m.duration_us > 0 {
            o.insert("mpris:length".into(), OwnedValue::from(m.duration_us));
        }
        if !m.uri.is_empty() {
            o.insert("xesam:url".into(), OwnedValue::from(Str::from(m.uri)));
        }
        if let Some(cover) = m.cover_path {
            if let Ok(uri) = glib::filename_to_uri(&cover, None) {
                o.insert(
                    "mpris:artUrl".into(),
                    OwnedValue::from(Str::from(uri.as_str())),
                );
            }
        }
        o
    }

    #[zbus(property)]
    async fn can_go_next(&self) -> bool {
        true
    }

    #[zbus(property)]
    async fn can_go_previous(&self) -> bool {
        true
    }

    #[zbus(property)]
    async fn can_play(&self) -> bool {
        true
    }

    #[zbus(property)]
    async fn can_pause(&self) -> bool {
        true
    }

    #[zbus(property)]
    async fn can_control(&self) -> bool {
        true
    }

    async fn set_position(&self, _track_id: ObjectPath<'_>, position: i64) -> fdo::Result<()> {
        self.audio
            .seek_us(position)
            .map_err(|e| fdo::Error::Failed(e.to_string()))
    }

    async fn open_uri(&self, uri: &str) -> fdo::Result<()> {
        let path = if uri.starts_with("file://") {
            glib::filename_from_uri(uri)
                .map(|(p, _)| p)
                .unwrap_or_else(|_| {
                    std::path::PathBuf::from(uri.strip_prefix("file://").unwrap_or(uri))
                })
        } else {
            std::path::PathBuf::from(uri)
        };
        self.audio
            .load(&path)
            .map_err(|e| fdo::Error::Failed(e.to_string()))?;
        self.play().await
    }
}

pub async fn serve(audio: Arc<AudioEngine>) -> zbus::Result<()> {
    let c = Connection::session().await?;
    c.request_name("org.mpris.MediaPlayer2.VinylPlayer").await?;
    c.object_server()
        .at("/org/mpris/MediaPlayer2", MprisRoot)
        .await?;
    c.object_server()
        .at("/org/mpris/MediaPlayer2", MprisPlayer { audio })
        .await?;
    std::future::pending::<()>().await;
    Ok(())
}
