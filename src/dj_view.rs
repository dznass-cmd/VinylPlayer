//! DJ Booth UI, dual-deck console rendering, and reactive Auto-DJ controller.

use crate::audio::{crossfade_gains, AudioEngine, ManualReleaseOutcome, PlaybackState};
use crate::dj::{
    self, AcousticAnalysis, DjConfig, DjCrossfadeCurve, DjEngine, DjMixMode, DjPick, DjPoolSource,
    TrackProfile,
};
use crate::i18n::{self, Language};
use crate::library;
use crate::library_settings::Catalog;
use crate::state::StateManager;
use crate::theme::Theme;
use crate::thumb_cache;
use crate::vu_meter::draw_rounded_rect;
use gtk::cairo;
use gtk::gdk_pixbuf;
use gtk::glib;
use gtk::prelude::*;
use gtk::{DrawingArea, Orientation};
use std::cell::{Cell, RefCell};
use std::f64::consts::TAU;
use std::path::PathBuf;
use std::rc::Rc;
use std::sync::mpsc;
use std::sync::Arc;
use std::time::{Duration, Instant};

type DjListener = (glib::WeakRef<gtk::Widget>, Rc<dyn Fn(&DjController)>);

pub struct DjController {
    pub engine: DjEngine,
    pub audio: Arc<AudioEngine>,
    pub state_mgr: StateManager,
    pub catalog: Rc<Catalog>,
    pub revision: u64,
    last_track_seq: u64,
    last_catalog_len: usize,
    crossfaded_seq: u64,
    silence_since: Option<Instant>,
    last_cache_flush: Instant,
    last_catalog_poll: Instant,
    analyzer_running: bool,
    bg_tx: mpsc::Sender<Vec<(PathBuf, AcousticAnalysis)>>,
    bg_rx: mpsc::Receiver<Vec<(PathBuf, AcousticAnalysis)>>,
    upcoming_cover_key: String,
    upcoming_cover_pb: Option<gdk_pixbuf::Pixbuf>,
    upcoming_preview: Vec<DjPick>,
    notice: Option<(String, String, bool)>,
    fader_geom: Cell<(f64, f64, f64)>,
    fader_dragging: Cell<bool>,
    fader_hover: Cell<bool>,
    listeners: Vec<DjListener>,
}

impl DjController {
    pub fn new(
        audio: Arc<AudioEngine>,
        state_mgr: StateManager,
        catalog: Rc<Catalog>,
    ) -> Rc<RefCell<Self>> {
        let initial_dj = state_mgr.snapshot().dj;
        audio
            .set_crossfade_managed(initial_dj.mode.is_active() && initial_dj.crossfade_secs >= 0.5);
        let engine = DjEngine::new(initial_dj, Some(dj::default_acoustic_cache_path()));
        let (bg_tx, bg_rx) = mpsc::channel();
        let initial_cat_len = catalog.report.borrow().tracks.len();
        let ctrl = Rc::new(RefCell::new(Self {
            engine,
            audio,
            state_mgr,
            catalog,
            revision: 1,
            last_track_seq: 0,
            last_catalog_len: initial_cat_len,
            crossfaded_seq: 0,
            silence_since: None,
            last_cache_flush: Instant::now(),
            last_catalog_poll: Instant::now(),
            analyzer_running: false,
            bg_tx,
            bg_rx,
            upcoming_cover_key: String::new(),
            upcoming_cover_pb: None,
            upcoming_preview: Vec::new(),
            notice: None,
            fader_geom: Cell::new((0.0, 0.0, 0.0)),
            fader_dragging: Cell::new(false),
            fader_hover: Cell::new(false),
            listeners: Vec::new(),
        }));
        {
            let mut guard = ctrl.borrow_mut();
            guard.refresh_upcoming(false);
        }
        Self::schedule_cover_decode(&ctrl);
        ctrl
    }

    pub fn connect(
        ctrl: &Rc<RefCell<Self>>,
        anchor: &impl IsA<gtk::Widget>,
        cb: impl Fn(&DjController) + 'static,
    ) {
        {
            let borrowed = ctrl.borrow();
            cb(&borrowed);
        }
        ctrl.borrow_mut()
            .listeners
            .push((anchor.as_ref().downgrade(), Rc::new(cb)));
    }

    fn emit_changed(&mut self) {
        self.revision = self.revision.wrapping_add(1);
        let callbacks: Vec<_> = {
            self.listeners.retain(|(weak, _)| weak.upgrade().is_some());
            self.listeners.iter().map(|(_, cb)| cb.clone()).collect()
        };
        for cb in callbacks {
            cb(self);
        }
    }

    fn sync_audio_and_state(&mut self) {
        self.audio.set_crossfade_managed(
            self.engine.config.mode.is_active() && self.engine.config.crossfade_secs >= 0.5,
        );
        self.state_mgr.update_dj(self.engine.config.clone());
        self.state_mgr.flush_if_dirty();
    }

    pub fn set_mode(ctrl: &Rc<RefCell<Self>>, mode: DjMixMode) {
        {
            let mut c = ctrl.borrow_mut();
            if c.engine.config.mode == mode {
                return;
            }
            c.engine.config.mode = mode;
            if mode == DjMixMode::SmartRadio {
                c.engine.seed_path = c.audio.current_track_path();
            }
            c.sync_audio_and_state();
            c.refresh_upcoming(true);
            c.emit_changed();
        }
        Self::schedule_cover_decode(ctrl);
        Self::maybe_spawn_analyzer(ctrl);
    }

    pub fn set_pool(ctrl: &Rc<RefCell<Self>>, pool: DjPoolSource) {
        {
            let mut c = ctrl.borrow_mut();
            if c.engine.config.pool == pool {
                return;
            }
            c.engine.config.pool = pool;
            c.sync_audio_and_state();
            c.refresh_upcoming(true);
            c.emit_changed();
        }
        Self::schedule_cover_decode(ctrl);
    }

    pub fn set_crossfade_secs(ctrl: &Rc<RefCell<Self>>, secs: f64) {
        let clamped = secs.clamp(0.0, 15.0);
        let mut c = ctrl.borrow_mut();
        if (c.engine.config.crossfade_secs - clamped).abs() < 0.05 {
            return;
        }
        c.engine.config.crossfade_secs = clamped;
        c.sync_audio_and_state();
        c.emit_changed();
    }

    pub fn set_curve(ctrl: &Rc<RefCell<Self>>, curve: DjCrossfadeCurve) {
        let mut c = ctrl.borrow_mut();
        if c.engine.config.curve == curve {
            return;
        }
        c.engine.config.curve = curve;
        c.audio.update_crossfade_curve(curve);
        c.sync_audio_and_state();
        c.emit_changed();
    }

    pub fn set_smart_cue(ctrl: &Rc<RefCell<Self>>, enabled: bool) {
        let mut c = ctrl.borrow_mut();
        if c.engine.config.smart_cue == enabled {
            return;
        }
        c.engine.config.smart_cue = enabled;
        c.sync_audio_and_state();
        c.emit_changed();
    }

    pub fn set_trim_intro(ctrl: &Rc<RefCell<Self>>, enabled: bool) {
        let mut c = ctrl.borrow_mut();
        if c.engine.config.trim_intro_silence == enabled {
            return;
        }
        c.engine.config.trim_intro_silence = enabled;
        c.sync_audio_and_state();
        c.emit_changed();
    }

    pub fn clear_notice(ctrl: &Rc<RefCell<Self>>) {
        let mut c = ctrl.borrow_mut();
        if c.notice.take().is_some() {
            c.emit_changed();
        }
    }

    fn set_notice_msg(&mut self, en: &str, pt: &str, is_error: bool) {
        self.notice = Some((en.to_string(), pt.to_string(), is_error));
    }

    pub fn set_tempo_match(ctrl: &Rc<RefCell<Self>>, enabled: bool) {
        let mut c = ctrl.borrow_mut();
        if c.engine.config.tempo_match == enabled {
            return;
        }
        c.engine.config.tempo_match = enabled;
        c.sync_audio_and_state();
        c.refresh_upcoming(false);
        c.emit_changed();
    }

    pub fn set_current_as_seed(ctrl: &Rc<RefCell<Self>>) {
        {
            let mut c = ctrl.borrow_mut();
            c.engine.seed_path = c.audio.current_track_path();
            c.refresh_upcoming(true);
            c.emit_changed();
        }
        Self::schedule_cover_decode(ctrl);
    }

    pub fn reroll_upcoming(ctrl: &Rc<RefCell<Self>>) {
        {
            let mut c = ctrl.borrow_mut();
            if !c.engine.config.mode.is_active() {
                c.engine.config.mode = DjMixMode::SmartFlow;
                c.sync_audio_and_state();
            }
            c.refresh_upcoming(true);
            c.emit_changed();
        }
        Self::schedule_cover_decode(ctrl);
    }

    fn refresh_upcoming(&mut self, force_reroll: bool) {
        let current = self.audio.current_track_path();
        let snap = self.audio.queue_snapshot();
        let lib_tracks = self.catalog.report.borrow().tracks.clone();
        self.engine.ensure_upcoming(
            current.as_deref(),
            &snap.tracks,
            &lib_tracks,
            &[],
            force_reroll,
        );

        // Rebuild compact "Up Next" preview (4 items following Deck B / current).
        let mut preview = Vec::new();
        let mut exclude = Vec::new();
        if let Some(ref cur) = current {
            exclude.push(cur.clone());
        }
        if let Some(ref up) = self.engine.upcoming {
            exclude.push(up.path.clone());
        }
        if self.engine.config.mode.is_active() {
            let anchor = self
                .engine
                .upcoming
                .as_ref()
                .map(|u| u.path.clone())
                .or(current);
            preview = self.engine.generate_pick_set(
                anchor.as_deref(),
                &snap.tracks,
                &lib_tracks,
                &exclude,
                4,
            );
        } else if !snap.tracks.is_empty() {
            let start = snap.current.map(|i| i + 1).unwrap_or(0);
            for (offset, p) in snap.tracks.iter().skip(start).enumerate() {
                if preview.len() >= 4 {
                    break;
                }
                if exclude.contains(p) {
                    continue;
                }
                preview.push(DjPick {
                    path: p.clone(),
                    queue_index: Some(start + offset),
                    score: 100.0,
                    match_pct: None,
                    reason_en: "In Queue".into(),
                    reason_pt: "Na Fila".into(),
                    target_rate: None,
                    journey_phase: None,
                });
            }
        }
        self.upcoming_preview = preview;
        if !lib_tracks.is_empty() || !snap.tracks.is_empty() {
            if let Some((_, _, true)) = self.notice {
                // Keep explicit action error only if explicitly triggered; clear stale empty-library warning
            } else {
                self.notice = None;
            }
        }
    }

    fn schedule_cover_decode(ctrl: &Rc<RefCell<Self>>) {
        let next_path = {
            let c = ctrl.borrow();
            c.engine.upcoming.as_ref().map(|u| u.path.clone())
        };
        let Some(track_path) = next_path else {
            let mut c = ctrl.borrow_mut();
            c.upcoming_cover_key.clear();
            c.upcoming_cover_pb = None;
            return;
        };

        let key = track_path.to_string_lossy().to_string();
        {
            let c = ctrl.borrow();
            if c.upcoming_cover_key == key {
                return;
            }
        }
        {
            let mut c = ctrl.borrow_mut();
            c.upcoming_cover_key = key.clone();
            c.upcoming_cover_pb = None;
        }

        let weak_ctrl = Rc::downgrade(ctrl);
        glib::MainContext::default().spawn_local(async move {
            let decoded = gtk::gio::spawn_blocking(move || {
                let cover = crate::audio::find_cover(&track_path)?;
                let pb = thumb_cache::load_cached_thumbnail(&cover, 256)?;
                Some((
                    pb.width(),
                    pb.height(),
                    pb.rowstride(),
                    pb.has_alpha(),
                    pb.read_pixel_bytes(),
                ))
            })
            .await
            .ok()
            .flatten();

            let Some(rc) = weak_ctrl.upgrade() else {
                return;
            };
            if let Some((w, h, stride, alpha, bytes)) = decoded {
                let mut c = rc.borrow_mut();
                if c.upcoming_cover_key == key {
                    let pb = gdk_pixbuf::Pixbuf::from_bytes(
                        &bytes,
                        gdk_pixbuf::Colorspace::Rgb,
                        alpha,
                        8,
                        w,
                        h,
                        stride,
                    );
                    c.upcoming_cover_pb = Some(pb);
                    c.emit_changed();
                }
            }
        });
    }

