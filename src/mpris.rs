use crate::audio::AudioEngine;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;
use zbus::{fdo, interface, Connection};
use zvariant::{ObjectPath, OwnedValue, Str, Value};

pub struct MprisRoot {
    pub on_raise: Arc<dyn Fn() + Send + Sync>,
    pub on_quit: Arc<dyn Fn() + Send + Sync>,
}

#[interface(name = "org.mpris.MediaPlayer2")]
impl MprisRoot {
    async fn raise(&self) {
        (self.on_raise)();
    }

    async fn quit(&self) {
        (self.on_quit)();
    }

    #[zbus(property)]
    async fn identity(&self) -> &str {
        "VinylPlayer"
    }

    #[zbus(property)]
    async fn desktop_entry(&self) -> &str {
        "io.github.vinylplayer"
    }

    #[zbus(property)]
    async fn can_quit(&self) -> bool {
        true
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
        vec![
            "audio/flac",
            "audio/mpeg",
            "audio/ogg",
            "audio/wav",
            "audio/x-wav",
            "audio/mp4",
            "audio/aac",
            "audio/opus",
        ]
    }
}

pub struct MprisPlayer {
    pub audio: Arc<AudioEngine>,
}

fn is_supported_audio_file(path: &Path) -> bool {
    if !path.is_file() {
        return false;
    }
    let ext = path
        .extension()
        .and_then(|s| s.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();
    matches!(
        ext.as_str(),
        "flac" | "mp3" | "ogg" | "oga" | "wav" | "m4a" | "aac" | "opus" | "alac" | "aif" | "aiff" | "wma"
    )
}

pub(crate) fn resolve_local_audio_uri(uri: &str) -> Result<PathBuf, String> {
    let trimmed = uri.trim();
    if trimmed.is_empty() {
        return Err("Empty URI".into());
    }
    let path = if trimmed.starts_with("file://") {
        let (p, host) = glib::filename_from_uri(trimmed)
            .map_err(|e| format!("Invalid file URI: {e}"))?;
        if host
            .as_deref()
            .is_some_and(|h| !h.is_empty() && h != "localhost")
        {
            return Err("Remote file URIs are not supported".into());
        }
        p
    } else if trimmed.contains("://") {
        return Err("Only local file:// URIs are supported".into());
    } else {
        PathBuf::from(trimmed)
    };
    if !path.is_absolute() || !is_supported_audio_file(&path) {
        return Err(format!("Unavailable or unsupported audio file: {}", path.display()));
    }
    Ok(path)
}

pub(crate) fn clamp_seek_target(cur_us: i64, dur_us: i64, offset_us: i64) -> i64 {
    if dur_us > 0 {
        cur_us.saturating_add(offset_us).clamp(0, dur_us)
    } else {
        cur_us.saturating_add(offset_us).max(0)
    }
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

    async fn seek(&self, offset: i64) -> fdo::Result<()> {
        let target = clamp_seek_target(self.audio.position_us(), self.audio.duration_us(), offset);
        self.audio
            .seek_us(target)
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
    async fn volume(&self) -> f64 {
        self.audio.volume()
    }

    #[zbus(property)]
    async fn set_volume(&self, volume: f64) -> zbus::Result<()> {
        self.audio.set_volume(volume);
        Ok(())
    }

    #[zbus(property)]
    async fn rate(&self) -> f64 {
        self.audio.rate()
    }

    #[zbus(property)]
    async fn set_rate(&self, rate: f64) -> zbus::Result<()> {
        self.audio.set_rate(rate);
        Ok(())
    }

    #[zbus(property)]
    async fn minimum_rate(&self) -> f64 {
        0.92
    }

    #[zbus(property)]
    async fn maximum_rate(&self) -> f64 {
        1.08
    }

    #[zbus(property)]
    async fn metadata(&self) -> std::collections::HashMap<String, OwnedValue> {
        let m = self.audio.metadata();
        let mut o = std::collections::HashMap::new();
        let track_path = if m.uri.is_empty() {
            "/org/mpris/MediaPlayer2/TrackList/NoTrack".to_string()
        } else {
            format!("/org/mpris/MediaPlayer2/track/{}", self.audio.track_sequence())
        };
        o.insert(
            "mpris:trackid".into(),
            OwnedValue::from(ObjectPath::try_from(track_path.as_str()).unwrap_or_else(|_| {
                ObjectPath::from_static_str("/org/mpris/MediaPlayer2/TrackList/NoTrack").unwrap()
            })),
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
        let duration = if m.duration_us > 0 {
            m.duration_us
        } else {
            self.audio.duration_us()
        };
        if duration > 0 {
            o.insert("mpris:length".into(), OwnedValue::from(duration));
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
        let (len, current) = {
            let snap = self.audio.queue_snapshot();
            (snap.tracks.len(), snap.current)
        };
        if len == 0 {
            return false;
        }
        if self.audio.repeat_mode() != crate::audio::RepeatMode::Off || self.audio.shuffle() {
            return true;
        }
        current.is_some_and(|i| i + 1 < len)
    }

    #[zbus(property)]
    async fn can_go_previous(&self) -> bool {
        let (len, current) = {
            let snap = self.audio.queue_snapshot();
            (snap.tracks.len(), snap.current)
        };
        if len == 0 {
            return false;
        }
        if self.audio.repeat_mode() != crate::audio::RepeatMode::Off || self.audio.shuffle() {
            return true;
        }
        current.is_some_and(|i| i > 0)
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
    async fn can_seek(&self) -> bool {
        true
    }

    #[zbus(property)]
    async fn can_control(&self) -> bool {
        true
    }

    async fn set_position(&self, _track_id: ObjectPath<'_>, position: i64) -> fdo::Result<()> {
        let dur = self.audio.duration_us();
        let clamped = if dur > 0 {
            position.clamp(0, dur)
        } else {
            position.max(0)
        };
        self.audio
            .seek_us(clamped)
            .map_err(|e| fdo::Error::Failed(e.to_string()))
    }

    async fn open_uri(&self, uri: &str) -> fdo::Result<()> {
        let path = resolve_local_audio_uri(uri).map_err(fdo::Error::InvalidArgs)?;
        self.audio
            .load(&path)
            .map_err(|e| fdo::Error::Failed(e.to_string()))?;
        self.play().await
    }
}

pub async fn serve(
    audio: Arc<AudioEngine>,
    on_raise: Arc<dyn Fn() + Send + Sync>,
    on_quit: Arc<dyn Fn() + Send + Sync>,
) -> zbus::Result<()> {
    let c = Connection::session().await?;
    c.object_server()
        .at("/org/mpris/MediaPlayer2", MprisRoot { on_raise, on_quit })
        .await?;
    c.object_server()
        .at(
            "/org/mpris/MediaPlayer2",
            MprisPlayer {
                audio: audio.clone(),
            },
        )
        .await?;
    // Export interfaces before claiming the bus name so callers never race NameOwnerChanged.
    c.request_name("org.mpris.MediaPlayer2.VinylPlayer").await?;

    let iface_ref = c
        .object_server()
        .interface::<_, MprisPlayer>("/org/mpris/MediaPlayer2")
        .await?;
    let mut last_state = audio.state();
    let mut last_meta_ver = audio.metadata_version();
    let mut last_track_seq = audio.track_sequence();
    let mut last_repeat = audio.repeat_mode();
    let mut last_shuffle = audio.shuffle();
    let mut last_vol_milli = (audio.volume() * 1000.0).round() as i64;
    let mut last_rate_milli = (audio.rate() * 1000.0).round() as i64;

    loop {
        glib::timeout_future(Duration::from_millis(250)).await;
        let state = audio.state();
        let meta_ver = audio.metadata_version();
        let track_seq = audio.track_sequence();
        let repeat = audio.repeat_mode();
        let shuffle = audio.shuffle();
        let vol_milli = (audio.volume() * 1000.0).round() as i64;
        let rate_milli = (audio.rate() * 1000.0).round() as i64;

        let iface = iface_ref.get().await;
        let ctx = iface_ref.signal_context();
        if state != last_state {
            last_state = state;
            let _ = iface.playback_status_changed(ctx).await;
        }
        if meta_ver != last_meta_ver || track_seq != last_track_seq {
            last_meta_ver = meta_ver;
            last_track_seq = track_seq;
            let _ = iface.metadata_changed(ctx).await;
        }
        if repeat != last_repeat {
            last_repeat = repeat;
            let _ = iface.loop_status_changed(ctx).await;
        }
        if shuffle != last_shuffle {
            last_shuffle = shuffle;
            let _ = iface.shuffle_changed(ctx).await;
        }
        if vol_milli != last_vol_milli {
            last_vol_milli = vol_milli;
            let _ = iface.volume_changed(ctx).await;
        }
        if rate_milli != last_rate_milli {
            last_rate_milli = rate_milli;
            let _ = iface.rate_changed(ctx).await;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolves_local_audio_uri_and_rejects_remote_or_missing() {
        let config = crate::state::TestConfig::new();
        let wav = config.path().with_file_name("song #1.wav");
        let txt = config.path().with_file_name("notes.txt");
        std::fs::write(&wav, b"fixture").unwrap();
        std::fs::write(&txt, b"text").unwrap();

        let uri = glib::filename_to_uri(&wav, None).unwrap();
        assert_eq!(resolve_local_audio_uri(&uri).unwrap(), wav);
        assert_eq!(
            resolve_local_audio_uri(wav.to_str().unwrap()).unwrap(),
            wav
        );
        assert!(resolve_local_audio_uri("https://example.invalid/a.flac").is_err());
        assert!(resolve_local_audio_uri("file://remote-host/tmp/a.flac").is_err());
        assert!(resolve_local_audio_uri(&glib::filename_to_uri(&txt, None).unwrap()).is_err());
        assert!(resolve_local_audio_uri("/nonexistent/missing.flac").is_err());
        assert!(resolve_local_audio_uri("   ").is_err());
    }

    #[test]
    fn clamp_seek_target_bounds_positions() {
        assert_eq!(clamp_seek_target(1_000, 10_000, -5_000), 0);
        assert_eq!(clamp_seek_target(5_000, 10_000, 8_000), 10_000);
        assert_eq!(clamp_seek_target(5_000, 10_000, 2_000), 7_000);
        assert_eq!(clamp_seek_target(2_000, 0, -5_000), 0);
        assert_eq!(clamp_seek_target(2_000, 0, 3_000), 5_000);
    }
}
