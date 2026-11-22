use crate::playback_queue::{PlaybackQueue, QueueSnapshot};
use anyhow::{Context, Result};
use gst::prelude::*;
use gstreamer as gst;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, AtomicU8, Ordering};
use std::sync::{Arc, Mutex};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[repr(u8)]
pub enum RepeatMode {
    #[default]
    Off = 0,
    All = 1,
    Track = 2,
}

impl RepeatMode {
    pub fn from_u8(val: u8) -> Self {
        match val {
            1 => RepeatMode::All,
            2 => RepeatMode::Track,
            _ => RepeatMode::Off,
        }
    }

    pub fn to_u8(self) -> u8 {
        self as u8
    }

    pub fn next_cycle(self) -> Self {
        match self {
            RepeatMode::Off => RepeatMode::All,
            RepeatMode::All => RepeatMode::Track,
            RepeatMode::Track => RepeatMode::Off,
        }
    }

    pub fn to_mpris_str(self) -> &'static str {
        match self {
            RepeatMode::Off => "None",
            RepeatMode::All => "Playlist",
            RepeatMode::Track => "Track",
        }
    }

    pub fn from_mpris_str(s: &str) -> Self {
        match s {
            "Playlist" => RepeatMode::All,
            "Track" => RepeatMode::Track,
            _ => RepeatMode::Off,
        }
    }

    pub fn to_str(self) -> &'static str {
        match self {
            RepeatMode::Off => "off",
            RepeatMode::All => "all",
            RepeatMode::Track => "track",
        }
    }

    pub fn from_str(s: &str) -> Self {
        match s {
            "all" => RepeatMode::All,
            "track" => RepeatMode::Track,
            _ => RepeatMode::Off,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum PlaybackState {
    #[default]
    Stopped,
    Playing,
    Paused,
}

#[allow(dead_code)]
#[derive(Clone, Debug, Default)]
pub struct TrackMetadata {
    pub title: String,
    pub artist: String,
    pub album: String,
    pub uri: String,
    pub duration_us: i64,
    pub codec: String,
    pub sample_rate: Option<i32>,
    pub bit_depth: Option<i32>,
    pub cover_path: Option<PathBuf>,
    pub cover_bytes: Option<Vec<u8>>,
}

#[allow(dead_code)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum AudioQuality {
    HiRes,
    HiFi,
    Standard,
}

#[allow(dead_code)]
pub(crate) fn classify_quality(
    bit_depth: Option<i32>,
    sample_rate: Option<i32>,
    lossless: bool,
) -> AudioQuality {
    if lossless && bit_depth.unwrap_or(0) >= 24 && sample_rate.unwrap_or(0) >= 96_000 {
        AudioQuality::HiRes
    } else if lossless && bit_depth.unwrap_or(0) >= 16 && sample_rate.unwrap_or(0) >= 44_100 {
        AudioQuality::HiFi
    } else {
        AudioQuality::Standard
    }
}

fn cyclic_index(current: usize, len: usize, forward: bool) -> Option<usize> {
    if len == 0 {
        return None;
    }
    Some(if forward {
        (current + 1) % len
    } else {
        (current + len - 1) % len
    })
}

pub fn pick_shuffle_index(current: Option<usize>, len: usize) -> Option<usize> {
    if len == 0 {
        return None;
    }
    if len == 1 {
        return Some(0);
    }
    let cur = current.unwrap_or(0);
    // SplitMix64 PRNG com seed derivada do tempo monotônico e do índice atual
    let seed = glib::monotonic_time() as u64 ^ (cur as u64).wrapping_mul(0x9E3779B97F4A7C15);
    let mut z = seed.wrapping_add(0x9E3779B97F4A7C15);
    z = (z ^ (z >> 30)).wrapping_mul(0xBF58476D1CE4E5B9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94D049BB133111EB);
    let rand_val = (z ^ (z >> 31)) as usize;
    let offset = 1 + (rand_val % (len - 1));
    Some((cur + offset) % len)
}

fn find_track(queue: &[PathBuf], query: &str) -> Option<PathBuf> {
    let q = query.trim().to_lowercase();
    if q.is_empty() {
        return None;
    }
    queue
        .iter()
        .find(|path| {
            path.file_stem()
                .and_then(|s| s.to_str())
                .map(|s| s.to_lowercase().contains(&q))
                .unwrap_or(false)
        })
        .cloned()
}

#[derive(Clone, Debug)]
struct GaplessPending {
    index: usize,
    path: PathBuf,
    uri: String,
}

fn next_auto_index_from(queue: &PlaybackQueue, shuffle: bool, repeat: RepeatMode) -> Option<usize> {
    let len = queue.tracks.len();
    if len == 0 {
        return None;
    }
    if repeat == RepeatMode::Track {
        return queue.current.filter(|i| *i < len);
    }
    if shuffle {
        pick_shuffle_index(queue.current, len)
    } else {
        let next = queue
            .current
            .and_then(|i| i.checked_add(1))
            .filter(|i| *i < len);
        if next.is_some() {
            next
        } else if repeat == RepeatMode::All {
            Some(0)
        } else {
            None
        }
    }
}

#[derive(Clone)]
pub struct AudioEngine {
    playbin: gst::Element,
    state: Arc<Mutex<PlaybackState>>,
    metadata: Arc<Mutex<TrackMetadata>>,
    metadata_ver: Arc<AtomicU64>,
    attempts: Arc<AtomicU64>,
    track_seq: Arc<AtomicU64>,
    queue: Arc<Mutex<PlaybackQueue>>,
    shuffle: Arc<AtomicBool>,
    repeat_mode: Arc<AtomicU8>,
    level_l: Arc<AtomicU32>,
    level_r: Arc<AtomicU32>,
    rate: Arc<AtomicU32>,
    pending_gapless: Arc<Mutex<Option<GaplessPending>>>,
}

impl AudioEngine {
    /// Restaura somente a seleção; não chama play/pausa nem decodifica a faixa.
    pub fn with_queue(tracks: Vec<PathBuf>, current: Option<usize>) -> Result<Self> {
        gst::init().context("inicializar GStreamer")?;
        let playbin = gst::ElementFactory::make("playbin3")
            .name("vinylplayer")
            .build()
            .context("plugin playbin3 ausente")?;
        // Keep conversion/resampling inside the audio sink.  This avoids
        // exposing FLAC caps (notably 24-bit/rates) directly to PipeWire.
        let sink = make_audio_sink()?;
        playbin.set_property("audio-sink", &sink);
        let selected = if tracks.is_empty() {
            None
        } else {
            Some(current.filter(|i| *i < tracks.len()).unwrap_or(0))
        };
        let mut queue = PlaybackQueue::new(tracks);
        queue.current = selected;
        let engine = Self {
            playbin,
            state: Default::default(),
            metadata: Default::default(),
            metadata_ver: Arc::new(AtomicU64::new(1)),
            attempts: Arc::new(AtomicU64::new(0)),
            track_seq: Arc::new(AtomicU64::new(0)),
            queue: Arc::new(Mutex::new(queue)),
            shuffle: Arc::new(AtomicBool::new(false)),
            repeat_mode: Arc::new(AtomicU8::new(RepeatMode::Off.to_u8())),
            level_l: Arc::new(AtomicU32::new((-80.0f32).to_bits())),
            level_r: Arc::new(AtomicU32::new((-80.0f32).to_bits())),
            rate: Arc::new(AtomicU32::new((1.0f32).to_bits())),
            pending_gapless: Default::default(),
        };
        engine.connect_gapless();
        if let Some(index) = selected {
            let queue = engine.queue.lock().unwrap();
            engine.set_track(&queue.tracks[index])?;
        }
        Ok(engine)
    }

    fn connect_gapless(&self) {
        let weak_playbin = self.playbin.downgrade();
        let state = self.state.clone();
        let queue = self.queue.clone();
        let shuffle = self.shuffle.clone();
        let repeat_mode = self.repeat_mode.clone();
        let pending_gapless = self.pending_gapless.clone();
        self.playbin.connect("about-to-finish", false, move |_| {
            let pb = weak_playbin.upgrade()?;
            if *state.lock().unwrap() == PlaybackState::Stopped {
                return None;
            }
            let next_uri = {
                // Use try_lock to prevent AB-BA deadlock while set_track holds queue
                // and calls playbin.set_state(Ready) waiting on the streaming thread.
                let mut guard = None;
                for _ in 0..4 {
                    if let Ok(q) = queue.try_lock() {
                        guard = Some(q);
                        break;
                    }
                    std::thread::yield_now();
                }
                let q = guard?;
                let mut pending = pending_gapless.lock().unwrap();
                if pending.is_some() {
                    return None;
                }
                let idx = next_auto_index_from(
                    &q,
                    shuffle.load(Ordering::Relaxed),
                    RepeatMode::from_u8(repeat_mode.load(Ordering::Relaxed)),
                )?;
                let path = q.tracks.get(idx)?.clone();
                let uri = glib::filename_to_uri(&path, None).ok()?;
                *pending = Some(GaplessPending {
                    index: idx,
                    path,
                    uri: uri.to_string(),
                });
                Some(uri)
            };
            if let Some(uri) = next_uri {
                pb.set_property("uri", &uri);
            }
            None
        });
    }
    pub fn load(&self, path: impl AsRef<Path>) -> Result<()> {
        let path = path.as_ref();
        let mut queue = self.queue.lock().unwrap();
        self.set_track(path)?;
        queue.select_path(path);
        Ok(())
    }

    fn apply_track_metadata(&self, path: &Path, expected_uri: String) {
        let initial_cover = if path.is_file() {
            match find_cached_or_local_cover(path) {
                Ok(cover) => cover,
                Err(cache_out) => {
                    let path_bg = path.to_path_buf();
                    let meta_bg = self.metadata.clone();
                    let ver_bg = self.metadata_ver.clone();
                    let uri_bg = expected_uri.clone();
                    let _ = std::thread::Builder::new()
                        .name("vinyl-cover-extract".into())
                        .spawn(move || {
                            if let Some(extracted) = extract_flac_cover(&path_bg, &cache_out) {
                                let mut m = meta_bg.lock().unwrap();
                                if m.uri == uri_bg && m.cover_path.is_none() {
                                    m.cover_path = Some(extracted);
                                    drop(m);
                                    ver_bg.fetch_add(1, Ordering::Relaxed);
                                }
                            }
                        });
                    None
                }
            }
        } else {
            None
        };
        let mut m = self.metadata.lock().unwrap();
        *m = TrackMetadata {
            uri: expected_uri,
            title: path
                .file_stem()
                .and_then(|s| s.to_str())
                .unwrap_or("Track")
                .into(),
            cover_path: initial_cover,
            ..Default::default()
        };
        drop(m);
        self.metadata_ver.fetch_add(1, Ordering::Relaxed);
        self.track_seq.fetch_add(1, Ordering::Relaxed);
    }

    fn apply_current_rate(&self, pos_us: i64) {
        let cur_rate = self.rate();
        if (cur_rate - 1.0).abs() > 0.002 {
            let pos = (pos_us.max(0) as u64).saturating_mul(1000);
            let seek_event = gst::event::Seek::new(
                cur_rate,
                gst::SeekFlags::FLUSH | gst::SeekFlags::ACCURATE,
                gst::SeekType::Set,
                gst::ClockTime::from_nseconds(pos),
                gst::SeekType::None,
                gst::ClockTime::NONE,
            );
            let _ = self.playbin.send_event(seek_event);
        }
    }

    fn commit_pending_gapless(&self, apply_rate: bool) {
        let committed = {
            let mut queue = self.queue.lock().unwrap();
            let Some(pending) = self.pending_gapless.lock().unwrap().take() else {
                return;
            };
            if self.state() == PlaybackState::Stopped {
                return;
            }
            if queue.tracks.get(pending.index) == Some(&pending.path) {
                queue.current = Some(pending.index);
            } else if let Some(pos) = queue.tracks.iter().position(|p| p == &pending.path) {
                queue.current = Some(pos);
            }
            pending
        };
        self.apply_track_metadata(&committed.path, committed.uri);
        let d = self
            .playbin
            .query_duration::<gst::ClockTime>()
            .map(|v| v.nseconds() as i64 / 1000)
            .unwrap_or(0);
        if d > 0 {
            self.metadata.lock().unwrap().duration_us = d;
            self.metadata_ver.fetch_add(1, Ordering::Relaxed);
        }
        if apply_rate {
            self.apply_current_rate(0);
        }
    }

    fn set_track(&self, path: &Path) -> Result<()> {
        let uri = glib::filename_to_uri(path, None).context("converter caminho para URI")?;
        // playbin can reject a URI change while PAUSED/PLAYING.  READY keeps
        // the sink alive while fully releasing the previous stream.
        let bus = self.playbin.bus().expect("bus do player");
        bus.set_flushing(true);
        let ready = self.playbin.set_state(gst::State::Ready);
        bus.set_flushing(false);
        ready.map_err(|e| anyhow::anyhow!("preparar nova faixa: {e:?}"))?;
        *self.pending_gapless.lock().unwrap() = None;
        self.playbin.set_property("uri", &uri);
        self.apply_track_metadata(path, uri.to_string());
        Ok(())
    }

    pub fn play_album(&self, tracks: &[PathBuf], start: usize) -> Result<()> {
        self.play_files(tracks, start)
    }

    /// Reproduz a seleção recebida, preservando a ordem dos arquivos.
    pub fn play_files(&self, tracks: &[PathBuf], start: usize) -> Result<()> {
        let path = tracks
            .get(start)
            .context("seleção vazia ou faixa inválida")?
            .clone();
        let mut queue = self.queue.lock().unwrap();
        self.set_track(&path)?;
        queue.replace(tracks, start);
        self.play()
    }

    pub fn queue_status(&self) -> (u64, Option<usize>, usize) {
        self.queue.lock().unwrap().status()
    }

    pub fn queue_snapshot(&self) -> QueueSnapshot {
        self.queue.lock().unwrap().snapshot()
    }

    /// Amplia a fila sem recarregar o stream atual ou modificar play/pausa.
    pub fn enqueue_files(&self, tracks: &[PathBuf], next: bool) -> Result<()> {
        anyhow::ensure!(
            tracks.iter().all(|p| p.is_absolute() && p.is_file()),
            crate::i18n::pick(
                "One of the tracks is not available. Refresh your library and try again.",
                "Uma das músicas não está disponível. Atualize a biblioteca e tente novamente.",
            )
        );
        let mut queue = self.queue.lock().unwrap();
        if queue.tracks.is_empty() {
            if let Some(first) = tracks.first() {
                self.set_track(first)?;
            }
        }
        queue.enqueue(tracks, next);
        Ok(())
    }

    /// Descoberta inicial não substitui uma fila recebida enquanto o worker trabalhava.
    pub fn seed_library_if_unchanged(&self, revision: u64, tracks: &[PathBuf]) -> Result<()> {
        let mut queue = self.queue.lock().unwrap();
        if queue.status().0 != revision || tracks.is_empty() {
            return Ok(());
        }
        let selected = queue.current.and_then(|i| queue.tracks.get(i)).cloned();
        let mut tracks = tracks.to_vec();
        let index = if let Some(path) = selected {
            tracks.iter().position(|p| p == &path).unwrap_or_else(|| {
                tracks.push(path);
                tracks.len() - 1
            })
        } else {
            self.set_track(&tracks[0])?;
            0
        };
        queue.replace(&tracks, index);
        Ok(())
    }

    pub fn play_queue_item(&self, revision: u64, index: usize) -> Result<()> {
        let mut queue = self.queue.lock().unwrap();
        anyhow::ensure!(
            queue.accepts(revision, index),
            crate::i18n::pick(
                "The queue changed. Try again.",
                "A fila mudou. Tente novamente."
            )
        );
        self.set_track(&queue.tracks[index])?;
        queue.current = Some(index);
        self.play()
    }

    pub fn move_queue_item(&self, revision: u64, from: usize, to: usize) -> Result<()> {
        let mut queue = self.queue.lock().unwrap();
        anyhow::ensure!(
            queue.accepts(revision, from) && to < queue.tracks.len(),
            crate::i18n::pick(
                "The queue changed. Try again.",
                "A fila mudou. Tente novamente."
            )
        );
        queue.move_item(from, to);
        Ok(())
    }

    pub fn remove_queue_item(&self, revision: u64, index: usize) -> Result<()> {
        let mut queue = self.queue.lock().unwrap();
        anyhow::ensure!(
            queue.accepts(revision, index),
            crate::i18n::pick(
                "The queue changed. Try again.",
                "A fila mudou. Tente novamente."
            )
        );
        let removing_current = queue.current == Some(index);
        let previous = self.state();
        queue.remove(index);
        if removing_current {
            *self.pending_gapless.lock().unwrap() = None;
            if let Some(current) = queue.current {
                self.set_track(&queue.tracks[current])?;
                self.resume_after_load(previous)?;
            } else {
                self.playbin
                    .set_state(gst::State::Null)
                    .map_err(|e| anyhow::anyhow!("esvaziar reprodução: {e:?}"))?;
                self.set_state(PlaybackState::Stopped);
                *self.metadata.lock().unwrap() = TrackMetadata::default();
                self.metadata_ver.fetch_add(1, Ordering::Relaxed);
            }
        } else {
            let mut pending_guard = self.pending_gapless.lock().unwrap();
            if let Some(pending) = pending_guard.as_mut() {
                if pending.index == index {
                    *pending_guard = None;
                } else if pending.index > index {
                    pending.index -= 1;
                }
            }
        }
        Ok(())
    }

    fn resume_after_load(&self, previous: PlaybackState) -> Result<()> {
        match previous {
            PlaybackState::Playing => self.play(),
            PlaybackState::Paused => self.pause(),
            PlaybackState::Stopped => {
                self.set_state(PlaybackState::Stopped);
                Ok(())
            }
        }
    }

    pub fn shuffle(&self) -> bool {
        self.shuffle.load(Ordering::Relaxed)
    }

    pub fn set_shuffle(&self, shuffle: bool) {
        self.shuffle.store(shuffle, Ordering::Relaxed);
    }

    pub fn repeat_mode(&self) -> RepeatMode {
        RepeatMode::from_u8(self.repeat_mode.load(Ordering::Relaxed))
    }

    pub fn set_repeat_mode(&self, mode: RepeatMode) {
        self.repeat_mode.store(mode.to_u8(), Ordering::Relaxed);
    }

    /// Avança automaticamente respeitando RepeatMode e Shuffle.
    pub fn advance_after_eos(&self) -> Result<bool> {
        let mut queue = self.queue.lock().unwrap();
        *self.pending_gapless.lock().unwrap() = None;
        let next_idx = next_auto_index_from(&queue, self.shuffle(), self.repeat_mode());
        if let Some(index) = next_idx {
            self.set_track(&queue.tracks[index])?;
            queue.current = Some(index);
            self.play()?;
            Ok(true)
        } else {
            self.stop()?;
            Ok(false)
        }
    }

    /// Recuperação de erro não repete nem embaralha: avança na ordem da fila.
    pub fn skip_failed_track(&self) -> Result<bool> {
        let mut queue = self.queue.lock().unwrap();
        let next = queue
            .current
            .and_then(|i| i.checked_add(1))
            .filter(|i| *i < queue.tracks.len());
        if let Some(index) = next {
            self.set_track(&queue.tracks[index])?;
            queue.current = Some(index);
            self.play()?;
            Ok(true)
        } else {
            self.stop_after_error()?;
            Ok(false)
        }
    }

    pub fn next(&self) -> Result<()> {
        self.navigate(true)
    }

    pub fn prev(&self) -> Result<()> {
        self.navigate(false)
    }

    fn navigate(&self, forward: bool) -> Result<()> {
        let mut queue = self.queue.lock().unwrap();
        let len = queue.tracks.len();
        if len == 0 {
            return Ok(());
        }
        let index = if forward && self.shuffle() {
            pick_shuffle_index(queue.current, len).unwrap_or(0)
        } else {
            cyclic_index(queue.current.unwrap_or(0), len, forward).unwrap_or(0)
        };
        let previous = self.state();
        self.set_track(&queue.tracks[index])?;
        queue.current = Some(index);
        self.resume_after_load(previous)
    }
    pub fn search_and_load(&self, query: &str) -> Option<PathBuf> {
        let was_playing = self.playbin.current_state() == gst::State::Playing;
        let found = find_track(&self.queue.lock().unwrap().tracks, query)?;
        let _ = self.load(&found);
        if was_playing {
            let _ = self.play();
        }
        Some(found)
    }

    pub fn play(&self) -> Result<()> {
        if self.metadata.lock().unwrap().uri.is_empty() {
            return Ok(());
        }
        self.attempts.fetch_add(1, Ordering::Relaxed);
        self.playbin
            .set_state(gst::State::Playing)
            .map(|_| ())
            .map_err(|e| anyhow::anyhow!("play: {e:?}"))?;
        *self.state.lock().unwrap() = PlaybackState::Playing;

        self.apply_current_rate(self.position_us());
        Ok(())
    }
    pub fn pause(&self) -> Result<()> {
        if self.state() == PlaybackState::Stopped
            && self.playbin.current_state() != gst::State::Playing
        {
            return Ok(());
        }
        self.playbin
            .set_state(gst::State::Paused)
            .map(|_| ())
            .map_err(|e| anyhow::anyhow!("pause: {e:?}"))?;
        *self.state.lock().unwrap() = PlaybackState::Paused;
        Ok(())
    }
    pub fn stop(&self) -> Result<()> {
        *self.pending_gapless.lock().unwrap() = None;
        self.playbin
            .set_state(gst::State::Ready)
            .map(|_| ())
            .map_err(|e| anyhow::anyhow!("stop: {e:?}"))?;
        *self.state.lock().unwrap() = PlaybackState::Stopped;
        Ok(())
    }
    pub fn stop_after_error(&self) -> Result<()> {
        *self.pending_gapless.lock().unwrap() = None;
        self.playbin
            .set_state(gst::State::Null)
            .map_err(|e| anyhow::anyhow!("encerrar áudio após falha: {e:?}"))?;
        self.set_state(PlaybackState::Stopped);
        Ok(())
    }
    pub fn seek_us(&self, position: i64) -> Result<()> {
        let cur_rate = self.rate();
        let pos_ns = (position.max(0) as u64).saturating_mul(1000);
        let seek_event = gst::event::Seek::new(
            cur_rate,
            gst::SeekFlags::FLUSH | gst::SeekFlags::KEY_UNIT,
            gst::SeekType::Set,
            gst::ClockTime::from_nseconds(pos_ns),
            gst::SeekType::None,
            gst::ClockTime::NONE,
        );
        if self.playbin.send_event(seek_event) {
            Ok(())
        } else {
            self.playbin
                .seek_simple(
                    gst::SeekFlags::FLUSH | gst::SeekFlags::KEY_UNIT,
                    gst::ClockTime::from_nseconds(pos_ns),
                )
                .context("seek")
        }
    }
    pub fn set_volume(&self, volume: f64) {
        self.playbin.set_property("volume", volume.clamp(0.0, 1.0));
    }
    pub fn volume(&self) -> f64 {
        self.playbin.property::<f64>("volume")
    }
    pub fn rate(&self) -> f64 {
        f32::from_bits(self.rate.load(Ordering::Relaxed)) as f64
    }
    pub fn set_rate(&self, rate: f64) {
        let rate = rate.clamp(0.92, 1.08);
        self.rate.store((rate as f32).to_bits(), Ordering::Relaxed);
        if self.state() == PlaybackState::Playing {
            let pos = self.position_us() * 1000;
            let seek_event = gst::event::Seek::new(
                rate,
                gst::SeekFlags::FLUSH | gst::SeekFlags::ACCURATE,
                gst::SeekType::Set,
                gst::ClockTime::from_nseconds(pos.max(0) as u64),
                gst::SeekType::None,
                gst::ClockTime::NONE,
            );
            let _ = self.playbin.send_event(seek_event);
        }
    }
    pub fn position_us(&self) -> i64 {
        self.playbin
            .query_position::<gst::ClockTime>()
            .map(|v| v.nseconds() as i64 / 1000)
            .unwrap_or(0)
    }
    #[allow(dead_code)]
    pub fn duration_us(&self) -> i64 {
        self.playbin
            .query_duration::<gst::ClockTime>()
            .map(|v| v.nseconds() as i64 / 1000)
            .unwrap_or(0)
    }
    pub fn state(&self) -> PlaybackState {
        *self.state.lock().unwrap()
    }
    pub fn set_state(&self, state: PlaybackState) {
        if state != PlaybackState::Playing {
            self.reset_vu_levels();
        }
        *self.state.lock().unwrap() = state;
    }
    pub fn reset_vu_levels(&self) {
        self.level_l.store((-80.0f32).to_bits(), Ordering::Relaxed);
        self.level_r.store((-80.0f32).to_bits(), Ordering::Relaxed);
    }
    pub fn vu_levels(&self) -> (f64, f64) {
        let l = f32::from_bits(self.level_l.load(Ordering::Relaxed)) as f64;
        let r = f32::from_bits(self.level_r.load(Ordering::Relaxed)) as f64;
        (l, r)
    }
    pub fn metadata(&self) -> TrackMetadata {
        self.metadata.lock().unwrap().clone()
    }
    pub fn metadata_version(&self) -> u64 {
        self.metadata_ver.load(Ordering::Relaxed)
    }
    pub fn playback_attempt(&self) -> u64 {
        self.attempts.load(Ordering::Relaxed)
    }
    pub fn track_sequence(&self) -> u64 {
        self.track_seq.load(Ordering::Relaxed)
    }

    /// Identidade leve para os indicadores: não copia imagens ou demais tags.
    pub fn playback_identity(&self) -> (PlaybackState, String) {
        (self.state(), self.metadata.lock().unwrap().uri.clone())
    }
    pub fn element(&self) -> &gst::Element {
        &self.playbin
    }

    /// Processa TAG/STREAM e mensagens level do bus para a UI consultar metadados e dinâmica de áudio.
    #[allow(dead_code)]
    pub fn handle_message(&self, message: &gst::Message) {
        use gst::MessageView;
        match message.view() {
            MessageView::Element(element_msg) => {
                if let Some(s) = element_msg.structure() {
                    if s.name() == "level" {
                        if let Ok(rms_arr) = s.get::<glib::ValueArray>("rms") {
                            let slice = rms_arr.as_slice();
                            if !slice.is_empty() {
                                let db_l = slice[0].get::<f64>().unwrap_or(-80.0);
                                let db_r = if slice.len() > 1 {
                                    slice[1].get::<f64>().unwrap_or(db_l)
                                } else {
                                    db_l
                                };
                                self.level_l
                                    .store((db_l as f32).to_bits(), Ordering::Relaxed);
                                self.level_r
                                    .store((db_r as f32).to_bits(), Ordering::Relaxed);
                            }
                        }
                    }
                }
            }
            MessageView::Tag(tags) => {
                let mut m = self.metadata.lock().unwrap();
                if let Some(v) = tags.tags().get::<gst::tags::Title>() {
                    m.title = v.get().to_string();
                }
                if let Some(v) = tags.tags().get::<gst::tags::Artist>() {
                    m.artist = v.get().to_string();
                } else if m.artist.is_empty() {
                    if let Some(v) = tags.tags().get::<gst::tags::AlbumArtist>() {
                        m.artist = v.get().to_string();
                    } else if let Some(v) = tags.tags().get::<gst::tags::Performer>() {
                        m.artist = v.get().to_string();
                    }
                }
                if let Some(v) = tags.tags().get::<gst::tags::Album>() {
                    m.album = v.get().to_string();
                }
                if let Some(v) = tags.tags().get::<gst::tags::AudioCodec>() {
                    m.codec = v.get().to_string();
                }
                if m.cover_bytes.is_none() {
                    let sample = tags
                        .tags()
                        .get::<gst::tags::Image>()
                        .or_else(|| tags.tags().get::<gst::tags::PreviewImage>());
                    if let Some(s) = sample {
                        let sample = s.get();
                        if let Some(buffer) = sample.buffer() {
                            if let Ok(map) = buffer.map_readable() {
                                m.cover_bytes = Some(map.as_slice().to_vec());
                            }
                        }
                    }
                }
                drop(m);
                self.metadata_ver.fetch_add(1, Ordering::Relaxed);
            }
            MessageView::StreamStart(_) => {
                self.commit_pending_gapless(true);
                self.refresh_stream_info();
            }
            MessageView::AsyncDone(_) | MessageView::DurationChanged(_) => {
                let d = self
                    .playbin
                    .query_duration::<gst::ClockTime>()
                    .map(|v| v.nseconds() as i64 / 1000)
                    .unwrap_or(0);
                self.metadata.lock().unwrap().duration_us = d;
                self.metadata_ver.fetch_add(1, Ordering::Relaxed);
            }
            MessageView::Error(error) => {
                self.commit_pending_gapless(false);
                eprintln!(
                    "GStreamer audio error: {} ({:?})",
                    error.error(),
                    error.debug()
                );
            }
            _ => {}
        }
    }

    fn refresh_stream_info(&self) {
        let Some(pad) = self.playbin.static_pad("audio_0") else {
            return;
        };
        let Some(caps) = pad.current_caps() else {
            return;
        };
        let Some(s) = caps.structure(0) else { return };
        let mut m = self.metadata.lock().unwrap();
        let name = s.name();
        if m.codec.is_empty() || name != "audio/x-raw" {
            m.codec = name.to_string();
        }
        m.sample_rate = s.get::<i32>("rate").ok();
        m.bit_depth = s.get::<i32>("depth").ok();
        drop(m);
        self.metadata_ver.fetch_add(1, Ordering::Relaxed);
    }
}

/// Converte nível RMS em dB (-80..0 dB) para deflexão do ponteiro de VU [0.0..1.0]
/// Baseado na física linear de tensão/tensão média retificada do galvanômetro D'Arsonval.
pub fn db_to_vu_deflection(db: f64) -> f64 {
    if db <= -40.0 || db.is_nan() {
        return 0.0;
    }
    // 0 VU (+100% modulação) = -3 dBFS / 0 dB na escala do player
    // +3 dB VU (headroom máximo) = 1.4125
    let amp = 10.0_f64.powf((db + 3.0) / 20.0);
    (amp / 1.4125).clamp(0.0, 1.0)
}

fn make_audio_sink() -> Result<gst::Element> {
    let bin = gst::Bin::new();
    let convert = gst::ElementFactory::make("audioconvert").build()?;
    let resample = gst::ElementFactory::make("audioresample").build()?;
    let level = gst::ElementFactory::make("level")
        .property("interval", 25_000_000_u64)
        .property("post-messages", true)
        .build()?;
    let sink = gst::ElementFactory::make("autoaudiosink")
        .build()
        .context("plugin autoaudiosink ausente")?;
    bin.add_many([&convert, &resample, &level, &sink])?;
    gst::Element::link_many([&convert, &resample, &level, &sink])?;
    let pad = convert
        .static_pad("sink")
        .context("pad do audioconvert ausente")?;
    let ghost = gst::GhostPad::builder_with_target(&pad)?
        .name("sink")
        .build();
    bin.add_pad(&ghost)?;
    Ok(bin.upcast())
}

/// Busca rápida (sem invocar processos externos): procura imagem na pasta ou no cache de capas FLAC.
/// Retorna `Err(out_path)` apenas quando o arquivo é um `.flac` ainda não verificado no cache.
fn find_cached_or_local_cover(path: &Path) -> std::result::Result<Option<PathBuf>, PathBuf> {
    let Some(dir) = path.parent() else {
        return Ok(None);
    };

    // 1. Um único read_dir no diretório para achar capas locais sem disparar dezenas de syscalls de stat
    if let Ok(entries) = std::fs::read_dir(dir) {
        let mut priority_match = None;
        let mut fallback_match = None;
        let priority_names = ["cover", "folder", "front", "album", "artwork", "art"];

        for entry in entries.flatten() {
            let p = entry.path();
            if !p.is_file() {
                continue;
            }
            let Some(ext) = p.extension().and_then(|s| s.to_str()) else {
                continue;
            };
            let ext_lower = ext.to_ascii_lowercase();
            if !matches!(ext_lower.as_str(), "jpg" | "jpeg" | "png" | "webp") {
                continue;
            }
            let stem = p
                .file_stem()
                .and_then(|s| s.to_str())
                .unwrap_or("")
                .to_ascii_lowercase();
            if priority_names.contains(&stem.as_str()) {
                priority_match = Some(p);
                break;
            } else if fallback_match.is_none() {
                fallback_match = Some(p);
            }
        }

        if let Some(cover) = priority_match.or(fallback_match) {
            return Ok(Some(cover));
        }
    }

    // 2. Se for arquivo FLAC e não houver imagem solta na pasta, consulta cache positivo/negativo
    if let Some(ext) = path.extension().and_then(|s| s.to_str()) {
        if ext.eq_ignore_ascii_case("flac") {
            use std::collections::hash_map::DefaultHasher;
            use std::hash::{Hash, Hasher};

            let cache_dir = glib::user_cache_dir()
                .join("vinylplayer")
                .join("extracted-covers");
            let _ = std::fs::create_dir_all(&cache_dir);

            let mtime = std::fs::metadata(path)
                .and_then(|m| m.modified())
                .map(|t| {
                    t.duration_since(std::time::UNIX_EPOCH)
                        .unwrap_or_default()
                        .as_secs()
                })
                .unwrap_or(0);

            let mut hasher = DefaultHasher::new();
            path.hash(&mut hasher);
            mtime.hash(&mut hasher);
            let hash_val = hasher.finish();
            let out = cache_dir.join(format!("{hash_val:016x}.jpg"));
            let none_marker = cache_dir.join(format!("{hash_val:016x}.none"));

            if out.is_file() {
                return Ok(Some(out));
            }
            if none_marker.is_file() {
                return Ok(None);
            }

            // Migra cópia temporária de /tmp caso já tenha sido extraída anteriormente
            let mut legacy_hasher = DefaultHasher::new();
            path.hash(&mut legacy_hasher);
            let legacy_out =
                std::env::temp_dir().join(format!("vinyl_cover_{:x}.jpg", legacy_hasher.finish()));
            if legacy_out.is_file() {
                if std::fs::copy(&legacy_out, &out).is_ok() {
                    return Ok(Some(out));
                }
                return Ok(Some(legacy_out));
            }

            return Err(out);
        }
    }

    Ok(None)
}

fn extract_flac_cover(path: &Path, out: &Path) -> Option<PathBuf> {
    let tmp_out = out.with_extension(format!("tmp.{}", std::process::id()));
    let status = std::process::Command::new("metaflac")
        .arg(format!("--export-picture-to={}", tmp_out.display()))
        .arg(path)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status();

    if let Ok(status) = status {
        if status.success() && tmp_out.is_file() {
            let _ = std::fs::rename(&tmp_out, out);
            return Some(out.to_path_buf());
        }
        // Cache negativo: registra que este FLAC (neste mtime) não possui bloco PICTURE
        let none_marker = out.with_extension("none");
        let _ = std::fs::write(none_marker, b"");
    }
    let _ = std::fs::remove_file(&tmp_out);
    None
}

pub(crate) fn find_cover(path: &Path) -> Option<PathBuf> {
    match find_cached_or_local_cover(path) {
        Ok(cover) => cover,
        Err(out) => extract_flac_cover(path, &out),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_fakesink_engine(connect_gapless_cb: bool) -> AudioEngine {
        gst::init().unwrap();
        let playbin = gst::ElementFactory::make("playbin3").build().unwrap();
        let sink = gst::ElementFactory::make("fakesink").build().unwrap();
        playbin.set_property("audio-sink", &sink);
        let engine = AudioEngine {
            playbin,
            state: Default::default(),
            metadata: Default::default(),
            metadata_ver: Arc::new(AtomicU64::new(1)),
            attempts: Arc::new(AtomicU64::new(0)),
            track_seq: Arc::new(AtomicU64::new(0)),
            queue: Default::default(),
            shuffle: Arc::new(AtomicBool::new(false)),
            repeat_mode: Arc::new(AtomicU8::new(RepeatMode::Off.to_u8())),
            level_l: Arc::new(AtomicU32::new((-80.0f32).to_bits())),
            level_r: Arc::new(AtomicU32::new((-80.0f32).to_bits())),
            rate: Arc::new(AtomicU32::new((1.0f32).to_bits())),
            pending_gapless: Default::default(),
        };
        if connect_gapless_cb {
            engine.connect_gapless();
        }
        engine
    }

    fn make_wav_bytes(samples: usize) -> Vec<u8> {
        let data_bytes = (samples * 2) as u32;
        let riff_size = 36 + data_bytes;
        let mut wav = Vec::with_capacity(44 + data_bytes as usize);
        wav.extend_from_slice(b"RIFF");
        wav.extend_from_slice(&riff_size.to_le_bytes());
        wav.extend_from_slice(b"WAVEfmt ");
        wav.extend_from_slice(&16u32.to_le_bytes());
        wav.extend_from_slice(&1u16.to_le_bytes());
        wav.extend_from_slice(&1u16.to_le_bytes());
        wav.extend_from_slice(&8000u32.to_le_bytes());
        wav.extend_from_slice(&16000u32.to_le_bytes());
        wav.extend_from_slice(&2u16.to_le_bytes());
        wav.extend_from_slice(&16u16.to_le_bytes());
        wav.extend_from_slice(b"data");
        wav.extend_from_slice(&data_bytes.to_le_bytes());
        wav.resize(44 + data_bytes as usize, 0);
        wav
    }

    #[test]
    fn album_queue_preserves_order_and_stops_after_last_track() {
        let engine = test_fakesink_engine(false);
        assert!(engine.play_album(&[], 0).is_err());
        let dir = std::env::temp_dir().join(format!(
            "vinyl-album-test-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir(&dir).unwrap();
        let mut wav = Vec::new();
        wav.extend_from_slice(b"RIFF");
        wav.extend_from_slice(&196u32.to_le_bytes());
        wav.extend_from_slice(b"WAVEfmt ");
        wav.extend_from_slice(&16u32.to_le_bytes());
        wav.extend_from_slice(&1u16.to_le_bytes());
        wav.extend_from_slice(&1u16.to_le_bytes());
        wav.extend_from_slice(&8000u32.to_le_bytes());
        wav.extend_from_slice(&16000u32.to_le_bytes());
        wav.extend_from_slice(&2u16.to_le_bytes());
        wav.extend_from_slice(&16u16.to_le_bytes());
        wav.extend_from_slice(b"data");
        wav.extend_from_slice(&160u32.to_le_bytes());
        wav.extend_from_slice(&[0; 160]);
        let tracks: Vec<_> = ["03.wav", "01.wav", "02.wav"]
            .iter()
            .map(|name| {
                let path = dir.join(name);
                std::fs::write(&path, &wav).unwrap();
                path
            })
            .collect();
        engine.play_album(&tracks, 1).unwrap();
        assert_eq!(engine.queue_snapshot().tracks, tracks);
        assert_eq!(engine.queue_snapshot().current, Some(1));
        assert!(engine.advance_after_eos().unwrap());
        assert_eq!(engine.queue_snapshot().current, Some(2));
        assert!(engine.metadata().uri.ends_with("02.wav"));
        assert!(!engine.advance_after_eos().unwrap());
        assert_eq!(engine.state(), PlaybackState::Stopped);
        // Edições de faixas vizinhas preservam a faixa e o estado atuais.
        engine.play_files(&tracks, 1).unwrap();
        let uri = engine.metadata().uri;
        let revision = engine.queue_snapshot().revision;
        engine.move_queue_item(revision, 1, 0).unwrap();
        assert_eq!(engine.metadata().uri, uri);
        assert_eq!(engine.queue_snapshot().current, Some(0));
        assert_eq!(engine.state(), PlaybackState::Playing);
        assert!(engine.remove_queue_item(revision, 0).is_err());
        let revision = engine.queue_snapshot().revision;
        engine.remove_queue_item(revision, 2).unwrap();
        assert_eq!(engine.metadata().uri, uri);
        assert_eq!(engine.state(), PlaybackState::Playing);
        // Remover a atual avança; em pausa, mantém a próxima em pausa.
        engine.pause().unwrap();
        let revision = engine.queue_snapshot().revision;
        engine.remove_queue_item(revision, 0).unwrap();
        assert_eq!(engine.queue_snapshot().tracks, vec![tracks[0].clone()]);
        assert_eq!(engine.state(), PlaybackState::Paused);
        assert!(engine.metadata().uri.ends_with("03.wav"));
        let revision = engine.queue_snapshot().revision;
        engine.remove_queue_item(revision, 0).unwrap();
        assert!(engine.queue_snapshot().tracks.is_empty());
        assert_eq!(engine.state(), PlaybackState::Stopped);
        assert!(engine.metadata().uri.is_empty());
        engine.play().unwrap();
        assert_eq!(engine.state(), PlaybackState::Stopped);
        let revision = engine.queue_snapshot().revision;
        engine.seed_library_if_unchanged(revision, &tracks).unwrap();
        assert_eq!(engine.queue_snapshot().tracks, tracks);
        assert_eq!(engine.state(), PlaybackState::Stopped);
        let revision = engine.queue_snapshot().revision;
        engine.play_files(&[tracks[1].clone()], 0).unwrap();
        engine.seed_library_if_unchanged(revision, &tracks).unwrap();
        assert_eq!(engine.queue_snapshot().tracks, vec![tracks[1].clone()]);
        engine.pause().unwrap();
        let revision = engine.queue_snapshot().revision;
        let selected_uri = engine.metadata().uri;
        engine.seed_library_if_unchanged(revision, &tracks).unwrap();
        assert_eq!(engine.queue_snapshot().current, Some(1));
        assert_eq!(engine.metadata().uri, selected_uri);
        assert_eq!(engine.state(), PlaybackState::Paused);
        for repeat in [RepeatMode::Off, RepeatMode::All, RepeatMode::Track] {
            for shuffle in [false, true] {
                engine.set_repeat_mode(repeat);
                engine.set_shuffle(shuffle);
                engine.play_files(&tracks, 0).unwrap();
                engine
                    .playbin
                    .state(Some(gst::ClockTime::from_seconds(2)))
                    .0
                    .unwrap();
                assert!(engine.skip_failed_track().unwrap());
                engine
                    .playbin
                    .state(Some(gst::ClockTime::from_seconds(2)))
                    .0
                    .unwrap();
                assert_eq!(engine.queue_snapshot().current, Some(1));
                assert_eq!(engine.queue_snapshot().tracks, tracks);
                assert!(engine.skip_failed_track().unwrap());
                engine
                    .playbin
                    .state(Some(gst::ClockTime::from_seconds(2)))
                    .0
                    .unwrap();
                assert!(!engine.skip_failed_track().unwrap());
                assert_eq!(engine.state(), PlaybackState::Stopped);
                assert_eq!(engine.queue_snapshot().current, Some(2));
                assert_eq!(engine.repeat_mode(), repeat);
                assert_eq!(engine.shuffle(), shuffle);
            }
        }
        engine.playbin.set_state(gst::State::Null).unwrap();
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn next_wraps_from_last_and_prev_wraps_from_first() {
        assert_eq!(cyclic_index(2, 3, true), Some(0));
        assert_eq!(cyclic_index(0, 3, false), Some(2));
        assert_eq!(cyclic_index(0, 0, true), None);
    }

    #[test]
    fn navigation_preserves_middle_positions() {
        assert_eq!(cyclic_index(1, 3, true), Some(2));
        assert_eq!(cyclic_index(1, 3, false), Some(0));
    }

    #[test]
    fn search_is_case_insensitive_and_matches_stem() {
        let queue = vec![
            PathBuf::from("/music/01 - Aurora.FLAC"),
            PathBuf::from("/music/Blue.mp3"),
        ];
        assert_eq!(find_track(&queue, " AURORA "), Some(queue[0].clone()));
        assert_eq!(find_track(&queue, "blue"), Some(queue[1].clone()));
        assert_eq!(find_track(&queue, "missing"), None);
        assert_eq!(find_track(&queue, "   "), None);
    }

    #[test]
    fn quality_classification_covers_hi_res_hifi_and_standard() {
        assert_eq!(
            classify_quality(Some(24), Some(96_000), true),
            AudioQuality::HiRes
        );
        assert_eq!(
            classify_quality(Some(32), Some(192_000), true),
            AudioQuality::HiRes
        );
        assert_eq!(
            classify_quality(Some(16), Some(44_100), true),
            AudioQuality::HiFi
        );
        assert_eq!(
            classify_quality(Some(16), Some(44_100), false),
            AudioQuality::Standard
        );
    }

    #[test]
    fn repeat_mode_cycles_and_converts_mpris_and_strings() {
        let mut mode = RepeatMode::Off;
        assert_eq!(mode.to_mpris_str(), "None");
        assert_eq!(mode.to_str(), "off");
        assert_eq!(mode.to_u8(), 0);

        mode = mode.next_cycle();
        assert_eq!(mode, RepeatMode::All);
        assert_eq!(mode.to_mpris_str(), "Playlist");
        assert_eq!(mode.to_str(), "all");
        assert_eq!(mode.to_u8(), 1);

        mode = mode.next_cycle();
        assert_eq!(mode, RepeatMode::Track);
        assert_eq!(mode.to_mpris_str(), "Track");
        assert_eq!(mode.to_str(), "track");
        assert_eq!(mode.to_u8(), 2);

        mode = mode.next_cycle();
        assert_eq!(mode, RepeatMode::Off);

        assert_eq!(RepeatMode::from_mpris_str("Playlist"), RepeatMode::All);
        assert_eq!(RepeatMode::from_mpris_str("Track"), RepeatMode::Track);
        assert_eq!(RepeatMode::from_mpris_str("None"), RepeatMode::Off);
        assert_eq!(RepeatMode::from_mpris_str("other"), RepeatMode::Off);

        assert_eq!(RepeatMode::from_str("all"), RepeatMode::All);
        assert_eq!(RepeatMode::from_str("track"), RepeatMode::Track);
        assert_eq!(RepeatMode::from_str("off"), RepeatMode::Off);
        assert_eq!(RepeatMode::from_str("invalid"), RepeatMode::Off);

        assert_eq!(RepeatMode::from_u8(0), RepeatMode::Off);
        assert_eq!(RepeatMode::from_u8(1), RepeatMode::All);
        assert_eq!(RepeatMode::from_u8(2), RepeatMode::Track);
        assert_eq!(RepeatMode::from_u8(99), RepeatMode::Off);
    }

    #[test]
    fn pick_shuffle_index_selects_valid_and_distinct_indices() {
        assert_eq!(pick_shuffle_index(None, 0), None);
        assert_eq!(pick_shuffle_index(Some(0), 1), Some(0));

        let len = 5;
        for current in 0..len {
            for _ in 0..20 {
                let picked = pick_shuffle_index(Some(current), len);
                assert!(picked.is_some());
                let idx = picked.unwrap();
                assert!(idx < len);
                assert_ne!(
                    idx, current,
                    "shuffle não deve repetir a faixa atual em fila > 1"
                );
            }
        }
    }

    #[test]
    fn volume_getter_and_setter_clamp_properly() {
        let engine = AudioEngine::with_queue(Vec::new(), None).unwrap();
        engine.set_volume(0.65);
        assert!((engine.volume() - 0.65).abs() < 1e-4);

        engine.set_volume(1.5);
        assert!((engine.volume() - 1.0).abs() < 1e-4);

        engine.set_volume(-0.5);
        assert!((engine.volume() - 0.0).abs() < 1e-4);
    }

    #[test]
    fn vu_reads_real_level_messages_for_mono_and_stereo() {
        gst::init().unwrap();
        let engine = AudioEngine::with_queue(Vec::new(), None).unwrap();
        for channels in [1, 2] {
            engine.reset_vu_levels();
            let pipeline = gst::parse::launch(&format!("audiotestsrc num-buffers=8 volume=0.5 ! audio/x-raw,channels={channels} ! level interval=25000000 ! fakesink"))
                .unwrap().downcast::<gst::Pipeline>().unwrap();
            let bus = pipeline.bus().unwrap();
            pipeline.set_state(gst::State::Playing).unwrap();
            let deadline = std::time::Instant::now() + std::time::Duration::from_secs(3);
            let mut received = false;
            while std::time::Instant::now() < deadline {
                if let Some(message) = bus.timed_pop(gst::ClockTime::from_mseconds(100)) {
                    if let gst::MessageView::Element(element) = message.view() {
                        if element.structure().is_some_and(|s| s.name() == "level") {
                            engine.handle_message(&message);
                            received = true;
                            break;
                        }
                    }
                }
            }
            pipeline.set_state(gst::State::Null).unwrap();
            assert!(received);
            let (left, right) = engine.vu_levels();
            assert!((left + 9.03).abs() < 0.5, "RMS esquerdo: {left}");
            assert!((right + 9.03).abs() < 0.5, "RMS direito: {right}");
        }
    }

    #[test]
    fn db_to_vu_deflection_maps_accurately_to_physical_scale() {
        // Silêncio profundo deve defletir 0
        assert_eq!(db_to_vu_deflection(-80.0), 0.0);
        assert_eq!(db_to_vu_deflection(-45.0), 0.0);
        assert_eq!(db_to_vu_deflection(f64::NAN), 0.0);

        // 0 dBFS no digital deve alcançar 100% de deflexão (+3 dB VU)
        let def_max = db_to_vu_deflection(0.0);
        assert!((def_max - 1.0).abs() < 1e-3);

        // -3 dBFS no digital corresponde a 0 VU (~70.8% da escala)
        let def_0vu = db_to_vu_deflection(-3.0);
        assert!((def_0vu - 0.7079).abs() < 1e-3);

        // -6 dBFS no digital corresponde a -3 dB VU (~50% da escala)
        let def_half = db_to_vu_deflection(-6.0);
        assert!((def_half - 0.50).abs() < 1e-2);

        // Monotonia: níveis maiores produzem maior deflexão
        assert!(db_to_vu_deflection(-1.0) > db_to_vu_deflection(-5.0));
        assert!(db_to_vu_deflection(-5.0) > db_to_vu_deflection(-15.0));
        assert!(db_to_vu_deflection(-15.0) > db_to_vu_deflection(-30.0));
    }

    #[test]
    fn vu_levels_starts_at_silence_and_resets_properly() {
        let engine = AudioEngine::with_queue(Vec::new(), None).unwrap();
        let (l, r) = engine.vu_levels();
        assert_eq!(l, -80.0);
        assert_eq!(r, -80.0);

        engine
            .level_l
            .store((-10.0f32).to_bits(), Ordering::Relaxed);
        engine
            .level_r
            .store((-12.0f32).to_bits(), Ordering::Relaxed);
        let (l2, r2) = engine.vu_levels();
        assert!((l2 - (-10.0)).abs() < 1e-3);
        assert!((r2 - (-12.0)).abs() < 1e-3);

        engine.reset_vu_levels();
        let (l3, r3) = engine.vu_levels();
        assert_eq!(l3, -80.0);
        assert_eq!(r3, -80.0);
    }

    #[test]
    fn pause_while_stopped_keeps_stopped_state() {
        let engine = AudioEngine::with_queue(Vec::new(), None).unwrap();
        assert_eq!(engine.state(), PlaybackState::Stopped);
        engine.pause().unwrap();
        assert_eq!(engine.state(), PlaybackState::Stopped);
    }

    #[test]
    fn gapless_about_to_finish_transitions_without_eos() {
        let engine = test_fakesink_engine(true);
        let dir = std::env::temp_dir().join(format!(
            "vinyl-gapless-test-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir(&dir).unwrap();
        let wav = make_wav_bytes(1600); // 200 ms at 8 kHz
        let t1 = dir.join("01.wav");
        let t2 = dir.join("02.wav");
        std::fs::write(&t1, &wav).unwrap();
        std::fs::write(&t2, &wav).unwrap();

        engine.play_files(&[t1.clone(), t2.clone()], 0).unwrap();
        let start_seq = engine.track_sequence();
        let mut switched_gaplessly = false;
        let mut saw_eos_before_switch = false;
        let mut saw_final_eos = false;
        let bus = engine.element().bus().unwrap();
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
        while std::time::Instant::now() < deadline {
            if let Some(msg) = bus.timed_pop(gst::ClockTime::from_mseconds(50)) {
                match msg.view() {
                    gst::MessageView::Eos(_) => {
                        if !switched_gaplessly {
                            saw_eos_before_switch = true;
                        } else {
                            saw_final_eos = true;
                            break;
                        }
                    }
                    gst::MessageView::Error(err) => {
                        panic!("unexpected GStreamer error: {:?}", err.error());
                    }
                    _ => {
                        engine.handle_message(&msg);
                        if !switched_gaplessly && engine.track_sequence() > start_seq {
                            switched_gaplessly = true;
                        }
                    }
                }
            }
        }
        assert!(
            !saw_eos_before_switch,
            "Eos must not fire between gapless tracks"
        );
        assert!(
            switched_gaplessly,
            "about-to-finish should transition from track 1 to track 2"
        );
        assert!(saw_final_eos, "final track should finish with Eos");
        assert_eq!(engine.queue_snapshot().current, Some(1));
        assert!(engine.metadata().uri.ends_with("02.wav"));
        assert!(!engine.advance_after_eos().unwrap());
        assert_eq!(engine.state(), PlaybackState::Stopped);
        let _ = engine.playbin.set_state(gst::State::Null);
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn seek_us_preserves_playback_rate() {
        let engine = test_fakesink_engine(false);
        let dir = std::env::temp_dir().join(format!(
            "vinyl-seek-test-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir(&dir).unwrap();
        let wav = make_wav_bytes(8000); // 1000 ms at 8 kHz
        let t1 = dir.join("seek_test.wav");
        std::fs::write(&t1, &wav).unwrap();

        engine.play_files(&[t1], 0).unwrap();
        let bus = engine.element().bus().unwrap();
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(3);
        while std::time::Instant::now() < deadline {
            if let Some(msg) = bus.timed_pop(gst::ClockTime::from_mseconds(50)) {
                engine.handle_message(&msg);
                if let gst::MessageView::AsyncDone(_) = msg.view() {
                    break;
                }
            }
        }

        engine.set_rate(1.05);
        assert!((engine.rate() - 1.05).abs() < 0.001);

        // Seek deve suceder e preservar o rate customizado de 1.05x
        let seek_res = engine.seek_us(100_000); // 100 ms
        assert!(
            seek_res.is_ok(),
            "seek_us deve ser aceito pelo pipeline: {seek_res:?}"
        );
        assert!((engine.rate() - 1.05).abs() < 0.001);

        let _ = engine.playbin.set_state(gst::State::Null);
        std::fs::remove_dir_all(&dir).ok();
    }
}