    fn maybe_spawn_analyzer(ctrl: &Rc<RefCell<Self>>) {
        let (batch, tx) = {
            let mut c = ctrl.borrow_mut();
            if !c.engine.config.mode.is_active() || c.analyzer_running {
                return;
            }
            let cur = c.audio.current_track_path();
            let snap = c.audio.queue_snapshot();
            let lib_tracks = c.catalog.report.borrow().tracks.clone();
            let mut pool = snap.tracks;
            if c.engine.config.pool == DjPoolSource::LibraryAutoFill {
                pool.extend(lib_tracks);
            }
            let batch = c.engine.next_paths_to_analyze(cur.as_deref(), &pool, 4);
            if batch.is_empty() {
                return;
            }
            c.analyzer_running = true;
            (batch, c.bg_tx.clone())
        };

        let _ = std::thread::Builder::new()
            .name("vinyl-dj-analyze".into())
            .spawn(move || {
                let mut results = Vec::with_capacity(batch.len());
                for path in batch {
                    // Warm tag cache as well so genre/bpm/year tags are ready for scoring.
                    let _ = library::read_order(&path);
                    if let Some(analysis) = dj::analyze_track_acoustics_gst(&path) {
                        results.push((path, analysis));
                    }
                }
                let _ = tx.send(results);
            });
    }

    pub fn build_queue_set(ctrl: &Rc<RefCell<Self>>, count: usize) -> usize {
        let added_count = {
            let mut c = ctrl.borrow_mut();
            if !c.engine.config.mode.is_active() {
                c.engine.config.mode = DjMixMode::SmartFlow;
                c.sync_audio_and_state();
            }
            let current = c.audio.current_track_path();
            let lib_tracks = c.catalog.report.borrow().tracks.clone();
            let queue_tracks = c.audio.queue_snapshot().tracks;
            let pool = if c.engine.config.pool == DjPoolSource::LibraryAutoFill
                && !lib_tracks.is_empty()
            {
                lib_tracks
            } else if !queue_tracks.is_empty() {
                queue_tracks
            } else {
                lib_tracks
            };
            let set = c.engine.generate_set(current.as_deref(), &pool, count);
            let n = set.len();
            if n > 0 {
                c.notice = None;
                let _ = c.audio.replace_upcoming_tracks(&set);
                c.refresh_upcoming(false);
            } else {
                c.set_notice_msg(
                    "No tracks available to build a set. Check your music folders or add tracks first.",
                    "Nenhuma música disponível para montar o set. Confira suas pastas de música ou o HD.",
                    true,
                );
            }
            c.emit_changed();
            n
        };
        Self::schedule_cover_decode(ctrl);
        added_count
    }

    /// Resolves queue index for `path`, appending to queue cleanly if needed.
    fn resolve_queue_index(&self, path: &PathBuf) -> Result<usize, String> {
        let snap = self.audio.queue_snapshot();
        if let Some(idx) = snap.tracks.iter().position(|p| p == path) {
            Ok(idx)
        } else {
            self.audio.enqueue_dj_track(path).map_err(|e| e.to_string())
        }
    }

    fn resolve_default_pick(&mut self) -> Option<DjPick> {
        if self.engine.config.mode.is_active() {
            self.refresh_upcoming(false);
            self.engine.upcoming.clone()
        } else {
            let snap = self.audio.queue_snapshot();
            let next_idx = snap
                .current
                .map(|i| i + 1)
                .filter(|i| *i < snap.tracks.len())
                .or_else(|| (!snap.tracks.is_empty()).then_some(0));
            next_idx.and_then(|idx| {
                snap.tracks.get(idx).map(|p| DjPick {
                    path: p.clone(),
                    queue_index: Some(idx),
                    score: 100.0,
                    match_pct: None,
                    reason_en: "Queue Mix".into(),
                    reason_pt: "Mix da Fila".into(),
                    target_rate: None,
                    journey_phase: None,
                })
            })
        }
    }

    fn execute_pick_transition(
        &mut self,
        pick: DjPick,
        xfade_secs_override: Option<f64>,
    ) -> Result<(), String> {
        let target_index = self.resolve_queue_index(&pick.path)?;
        let current = self.audio.current_track_path();
        let cur_seq = self.audio.track_sequence();
        self.crossfaded_seq = cur_seq;
        if let Some(ref cur) = current {
            self.engine.record_played(cur);
        }

        let xfade_secs = xfade_secs_override.unwrap_or(self.engine.config.crossfade_secs);
        let curve = self.engine.config.curve;
        let cue_in_us = self.engine.intro_cue_us_for(&pick.path).unwrap_or(0);

        if self.audio.state() == PlaybackState::Playing && xfade_secs >= 0.5 {
            if !self.audio.resume_crossfade(xfade_secs, curve) {
                self.audio
                    .start_crossfade_with_options(
                        target_index,
                        &pick.path,
                        xfade_secs,
                        pick.target_rate,
                        curve,
                        cue_in_us,
                        false,
                        0.0,
                        pick.reason_en,
                        pick.reason_pt,
                    )
                    .map_err(|e| e.to_string())?;
            }
        } else {
            let (rev, _, _) = self.audio.queue_status();
            self.audio
                .play_queue_item_with_cue(rev, target_index, cue_in_us)
                .map_err(|e| e.to_string())?;
        }
        self.silence_since = None;
        self.notice = None;
        self.emit_changed();
        Ok(())
    }

    /// Triggers an immediate crossfade (or starts playback) to the upcoming DJ track.
    pub fn trigger_mix_now(ctrl: &Rc<RefCell<Self>>) -> Result<(), String> {
        let result = {
            let mut c = ctrl.borrow_mut();
            if let Some(snap) = c.audio.crossfade_snapshot() {
                if snap.manual {
                    let secs = c.engine.config.crossfade_secs.max(2.5);
                    let curve = c.engine.config.curve;
                    c.audio.resume_crossfade(secs, curve);
                    c.emit_changed();
                    return Ok(());
                } else {
                    c.audio.finish_crossfade();
                    c.refresh_upcoming(true);
                    c.emit_changed();
                    return Ok(());
                }
            }
            c.trigger_transition_internal(true)
        };
        if let Err(ref err) = result {
            let mut c = ctrl.borrow_mut();
            c.set_notice_msg(err, err, true);
            c.emit_changed();
        }
        Self::schedule_cover_decode(ctrl);
        result
    }

    /// Starts or resumes an active Auto-DJ session right from the booth (even from stopped/empty queue).
    pub fn start_session(ctrl: &Rc<RefCell<Self>>) -> Result<(), String> {
        let res = {
            let mut c = ctrl.borrow_mut();
            if !c.engine.config.mode.is_active() {
                c.engine.config.mode = DjMixMode::SmartFlow;
                c.sync_audio_and_state();
            }
            let st = c.audio.state();
            if st == PlaybackState::Playing {
                if c.audio.current_track_path().is_some() {
                    c.notice = None;
                    c.refresh_upcoming(false);
                    c.emit_changed();
                    Ok(())
                } else {
                    c.trigger_transition_internal(false)
                }
            } else if st == PlaybackState::Paused && c.audio.current_track_path().is_some() {
                c.audio.play().map_err(|e| e.to_string())?;
                c.notice = None;
                c.refresh_upcoming(false);
                c.emit_changed();
                Ok(())
            } else {
                let snap = c.audio.queue_snapshot();
                let lib_tracks = c.catalog.report.borrow().tracks.clone();
                let pick = if let Some(cur) = snap.current.and_then(|i| snap.tracks.get(i)) {
                    Some((cur.clone(), Some(snap.current.unwrap_or(0))))
                } else if !snap.tracks.is_empty() {
                    Some((snap.tracks[0].clone(), Some(0)))
                } else {
                    let salt = (glib::monotonic_time() as u64) ^ c.engine.roll_nonce;
                    c.engine
                        .pick_next(None, &snap.tracks, &lib_tracks, &[], salt)
                        .map(|p| (p.path, p.queue_index))
                        .or_else(|| lib_tracks.first().cloned().map(|p| (p, None)))
                };
                if let Some((path, maybe_idx)) = pick {
                    let idx = if let Some(i) = maybe_idx {
                        i
                    } else {
                        c.resolve_queue_index(&path)?
                    };
                    let cue_in_us = c.engine.intro_cue_us_for(&path).unwrap_or(0);
                    let (rev, _, _) = c.audio.queue_status();
                    c.audio
                        .play_queue_item_with_cue(rev, idx, cue_in_us)
                        .map_err(|e| e.to_string())?;
                    if c.engine.config.mode == DjMixMode::SmartRadio {
                        c.engine.seed_path = Some(path);
                    }
                    c.notice = None;
                    c.refresh_upcoming(true);
                    c.emit_changed();
                    Ok(())
                } else {
                    let msg = i18n::pick(
                        "No tracks available to start DJ session. Check your music folders.",
                        "Nenhuma música encontrada para iniciar o DJ. Confira suas pastas de música.",
                    );
                    c.set_notice_msg(msg, msg, true);
                    c.emit_changed();
                    Err(msg.into())
                }
            }
        };
        Self::schedule_cover_decode(ctrl);
        Self::maybe_spawn_analyzer(ctrl);
        res
    }

    /// Global Next track entry point (transport Next, Right arrow, Page Down).
    /// When Auto-DJ is active (or crossfading), advances via DJ Deck B instead of raw linear queue.
    pub fn next_track(ctrl: &Rc<RefCell<Self>>) -> Result<(), String> {
        let res = {
            let mut c = ctrl.borrow_mut();
            if c.audio.is_crossfading() {
                c.audio.finish_crossfade();
                c.last_track_seq = c.audio.track_sequence();
                c.silence_since = None;
                c.refresh_upcoming(true);
                c.emit_changed();
                Ok(())
            } else if c.engine.config.mode.is_active() {
                if let Some(pick) = c.resolve_default_pick() {
                    let snappy = if c.engine.config.crossfade_secs >= 0.5
                        && c.audio.state() == PlaybackState::Playing
                    {
                        Some(c.engine.config.crossfade_secs.clamp(1.2, 2.4))
                    } else {
                        Some(0.0)
                    };
                    c.execute_pick_transition(pick, snappy)
                } else {
                    c.audio.next().map_err(|e| e.to_string())
                }
            } else {
                c.audio.next().map_err(|e| e.to_string())
            }
        };
        Self::schedule_cover_decode(ctrl);
        res
    }

    /// Jumps / crossfades directly to a chosen upcoming track (e.g. from "Up Next" preview row).
    pub fn mix_to_path(ctrl: &Rc<RefCell<Self>>, path: PathBuf) -> Result<(), String> {
        let res = {
            let mut c = ctrl.borrow_mut();
            if c.audio.is_crossfading() {
                c.audio.finish_crossfade();
            }
            let snappy = if c.audio.state() == PlaybackState::Playing
                && c.engine.config.crossfade_secs >= 0.5
            {
                Some(c.engine.config.crossfade_secs.clamp(1.5, 3.0))
            } else {
                None
            };
            let pick = DjPick {
                path: path.clone(),
                queue_index: None,
                score: 100.0,
                match_pct: None,
                reason_en: "Direct Cue".into(),
                reason_pt: "Disparo Direto".into(),
                target_rate: None,
                journey_phase: None,
            };
            c.engine.upcoming = Some(pick.clone());
            c.execute_pick_transition(pick, snappy)
        };
        Self::schedule_cover_decode(ctrl);
        res
    }

    /// Updates manual crossfader position `0.0..=1.0` while dragging on the mixer console.
    pub fn drag_crossfader(ctrl: &Rc<RefCell<Self>>, progress: f64) {
        let mut c = ctrl.borrow_mut();
        c.fader_dragging.set(true);
        if c.audio.state() != PlaybackState::Playing {
            c.set_notice_msg(
                "Start playback first to crossfade manually between Deck A and Deck B.",
                "Inicie a reprodução para mixar manualmente entre os Decks A e B.",
                false,
            );
            c.emit_changed();
            return;
        }
        if !c.audio.is_crossfading() {
            let Some(pick) = c.resolve_default_pick() else {
                c.set_notice_msg(
                    "No track queued for Deck B yet. Add music folders or build a set.",
                    "Nenhuma faixa pronta no Deck B. Adicione pastas de música ou gere um set.",
                    true,
                );
                c.emit_changed();
                return;
            };
            let Ok(target_index) = c.resolve_queue_index(&pick.path) else {
                return;
            };
            let curve = c.engine.config.curve;
            let cue_us = c.engine.intro_cue_us_for(&pick.path).unwrap_or(0);
            if let Err(err) = c.audio.set_manual_crossfade(
                target_index,
                &pick.path,
                progress,
                curve,
                pick.target_rate,
                cue_us,
                pick.reason_en,
                pick.reason_pt,
            ) {
                c.set_notice_msg(&err.to_string(), &err.to_string(), true);
            }
        } else {
            let curve = c.engine.config.curve;
            let dummy = PathBuf::new();
            let _ = c.audio.set_manual_crossfade(
                0,
                &dummy,
                progress,
                curve,
                None,
                0,
                String::new(),
                String::new(),
            );
        }
        c.emit_changed();
    }

    pub fn release_crossfader(ctrl: &Rc<RefCell<Self>>) {
        let completed = {
            let mut c = ctrl.borrow_mut();
            c.fader_dragging.set(false);
            let outcome = c.audio.release_manual_crossfade();
            if outcome == ManualReleaseOutcome::CompletedToDeckB {
                if let Some(cur) = c.audio.current_track_path() {
                    c.engine.record_played(&cur);
                }
                c.crossfaded_seq = c.audio.track_sequence();
                c.refresh_upcoming(true);
                c.emit_changed();
                true
            } else {
                c.emit_changed();
                false
            }
        };
        if completed {
            Self::schedule_cover_decode(ctrl);
        }
    }

    fn trigger_transition_internal(&mut self, manual: bool) -> Result<(), String> {
        let Some(pick) = self.resolve_default_pick() else {
            return Err(i18n::pick(
                "No eligible next track available. Add folders or verify mounted music drive.",
                "Nenhuma próxima faixa disponível. Adicione pastas ou verifique seu disco de músicas.",
            )
            .into());
        };
        let xfade_secs = if manual {
            self.engine.config.crossfade_secs.max(3.5)
        } else {
            self.engine.config.crossfade_secs
        };
        self.execute_pick_transition(pick, Some(xfade_secs))
    }

    /// Fallback used when `Eos` fires while DJ mode is active (e.g. crossfade == 0s or tiny track).
    pub fn handle_eos_advance(ctrl: &Rc<RefCell<Self>>) -> anyhow::Result<bool> {
        let res = {
            let mut c = ctrl.borrow_mut();
            if c.audio.is_crossfading() {
                c.audio.finish_crossfade();
                c.refresh_upcoming(true);
                c.emit_changed();
                Ok(true)
            } else if c.engine.config.mode.is_active() {
                let snap = c.audio.queue_snapshot();
                let lib_tracks = c.catalog.report.borrow().tracks.clone();
                let current = c.audio.current_track_path();
                if let Some(ref cur) = current {
                    c.engine.record_played(cur);
                }
                c.refresh_upcoming(false);
                if let Some(pick) = c.engine.upcoming.clone() {
                    let target_index =
                        if let Some(idx) = snap.tracks.iter().position(|p| p == &pick.path) {
                            idx
                        } else {
                            c.audio.enqueue_dj_track(&pick.path)?
                        };
                    let cue_in_us = c.engine.intro_cue_us_for(&pick.path).unwrap_or(0);
                    let (rev, _, _) = c.audio.queue_status();
                    c.audio
                        .play_queue_item_with_cue(rev, target_index, cue_in_us)?;
                    c.emit_changed();
                    Ok(true)
                } else if lib_tracks.is_empty() && snap.tracks.is_empty() {
                    c.audio.stop()?;
                    Ok(false)
                } else {
                    c.audio.advance_after_eos()
                }
            } else {
                c.audio.advance_after_eos()
            }
        };
        Self::schedule_cover_decode(ctrl);
        res
    }

    /// Periodic 250ms DJ supervisor: collects background analyses, picks Deck B, triggers auto-crossfade.
    pub fn tick(ctrl: &Rc<RefCell<Self>>) {
        let mut changed = false;
        let mut cover_needed = false;
        let mut spawn_needed = false;
        {
            let mut c = ctrl.borrow_mut();

            // 1. Drain completed background acoustic analyses.
            let mut got_results = false;
            while let Ok(batch) = c.bg_rx.try_recv() {
                c.analyzer_running = false;
                got_results = true;
                for (path, analysis) in batch {
                    c.engine.insert_analysis(path, analysis);
                }
            }
            if got_results {
                changed = true;
                spawn_needed = true;
            }
            if c.last_cache_flush.elapsed() >= Duration::from_secs(6) {
                c.engine.flush_cache_if_dirty();
                c.last_cache_flush = Instant::now();
            }

            // 1b. Refresh Deck B & analyzer automatically once library scan finishes or grows.
            if c.last_catalog_poll.elapsed() >= Duration::from_secs(2) {
                c.last_catalog_poll = Instant::now();
                let cat_len = c.catalog.report.borrow().tracks.len();
                if cat_len != c.last_catalog_len {
                    c.last_catalog_len = cat_len;
                    if c.engine.config.mode.is_active() {
                        c.refresh_upcoming(false);
                        cover_needed = true;
                        spawn_needed = true;
                        changed = true;
                    }
                }
            }

            // 2. Track switch detection -> refresh Deck B preview & clear old silence state.
            let cur_seq = c.audio.track_sequence();
            if cur_seq != c.last_track_seq {
                c.last_track_seq = cur_seq;
                c.silence_since = None;
                if c.engine.config.mode.is_active() {
                    if let Some(cur) = c.audio.current_track_path() {
                        if c.engine.config.mode == DjMixMode::SmartRadio
                            && c.engine.seed_path.is_none()
                        {
                            c.engine.seed_path = Some(cur.clone());
                        }
                    }
                    c.refresh_upcoming(true);
                    cover_needed = true;
                    spawn_needed = true;
                }
                changed = true;
            }

            // 3. Auto-crossfade trigger & Smart Outro Cue while Playing.
            if c.engine.config.mode.is_active()
                && c.audio.state() == PlaybackState::Playing
                && !c.audio.is_crossfading()
                && c.crossfaded_seq != cur_seq
            {
                if c.engine.upcoming.is_none() {
                    c.refresh_upcoming(false);
                    cover_needed = true;
                }
                let dur_us = c.audio.duration_us();
                let pos_us = c.audio.position_us();
                if dur_us >= 5_000_000 && pos_us >= 1_500_000 {
                    let dur_s = dur_us as f64 / 1_000_000.0;
                    let pos_s = pos_us as f64 / 1_000_000.0;
                    let rem_s = (dur_s - pos_s).max(0.0);
                    let cfg_xfade = c.engine.config.crossfade_secs;
                    let effective_xfade = if cfg_xfade < 0.5 {
                        0.0
                    } else {
                        cfg_xfade.min(dur_s * 0.35)
                    };

                    let mut should_mix =
                        effective_xfade >= 0.5 && rem_s <= effective_xfade && pos_s >= 2.0;

                    if !should_mix
                        && c.engine.config.smart_cue
                        && effective_xfade >= 0.5
                        && rem_s <= 22.0
                        && pos_s >= (dur_s * 0.60).max(12.0)
                    {
                        let (vl, vr) = c.audio.vu_levels();
                        if vl.max(vr) < -36.0 {
                            let now = Instant::now();
                            let since = *c.silence_since.get_or_insert(now);
                            if now.duration_since(since) >= Duration::from_millis(1400) {
                                should_mix = true;
                            }
                        } else {
                            c.silence_since = None;
                        }
                    } else if rem_s > 22.0 {
                        c.silence_since = None;
                    }

                    if should_mix {
                        if let Err(err) = c.trigger_transition_internal(false) {
                            eprintln!("Auto-DJ crossfade skipped: {err}");
                        }
                        changed = true;
                        cover_needed = true;
                    }
                }
            }

            if changed {
                c.emit_changed();
            }
        }
        if cover_needed {
            Self::schedule_cover_decode(ctrl);
        }
        if spawn_needed {
            Self::maybe_spawn_analyzer(ctrl);
        }
    }

    pub fn upcoming_profile(&self) -> Option<(TrackProfile, DjPick)> {
        let pick = self.engine.upcoming.clone()?;
        let prof = self.engine.profile_for(&pick.path);
        Some((prof, pick))
    }

    pub fn upcoming_preview(&self) -> Vec<(TrackProfile, DjPick)> {
        self.upcoming_preview
            .iter()
            .cloned()
            .map(|pick| (self.engine.profile_for(&pick.path), pick))
            .collect()
    }

    pub fn config(&self) -> DjConfig {
        self.engine.config.clone()
    }
}

/// Renders the interactive Dual-Deck Mixer & Crossfader console in Cairo.
pub fn draw_dj_console(
    cr: &cairo::Context,
    w: i32,
    h: i32,
    ctrl: &DjController,
    theme: Theme,
    disc_angle: f64,
    vu_level: f64,
) {
    let w = w as f64;
    let h = h as f64;
    if w < 100.0 || h < 80.0 {
        return;
    }
    let lang = i18n::current();
    let (ar, ag, ab) = theme.accent_rgb();

    // Brushed dark console chassis
    let grad = cairo::LinearGradient::new(0.0, 0.0, w, h);
    grad.add_color_stop_rgba(0.0, 0.125, 0.13, 0.14, 1.0);
    grad.add_color_stop_rgba(0.5, 0.095, 0.10, 0.11, 1.0);
    grad.add_color_stop_rgba(1.0, 0.075, 0.08, 0.09, 1.0);
    let _ = cr.set_source(&grad);
    draw_rounded_rect(cr, 1.0, 1.0, w - 2.0, h - 2.0, 14.0);
    let _ = cr.fill();

    cr.set_source_rgba(1.0, 1.0, 1.0, 0.08);
    cr.set_line_width(1.2);
    draw_rounded_rect(cr, 1.0, 1.0, w - 2.0, h - 2.0, 14.0);
    let _ = cr.stroke();

    let xfade_snap = ctrl.audio.crossfade_snapshot();
    let is_xfading = xfade_snap.is_some();
    let active_curve = xfade_snap
        .as_ref()
        .map(|s| s.curve)
        .unwrap_or(ctrl.engine.config.curve);
    let is_manual = xfade_snap.as_ref().is_some_and(|s| s.manual);
    let progress = xfade_snap.as_ref().map(|s| s.progress).unwrap_or(0.0);
    let (g_out, g_in) = if is_xfading {
        crossfade_gains(progress, active_curve)
    } else {
        (1.0, 0.0)
    };

    let center_w = (w * 0.24).clamp(150.0, 230.0);
    let deck_w = ((w - center_w - 40.0) / 2.0).max(120.0);
    let deck_y = 14.0;
    let deck_h = h - 28.0;

    // DECK A (Left — On Air)
    let ax = 14.0;
    draw_rounded_rect(cr, ax, deck_y, deck_w, deck_h, 10.0);
    cr.set_source_rgba(0.06, 0.065, 0.072, 0.85);
    let _ = cr.fill_preserve();
    cr.set_source_rgba(ar, ag, ab, if is_xfading { 0.35 } else { 0.55 });
    cr.set_line_width(1.1);
    let _ = cr.stroke();

    let disc_r = (deck_h * 0.31).clamp(26.0, 50.0);
    let disc_ax = ax + 16.0 + disc_r;
    let disc_ay = deck_y + deck_h * 0.52;
    let playing = ctrl.audio.state() == PlaybackState::Playing;
    draw_mini_turntable_disc(
        cr,
        disc_ax,
        disc_ay,
        disc_r,
        if playing { disc_angle } else { 0.3 },
        None,
        ar,
        ag,
        ab,
        g_out,
    );

    let meta_a = ctrl.audio.metadata();
    let prof_a = ctrl
        .audio
        .current_track_path()
        .map(|p| ctrl.engine.profile_for(&p));
    let text_ax = disc_ax + disc_r + 14.0;
    let avail_aw = (ax + deck_w - 14.0 - text_ax).max(40.0);

    cr.select_font_face("Sans", cairo::FontSlant::Normal, cairo::FontWeight::Bold);
    cr.set_font_size(9.5);
    cr.set_source_rgba(ar, ag, ab, 0.95);
    cr.move_to(text_ax, deck_y + 22.0);
    let tag_a = if is_xfading {
        i18n::pick_for(lang, "DECK A · FADING OUT", "DECK A · SAINDO")
    } else if playing {
        i18n::pick_for(lang, "DECK A · ON AIR", "DECK A · NO AR")
    } else {
        i18n::pick_for(lang, "DECK A · READY", "DECK A · PRONTO")
    };
    let _ = cr.show_text(tag_a);

    cr.set_font_size(13.0);
    cr.set_source_rgba(0.95, 0.94, 0.91, 1.0);
    let title_a = if meta_a.title.is_empty() {
        i18n::pick_for(lang, "No track loaded", "Nenhuma faixa")
    } else {
        &meta_a.title
    };
    cr.move_to(text_ax, deck_y + 42.0);
    let _ = cr.show_text(&clip_text(cr, title_a, avail_aw));

    cr.select_font_face("Sans", cairo::FontSlant::Normal, cairo::FontWeight::Normal);
    cr.set_font_size(11.0);
    cr.set_source_rgba(0.68, 0.70, 0.73, 1.0);
    let sub_a = if meta_a.artist.is_empty() {
        prof_a
            .as_ref()
            .map(|p| p.artist.as_str())
            .filter(|s| !s.is_empty())
            .unwrap_or("VinylPlayer")
    } else {
        &meta_a.artist
    };
    cr.move_to(text_ax, deck_y + 58.0);
    let _ = cr.show_text(&clip_text(cr, sub_a, avail_aw));

    if let Some(ref pa) = prof_a {
        let mut specs = Vec::new();
        if let Some(bpm) = pa.bpm {
            specs.push(format!("{bpm} BPM"));
        }
        if let Some(e) = pa.energy {
            specs.push(format!("{}%", (e * 100.0).round() as u32));
        }
        if let Some(ref g) = pa.genre {
            specs.push(g.clone());
        }
        if !specs.is_empty() {
            cr.set_font_size(9.5);
            cr.set_source_rgba(ar, ag, ab, 0.80);
            cr.move_to(text_ax, deck_y + 76.0);
            let _ = cr.show_text(&clip_text(cr, &specs.join(" · "), avail_aw));
        }
    }

    // Track progress bar + Outro Crossfade Zone highlight
    let bar_x = text_ax;
    let bar_y = deck_y + deck_h - 22.0;
    let bar_w = avail_aw;
    draw_rounded_rect(cr, bar_x, bar_y, bar_w, 5.0, 2.5);
    cr.set_source_rgba(0.22, 0.24, 0.27, 1.0);
    let _ = cr.fill();

    let dur_us = ctrl.audio.duration_us();
    let pos_us = ctrl.audio.position_us();
    if dur_us > 0 {
        let dur_s = dur_us as f64 / 1_000_000.0;
        let frac = (pos_us as f64 / dur_us as f64).clamp(0.0, 1.0);
        let xfade_frac = (ctrl.engine.config.crossfade_secs / dur_s).clamp(0.0, 0.35);
        if ctrl.engine.config.mode.is_active() && xfade_frac > 0.01 {
            let zone_x = bar_x + bar_w * (1.0 - xfade_frac);
            draw_rounded_rect(cr, zone_x, bar_y, bar_w * xfade_frac, 5.0, 2.5);
            cr.set_source_rgba(ar, ag, ab, 0.28);
            let _ = cr.fill();
        }
        draw_rounded_rect(cr, bar_x, bar_y, bar_w * frac, 5.0, 2.5);
        cr.set_source_rgba(ar, ag, ab, 0.92);
        let _ = cr.fill();
    }

    // CENTER MIXER (Crossfader + Equal-Power Curve + Level Meters)
    let cx_box = ax + deck_w + 6.0;
    let cx_mid = cx_box + center_w * 0.5;
    cr.select_font_face("Sans", cairo::FontSlant::Normal, cairo::FontWeight::Bold);
    cr.set_font_size(9.0);
    let mix_hdr = if is_xfading {
        if is_manual {
            format!(
                "{} {:.0}%",
                i18n::pick_for(lang, "MANUAL MIX", "MIX MANUAL"),
                progress * 100.0
            )
        } else {
            format!(
                "{} {:.0}%",
                i18n::pick_for(lang, "MIXING", "MIXANDO"),
                progress * 100.0
            )
        }
    } else if ctrl.engine.config.mode.is_active() {
        ctrl.engine.config.mode.short_label(lang).to_string()
    } else {
        i18n::pick_for(lang, "DJ STANDBY", "DJ ESPERA").to_string()
    };
    cr.set_source_rgba(
        if is_xfading || ctrl.engine.config.mode.is_active() {
            ar
        } else {
            0.60
        },
        if is_xfading || ctrl.engine.config.mode.is_active() {
            ag
        } else {
            0.62
        },
        if is_xfading || ctrl.engine.config.mode.is_active() {
            ab
        } else {
            0.65
        },
        0.95,
    );
    if let Ok(ext) = cr.text_extents(&mix_hdr) {
        cr.move_to(cx_mid - ext.width() / 2.0, deck_y + 22.0);
        let _ = cr.show_text(&mix_hdr);
    }

    // Active Crossfade Curve visualizer
    let curve_w = center_w - 34.0;
    let curve_h = 34.0;
    let curve_x = cx_mid - curve_w / 2.0;
    let curve_y = deck_y + 34.0;
    draw_rounded_rect(
        cr,
        curve_x - 4.0,
        curve_y - 4.0,
        curve_w + 8.0,
        curve_h + 8.0,
        6.0,
    );
    cr.set_source_rgba(0.05, 0.055, 0.06, 0.9);
    let _ = cr.fill();

    // Outgoing fade-out curve (Deck A)
    cr.set_line_width(1.6);
    cr.set_source_rgba(0.85, 0.85, 0.85, if is_xfading { 0.48 } else { 0.30 });
    for i in 0..=28 {
        let t = i as f64 / 28.0;
        let (ga, _) = crossfade_gains(t, active_curve);
        let px = curve_x + t * curve_w;
        let py = curve_y + curve_h * (1.0 - ga);
        if i == 0 {
            cr.move_to(px, py);
        } else {
            cr.line_to(px, py);
        }
    }
    let _ = cr.stroke();

    // Incoming fade-in curve (Deck B)
    cr.set_source_rgba(ar, ag, ab, if is_xfading { 0.95 } else { 0.58 });
    for i in 0..=28 {
        let t = i as f64 / 28.0;
        let (_, gb) = crossfade_gains(t, active_curve);
        let px = curve_x + t * curve_w;
        let py = curve_y + curve_h * (1.0 - gb);
        if i == 0 {
            cr.move_to(px, py);
        } else {
            cr.line_to(px, py);
        }
    }
    let _ = cr.stroke();

    // Channel A & B vertical VU bars flanking the curve
    let meter_h = 34.0;
    let meter_w = 5.0;
    let vu_a = (vu_level * g_out).clamp(0.0, 1.0);
    let vu_b = (vu_level * g_in).clamp(0.0, 1.0);
    for (mx, val) in [(curve_x - 11.0, vu_a), (curve_x + curve_w + 6.0, vu_b)] {
        draw_rounded_rect(cr, mx, curve_y, meter_w, meter_h, 2.0);
        cr.set_source_rgba(0.18, 0.20, 0.22, 1.0);
        let _ = cr.fill();
        if val > 0.02 {
            let fh = meter_h * val;
            draw_rounded_rect(cr, mx, curve_y + meter_h - fh, meter_w, fh, 2.0);
            cr.set_source_rgba(ar, ag, ab, 0.9);
            let _ = cr.fill();
        }
    }

    // Horizontal Crossfader Track & Interactive Aluminum Knob
    let fader_w = (center_w - 28.0).max(90.0);
    let fader_x = cx_mid - fader_w / 2.0;
    let fader_y = deck_y + deck_h - 32.0;
    ctrl.fader_geom.set((fader_x, fader_w, fader_y));

    cr.set_source_rgba(0.04, 0.045, 0.05, 1.0);
    draw_rounded_rect(cr, fader_x, fader_y + 6.0, fader_w, 6.0, 3.0);
    let _ = cr.fill();

    // Tick marks along crossfader rail
    cr.set_line_width(1.0);
    for i in 0..=8 {
        let tx = fader_x + (i as f64 / 8.0) * fader_w;
        cr.set_source_rgba(1.0, 1.0, 1.0, if i == 4 { 0.45 } else { 0.18 });
        cr.move_to(tx, fader_y + 1.0);
        cr.line_to(tx, fader_y + 4.0);
        cr.move_to(tx, fader_y + 14.0);
        cr.line_to(tx, fader_y + 17.0);
        let _ = cr.stroke();
    }

    let fader_active = ctrl.fader_dragging.get() || ctrl.fader_hover.get();
    let knob_x = fader_x + progress * fader_w;
    let kw = if fader_active { 18.0 } else { 16.0 };
    let kh = if fader_active { 26.0 } else { 24.0 };
    let ky = fader_y - if fader_active { 4.0 } else { 3.0 };
    draw_rounded_rect(cr, knob_x - kw / 2.0, ky, kw, kh, 3.5);
    let kgrad = cairo::LinearGradient::new(knob_x - kw / 2.0, ky, knob_x + kw / 2.0, ky);
    let boost = if fader_active { 0.10 } else { 0.0 };
    kgrad.add_color_stop_rgba(0.0, 0.45 + boost, 0.47 + boost, 0.50 + boost, 1.0);
    kgrad.add_color_stop_rgba(0.5, 0.65 + boost, 0.67 + boost, 0.70 + boost, 1.0);
    kgrad.add_color_stop_rgba(1.0, 0.35 + boost, 0.37 + boost, 0.40 + boost, 1.0);
    let _ = cr.set_source(&kgrad);
    let _ = cr.fill_preserve();
    if fader_active {
        cr.set_source_rgba(ar, ag, ab, 0.85);
        cr.set_line_width(1.2);
        let _ = cr.stroke();
    } else {
        cr.new_path();
    }

    cr.set_source_rgba(ar, ag, ab, 1.0);
    cr.set_line_width(2.0);
    cr.move_to(knob_x, ky + 2.0);
    cr.line_to(knob_x, ky + kh - 2.0);
    let _ = cr.stroke();

    // Subtle A / B labels flanking crossfader rail
    cr.select_font_face("Sans", cairo::FontSlant::Normal, cairo::FontWeight::Bold);
    cr.set_font_size(8.0);
    cr.set_source_rgba(0.65, 0.68, 0.72, 0.75);
    cr.move_to(fader_x - 10.0, fader_y + 12.0);
    let _ = cr.show_text("A");
    cr.move_to(fader_x + fader_w + 4.0, fader_y + 12.0);
    let _ = cr.show_text("B");

    // DECK B (Right — Up Next / Mixing In)
    let bx = w - deck_w - 14.0;
    draw_rounded_rect(cr, bx, deck_y, deck_w, deck_h, 10.0);
    cr.set_source_rgba(0.06, 0.065, 0.072, 0.85);
    let _ = cr.fill_preserve();
    cr.set_source_rgba(ar, ag, ab, if is_xfading { 0.80 } else { 0.25 });
    cr.set_line_width(if is_xfading { 1.6 } else { 1.0 });
    let _ = cr.stroke();

    let disc_bx = bx + deck_w - 16.0 - disc_r;
    let disc_by = disc_ay;
    draw_mini_turntable_disc(
        cr,
        disc_bx,
        disc_by,
        disc_r,
        if is_xfading { disc_angle * 1.05 } else { 0.0 },
        ctrl.upcoming_cover_pb.as_ref(),
        ar,
        ag,
        ab,
        if is_xfading { g_in } else { 0.45 },
    );

    let text_bx = bx + 14.0;
    let avail_bw = (disc_bx - disc_r - 14.0 - text_bx).max(40.0);

    cr.select_font_face("Sans", cairo::FontSlant::Normal, cairo::FontWeight::Bold);
    cr.set_font_size(9.5);
    cr.set_source_rgba(ar, ag, ab, if is_xfading { 1.0 } else { 0.78 });
    cr.move_to(text_bx, deck_y + 22.0);
    let tag_b = if is_xfading {
        i18n::pick_for(lang, "DECK B · MIXING IN", "DECK B · ENTRANDO")
    } else if ctrl.engine.config.mode.is_active() {
        i18n::pick_for(lang, "DECK B · CUED NEXT", "DECK B · PRÓXIMA")
    } else {
        i18n::pick_for(lang, "DECK B · IDLE", "DECK B · INATIVO")
    };
    let _ = cr.show_text(tag_b);

    if let Some(ref snap_xf) = xfade_snap {
        cr.set_font_size(13.0);
        cr.set_source_rgba(0.95, 0.94, 0.91, 1.0);
        cr.move_to(text_bx, deck_y + 42.0);
        let _ = cr.show_text(&clip_text(cr, &snap_xf.incoming_meta.title, avail_bw));

        cr.select_font_face("Sans", cairo::FontSlant::Normal, cairo::FontWeight::Normal);
        cr.set_font_size(11.0);
        cr.set_source_rgba(0.68, 0.70, 0.73, 1.0);
        cr.move_to(text_bx, deck_y + 58.0);
        let artist_xf = if !snap_xf.incoming_meta.artist.is_empty() {
            snap_xf.incoming_meta.artist.as_str()
        } else {
            "VinylPlayer"
        };
        let _ = cr.show_text(&clip_text(cr, artist_xf, avail_bw));

        cr.set_font_size(9.5);
        cr.set_source_rgba(ar, ag, ab, 0.92);
        cr.move_to(text_bx, deck_y + deck_h - 18.0);
        let rsn = match lang {
            Language::En => &snap_xf.reason_en,
            Language::PtBr => &snap_xf.reason_pt,
        };
        let _ = cr.show_text(&clip_text(cr, rsn, avail_bw));
    } else if let Some((prof_b, pick)) = ctrl.upcoming_profile() {
        cr.set_font_size(13.0);
        cr.set_source_rgba(0.95, 0.94, 0.91, 1.0);
        cr.move_to(text_bx, deck_y + 42.0);
        let _ = cr.show_text(&clip_text(cr, &prof_b.title, avail_bw));

        cr.select_font_face("Sans", cairo::FontSlant::Normal, cairo::FontWeight::Normal);
        cr.set_font_size(11.0);
        cr.set_source_rgba(0.68, 0.70, 0.73, 1.0);
        cr.move_to(text_bx, deck_y + 58.0);
        let artist_b = if prof_b.artist.is_empty() {
            "VinylPlayer"
        } else {
            &prof_b.artist
        };
        let _ = cr.show_text(&clip_text(cr, artist_b, avail_bw));

        let mut specs_b = Vec::new();
        if let Some(bpm) = prof_b.bpm {
            specs_b.push(format!("{bpm} BPM"));
        }
        if let Some(e) = prof_b.energy {
            specs_b.push(format!("{}%", (e * 100.0).round() as u32));
        }
        if let Some(ref g) = prof_b.genre {
            specs_b.push(g.clone());
        }
        if let Some(cue_us) = ctrl.engine.intro_cue_us_for(&pick.path) {
            specs_b.push(format!("-{:.1}s intro", cue_us as f64 / 1_000_000.0));
        }
        if !specs_b.is_empty() {
            cr.set_font_size(9.5);
            cr.set_source_rgba(0.75, 0.77, 0.80, 0.88);
            cr.move_to(text_bx, deck_y + 76.0);
            let _ = cr.show_text(&clip_text(cr, &specs_b.join(" · "), avail_bw));
        }

        cr.set_font_size(9.5);
        cr.set_source_rgba(ar, ag, ab, 0.92);
        cr.move_to(text_bx, deck_y + deck_h - 18.0);
        let _ = cr.show_text(&clip_text(cr, pick.reason_for(lang), avail_bw));
    } else {
        cr.select_font_face("Sans", cairo::FontSlant::Normal, cairo::FontWeight::Normal);
        cr.set_font_size(11.5);
        cr.set_source_rgba(0.55, 0.58, 0.62, 1.0);
        cr.move_to(text_bx, deck_y + 46.0);
        let empty_msg = i18n::pick_for(
            lang,
            "Select a DJ Mix mode below",
            "Escolha um modo DJ abaixo",
        );
        let _ = cr.show_text(&clip_text(cr, empty_msg, avail_bw));
    }
}

#[allow(clippy::too_many_arguments)]
fn draw_mini_turntable_disc(
    cr: &cairo::Context,
    cx: f64,
    cy: f64,
    r: f64,
    angle: f64,
    cover_pb: Option<&gdk_pixbuf::Pixbuf>,
    ar: f64,
    ag: f64,
    ab: f64,
    glow: f64,
) {
    cr.save().ok();
    cr.translate(cx, cy);
    cr.rotate(angle);

    // Outer platter rim glow proportional to channel gain
    cr.arc(0.0, 0.0, r + 2.0, 0.0, TAU);
    cr.set_source_rgba(ar, ag, ab, 0.15 + glow * 0.45);
    cr.set_line_width(2.0);
    let _ = cr.stroke();

    // Vinyl disc body
    cr.arc(0.0, 0.0, r, 0.0, TAU);
    cr.set_source_rgb(0.08, 0.082, 0.088);
    let _ = cr.fill();

    // Subtle concentric vinyl grooves
    cr.set_source_rgba(1.0, 1.0, 1.0, 0.06);
    cr.set_line_width(0.7);
    for frac in [0.85, 0.72, 0.60, 0.48] {
        cr.arc(0.0, 0.0, r * frac, 0.0, TAU);
        let _ = cr.stroke();
    }

    // Center label (album cover or themed label)
    let label_r = r * 0.36;
    cr.save().ok();
    cr.arc(0.0, 0.0, label_r, 0.0, TAU);
    cr.clip();
    if let Some(pb) = cover_pb {
        let sx = (label_r * 2.0) / pb.width().max(1) as f64;
        let sy = (label_r * 2.0) / pb.height().max(1) as f64;
        let scale = sx.max(sy);
        cr.save().ok();
        cr.scale(scale, scale);
        gtk::prelude::GdkCairoContextExt::set_source_pixbuf(
            cr,
            pb,
            -(pb.width() as f64) / 2.0,
            -(pb.height() as f64) / 2.0,
        );
        let _ = cr.paint();
        cr.restore().ok();
    } else {
        cr.set_source_rgba(ar, ag, ab, 0.85);
        let _ = cr.paint();
    }
    cr.restore().ok();

    // Spindle mark & rotation strobe notch
    cr.arc(0.0, 0.0, 2.5, 0.0, TAU);
    cr.set_source_rgb(0.85, 0.86, 0.88);
    let _ = cr.fill();

    cr.set_source_rgba(1.0, 1.0, 1.0, 0.35);
    cr.set_line_width(1.5);
    cr.move_to(label_r + 3.0, 0.0);
    cr.line_to(r - 4.0, 0.0);
    let _ = cr.stroke();

    cr.restore().ok();
}

fn clip_text(cr: &cairo::Context, text: &str, max_w: f64) -> String {
    if let Ok(ext) = cr.text_extents(text) {
        if ext.width() <= max_w {
            return text.to_string();
        }
    }
    let mut out = String::new();
    for ch in text.chars() {
        out.push(ch);
        let candidate = format!("{out}…");
        if let Ok(ext) = cr.text_extents(&candidate) {
            if ext.width() > max_w {
                out.pop();
                return format!("{out}…");
            }
        }
    }
    out
}

/// Builds the full DJ Booth view (`"dj"` page in main stack, shortcut Alt+5).
pub fn dj_booth_view(
    ctrl: &Rc<RefCell<DjController>>,
    angle: &Rc<Cell<f64>>,
    state_mgr: &StateManager,
) -> gtk::Widget {
    let scroll = gtk::ScrolledWindow::new();
    scroll.set_policy(gtk::PolicyType::Never, gtk::PolicyType::Automatic);
    scroll.set_hexpand(true);
    scroll.set_vexpand(true);

    let root = gtk::Box::new(Orientation::Vertical, 11);
    root.add_css_class("dj-booth");
    root.set_margin_start(28);
    root.set_margin_end(28);
    root.set_margin_top(14);
    root.set_margin_bottom(14);

    // 1. Header & Instant Action Buttons
    let header = gtk::Box::new(Orientation::Horizontal, 12);
    let title_col = gtk::Box::new(Orientation::Vertical, 2);
    title_col.set_hexpand(true);

    let title_row = gtk::Box::new(Orientation::Horizontal, 10);
    let title_lbl = gtk::Label::new(None);
    i18n::bind_label(&title_lbl, |l| i18n::pick_for(l, "DJ Booth", "Cabine DJ"));
    title_lbl.add_css_class("page-title");
    title_lbl.set_xalign(0.0);

    let status_pill = gtk::Label::new(None);
    status_pill.add_css_class("dj-status-pill");
    status_pill.set_valign(gtk::Align::Center);

    title_row.append(&title_lbl);
    title_row.append(&status_pill);

    let subtitle = gtk::Label::new(None);
    subtitle.add_css_class("bottom-artist-title");
    subtitle.set_xalign(0.0);
    subtitle.set_ellipsize(gtk::pango::EllipsizeMode::End);

    title_col.append(&title_row);
    title_col.append(&subtitle);
    header.append(&title_col);

    let start_btn = gtk::Button::new();
    start_btn.add_css_class("folder-button");
    start_btn.add_css_class("dj-start-btn");
    let start_box = gtk::Box::new(Orientation::Horizontal, 6);
    start_box.append(&gtk::Image::from_icon_name("media-playback-start-symbolic"));
    let start_lbl = gtk::Label::new(None);
    i18n::bind_label(&start_lbl, |l| i18n::pick_for(l, "Start DJ", "Iniciar DJ"));
    start_box.append(&start_lbl);
    start_btn.set_child(Some(&start_box));
    i18n::bind_tooltip(&start_btn, |l| {
        i18n::pick_for(
            l,
            "Start or resume Auto-DJ playback right from the booth",
            "Inicia ou retoma a reprodução do Auto-DJ direto da cabine",
        )
    });

    let reroll_btn = gtk::Button::new();
    reroll_btn.add_css_class("folder-button");
    let reroll_box = gtk::Box::new(Orientation::Horizontal, 6);
    reroll_box.append(&gtk::Image::from_icon_name("view-refresh-symbolic"));
    let reroll_lbl = gtk::Label::new(None);
    i18n::bind_label(&reroll_lbl, |l| {
        i18n::pick_for(l, "Re-roll Next", "Trocar Próxima")
    });
    reroll_box.append(&reroll_lbl);
    reroll_btn.set_child(Some(&reroll_box));
    i18n::bind_tooltip(&reroll_btn, |l| {
        i18n::pick_for(
            l,
            "Pick another candidate track for Deck B",
            "Sortear/escolher outra música candidata para o Deck B",
        )
    });

    let queue_set_btn = gtk::Button::new();
    queue_set_btn.add_css_class("folder-button");
    let qset_box = gtk::Box::new(Orientation::Horizontal, 6);
    qset_box.append(&gtk::Image::from_icon_name("view-list-symbolic"));
    let qset_lbl = gtk::Label::new(None);
    i18n::bind_label(&qset_lbl, |l| {
        i18n::pick_for(l, "Build Set (+20)", "Gerar Set (+20)")
    });
    qset_box.append(&qset_lbl);
    queue_set_btn.set_child(Some(&qset_box));
    i18n::bind_tooltip(&queue_set_btn, |l| {
        i18n::pick_for(
            l,
            "Populate the upcoming queue with 20 DJ-curated tracks using the active mode",
            "Preenche a sequência da fila com 20 faixas curadas pelo modo DJ ativo",
        )
    });

    let mix_now_btn = gtk::Button::new();
    mix_now_btn.add_css_class("open-button");
    mix_now_btn.add_css_class("dj-mix-btn");
    let mix_box = gtk::Box::new(Orientation::Horizontal, 6);
    mix_box.append(&gtk::Image::from_icon_name("media-skip-forward-symbolic"));
    let mix_lbl = gtk::Label::new(None);
    i18n::bind_label(&mix_lbl, |l| i18n::pick_for(l, "Mix Now", "Mixar Agora"));
    mix_box.append(&mix_lbl);
    mix_now_btn.set_child(Some(&mix_box));
    i18n::bind_tooltip(&mix_now_btn, |l| {
        i18n::pick_for(
            l,
            "Trigger an Equal-Power crossfade to Deck B right now",
            "Dispara imediatamente um crossfade Equal-Power para o Deck B",
        )
    });

    header.append(&start_btn);
    header.append(&reroll_btn);
    header.append(&queue_set_btn);
    header.append(&mix_now_btn);
    root.append(&header);

    // 1b. Inline feedback / empty-library banner
    let notice_bar = gtk::Box::new(Orientation::Horizontal, 10);
    notice_bar.add_css_class("dj-notice-banner");
    notice_bar.set_visible(false);
    let notice_icon = gtk::Image::from_icon_name("dialog-information-symbolic");
    let notice_lbl = gtk::Label::new(None);
    notice_lbl.set_hexpand(true);
    notice_lbl.set_xalign(0.0);
    notice_lbl.set_wrap(true);
    let notice_close = gtk::Button::from_icon_name("window-close-symbolic");
    notice_close.add_css_class("queue-clear");
    notice_bar.append(&notice_icon);
    notice_bar.append(&notice_lbl);
    notice_bar.append(&notice_close);
    {
        let c_nc = ctrl.clone();
        notice_close.connect_clicked(move |_| {
            DjController::clear_notice(&c_nc);
        });
    }
    root.append(&notice_bar);

    // 2. Interactive Dual-Deck Cairo Console (clickable / draggable crossfader)
    let console = DrawingArea::new();
    console.set_content_height(152);
    console.set_hexpand(true);
    {
        let c_draw = ctrl.clone();
        let angle_draw = angle.clone();
        let state_draw = state_mgr.clone();
        console.set_draw_func(move |_, cr, w, h| {
            let ctrl_ref = c_draw.borrow();
            let theme = Theme::from_id(state_draw.snapshot().theme());
            let (vl, vr) = ctrl_ref.audio.vu_levels();
            let vu = crate::audio::db_to_vu_deflection(vl.max(vr));
            draw_dj_console(cr, w, h, &ctrl_ref, theme, angle_draw.get(), vu);
        });
    }
    {
        let c_drag = ctrl.clone();
        let drag = gtk::GestureDrag::new();
        let drag_active = Rc::new(Cell::new(false));
        let drag_start_x = Rc::new(Cell::new(0.0_f64));
        {
            let c_b = c_drag.clone();
            let da = drag_active.clone();
            let sx = drag_start_x.clone();
            drag.connect_drag_begin(move |_, x, y| {
                let (fx, fw, fy) = c_b.borrow().fader_geom.get();
                if fw > 10.0
                    && x >= fx - 20.0
                    && x <= fx + fw + 20.0
                    && y >= fy - 18.0
                    && y <= fy + 34.0
                {
                    da.set(true);
                    sx.set(x);
                    let t = ((x - fx) / fw).clamp(0.0, 1.0);
                    DjController::drag_crossfader(&c_b, t);
                } else {
                    da.set(false);
                }
            });
        }
        {
            let c_u = c_drag.clone();
            let da = drag_active.clone();
            let sx = drag_start_x.clone();
            drag.connect_drag_update(move |_, dx, _| {
                if !da.get() {
                    return;
                }
                let (fx, fw, _) = c_u.borrow().fader_geom.get();
                if fw > 10.0 {
                    let cur_x = sx.get() + dx;
                    let t = ((cur_x - fx) / fw).clamp(0.0, 1.0);
                    DjController::drag_crossfader(&c_u, t);
                }
            });
        }
        {
            let c_e = c_drag.clone();
            let da = drag_active.clone();
            drag.connect_drag_end(move |_, _, _| {
                if da.get() {
                    da.set(false);
                    DjController::release_crossfader(&c_e);
                }
            });
        }
        console.add_controller(drag);

        let motion = gtk::EventControllerMotion::new();
        let c_m = ctrl.clone();
        let con_w = console.downgrade();
        motion.connect_motion(move |_, x, y| {
            let (fx, fw, fy) = c_m.borrow().fader_geom.get();
            let over = fw > 10.0
                && x >= fx - 18.0
                && x <= fx + fw + 18.0
                && y >= fy - 14.0
                && y <= fy + 30.0;
            let prev = c_m.borrow().fader_hover.replace(over);
            if prev != over {
                if let Some(da) = con_w.upgrade() {
                    da.set_cursor_from_name(if over { Some("ew-resize") } else { None });
                    da.queue_draw();
                }
            }
        });
        let c_leave = ctrl.clone();
        let con_wl = console.downgrade();
        motion.connect_leave(move |_| {
            if c_leave.borrow().fader_hover.replace(false) {
                if let Some(da) = con_wl.upgrade() {
                    da.set_cursor_from_name(None);
                    da.queue_draw();
                }
            }
        });
        console.add_controller(motion);
    }
    root.append(&console);

    // Smooth animation redraw while DJ console is mapped & playing/crossfading
    {
        let c_tick = ctrl.clone();
        let last_redraw = Cell::new(0_i64);
        console.add_tick_callback(move |widget, clock| {
            let t = clock.frame_time();
            let c = c_tick.borrow();
            let active = c.audio.state() == PlaybackState::Playing || c.audio.is_crossfading();
            let min_dt = if c.audio.is_crossfading() {
                30_000
            } else {
                90_000
            };
            if active && t - last_redraw.get() >= min_dt {
                last_redraw.set(t);
                widget.queue_draw();
            }
            glib::ControlFlow::Continue
        });
    }

    // 3. Mode Selection: Off Pill + Random Mixes Grid + Smart Mixes Grid
    let off_row = gtk::Box::new(Orientation::Horizontal, 10);
    let off_btn = gtk::ToggleButton::new();
    off_btn.add_css_class("dj-mode-card");
    off_btn.set_hexpand(true);
    let off_inner = gtk::Box::new(Orientation::Horizontal, 10);
    off_inner.append(&gtk::Image::from_icon_name("media-playback-stop-symbolic"));
    let off_title = gtk::Label::new(None);
    off_title.set_xalign(0.0);
    off_title.add_css_class("playlist-card-title");
    i18n::bind_label(&off_title, |l| DjMixMode::Off.label(l));
    let off_desc = gtk::Label::new(None);
    off_desc.set_xalign(0.0);
    off_desc.set_hexpand(true);
    off_desc.set_ellipsize(gtk::pango::EllipsizeMode::End);
    off_desc.add_css_class("bottom-artist-title");
    i18n::bind_label(&off_desc, |l| DjMixMode::Off.description(l));
    off_inner.append(&off_title);
    off_inner.append(&off_desc);
    off_btn.set_child(Some(&off_inner));
    off_row.append(&off_btn);
    root.append(&off_row);

    let mode_cards: Rc<RefCell<Vec<(DjMixMode, gtk::ToggleButton)>>> =
        Rc::new(RefCell::new(vec![(DjMixMode::Off, off_btn.clone())]));

    let make_section =
        |title_en: &'static str, title_pt: &'static str, modes: &[DjMixMode], is_smart: bool| {
            let sec_box = gtk::Box::new(Orientation::Vertical, 8);
            let sec_hdr = gtk::Label::new(None);
            i18n::bind_label(&sec_hdr, move |l| i18n::pick_for(l, title_en, title_pt));
            sec_hdr.add_css_class("sidebar-section-title");
            sec_hdr.set_xalign(0.0);
            sec_hdr.set_margin_start(2);
            sec_hdr.set_margin_bottom(2);
            sec_box.append(&sec_hdr);

            let row = gtk::Box::new(Orientation::Horizontal, 10);
            row.set_homogeneous(true);
            for &mode in modes {
                let card = gtk::ToggleButton::new();
                card.add_css_class("dj-mode-card");
                card.set_hexpand(true);
                let card_body = gtk::Box::new(Orientation::Vertical, 6);
                card_body.set_valign(gtk::Align::Start);

                let top_line = gtk::Box::new(Orientation::Horizontal, 8);
                let badge = gtk::Label::new(None);
                badge.add_css_class(if is_smart {
                    "dj-badge-smart"
                } else {
                    "dj-badge-random"
                });
                i18n::bind_label(&badge, move |l| {
                    if is_smart {
                        i18n::pick_for(l, "SMART", "INTELIGENTE")
                    } else {
                        i18n::pick_for(l, "RANDOM", "ALEATÓRIO")
                    }
                });
                let name = gtk::Label::new(None);
                name.add_css_class("playlist-card-title");
                name.set_xalign(0.0);
                name.set_hexpand(true);
                i18n::bind_label(&name, move |l| mode.label(l));
                top_line.append(&name);
                top_line.append(&badge);

                let desc = gtk::Label::new(None);
                desc.add_css_class("donate-desc");
                desc.set_wrap(true);
                desc.set_xalign(0.0);
                desc.set_valign(gtk::Align::Start);
                i18n::bind_label(&desc, move |l| mode.description(l));

                card_body.append(&top_line);
                card_body.append(&desc);

                if mode == DjMixMode::SmartRadio {
                    let seed_btn = gtk::Button::new();
                    seed_btn.add_css_class("queue-clear");
                    seed_btn.set_halign(gtk::Align::Start);
                    i18n::bind_button_label(&seed_btn, |l| {
                        i18n::pick_for(l, "Set Current as Seed", "Usar Faixa Atual como Semente")
                    });
                    let c_seed = ctrl.clone();
                    seed_btn.connect_clicked(move |_| {
                        DjController::set_current_as_seed(&c_seed);
                    });
                    card_body.append(&seed_btn);
                }

                card.set_child(Some(&card_body));
                row.append(&card);
                mode_cards.borrow_mut().push((mode, card));
            }
            sec_box.append(&row);
            sec_box
        };

    root.append(&make_section(
        "RANDOM MIX MODES",
        "MODOS DE MIX ALEATÓRIOS",
        &DjMixMode::RANDOM_MODES,
        false,
    ));
    root.append(&make_section(
        "INTELLIGENT MIX MODES",
        "MODOS DE MIX INTELIGENTES",
        &DjMixMode::SMART_MODES,
        true,
    ));

    // Wire mode card clicks cleanly with guard flag against programmatic updates
    let updating_ui = Rc::new(Cell::new(false));
    for (mode, card) in mode_cards.borrow().iter() {
        let m = *mode;
        let c_mode = ctrl.clone();
        let upd = updating_ui.clone();
        let cards_ref = mode_cards.clone();
        card.connect_toggled(move |btn| {
            if upd.get() {
                return;
            }
            if btn.is_active() {
                DjController::set_mode(&c_mode, m);
            } else if c_mode.borrow().engine.config.mode == m {
                // Keep active mode selected if re-clicked unless toggled to another card
                upd.set(true);
                btn.set_active(true);
                upd.set(false);
                let _ = cards_ref;
            }
        });
    }

    // 4. Mixer Tuning Bar (Crossfade Duration, Track Pool, Smart Cue & Tempo Match)
    let mixer_card = gtk::Box::new(Orientation::Vertical, 9);
    mixer_card.add_css_class("donate-card");

    let row1 = gtk::Box::new(Orientation::Horizontal, 18);
    row1.set_homogeneous(false);

    // Crossfade selector
    let xfade_box = gtk::Box::new(Orientation::Vertical, 6);
    xfade_box.set_hexpand(true);
    let xfade_lbl = gtk::Label::new(None);
    i18n::bind_label(&xfade_lbl, |l| {
        i18n::pick_for(l, "Crossfade Overlap", "Duração do Crossfade")
    });
    xfade_lbl.add_css_class("system-audio-title");
    xfade_lbl.set_xalign(0.0);
    let xfade_pills = gtk::Box::new(Orientation::Horizontal, 4);
    xfade_pills.add_css_class("dj-pill-group");
    xfade_pills.set_halign(gtk::Align::Start);

    let xfade_presets: [(f64, &str); 5] = [
        (0.0, "0s"),
        (3.0, "3s"),
        (6.0, "6s"),
        (9.0, "9s"),
        (12.0, "12s"),
    ];
    let mut xfade_btns = Vec::new();
    for (secs, text) in xfade_presets {
        let b = gtk::ToggleButton::with_label(text);
        b.add_css_class("dj-pill-btn");
        let c_xf = ctrl.clone();
        let upd = updating_ui.clone();
        b.connect_toggled(move |btn| {
            if upd.get() {
                return;
            }
            if btn.is_active() {
                DjController::set_crossfade_secs(&c_xf, secs);
            }
        });
        xfade_pills.append(&b);
        xfade_btns.push((secs, b));
    }
    xfade_box.append(&xfade_lbl);
    xfade_box.append(&xfade_pills);
    row1.append(&xfade_box);

    // Track Pool selector
    let pool_box = gtk::Box::new(Orientation::Vertical, 6);
    let pool_lbl = gtk::Label::new(None);
    i18n::bind_label(&pool_lbl, |l| {
        i18n::pick_for(l, "Selection Pool", "Fonte das Faixas")
    });
    pool_lbl.add_css_class("system-audio-title");
    pool_lbl.set_xalign(0.0);

    let pool_pills = gtk::Box::new(Orientation::Horizontal, 4);
    pool_pills.add_css_class("dj-pill-group");
    pool_pills.set_halign(gtk::Align::Start);

    let pool_queue_btn = gtk::ToggleButton::new();
    pool_queue_btn.add_css_class("dj-pill-btn");
    i18n::bind_button_label(&pool_queue_btn, |l| DjPoolSource::QueueOnly.label(l));
    let pool_lib_btn = gtk::ToggleButton::new();
    pool_lib_btn.add_css_class("dj-pill-btn");
    i18n::bind_button_label(&pool_lib_btn, |l| DjPoolSource::LibraryAutoFill.label(l));

    {
        let c_pq = ctrl.clone();
        let upd = updating_ui.clone();
        pool_queue_btn.connect_toggled(move |btn| {
            if !upd.get() && btn.is_active() {
                DjController::set_pool(&c_pq, DjPoolSource::QueueOnly);
            }
        });
    }
    {
        let c_pl = ctrl.clone();
        let upd = updating_ui.clone();
        pool_lib_btn.connect_toggled(move |btn| {
            if !upd.get() && btn.is_active() {
                DjController::set_pool(&c_pl, DjPoolSource::LibraryAutoFill);
            }
        });
    }
    pool_pills.append(&pool_queue_btn);
    pool_pills.append(&pool_lib_btn);
    pool_box.append(&pool_lbl);
    pool_box.append(&pool_pills);
    row1.append(&pool_box);
    mixer_card.append(&row1);

    // Crossfade Curve row + Background Acoustic Analysis count
    let row_curve = gtk::Box::new(Orientation::Horizontal, 18);
    let curve_box = gtk::Box::new(Orientation::Vertical, 6);
    let curve_lbl = gtk::Label::new(None);
    i18n::bind_label(&curve_lbl, |l| {
        i18n::pick_for(l, "Crossfade Curve", "Curva do Crossfade")
    });
    curve_lbl.add_css_class("system-audio-title");
    curve_lbl.set_xalign(0.0);

    let curve_pills = gtk::Box::new(Orientation::Horizontal, 4);
    curve_pills.add_css_class("dj-pill-group");
    curve_pills.set_halign(gtk::Align::Start);

    let mut curve_btns = Vec::new();
    for curve in DjCrossfadeCurve::ALL {
        let b = gtk::ToggleButton::new();
        b.add_css_class("dj-pill-btn");
        i18n::bind_button_label(&b, move |l| curve.label(l));
        let c_cv = ctrl.clone();
        let upd = updating_ui.clone();
        b.connect_toggled(move |btn| {
            if !upd.get() && btn.is_active() {
                DjController::set_curve(&c_cv, curve);
            }
        });
        curve_pills.append(&b);
        curve_btns.push((curve, b));
    }
    curve_box.append(&curve_lbl);
    curve_box.append(&curve_pills);

    let analysis_lbl = gtk::Label::new(None);
    analysis_lbl.add_css_class("donate-hint");
    analysis_lbl.set_hexpand(true);
    analysis_lbl.set_xalign(1.0);
    analysis_lbl.set_valign(gtk::Align::End);

    row_curve.append(&curve_box);
    row_curve.append(&analysis_lbl);
    mixer_card.append(&row_curve);

    let row2 = gtk::Box::new(Orientation::Horizontal, 16);
    let chk_smart_cue = gtk::CheckButton::new();
    {
        let weak = chk_smart_cue.downgrade();
        i18n::connect(&chk_smart_cue, move |l| {
            if let Some(b) = weak.upgrade() {
                b.set_label(Some(i18n::pick_for(
                    l,
                    "Smart Outro Cue (skip trailing silence)",
                    "Corte Final (antecipa fade-out)",
                )));
            }
        });
    }
    {
        let c_sc = ctrl.clone();
        let upd = updating_ui.clone();
        chk_smart_cue.connect_toggled(move |btn| {
            if !upd.get() {
                DjController::set_smart_cue(&c_sc, btn.is_active());
            }
        });
    }

    let chk_trim_intro = gtk::CheckButton::new();
    {
        let weak = chk_trim_intro.downgrade();
        i18n::connect(&chk_trim_intro, move |l| {
            if let Some(b) = weak.upgrade() {
                b.set_label(Some(i18n::pick_for(
                    l,
                    "Trim Intro Silence (Deck B cue-in)",
                    "Pular Silêncio Inicial (entrada Deck B)",
                )));
            }
        });
    }
    {
        let c_ti = ctrl.clone();
        let upd = updating_ui.clone();
        chk_trim_intro.connect_toggled(move |btn| {
            if !upd.get() {
                DjController::set_trim_intro(&c_ti, btn.is_active());
            }
        });
    }

    let chk_tempo = gtk::CheckButton::new();
    {
        let weak = chk_tempo.downgrade();
        i18n::connect(&chk_tempo, move |l| {
            if let Some(b) = weak.upgrade() {
                b.set_label(Some(i18n::pick_for(
                    l,
                    "Auto Tempo Blend (±6% BPM sync)",
                    "Sync Suave de BPM (±6%)",
                )));
            }
        });
    }
    {
        let c_tm = ctrl.clone();
        let upd = updating_ui.clone();
        chk_tempo.connect_toggled(move |btn| {
            if !upd.get() {
                DjController::set_tempo_match(&c_tm, btn.is_active());
            }
        });
    }

    row2.append(&chk_smart_cue);
    row2.append(&chk_trim_intro);
    row2.append(&chk_tempo);
    mixer_card.append(&row2);
    root.append(&mixer_card);

    // 5. Up Next Setlist Preview Card
    let upnext_card = gtk::Box::new(Orientation::Vertical, 6);
    upnext_card.add_css_class("donate-card");
    upnext_card.add_css_class("dj-upnext-card");
    let upnext_hdr = gtk::Label::new(None);
    i18n::bind_label(&upnext_hdr, |l| {
        i18n::pick_for(
            l,
            "UP NEXT · SETLIST PREVIEW (CLICK TO MIX)",
            "A SEGUIR · PRÉVIA DO SET (CLIQUE PARA MIXAR)",
        )
    });
    upnext_hdr.add_css_class("sidebar-section-title");
    upnext_hdr.set_xalign(0.0);
    let upnext_list = gtk::Box::new(Orientation::Vertical, 4);
    upnext_card.append(&upnext_hdr);
    upnext_card.append(&upnext_list);
    root.append(&upnext_card);

    // Wire header actions
    {
        let c_st = ctrl.clone();
        start_btn.connect_clicked(move |_| {
            let _ = DjController::start_session(&c_st);
        });
    }
    {
        let c_rr = ctrl.clone();
        reroll_btn.connect_clicked(move |_| {
            DjController::reroll_upcoming(&c_rr);
        });
    }
    {
        let c_bs = ctrl.clone();
        queue_set_btn.connect_clicked(move |_| {
            DjController::build_queue_set(&c_bs, 20);
        });
    }
    {
        let c_mx = ctrl.clone();
        mix_now_btn.connect_clicked(move |_| {
            let _ = DjController::trigger_mix_now(&c_mx);
        });
    }

    // Reactive UI state sync
    {
        let cards_sync = mode_cards.clone();
        let upd_sync = updating_ui.clone();
        let status_weak = status_pill.downgrade();
        let sub_weak = subtitle.downgrade();
        let console_weak = console.downgrade();
        let xfade_weak: Vec<(f64, glib::WeakRef<gtk::ToggleButton>)> = xfade_btns
            .iter()
            .map(|(s, b)| (*s, b.downgrade()))
            .collect();
        let curve_weak: Vec<(DjCrossfadeCurve, glib::WeakRef<gtk::ToggleButton>)> = curve_btns
            .iter()
            .map(|(cv, b)| (*cv, b.downgrade()))
            .collect();
        let pq_weak = pool_queue_btn.downgrade();
        let pl_weak = pool_lib_btn.downgrade();
        let sc_weak = chk_smart_cue.downgrade();
        let ti_weak = chk_trim_intro.downgrade();
        let tm_weak = chk_tempo.downgrade();
        let an_weak = analysis_lbl.downgrade();
        let start_weak = start_btn.downgrade();
        let reroll_weak = reroll_btn.downgrade();
        let mix_weak = mix_now_btn.downgrade();
        let mix_lbl_weak = mix_lbl.downgrade();
        let notice_bar_weak = notice_bar.downgrade();
        let notice_lbl_weak = notice_lbl.downgrade();
        let notice_icon_weak = notice_icon.downgrade();
        let upnext_card_weak = upnext_card.downgrade();
        let upnext_list_weak = upnext_list.downgrade();
        let last_preview_sig: Rc<RefCell<(Vec<PathBuf>, Language)>> =
            Rc::new(RefCell::new((Vec::new(), Language::En)));
        let c_upnext = ctrl.clone();

        let sync_fn = move |c: &DjController, lang: Language| {
            let cfg = c.config();
            upd_sync.set(true);
            for (m, btn) in cards_sync.borrow().iter() {
                let active = *m == cfg.mode;
                btn.set_active(active);
                if active {
                    btn.add_css_class("active");
                } else {
                    btn.remove_css_class("active");
                }
            }
            for (secs, w) in &xfade_weak {
                if let Some(btn) = w.upgrade() {
                    btn.set_active((cfg.crossfade_secs - *secs).abs() < 1.4);
                }
            }
            for (cv, w) in &curve_weak {
                if let Some(btn) = w.upgrade() {
                    btn.set_active(*cv == cfg.curve);
                }
            }
            if let Some(btn) = pq_weak.upgrade() {
                btn.set_active(cfg.pool == DjPoolSource::QueueOnly);
            }
            if let Some(btn) = pl_weak.upgrade() {
                btn.set_active(cfg.pool == DjPoolSource::LibraryAutoFill);
            }
            if let Some(chk) = sc_weak.upgrade() {
                chk.set_active(cfg.smart_cue);
            }
            if let Some(chk) = ti_weak.upgrade() {
                chk.set_active(cfg.trim_intro_silence);
            }
            if let Some(chk) = tm_weak.upgrade() {
                chk.set_active(cfg.tempo_match);
            }
            upd_sync.set(false);

            if let Some(lbl) = status_weak.upgrade() {
                lbl.set_text(cfg.mode.short_label(lang));
                if cfg.mode.is_active() {
                    lbl.add_css_class("active");
                } else {
                    lbl.remove_css_class("active");
                }
            }
            if let Some(lbl) = sub_weak.upgrade() {
                lbl.set_text(cfg.mode.description(lang));
            }
            let lib_len = c.catalog.report.borrow().tracks.len();
            let q_len = c.audio.queue_snapshot().tracks.len();
            if let Some(lbl) = an_weak.upgrade() {
                let tracks = &c.catalog.report.borrow().tracks;
                let prof = c.engine.analyzed_count(tracks);
                lbl.set_text(&match lang {
                    Language::En => format!("{prof}/{lib_len} profiled"),
                    Language::PtBr => format!("{prof}/{lib_len} analisadas"),
                });
            }

            // Notice bar: explicit error/info or empty library/queue warning when DJ active
            if let Some(bar) = notice_bar_weak.upgrade() {
                if let Some((ref msg_en, ref msg_pt, is_err)) = c.notice {
                    bar.set_visible(true);
                    if is_err {
                        bar.add_css_class("error");
                    } else {
                        bar.remove_css_class("error");
                    }
                    if let Some(ic) = notice_icon_weak.upgrade() {
                        ic.set_icon_name(Some(if is_err {
                            "dialog-warning-symbolic"
                        } else {
                            "dialog-information-symbolic"
                        }));
                    }
                    if let Some(l) = notice_lbl_weak.upgrade() {
                        l.set_text(match lang {
                            Language::En => msg_en,
                            Language::PtBr => msg_pt,
                        });
                    }
                } else if cfg.mode.is_active() && lib_len == 0 && q_len == 0 {
                    bar.set_visible(true);
                    bar.remove_css_class("error");
                    if let Some(ic) = notice_icon_weak.upgrade() {
                        ic.set_icon_name(Some("folder-music-symbolic"));
                    }
                    if let Some(l) = notice_lbl_weak.upgrade() {
                        l.set_text(i18n::pick_for(
                            lang,
                            "No tracks loaded yet — check that your music folders are mounted or add tracks to the queue.",
                            "Nenhuma música carregada ainda — confira se a pasta/HD de músicas está montado ou adicione faixas à fila.",
                        ));
                    }
                } else {
                    bar.set_visible(false);
                }
            }

            let is_playing = c.audio.state() == PlaybackState::Playing;
            let xf_snap = c.audio.crossfade_snapshot();
            if let Some(btn) = start_weak.upgrade() {
                btn.set_visible(!is_playing || c.audio.current_track_path().is_none());
                btn.set_sensitive(lib_len > 0 || q_len > 0);
            }
            if let Some(btn) = reroll_weak.upgrade() {
                btn.set_sensitive(cfg.mode.is_active());
            }
            if let Some(lbl) = mix_lbl_weak.upgrade() {
                lbl.set_text(if xf_snap.is_some() {
                    i18n::pick_for(lang, "Finish Mix", "Concluir Mix")
                } else {
                    i18n::pick_for(lang, "Mix Now", "Mixar Agora")
                });
            }
            if let Some(btn) = mix_weak.upgrade() {
                let has_target = c.engine.upcoming.is_some() || q_len > 0 || xf_snap.is_some();
                btn.set_sensitive(has_target);
            }

            // Up Next preview list (rebuild only when upcoming paths or language changed)
            let preview_items = c.upcoming_preview();
            let preview_paths: Vec<PathBuf> = preview_items
                .iter()
                .map(|(_, pk)| pk.path.clone())
                .collect();
            if let (Some(card), Some(list)) =
                (upnext_card_weak.upgrade(), upnext_list_weak.upgrade())
            {
                card.set_visible(!preview_items.is_empty());
                let mut sig = last_preview_sig.borrow_mut();
                if sig.0 != preview_paths || sig.1 != lang {
                    *sig = (preview_paths, lang);
                    while let Some(child) = list.first_child() {
                        list.remove(&child);
                    }
                    for (idx, (prof, pick)) in preview_items.iter().enumerate() {
                        let row = gtk::Box::new(Orientation::Horizontal, 10);
                        row.add_css_class("dj-upnext-row");

                        let num = gtk::Label::new(Some(&format!("{}.", idx + 1)));
                        num.add_css_class("donate-hint");
                        num.set_width_chars(2);

                        let meta_col = gtk::Box::new(Orientation::Vertical, 1);
                        meta_col.set_hexpand(true);
                        let t_lbl = gtk::Label::new(Some(&prof.title));
                        t_lbl.set_xalign(0.0);
                        t_lbl.set_ellipsize(gtk::pango::EllipsizeMode::End);
                        t_lbl.add_css_class("system-audio-title");
                        let sub_str = if prof.artist.is_empty() {
                            pick.reason_for(lang).to_string()
                        } else {
                            format!("{} · {}", prof.artist, pick.reason_for(lang))
                        };
                        let s_lbl = gtk::Label::new(Some(&sub_str));
                        s_lbl.set_xalign(0.0);
                        s_lbl.set_ellipsize(gtk::pango::EllipsizeMode::End);
                        s_lbl.add_css_class("donate-hint");
                        meta_col.append(&t_lbl);
                        meta_col.append(&s_lbl);

                        let spec_lbl = gtk::Label::new(None);
                        spec_lbl.add_css_class("dj-status-pill");
                        if let Some(bpm) = prof.bpm {
                            spec_lbl.set_text(&format!("{bpm} BPM"));
                        } else if let Some(pct) = pick.match_pct {
                            spec_lbl.set_text(&format!("{pct}%"));
                        } else {
                            spec_lbl.set_visible(false);
                        }

                        let play_row_btn =
                            gtk::Button::from_icon_name("media-playback-start-symbolic");
                        play_row_btn.add_css_class("queue-clear");
                        play_row_btn.set_tooltip_text(Some(i18n::pick_for(
                            lang,
                            "Mix to this track now",
                            "Mixar para esta faixa agora",
                        )));
                        let target_path = pick.path.clone();
                        let c_row = c_upnext.clone();
                        play_row_btn.connect_clicked(move |_| {
                            let _ = DjController::mix_to_path(&c_row, target_path.clone());
                        });

                        row.append(&num);
                        row.append(&meta_col);
                        row.append(&spec_lbl);
                        row.append(&play_row_btn);
                        list.append(&row);
                    }
                }
            }

            if let Some(da) = console_weak.upgrade() {
                da.queue_draw();
            }
        };

        let c_connect = ctrl.clone();
        let sync_for_ctrl = sync_fn.clone();
        DjController::connect(&c_connect, &root, move |c| {
            sync_for_ctrl(c, i18n::current());
        });

        let c_i18n = ctrl.clone();
        i18n::connect(&root, move |lang| {
            let c = c_i18n.borrow();
            sync_fn(&c, lang);
        });
    }

    scroll.set_child(Some(&root));
    scroll.upcast()
}

/// Builds the compact DJ Popover button placed in the bottom transport bar.
pub fn build_dj_bar_button(
    ctrl: &Rc<RefCell<DjController>>,
    open_booth: impl Fn() + 'static,
) -> gtk::MenuButton {
    let open_booth = Rc::new(open_booth);
    let menu_btn = gtk::MenuButton::new();
    menu_btn.add_css_class("control-button");
    menu_btn.add_css_class("dj-button");

    let inner = gtk::Box::new(Orientation::Horizontal, 4);
    let icon = gtk::Image::from_icon_name("audio-headphones-symbolic");
    let badge_lbl = gtk::Label::new(Some("DJ"));
    badge_lbl.add_css_class("dj-bar-label");
    inner.append(&icon);
    inner.append(&badge_lbl);
    menu_btn.set_child(Some(&inner));

    let pop = gtk::Popover::new();
    pop.add_css_class("dj-popover");
    let box_p = gtk::Box::new(Orientation::Vertical, 6);
    box_p.set_margin_start(6);
    box_p.set_margin_end(6);
    box_p.set_margin_top(6);
    box_p.set_margin_bottom(6);
    box_p.set_width_request(250);

    let title_p = gtk::Label::new(None);
    i18n::bind_label(&title_p, |l| {
        i18n::pick_for(l, "AUTO-DJ MIX MODES", "MODOS DE MIX AUTO-DJ")
    });
    title_p.add_css_class("sidebar-section-title");
    title_p.set_xalign(0.0);
    title_p.set_margin_bottom(2);
    box_p.append(&title_p);

    let mut mode_btns = Vec::new();
    for &mode in DjMixMode::ALL.iter() {
        let b = gtk::Button::new();
        b.add_css_class("popover-playlist-item");
        let row = gtk::Box::new(Orientation::Horizontal, 8);
        let lbl = gtk::Label::new(None);
        lbl.set_xalign(0.0);
        lbl.set_hexpand(true);
        i18n::bind_label(&lbl, move |l| mode.label(l));
        row.append(&lbl);

        if mode.is_active() {
            let tag = gtk::Label::new(None);
            tag.add_css_class(if mode.is_smart() {
                "dj-badge-smart"
            } else {
                "dj-badge-random"
            });
            i18n::bind_label(&tag, move |l| {
                if mode.is_smart() {
                    i18n::pick_for(l, "Smart", "Smart")
                } else {
                    i18n::pick_for(l, "Rnd", "Aleat.")
                }
            });
            row.append(&tag);
        }

        b.set_child(Some(&row));
        let c_m = ctrl.clone();
        let pop_w = pop.downgrade();
        b.connect_clicked(move |_| {
            DjController::set_mode(&c_m, mode);
            if let Some(p) = pop_w.upgrade() {
                p.popdown();
            }
        });
        box_p.append(&b);
        mode_btns.push((mode, b));
    }

    let sep = gtk::Separator::new(Orientation::Horizontal);
    sep.set_margin_top(4);
    sep.set_margin_bottom(4);
    box_p.append(&sep);

    let actions_row = gtk::Box::new(Orientation::Horizontal, 6);
    let mix_btn = gtk::Button::new();
    mix_btn.add_css_class("queue-clear");
    mix_btn.set_hexpand(true);
    i18n::bind_button_label(&mix_btn, |l| i18n::pick_for(l, "Mix Now", "Mixar Agora"));
    {
        let c_mx = ctrl.clone();
        let pop_w = pop.downgrade();
        let open_on_err = open_booth.clone();
        mix_btn.connect_clicked(move |_| {
            let res = DjController::trigger_mix_now(&c_mx);
            if let Some(p) = pop_w.upgrade() {
                p.popdown();
            }
            if res.is_err() {
                open_on_err();
            }
        });
    }

    let open_btn = gtk::Button::new();
    open_btn.add_css_class("play-album-button");
    open_btn.set_hexpand(true);
    i18n::bind_button_label(&open_btn, |l| {
        i18n::pick_for(l, "DJ Booth · Alt+5", "Cabine DJ · Alt+5")
    });
    {
        let pop_w = pop.downgrade();
        open_btn.connect_clicked(move |_| {
            if let Some(p) = pop_w.upgrade() {
                p.popdown();
            }
            open_booth();
        });
    }
    actions_row.append(&mix_btn);
    actions_row.append(&open_btn);
    box_p.append(&actions_row);

    pop.set_child(Some(&box_p));
    menu_btn.set_popover(Some(&pop));

    {
        let mb_weak = menu_btn.downgrade();
        let mode_weak: Vec<(DjMixMode, glib::WeakRef<gtk::Button>)> = mode_btns
            .into_iter()
            .map(|(m, b)| (m, b.downgrade()))
            .collect();
        let update_bar = move |c: &DjController, lang: Language| {
            let cfg = c.config();
            if let Some(mb) = mb_weak.upgrade() {
                if cfg.mode.is_active() {
                    mb.add_css_class("active");
                } else {
                    mb.remove_css_class("active");
                }
                let tip = match lang {
                    Language::En => format!(
                        "Auto-DJ: {} ({:.0}s fade) · Click for Mix Modes",
                        cfg.mode.label(lang),
                        cfg.crossfade_secs
                    ),
                    Language::PtBr => format!(
                        "Auto-DJ: {} ({:.0}s fade) · Clique para Modos de Mix",
                        cfg.mode.label(lang),
                        cfg.crossfade_secs
                    ),
                };
                mb.set_tooltip_text(Some(&tip));
            }
            for (m, w) in &mode_weak {
                if let Some(b) = w.upgrade() {
                    if *m == cfg.mode {
                        b.add_css_class("active-nav");
                    } else {
                        b.remove_css_class("active-nav");
                    }
                }
            }
        };

        let ub1 = update_bar.clone();
        DjController::connect(ctrl, &menu_btn, move |c| {
            ub1(c, i18n::current());
        });
        let c_i18n = ctrl.clone();
        i18n::connect(&menu_btn, move |lang| {
            let c = c_i18n.borrow();
            update_bar(&c, lang);
        });
    }

    menu_btn
}
