//! Intelligent & random Auto-DJ engine: acoustic profiling, harmonic mixing, and track selection.

use crate::i18n::Language;
use crate::library::{self, TrackTags};
use crate::search;
use gst::prelude::*;
use gstreamer as gst;
use std::collections::{HashMap, HashSet, VecDeque};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant, UNIX_EPOCH};

const MAX_HISTORY: usize = 48;
const MAX_RECENT_ARTISTS: usize = 6;
const ACOUSTIC_CACHE_VERSION: u64 = 1;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum DjCrossfadeCurve {
    #[default]
    EqualPower,
    LongFade,
    HardCut,
}

impl DjCrossfadeCurve {
    pub const ALL: [DjCrossfadeCurve; 3] = [
        DjCrossfadeCurve::EqualPower,
        DjCrossfadeCurve::LongFade,
        DjCrossfadeCurve::HardCut,
    ];

    pub fn id(self) -> &'static str {
        match self {
            DjCrossfadeCurve::EqualPower => "equal_power",
            DjCrossfadeCurve::LongFade => "long_fade",
            DjCrossfadeCurve::HardCut => "hard_cut",
        }
    }

    pub fn from_id(s: &str) -> Self {
        match s.trim().to_ascii_lowercase().as_str() {
            "long_fade" | "smooth" | "long" => DjCrossfadeCurve::LongFade,
            "hard_cut" | "cut" | "sharp" => DjCrossfadeCurve::HardCut,
            _ => DjCrossfadeCurve::EqualPower,
        }
    }

    pub fn label(self, lang: Language) -> &'static str {
        match (lang, self) {
            (Language::En, DjCrossfadeCurve::EqualPower) => "Equal-Power",
            (Language::PtBr, DjCrossfadeCurve::EqualPower) => "Equal-Power",
            (Language::En, DjCrossfadeCurve::LongFade) => "Long Smooth",
            (Language::PtBr, DjCrossfadeCurve::LongFade) => "Suave Longo",
            (Language::En, DjCrossfadeCurve::HardCut) => "Sharp Drop",
            (Language::PtBr, DjCrossfadeCurve::HardCut) => "Corte Rápido",
        }
    }

    /// Computes `(outgoing_gain, incoming_gain)` in `[0.0..=1.0]` for normalized crossfader progress `t`.
    pub fn gains(self, t: f64) -> (f64, f64) {
        let clamped = t.clamp(0.0, 1.0);
        match self {
            DjCrossfadeCurve::EqualPower => {
                let angle = clamped * std::f64::consts::FRAC_PI_2;
                (angle.cos(), angle.sin())
            }
            DjCrossfadeCurve::LongFade => {
                // Smoothstep envelope with generous middle overlap for warm, unhurried transitions.
                let smooth = clamped * clamped * (3.0 - 2.0 * clamped);
                ((1.0 - smooth).powf(0.65), smooth.powf(0.65))
            }
            DjCrossfadeCurve::HardCut => {
                // Club slam: incoming punches in swiftly while outgoing holds until the final drop.
                let g_in = (clamped / 0.28).min(1.0).powf(0.70);
                let g_out = if clamped < 0.72 {
                    1.0 - 0.12 * (clamped / 0.72)
                } else {
                    0.88 * (1.0 - ((clamped - 0.72) / 0.28)).powi(2)
                };
                (g_out.clamp(0.0, 1.0), g_in.clamp(0.0, 1.0))
            }
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum DjMixMode {
    #[default]
    Off,
    RandomFlow,
    RandomParty,
    SmartFlow,
    SmartRadio,
    SmartJourney,
}

impl DjMixMode {
    pub const ALL: [DjMixMode; 6] = [
        DjMixMode::Off,
        DjMixMode::RandomFlow,
        DjMixMode::RandomParty,
        DjMixMode::SmartFlow,
        DjMixMode::SmartRadio,
        DjMixMode::SmartJourney,
    ];

    pub const RANDOM_MODES: [DjMixMode; 2] = [DjMixMode::RandomFlow, DjMixMode::RandomParty];

    pub const SMART_MODES: [DjMixMode; 3] = [
        DjMixMode::SmartFlow,
        DjMixMode::SmartRadio,
        DjMixMode::SmartJourney,
    ];

    pub fn id(self) -> &'static str {
        match self {
            DjMixMode::Off => "off",
            DjMixMode::RandomFlow => "random_flow",
            DjMixMode::RandomParty => "random_party",
            DjMixMode::SmartFlow => "smart_flow",
            DjMixMode::SmartRadio => "smart_radio",
            DjMixMode::SmartJourney => "smart_journey",
        }
    }

    pub fn from_id(s: &str) -> Self {
        match s.trim().to_ascii_lowercase().as_str() {
            "random_flow" | "random" => DjMixMode::RandomFlow,
            "random_party" | "party" => DjMixMode::RandomParty,
            "smart_flow" | "smart" => DjMixMode::SmartFlow,
            "smart_radio" | "radio" => DjMixMode::SmartRadio,
            "smart_journey" | "journey" => DjMixMode::SmartJourney,
            _ => DjMixMode::Off,
        }
    }

    pub fn is_active(self) -> bool {
        self != DjMixMode::Off
    }

    #[allow(dead_code)]
    pub fn is_random(self) -> bool {
        matches!(self, DjMixMode::RandomFlow | DjMixMode::RandomParty)
    }

    pub fn is_smart(self) -> bool {
        matches!(
            self,
            DjMixMode::SmartFlow | DjMixMode::SmartRadio | DjMixMode::SmartJourney
        )
    }

    pub fn label(self, lang: Language) -> &'static str {
        match (lang, self) {
            (Language::En, DjMixMode::Off) => "Off (Standard)",
            (Language::PtBr, DjMixMode::Off) => "Desligado (Padrão)",
            (Language::En, DjMixMode::RandomFlow) => "Random Flow",
            (Language::PtBr, DjMixMode::RandomFlow) => "Aleatório Contínuo",
            (Language::En, DjMixMode::RandomParty) => "Party Chaos",
            (Language::PtBr, DjMixMode::RandomParty) => "Festa Aleatória",
            (Language::En, DjMixMode::SmartFlow) => "Smart Flow",
            (Language::PtBr, DjMixMode::SmartFlow) => "Mix Inteligente",
            (Language::En, DjMixMode::SmartRadio) => "Seed Radio",
            (Language::PtBr, DjMixMode::SmartRadio) => "Rádio da Faixa",
            (Language::En, DjMixMode::SmartJourney) => "Energy Journey",
            (Language::PtBr, DjMixMode::SmartJourney) => "Jornada de Energia",
        }
    }

    pub fn short_label(self, lang: Language) -> &'static str {
        match (lang, self) {
            (Language::En, DjMixMode::Off) => "OFF",
            (Language::PtBr, DjMixMode::Off) => "DESL.",
            (_, DjMixMode::RandomFlow) => "RANDOM FLOW",
            (Language::En, DjMixMode::RandomParty) => "PARTY MIX",
            (Language::PtBr, DjMixMode::RandomParty) => "FESTA MIX",
            (Language::En, DjMixMode::SmartFlow) => "SMART FLOW",
            (Language::PtBr, DjMixMode::SmartFlow) => "SMART MIX",
            (Language::En, DjMixMode::SmartRadio) => "SEED RADIO",
            (Language::PtBr, DjMixMode::SmartRadio) => "RÁDIO DJ",
            (Language::En, DjMixMode::SmartJourney) => "JOURNEY",
            (Language::PtBr, DjMixMode::SmartJourney) => "JORNADA",
        }
    }

    pub fn description(self, lang: Language) -> &'static str {
        match (lang, self) {
            (Language::En, DjMixMode::Off) => {
                "Pure audiophile playback without DJ mixing or automatic selection."
            }
            (Language::PtBr, DjMixMode::Off) => {
                "Reprodução audiófila pura, sem mixagem de DJ ou seleção automática."
            }
            (Language::En, DjMixMode::RandomFlow) => {
                "Endless anti-repeat shuffle avoiding back-to-back artists with smooth crossfades."
            }
            (Language::PtBr, DjMixMode::RandomFlow) => {
                "Embaralhamento contínuo anti-repetição evitando artistas seguidos com crossfade suave."
            }
            (Language::En, DjMixMode::RandomParty) => {
                "Upbeat party rotation prioritizing energetic tracks and maximum variety."
            }
            (Language::PtBr, DjMixMode::RandomParty) => {
                "Rotação animada de festa priorizando faixas enérgicas e máxima variedade."
            }
            (Language::En, DjMixMode::SmartFlow) => {
                "Harmonic & vibe matching using Genre, BPM, RMS energy, Camelot key, and era."
            }
            (Language::PtBr, DjMixMode::SmartFlow) => {
                "Mixagem harmônica por Gênero, BPM, energia RMS, tonalidade Camelot e época."
            }
            (Language::En, DjMixMode::SmartRadio) => {
                "Uses the seed track as musical DNA to curate matching tracks across your library."
            }
            (Language::PtBr, DjMixMode::SmartRadio) => {
                "Usa a faixa semente como referência musical para curar músicas afins na biblioteca."
            }
            (Language::En, DjMixMode::SmartJourney) => {
                "Dynamic DJ set arc flowing automatically through Warm-Up, Peak Time, and Chill."
            }
            (Language::PtBr, DjMixMode::SmartJourney) => {
                "Arco progressivo de set alternando entre Aquecimento, Pico da Pista e Chill."
            }
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum DjPoolSource {
    QueueOnly,
    #[default]
    LibraryAutoFill,
}

impl DjPoolSource {
    pub fn id(self) -> &'static str {
        match self {
            DjPoolSource::QueueOnly => "queue",
            DjPoolSource::LibraryAutoFill => "library",
        }
    }

    pub fn from_id(s: &str) -> Self {
        match s.trim().to_ascii_lowercase().as_str() {
            "queue" => DjPoolSource::QueueOnly,
            _ => DjPoolSource::LibraryAutoFill,
        }
    }

    pub fn label(self, lang: Language) -> &'static str {
        match (lang, self) {
            (Language::En, DjPoolSource::QueueOnly) => "Current Queue Only",
            (Language::PtBr, DjPoolSource::QueueOnly) => "Apenas Fila Atual",
            (Language::En, DjPoolSource::LibraryAutoFill) => "Full Library + Auto-Fill",
            (Language::PtBr, DjPoolSource::LibraryAutoFill) => "Biblioteca Inteira + Auto-Fill",
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct DjConfig {
    pub mode: DjMixMode,
    pub pool: DjPoolSource,
    pub crossfade_secs: f64,
    pub curve: DjCrossfadeCurve,
    pub smart_cue: bool,
    pub trim_intro_silence: bool,
    pub tempo_match: bool,
}

impl Default for DjConfig {
    fn default() -> Self {
        Self {
            mode: DjMixMode::Off,
            pool: DjPoolSource::LibraryAutoFill,
            crossfade_secs: 6.0,
            curve: DjCrossfadeCurve::EqualPower,
            smart_cue: true,
            trim_intro_silence: true,
            tempo_match: false,
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct AcousticAnalysis {
    pub stamp: String,
    pub bpm: Option<u32>,
    pub energy: Option<f64>,
    pub genre: Option<String>,
    pub camelot: Option<(u8, bool)>,
    pub duration_secs: Option<f64>,
    pub intro_silence_ms: Option<u32>,
}

#[derive(Clone, Debug, Default)]
pub struct TrackProfile {
    pub path: PathBuf,
    pub title: String,
    pub artist: String,
    pub album: String,
    pub genre: Option<String>,
    pub year: Option<u32>,
    pub bpm: Option<u32>,
    pub energy: Option<f64>,
    pub camelot: Option<(u8, bool)>,
    pub folder_hint: String,
}

#[derive(Clone, Debug, PartialEq)]
pub struct DjPick {
    pub path: PathBuf,
    pub queue_index: Option<usize>,
    pub score: f64,
    pub match_pct: Option<u8>,
    pub reason_en: String,
    pub reason_pt: String,
    pub target_rate: Option<f64>,
    pub journey_phase: Option<(&'static str, &'static str)>,
}

impl DjPick {
    pub fn reason_for(&self, lang: Language) -> &str {
        match lang {
            Language::En => &self.reason_en,
            Language::PtBr => &self.reason_pt,
        }
    }
}

/// Parses either Camelot notation ("8A", "11B") or classical musical keys ("Am", "F# minor", "Eb").
pub fn parse_camelot_key(raw: &str) -> Option<(u8, bool)> {
    let cleaned = raw.trim();
    if cleaned.is_empty() {
        return None;
    }
    let upper = cleaned.to_ascii_uppercase();
    // Direct Camelot check: 1A..12A or 1B..12B
    if upper.ends_with('A') || upper.ends_with('B') {
        let major = upper.ends_with('B');
        if let Ok(num) = upper[..upper.len() - 1].trim().parse::<u8>() {
            if (1..=12).contains(&num) {
                return Some((num, major));
            }
        }
    }

    let lower = cleaned.to_ascii_lowercase();
    let is_minor = lower.contains("min")
        || (lower.ends_with('m') && !lower.ends_with("maj") && !lower.ends_with("dim"));
    let note_part = lower
        .replace("minor", "")
        .replace("major", "")
        .replace("maj", "")
        .replace("min", "")
        .trim()
        .trim_end_matches('m')
        .replace('♭', "b")
        .replace('♯', "#")
        .trim()
        .to_string();

    if is_minor {
        match note_part.as_str() {
            "g#" | "ab" => Some((1, false)),
            "d#" | "eb" => Some((2, false)),
            "a#" | "bb" => Some((3, false)),
            "f" => Some((4, false)),
            "c" => Some((5, false)),
            "g" => Some((6, false)),
            "d" => Some((7, false)),
            "a" => Some((8, false)),
            "e" => Some((9, false)),
            "b" | "cb" => Some((10, false)),
            "f#" | "gb" => Some((11, false)),
            "c#" | "db" => Some((12, false)),
            _ => None,
        }
    } else {
        match note_part.as_str() {
            "b" | "cb" => Some((1, true)),
            "f#" | "gb" => Some((2, true)),
            "c#" | "db" => Some((3, true)),
            "g#" | "ab" => Some((4, true)),
            "d#" | "eb" => Some((5, true)),
            "a#" | "bb" => Some((6, true)),
            "f" => Some((7, true)),
            "c" => Some((8, true)),
            "g" => Some((9, true)),
            "d" => Some((10, true)),
            "a" => Some((11, true)),
            "e" => Some((12, true)),
            _ => None,
        }
    }
}

pub fn format_camelot(key: (u8, bool)) -> String {
    format!("{}{}", key.0, if key.1 { "B" } else { "A" })
}

/// Returns harmonic compatibility score in [0.0..=1.0] using the Camelot wheel.
pub fn camelot_compatibility(a: (u8, bool), b: (u8, bool)) -> f64 {
    if a == b {
        return 1.0;
    }
    let diff = ((a.0 as i8 - b.0 as i8).rem_euclid(12)) as u8;
    let ring_dist = diff.min(12 - diff);
    if a.1 == b.1 {
        match ring_dist {
            1 => 0.90,
            2 => 0.65,
            5 => 0.55,
            _ => 0.20,
        }
    } else if ring_dist == 0 {
        0.85 // relative major/minor (e.g., 8A <-> 8B)
    } else if ring_dist == 1 {
        0.50
    } else {
        0.15
    }
}

pub fn splitmix64(seed: u64) -> u64 {
    let mut z = seed.wrapping_add(0x9E3779B97F4A7C15);
    z = (z ^ (z >> 30)).wrapping_mul(0xBF58476D1CE4E5B9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94D049BB133111EB);
    z ^ (z >> 31)
}

/// Estimates track energy [0.05..1.0] and approximate BPM [62..188] from a 50Hz RMS dB envelope.
pub fn estimate_energy_and_bpm_from_rms_db(
    samples_db: &[f64],
    interval_ms: f64,
) -> (Option<f64>, Option<u32>) {
    let valid_db: Vec<f64> = samples_db
        .iter()
        .copied()
        .filter(|db| db.is_finite() && *db > -75.0 && *db <= 6.0)
        .collect();
    if valid_db.len() < 10 {
        return (None, None);
    }

    let audible: Vec<f64> = valid_db.iter().copied().filter(|db| *db > -55.0).collect();
    let ref_slice = if audible.len() >= 10 {
        &audible
    } else {
        &valid_db
    };
    let mean_db = ref_slice.iter().sum::<f64>() / ref_slice.len() as f64;
    let energy = ((mean_db + 30.0) / 22.0).clamp(0.05, 1.0);

    if samples_db.len() < 80 || interval_ms <= 0.0 {
        return (Some(energy), None);
    }

    let amps: Vec<f64> = samples_db
        .iter()
        .map(|db| {
            if !db.is_finite() || *db < -70.0 {
                0.0
            } else {
                10.0_f64.powf((*db).clamp(-70.0, 0.0) / 20.0)
            }
        })
        .collect();

    let mut onsets = vec![0.0_f64; amps.len()];
    for i in 1..amps.len() {
        onsets[i] = (amps[i] - amps[i - 1]).max(0.0);
    }

    let onset_energy: f64 = onsets.iter().map(|v| v * v).sum();
    if onset_energy <= 1e-9 {
        return (Some(energy), None);
    }

    let step_sec = interval_ms / 1000.0;
    let min_lag = ((60.0 / 185.0) / step_sec).round().max(3.0) as usize;
    let max_lag = ((60.0 / 63.0) / step_sec).round() as usize;
    if max_lag <= min_lag || max_lag >= onsets.len() / 2 {
        return (Some(energy), None);
    }

    let mut best_lag = min_lag;
    let mut best_score = 0.0_f64;
    for lag in min_lag..=max_lag {
        let mut corr = 0.0_f64;
        let count = onsets.len() - lag;
        for i in 0..count {
            corr += onsets[i] * onsets[i + lag];
        }
        // Check octave reinforcement (double period) to reduce false fast-subdivision peaks.
        if lag * 2 < onsets.len() {
            let mut sub_corr = 0.0_f64;
            let count2 = onsets.len() - lag * 2;
            for i in 0..count2 {
                sub_corr += onsets[i] * onsets[i + lag * 2];
            }
            corr += 0.35 * sub_corr;
        }
        let bpm_cand = 60.0 / (lag as f64 * step_sec);
        let octave_prior = if (82.0..=148.0).contains(&bpm_cand) {
            1.15
        } else {
            1.0
        };
        let norm = (corr / (count as f64)) * octave_prior;
        if norm > best_score {
            best_score = norm;
            best_lag = lag;
        }
    }

    if best_score <= 1e-10 {
        return (Some(energy), None);
    }

    let mut bpm = 60.0 / (best_lag as f64 * step_sec);
    while bpm < 70.0 {
        bpm *= 2.0;
    }
    while bpm > 175.0 {
        bpm *= 0.5;
    }
    (Some(energy), Some(bpm.round() as u32))
}

pub fn file_stamp(path: &Path) -> Option<String> {
    let m = std::fs::metadata(path).ok()?;
    let secs = m
        .modified()
        .ok()
        .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
        .map(|d| d.as_secs())
        .unwrap_or(0);
    Some(format!("{}_{}_{}", ACOUSTIC_CACHE_VERSION, m.len(), secs))
}

/// Fast, silent GStreamer acoustic analyzer (20s middle slice at CPU speed via fakesink sync=false).
pub fn analyze_track_acoustics_gst(path: &Path) -> Option<AcousticAnalysis> {
    if !path.is_file() {
        return None;
    }
    let stamp = file_stamp(path).unwrap_or_default();
    let _ = gst::init();
    let playbin = gst::ElementFactory::make("playbin3").build().ok()?;
    let bin = gst::Bin::new();
    let convert = gst::ElementFactory::make("audioconvert").build().ok()?;
    let resample = gst::ElementFactory::make("audioresample").build().ok()?;
    let caps = gst::Caps::builder("audio/x-raw")
        .field("channels", 1i32)
        .field("rate", 8000i32)
        .build();
    let filter = gst::ElementFactory::make("capsfilter")
        .property("caps", &caps)
        .build()
        .ok()?;
    let level = gst::ElementFactory::make("level")
        .property("interval", 20_000_000_u64)
        .property("post-messages", true)
        .build()
        .ok()?;
    let fakesink = gst::ElementFactory::make("fakesink")
        .property("sync", false)
        .build()
        .ok()?;

    bin.add_many([&convert, &resample, &filter, &level, &fakesink])
        .ok()?;
    gst::Element::link_many([&convert, &resample, &filter, &level, &fakesink]).ok()?;
    let sink_pad = convert.static_pad("sink")?;
    let ghost = gst::GhostPad::builder_with_target(&sink_pad)
        .ok()?
        .name("sink")
        .build();
    bin.add_pad(&ghost).ok()?;
    playbin.set_property("audio-sink", &bin);
    let uri = glib::filename_to_uri(path, None).ok()?;
    playbin.set_property("uri", &uri);

    if playbin.set_state(gst::State::Paused).is_err() {
        let _ = playbin.set_state(gst::State::Null);
        return None;
    }
    let bus = playbin.bus()?;

    let mut duration_secs = None;
    let mut tag_genre: Option<String> = None;
    let mut tag_bpm: Option<u32> = None;
    let mut tag_key: Option<String> = None;

    let preroll_deadline = Instant::now() + Duration::from_millis(900);
    while Instant::now() < preroll_deadline {
        if let Some(msg) = bus.timed_pop(gst::ClockTime::from_mseconds(40)) {
            match msg.view() {
                gst::MessageView::Tag(t) => {
                    collect_gst_dj_tags(&t.tags(), &mut tag_genre, &mut tag_bpm, &mut tag_key)
                }
                gst::MessageView::AsyncDone(_) => {
                    if let Some(dur) = playbin.query_duration::<gst::ClockTime>() {
                        duration_secs = Some(dur.nseconds() as f64 / 1_000_000_000.0);
                    }
                    break;
                }
                gst::MessageView::Error(_) => {
                    let _ = playbin.set_state(gst::State::Null);
                    return None;
                }
                _ => {}
            }
        }
    }

    if playbin.set_state(gst::State::Playing).is_err() {
        let _ = playbin.set_state(gst::State::Null);
        return None;
    }

    // Phase 1: measure leading silence from 0s (with CPU-speed fakesink sync=false).
    // Stops as soon as 2 consecutive frames rise above -45 dBFS (or after 14s max).
    let mut leading_silent_frames: u32 = 0;
    let mut consecutive_signal: u8 = 0;
    let mut intro_silence_ms: Option<u32> = None;
    let mut preseek_samples = Vec::with_capacity(64);
    let need_seek_middle = duration_secs.is_some_and(|d| d > 24.0);
    let intro_deadline = Instant::now() + Duration::from_millis(650);
    while Instant::now() < intro_deadline && leading_silent_frames < 700 {
        let Some(msg) = bus.timed_pop(gst::ClockTime::from_mseconds(25)) else {
            continue;
        };
        match msg.view() {
            gst::MessageView::Element(e) => {
                if let Some(s) = e.structure() {
                    if s.name() == "level" {
                        if let Ok(arr) = s.get::<glib::ValueArray>("rms") {
                            if let Some(first) = arr.as_slice().first() {
                                if let Ok(db) = first.get::<f64>() {
                                    preseek_samples.push(db);
                                    if db >= -45.0 {
                                        consecutive_signal += 1;
                                        if consecutive_signal >= 2 {
                                            let ms = leading_silent_frames.saturating_mul(20);
                                            intro_silence_ms = Some(if ms >= 500 {
                                                ms.saturating_sub(140).min(14_000)
                                            } else {
                                                0
                                            });
                                            break;
                                        }
                                    } else {
                                        consecutive_signal = 0;
                                        leading_silent_frames += 1;
                                    }
                                }
                            }
                        }
                    }
                }
            }
            gst::MessageView::Tag(t) => {
                collect_gst_dj_tags(&t.tags(), &mut tag_genre, &mut tag_bpm, &mut tag_key);
            }
            gst::MessageView::Eos(_) | gst::MessageView::Error(_) => break,
            _ => {}
        }
    }
    if intro_silence_ms.is_none() {
        let ms = leading_silent_frames.saturating_mul(20);
        intro_silence_ms = Some(if ms >= 500 {
            ms.saturating_sub(140).min(14_000)
        } else {
            0
        });
    }

    let mut samples_db = Vec::with_capacity(950);
    if need_seek_middle {
        if let Some(dur) = duration_secs {
            let start_ns = ((dur * 0.25) * 1_000_000_000.0) as u64;
            let _ = playbin.seek_simple(
                gst::SeekFlags::FLUSH | gst::SeekFlags::KEY_UNIT,
                gst::ClockTime::from_nseconds(start_ns),
            );
            // Drain any stale pre-seek async/tag messages briefly.
            while let Some(msg) = bus.pop() {
                if let gst::MessageView::Tag(t) = msg.view() {
                    collect_gst_dj_tags(&t.tags(), &mut tag_genre, &mut tag_bpm, &mut tag_key);
                }
            }
        }
    } else {
        samples_db.extend(preseek_samples);
    }

    let analyze_deadline = Instant::now() + Duration::from_millis(1350);
    while Instant::now() < analyze_deadline && samples_db.len() < 900 {
        let Some(msg) = bus.timed_pop(gst::ClockTime::from_mseconds(30)) else {
            continue;
        };
        match msg.view() {
            gst::MessageView::Element(e) => {
                if let Some(s) = e.structure() {
                    if s.name() == "level" {
                        if let Ok(arr) = s.get::<glib::ValueArray>("rms") {
                            if let Some(first) = arr.as_slice().first() {
                                if let Ok(db) = first.get::<f64>() {
                                    samples_db.push(db);
                                }
                            }
                        }
                    }
                }
            }
            gst::MessageView::Tag(t) => {
                collect_gst_dj_tags(&t.tags(), &mut tag_genre, &mut tag_bpm, &mut tag_key);
            }
            gst::MessageView::Eos(_) | gst::MessageView::Error(_) => break,
            _ => {}
        }
    }

    let _ = playbin.set_state(gst::State::Null);

    let (energy, est_bpm) = estimate_energy_and_bpm_from_rms_db(&samples_db, 20.0);
    let camelot = tag_key.as_deref().and_then(parse_camelot_key);

    Some(AcousticAnalysis {
        stamp,
        bpm: tag_bpm.or(est_bpm),
        energy,
        genre: tag_genre,
        camelot,
        duration_secs,
        intro_silence_ms,
    })
}

fn collect_gst_dj_tags(
    tags: &gst::TagList,
    genre: &mut Option<String>,
    bpm: &mut Option<u32>,
    key: &mut Option<String>,
) {
    if genre.is_none() {
        if let Some(v) = tags.get::<gst::tags::Genre>() {
            let g = v.get().trim();
            if !g.is_empty() {
                *genre = Some(g.to_string());
            }
        }
    }
    if bpm.is_none() {
        if let Some(v) = tags.get::<gst::tags::BeatsPerMinute>() {
            let val = v.get();
            if val.is_finite() && (25.0..=300.0).contains(&val) {
                *bpm = Some(val.round() as u32);
            }
        }
    }
    if key.is_none() {
        for idx in 0..tags.n_tags() {
            let Some(name) = tags.nth_tag_name(idx) else {
                continue;
            };
            let lower = name.as_str().to_ascii_lowercase();
            if matches!(lower.as_str(), "initialkey" | "initial-key" | "key") {
                if let Some(val) = tags.generic(name).and_then(|v| v.get::<String>().ok()) {
                    let k = val.trim();
                    if !k.is_empty() {
                        *key = Some(k.to_string());
                        break;
                    }
                }
            }
        }
    }
}

pub fn load_acoustic_cache_from(cache_path: &Path) -> HashMap<PathBuf, AcousticAnalysis> {
    let mut map = HashMap::new();
    let Ok(content) = std::fs::read_to_string(cache_path) else {
        return map;
    };
    let Ok(val) = serde_json::from_str::<serde_json::Value>(&content) else {
        return map;
    };
    if val.get("version").and_then(|v| v.as_u64()) != Some(ACOUSTIC_CACHE_VERSION) {
        return map;
    }
    let Some(entries) = val.get("entries").and_then(|v| v.as_object()) else {
        return map;
    };
    for (path_str, entry) in entries {
        let stamp = entry
            .get("stamp")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        let bpm = entry.get("bpm").and_then(|v| v.as_u64()).map(|n| n as u32);
        let energy = entry
            .get("energy")
            .and_then(|v| v.as_f64())
            .filter(|n| n.is_finite());
        let genre = entry
            .get("genre")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string());
        let camelot = entry
            .get("camelot")
            .and_then(|v| v.as_str())
            .and_then(parse_camelot_key);
        let duration_secs = entry
            .get("duration_secs")
            .and_then(|v| v.as_f64())
            .filter(|n| n.is_finite());
        let intro_silence_ms = entry
            .get("intro_silence_ms")
            .and_then(|v| v.as_u64())
            .map(|n| n.min(20_000) as u32);
        map.insert(
            PathBuf::from(path_str),
            AcousticAnalysis {
                stamp,
                bpm,
                energy,
                genre,
                camelot,
                duration_secs,
                intro_silence_ms,
            },
        );
    }
    map
}

pub fn save_acoustic_cache_to(cache_path: &Path, map: &HashMap<PathBuf, AcousticAnalysis>) {
    if let Some(parent) = cache_path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let mut entries = serde_json::Map::new();
    for (idx, (path, item)) in map.iter().enumerate() {
        if idx >= 20_000 {
            break;
        }
        let mut obj = serde_json::Map::new();
        obj.insert("stamp".into(), serde_json::json!(item.stamp));
        if let Some(bpm) = item.bpm {
            obj.insert("bpm".into(), serde_json::json!(bpm));
        }
        if let Some(energy) = item.energy {
            obj.insert("energy".into(), serde_json::json!(energy));
        }
        if let Some(ref g) = item.genre {
            obj.insert("genre".into(), serde_json::json!(g));
        }
        if let Some(cam) = item.camelot {
            obj.insert("camelot".into(), serde_json::json!(format_camelot(cam)));
        }
        if let Some(dur) = item.duration_secs {
            obj.insert("duration_secs".into(), serde_json::json!(dur));
        }
        if let Some(intro_ms) = item.intro_silence_ms {
            obj.insert("intro_silence_ms".into(), serde_json::json!(intro_ms));
        }
        entries.insert(
            path.to_string_lossy().to_string(),
            serde_json::Value::Object(obj),
        );
    }
    let payload = serde_json::json!({
        "version": ACOUSTIC_CACHE_VERSION,
        "entries": entries,
    });
    let Ok(json) = serde_json::to_string(&payload) else {
        return;
    };
    let tmp = cache_path.with_extension(format!("tmp.{}", std::process::id()));
    if std::fs::write(&tmp, json).is_ok() {
        let _ = std::fs::rename(tmp, cache_path);
    }
}

pub fn default_acoustic_cache_path() -> PathBuf {
    glib::user_cache_dir()
        .join("vinylplayer")
        .join("dj-acoustics-v1.json")
}

pub fn build_track_profile(
    path: &Path,
    tags: Option<&TrackTags>,
    acoustic: Option<&AcousticAnalysis>,
) -> TrackProfile {
    let owned_tags;
    let tags = match tags {
        Some(t) => Some(t),
        None => {
            owned_tags = library::read_cached_tags(path);
            owned_tags.as_ref()
        }
    };
    let stem = path
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("Track")
        .to_string();
    let album_fallback = path
        .parent()
        .and_then(|p| p.file_name())
        .and_then(|n| n.to_str())
        .unwrap_or("")
        .to_string();
    let artist_fallback = path
        .parent()
        .and_then(|p| p.parent())
        .and_then(|p| p.file_name())
        .and_then(|n| n.to_str())
        .filter(|n| !matches!(*n, "music" | "Music" | "Música" | "Albums" | "Tracks"))
        .unwrap_or("")
        .to_string();

    let folder_hint = path
        .parent()
        .and_then(|p| p.parent())
        .and_then(|p| p.parent())
        .and_then(|p| p.file_name())
        .and_then(|n| n.to_str())
        .filter(|n| !matches!(*n, "home" | "mnt" | "media" | "run"))
        .unwrap_or("")
        .to_string();

    let genre = tags
        .and_then(|t| t.genre.clone())
        .or_else(|| acoustic.and_then(|a| a.genre.clone()));
    let bpm = tags
        .and_then(|t| t.bpm)
        .or_else(|| acoustic.and_then(|a| a.bpm));
    let camelot = tags
        .and_then(|t| t.initial_key.as_deref())
        .and_then(parse_camelot_key)
        .or_else(|| acoustic.and_then(|a| a.camelot));

    TrackProfile {
        path: path.to_path_buf(),
        title: tags.and_then(|t| t.title.clone()).unwrap_or(stem),
        artist: tags
            .and_then(|t| t.artist.clone().or_else(|| t.performer.clone()))
            .unwrap_or(artist_fallback),
        album: tags.and_then(|t| t.album.clone()).unwrap_or(album_fallback),
        genre,
        year: tags.and_then(|t| t.year),
        bpm,
        energy: acoustic.and_then(|a| a.energy),
        camelot,
        folder_hint,
    }
}

fn genre_similarity(a: &TrackProfile, b: &TrackProfile) -> f64 {
    match (&a.genre, &b.genre) {
        (Some(ga), Some(gb)) => {
            let na = search::normalize_for_search(ga);
            let nb = search::normalize_for_search(gb);
            if na.is_empty() || nb.is_empty() {
                return 0.0;
            }
            if na == nb {
                return 1.0;
            }
            let ta: HashSet<&str> = na
                .split(|c: char| !c.is_alphanumeric())
                .filter(|w| w.len() >= 3)
                .collect();
            let tb: HashSet<&str> = nb
                .split(|c: char| !c.is_alphanumeric())
                .filter(|w| w.len() >= 3)
                .collect();
            let overlap = ta.intersection(&tb).count();
            if overlap > 0 {
                return (overlap as f64 / ta.len().max(tb.len()).max(1) as f64).max(0.65);
            }
            0.0
        }
        _ => {
            if !a.folder_hint.is_empty()
                && !b.folder_hint.is_empty()
                && search::normalize_for_search(&a.folder_hint)
                    == search::normalize_for_search(&b.folder_hint)
            {
                0.45
            } else {
                0.0
            }
        }
    }
}

fn bpm_proximity(bpm_a: u32, bpm_b: u32) -> f64 {
    let a = bpm_a as f64;
    let b = bpm_b as f64;
    if a <= 0.0 || b <= 0.0 {
        return 0.0;
    }
    let ratio = |x: f64, y: f64| (x - y).abs() / x.max(1.0);
    let direct = ratio(a, b);
    let half = ratio(a, b * 0.5);
    let double = ratio(a, b * 2.0);
    let best_diff = direct.min(half).min(double);
    if best_diff <= 0.03 {
        1.0
    } else if best_diff >= 0.25 {
        0.0
    } else {
        1.0 - (best_diff - 0.03) / 0.22
    }
}

fn soft_tempo_match_rate(cur_bpm: Option<u32>, next_bpm: Option<u32>) -> Option<f64> {
    let (a, b) = (cur_bpm? as f64, next_bpm? as f64);
    if a <= 0.0 || b <= 0.0 {
        return None;
    }
    let mut target_b = b;
    if (a - b * 2.0).abs() < (a - b).abs() {
        target_b = b * 2.0;
    } else if (a - b * 0.5).abs() < (a - b).abs() {
        target_b = b * 0.5;
    }
    let ratio = a / target_b;
    if (ratio - 1.0).abs() <= 0.06 {
        Some(ratio.clamp(0.94, 1.06))
    } else {
        None
    }
}

pub fn journey_target_for_step(step: usize) -> (f64, &'static str, &'static str) {
    // 10-track wave: Warm-up (0..2), Peak Time (3..6), Cooldown / Chill (7..9)
    let phase = step % 10;
    match phase {
        0 => (0.42, "Warm-up", "Aquecimento"),
        1 => (0.50, "Warm-up", "Aquecimento"),
        2 => (0.62, "Build-up", "Subida"),
        3 => (0.76, "Peak Time", "Pico da Pista"),
        4 => (0.86, "Peak Time", "Pico da Pista"),
        5 => (0.90, "Peak Time", "Pico da Pista"),
        6 => (0.80, "Peak Sustain", "Clímax"),
        7 => (0.64, "Groove Transition", "Transição"),
        8 => (0.48, "Cooldown", "Desaceleração"),
        _ => (0.36, "Afterhours Chill", "Chillout"),
    }
}

pub struct DjEngine {
    pub config: DjConfig,
    pub seed_path: Option<PathBuf>,
    pub history: VecDeque<PathBuf>,
    pub recent_artists: VecDeque<String>,
    pub journey_step: usize,
    pub upcoming: Option<DjPick>,
    pub analysis_cache: HashMap<PathBuf, AcousticAnalysis>,
    pub analyzing_paths: HashSet<PathBuf>,
    pub analyze_cursor: usize,
    pub cache_path: Option<PathBuf>,
    pub cache_dirty: bool,
    pub roll_nonce: u64,
}

impl Default for DjEngine {
    fn default() -> Self {
        Self::new(DjConfig::default(), Some(default_acoustic_cache_path()))
    }
}

impl DjEngine {
    pub fn new(config: DjConfig, cache_path: Option<PathBuf>) -> Self {
        let analysis_cache = cache_path
            .as_deref()
            .map(load_acoustic_cache_from)
            .unwrap_or_default();
        Self {
            config,
            seed_path: None,
            history: VecDeque::with_capacity(MAX_HISTORY),
            recent_artists: VecDeque::with_capacity(MAX_RECENT_ARTISTS),
            journey_step: 0,
            upcoming: None,
            analysis_cache,
            analyzing_paths: HashSet::new(),
            analyze_cursor: 0,
            cache_path,
            cache_dirty: false,
            roll_nonce: 1,
        }
    }

    pub fn flush_cache_if_dirty(&mut self) {
        if !self.cache_dirty {
            return;
        }
        if let Some(ref p) = self.cache_path {
            save_acoustic_cache_to(p, &self.analysis_cache);
            self.cache_dirty = false;
        }
    }

    pub fn record_played(&mut self, path: &Path) {
        self.history.retain(|p| p != path);
        self.history.push_back(path.to_path_buf());
        while self.history.len() > MAX_HISTORY {
            self.history.pop_front();
        }
        let prof = self.profile_for(path);
        let norm_artist = search::normalize_for_search(&prof.artist);
        if !norm_artist.is_empty() {
            self.recent_artists.retain(|a| a != &norm_artist);
            self.recent_artists.push_back(norm_artist);
            while self.recent_artists.len() > MAX_RECENT_ARTISTS {
                self.recent_artists.pop_front();
            }
        }
        if self.config.mode == DjMixMode::SmartJourney {
            self.journey_step = self.journey_step.saturating_add(1);
        }
    }

    pub fn intro_cue_us_for(&self, path: &Path) -> Option<i64> {
        if !self.config.trim_intro_silence {
            return None;
        }
        let ms = self.analysis_cache.get(path)?.intro_silence_ms?;
        (ms >= 500).then_some((ms as i64) * 1000)
    }

    pub fn insert_analysis(&mut self, path: PathBuf, analysis: AcousticAnalysis) {
        self.analyzing_paths.remove(&path);
        self.analysis_cache.insert(path, analysis);
        self.cache_dirty = true;
    }

    pub fn profile_for(&self, path: &Path) -> TrackProfile {
        build_track_profile(path, None, self.analysis_cache.get(path))
    }

    pub fn analyzed_count(&self, tracks: &[PathBuf]) -> usize {
        tracks
            .iter()
            .filter(|p| self.analysis_cache.contains_key(p.as_path()))
            .count()
    }

    /// Selects unanalyzed candidates across the entire library (prioritizing current & upcoming tracks).
    pub fn next_paths_to_analyze(
        &mut self,
        current: Option<&Path>,
        candidates: &[PathBuf],
        max_batch: usize,
    ) -> Vec<PathBuf> {
        let mut batch = Vec::new();
        let try_priority = |p: &Path,
                            analyzing: &mut HashSet<PathBuf>,
                            cache: &HashMap<PathBuf, AcousticAnalysis>,
                            out: &mut Vec<PathBuf>|
         -> bool {
            if out.len() >= max_batch || analyzing.contains(p) || !p.is_file() {
                return out.len() < max_batch;
            }
            let needs = match cache.get(p) {
                None => true,
                Some(cached) => file_stamp(p).is_some_and(|st| st != cached.stamp),
            };
            if needs {
                analyzing.insert(p.to_path_buf());
                out.push(p.to_path_buf());
            }
            out.len() < max_batch
        };

        if let Some(cur) = current {
            if !try_priority(
                cur,
                &mut self.analyzing_paths,
                &self.analysis_cache,
                &mut batch,
            ) {
                return batch;
            }
        }
        if let Some(up_path) = self.upcoming.as_ref().map(|u| u.path.clone()) {
            if !try_priority(
                &up_path,
                &mut self.analyzing_paths,
                &self.analysis_cache,
                &mut batch,
            ) {
                return batch;
            }
        }

        let total = candidates.len();
        if total == 0 || batch.len() >= max_batch {
            return batch;
        }

        let start = self.analyze_cursor % total;
        let mut checked = 0;
        while checked < total && batch.len() < max_batch {
            let idx = (start + checked) % total;
            checked += 1;
            let p = &candidates[idx];
            if self.analyzing_paths.contains(p.as_path())
                || self.analysis_cache.contains_key(p.as_path())
            {
                continue;
            }
            if p.is_file() {
                self.analyzing_paths.insert(p.clone());
                batch.push(p.clone());
            }
        }
        self.analyze_cursor = (start + checked) % total;
        batch
    }

    /// Computes candidate sample paths (bounded for instant response even on 10k-track libraries).
    pub fn gather_candidates(
        &self,
        current_path: Option<&Path>,
        queue_tracks: &[PathBuf],
        library_tracks: &[PathBuf],
        exclude_paths: &[PathBuf],
        salt: u64,
    ) -> Vec<(PathBuf, Option<usize>)> {
        let mut seen = HashSet::new();
        if let Some(cur) = current_path {
            seen.insert(cur.to_path_buf());
        }
        for ex in exclude_paths {
            seen.insert(ex.clone());
        }

        let mut candidates = Vec::new();
        for (idx, p) in queue_tracks.iter().enumerate() {
            if seen.insert(p.clone()) {
                candidates.push((p.clone(), Some(idx)));
            }
        }

        if self.config.pool == DjPoolSource::LibraryAutoFill && !library_tracks.is_empty() {
            let mut pool_only: Vec<&PathBuf> = library_tracks
                .iter()
                .filter(|p| !seen.contains(*p))
                .collect();

            if pool_only.len() <= 140 {
                for p in pool_only {
                    seen.insert(p.clone());
                    candidates.push((p.clone(), None));
                }
            } else {
                // Prioritize tracks already profiled/cached plus rotated random slices for full coverage.
                let mut cached_slice = Vec::new();
                for p in &pool_only {
                    if self.analysis_cache.contains_key(p.as_path()) {
                        cached_slice.push(*p);
                    }
                }
                let start = (splitmix64(salt) as usize) % pool_only.len();
                let step = 1 + ((pool_only.len() / 80).max(1) % 7);
                let mut sampled = 0;
                for offset in 0..pool_only.len() {
                    if sampled >= 80 {
                        break;
                    }
                    let idx = (start + offset * step) % pool_only.len();
                    let p = pool_only[idx];
                    if seen.insert(p.clone()) {
                        candidates.push((p.clone(), None));
                        sampled += 1;
                    }
                }
                for p in cached_slice.into_iter().take(60) {
                    if seen.insert(p.clone()) {
                        candidates.push((p.clone(), None));
                    }
                }
                pool_only.clear();
            }
        }

        // If everything was filtered out (e.g. tiny 2-track queue), allow non-current tracks anyway.
        if candidates.is_empty() {
            for (idx, p) in queue_tracks.iter().enumerate() {
                if current_path.is_none_or(|cur| cur != p.as_path()) {
                    candidates.push((p.clone(), Some(idx)));
                }
            }
        }
        candidates
    }

    pub fn pick_next_with_profiles(
        &self,
        current_profile: Option<&TrackProfile>,
        seed_profile: Option<&TrackProfile>,
        candidates: &[(TrackProfile, Option<usize>)],
        salt: u64,
    ) -> Option<DjPick> {
        if !self.config.mode.is_active() || candidates.is_empty() {
            return None;
        }

        let recent_window = self.history.len().min(MAX_HISTORY);
        let is_recent = |path: &Path| {
            self.history
                .iter()
                .rev()
                .take(recent_window)
                .any(|p| p == path)
        };

        // Filter out recent tracks if non-recent options exist.
        let non_recent_count = candidates
            .iter()
            .filter(|(p, _)| !is_recent(&p.path))
            .count();
        let active_candidates: Vec<&(TrackProfile, Option<usize>)> = if non_recent_count > 0 {
            candidates
                .iter()
                .filter(|(p, _)| !is_recent(&p.path))
                .collect()
        } else {
            candidates.iter().collect()
        };

        let cur_artist = current_profile
            .map(|c| search::normalize_for_search(&c.artist))
            .unwrap_or_default();
        let cur_album = current_profile
            .map(|c| search::normalize_for_search(&c.album))
            .unwrap_or_default();
        let seed_or_cur = seed_profile.or(current_profile);

        match self.config.mode {
            DjMixMode::Off => None,
            DjMixMode::RandomFlow | DjMixMode::RandomParty => {
                let is_party = self.config.mode == DjMixMode::RandomParty;
                let weighted: Vec<(&(TrackProfile, Option<usize>), f64)> = active_candidates
                    .into_iter()
                    .map(|item| {
                        let cand = &item.0;
                        let mut w = 1.0_f64;
                        let cand_artist = search::normalize_for_search(&cand.artist);
                        let same_artist = !cur_artist.is_empty() && cand_artist == cur_artist;
                        let recent_artist = !same_artist
                            && !cand_artist.is_empty()
                            && self.recent_artists.iter().any(|a| a == &cand_artist);
                        let same_album = !cur_album.is_empty()
                            && search::normalize_for_search(&cand.album) == cur_album;
                        if same_artist {
                            w *= if is_party { 0.03 } else { 0.08 };
                        } else if recent_artist {
                            w *= 0.22;
                        } else if same_album {
                            w *= 0.20;
                        }
                        if is_party {
                            if let Some(e) = cand.energy {
                                w *= 0.45 + e * 1.4;
                            }
                            if let Some(bpm) = cand.bpm {
                                if bpm >= 108 {
                                    w *= 1.35;
                                }
                            }
                        }
                        (item, w.max(0.01))
                    })
                    .collect();

                let total_weight: f64 = weighted.iter().map(|(_, w)| *w).sum();
                let roll_u64 = splitmix64(salt ^ self.roll_nonce.wrapping_mul(0x517cc1b727220a95));
                let mut pick_val = ((roll_u64 % 1_000_000) as f64 / 1_000_000.0) * total_weight;
                let mut chosen = &weighted[0];
                for entry in &weighted {
                    if pick_val <= entry.1 {
                        chosen = entry;
                        break;
                    }
                    pick_val -= entry.1;
                }
                let cand = &chosen.0 .0;
                let q_idx = chosen.0 .1;
                let (reason_en, reason_pt) = if is_party {
                    let extra = cand.bpm.map(|b| format!(" · {b} BPM")).unwrap_or_default();
                    (
                        format!("Party Mix · High-energy discovery{extra}"),
                        format!("Festa Aleatória · Alto astral{extra}"),
                    )
                } else {
                    (
                        "Random Flow · Anti-repeat selection".to_string(),
                        "Aleatório Contínuo · Sem repetição".to_string(),
                    )
                };
                let target_rate = if self.config.tempo_match {
                    current_profile.and_then(|c| soft_tempo_match_rate(c.bpm, cand.bpm))
                } else {
                    None
                };

                Some(DjPick {
                    path: cand.path.clone(),
                    queue_index: q_idx,
                    score: chosen.1,
                    match_pct: None,
                    reason_en,
                    reason_pt,
                    target_rate,
                    journey_phase: None,
                })
            }
            DjMixMode::SmartFlow | DjMixMode::SmartRadio | DjMixMode::SmartJourney => {
                let reference = if self.config.mode == DjMixMode::SmartRadio {
                    seed_or_cur
                } else {
                    current_profile
                };
                let (target_energy, phase_en, phase_pt) =
                    if self.config.mode == DjMixMode::SmartJourney {
                        let (te, pe, pp) = journey_target_for_step(self.journey_step);
                        (Some(te), Some(pe), Some(pp))
                    } else {
                        (None, None, None)
                    };

                type ScoredCandidate<'a> = (
                    &'a (TrackProfile, Option<usize>),
                    f64,
                    Vec<&'static str>,
                    Vec<&'static str>,
                );
                let mut scored: Vec<ScoredCandidate<'_>> = active_candidates
                    .into_iter()
                    .map(|item| {
                        let cand = &item.0;
                        let mut pts = 30.0_f64; // base compatibility
                        let mut tags_en = Vec::new();
                        let mut tags_pt = Vec::new();

                        if let Some(ref_prof) = reference {
                            let g_sim = genre_similarity(ref_prof, cand);
                            if g_sim > 0.0 {
                                pts += g_sim * 26.0;
                                if g_sim >= 0.65 {
                                    tags_en.push("Genre");
                                    tags_pt.push("Gênero");
                                }
                            }

                            if let (Some(ba), Some(bb)) = (ref_prof.bpm, cand.bpm) {
                                let b_sim = bpm_proximity(ba, bb);
                                pts += b_sim * 22.0;
                                if b_sim >= 0.72 {
                                    tags_en.push("BPM Match");
                                    tags_pt.push("BPM Próximo");
                                }
                            }

                            if let (Some(ca), Some(cb)) = (ref_prof.camelot, cand.camelot) {
                                let h_sim = camelot_compatibility(ca, cb);
                                pts += h_sim * 16.0;
                                if h_sim >= 0.85 {
                                    tags_en.push("Harmonic Key");
                                    tags_pt.push("Harmonia Tonal");
                                }
                            }

                            if let (Some(ya), Some(yb)) = (ref_prof.year, cand.year) {
                                let dy = ya.abs_diff(yb);
                                if dy <= 3 {
                                    pts += 10.0;
                                    tags_en.push("Same Era");
                                    tags_pt.push("Mesma Época");
                                } else if dy <= 8 {
                                    pts += 6.0;
                                } else if dy <= 15 {
                                    pts += 2.5;
                                }
                            }
                        }

                        if let Some(target_e) = target_energy {
                            if let Some(cand_e) = cand.energy {
                                let e_closeness = (1.0 - (cand_e - target_e).abs() * 2.2).max(0.0);
                                pts += e_closeness * 34.0;
                            } else if let Some(bpm) = cand.bpm {
                                let inferred_e = ((bpm as f64 - 65.0) / 100.0).clamp(0.15, 0.95);
                                let e_closeness =
                                    (1.0 - (inferred_e - target_e).abs() * 2.2).max(0.0);
                                pts += e_closeness * 20.0;
                            }
                        } else if let Some(ref_prof) = reference {
                            if let (Some(ea), Some(eb)) = (ref_prof.energy, cand.energy) {
                                let e_sim = (1.0 - (ea - eb).abs() * 2.0).max(0.0);
                                pts += e_sim * 22.0;
                                if e_sim >= 0.72 {
                                    tags_en.push("Vibe");
                                    tags_pt.push("Energia");
                                }
                            }
                        }

                        // Anti-monotony penalty on consecutive / recent same artist & album
                        let cand_artist = search::normalize_for_search(&cand.artist);
                        let same_artist = !cur_artist.is_empty() && cand_artist == cur_artist;
                        let recent_artist = !same_artist
                            && !cand_artist.is_empty()
                            && self.recent_artists.iter().any(|a| a == &cand_artist);
                        let same_album = !cur_album.is_empty()
                            && search::normalize_for_search(&cand.album) == cur_album;
                        if self.config.mode == DjMixMode::SmartRadio {
                            if same_artist {
                                pts -= 22.0;
                            } else if recent_artist {
                                pts -= 16.0;
                            }
                            if same_album {
                                pts -= 28.0;
                            }
                        } else {
                            if same_artist {
                                pts -= 34.0;
                            } else if recent_artist {
                                pts -= 20.0;
                            }
                            if same_album {
                                pts -= 36.0;
                            }
                        }

                        // Deterministic jitter tie-breaker so equally scored items vary naturally
                        let jitter = (splitmix64(salt ^ (cand.title.len() as u64).wrapping_mul(31))
                            % 500) as f64
                            / 100.0;
                        (item, (pts + jitter).max(5.0), tags_en, tags_pt)
                    })
                    .collect();

                scored.sort_by(|a, b| b.1.total_cmp(&a.1));

                // Weighted draw among top compatible picks (>= 72% of best score, up to 4 candidates)
                let best_score = scored[0].1;
                let threshold = best_score * 0.72;
                let top_k = scored
                    .iter()
                    .take(4)
                    .take_while(|(_, s, _, _)| *s >= threshold)
                    .count()
                    .max(1);
                let top_slice = &scored[..top_k];
                let weights: Vec<f64> = top_slice
                    .iter()
                    .map(|(_, s, _, _)| (s / 20.0).powi(3).max(0.1))
                    .collect();
                let w_sum: f64 = weights.iter().sum();
                let r =
                    ((splitmix64(salt ^ self.roll_nonce) % 1_000_000) as f64 / 1_000_000.0) * w_sum;
                let mut acc = 0.0;
                let mut chosen_idx = 0;
                for (i, w) in weights.iter().enumerate() {
                    acc += w;
                    if r <= acc {
                        chosen_idx = i;
                        break;
                    }
                }

                let (item, raw_score, ref tags_en, ref tags_pt) = top_slice[chosen_idx];
                let cand = &item.0;
                let q_idx = item.1;
                let pct = ((raw_score / 115.0) * 100.0).round().clamp(52.0, 99.0) as u8;

                let (reason_en, reason_pt) = match self.config.mode {
                    DjMixMode::SmartRadio => {
                        let seed_title = seed_or_cur.map(|s| s.title.as_str()).unwrap_or("Seed");
                        let detail = if tags_en.is_empty() {
                            String::new()
                        } else {
                            format!(" ({})", tags_en.join(" + "))
                        };
                        let detail_pt = if tags_pt.is_empty() {
                            String::new()
                        } else {
                            format!(" ({})", tags_pt.join(" + "))
                        };
                        (
                            format!("Seed Radio {pct}% · Like \"{seed_title}\"{detail}"),
                            format!("Rádio {pct}% · Afim com \"{seed_title}\"{detail_pt}"),
                        )
                    }
                    DjMixMode::SmartJourney => {
                        let pe = phase_en.unwrap_or("Flow");
                        let pp = phase_pt.unwrap_or("Fluxo");
                        let energy_str = target_energy
                            .map(|e| format!(" · Target {:.0}%", e * 100.0))
                            .unwrap_or_default();
                        (
                            format!("Journey · {pe}{energy_str} · {pct}% fit"),
                            format!("Jornada · {pp}{energy_str} · {pct}% compatível"),
                        )
                    }
                    _ => {
                        let mut parts_en: Vec<String> =
                            tags_en.iter().map(|s| (*s).to_string()).collect();
                        let mut parts_pt: Vec<String> =
                            tags_pt.iter().map(|s| (*s).to_string()).collect();
                        if parts_en.is_empty() {
                            if let Some(ref g) = cand.genre {
                                parts_en.push(g.clone());
                                parts_pt.push(g.clone());
                            }
                            if let Some(b) = cand.bpm {
                                parts_en.push(format!("{b} BPM"));
                                parts_pt.push(format!("{b} BPM"));
                            }
                        }
                        let summary_en = if parts_en.is_empty() {
                            "Balanced Vibe".to_string()
                        } else {
                            parts_en.join(" · ")
                        };
                        let summary_pt = if parts_pt.is_empty() {
                            "Vibe Equilibrada".to_string()
                        } else {
                            parts_pt.join(" · ")
                        };
                        (
                            format!("Smart Flow {pct}% · {summary_en}"),
                            format!("Mix Inteligente {pct}% · {summary_pt}"),
                        )
                    }
                };

                let target_rate = if self.config.tempo_match {
                    current_profile.and_then(|c| soft_tempo_match_rate(c.bpm, cand.bpm))
                } else {
                    None
                };

                Some(DjPick {
                    path: cand.path.clone(),
                    queue_index: q_idx,
                    score: raw_score,
                    match_pct: Some(pct),
                    reason_en,
                    reason_pt,
                    target_rate,
                    journey_phase: phase_en.zip(phase_pt),
                })
            }
        }
    }

    pub fn pick_next(
        &self,
        current_path: Option<&Path>,
        queue_tracks: &[PathBuf],
        library_tracks: &[PathBuf],
        exclude_paths: &[PathBuf],
        salt: u64,
    ) -> Option<DjPick> {
        let cur_prof = current_path.map(|p| self.profile_for(p));
        let seed_prof = self.seed_path.as_deref().map(|p| self.profile_for(p));
        let raw_cands = self.gather_candidates(
            current_path,
            queue_tracks,
            library_tracks,
            exclude_paths,
            salt,
        );
        let profiled: Vec<(TrackProfile, Option<usize>)> = raw_cands
            .into_iter()
            .map(|(p, idx)| (self.profile_for(&p), idx))
            .collect();
        self.pick_next_with_profiles(cur_prof.as_ref(), seed_prof.as_ref(), &profiled, salt)
    }

    /// Ensures `self.upcoming` holds a valid, up-to-date next track for Deck B.
    pub fn ensure_upcoming(
        &mut self,
        current_path: Option<&Path>,
        queue_tracks: &[PathBuf],
        library_tracks: &[PathBuf],
        exclude_paths: &[PathBuf],
        force_reroll: bool,
    ) -> Option<&DjPick> {
        if !self.config.mode.is_active() {
            self.upcoming = None;
            return None;
        }
        let need_new = force_reroll
            || self.upcoming.as_ref().is_none_or(|up| {
                !up.path.is_file()
                    || current_path.is_some_and(|c| c == up.path.as_path())
                    || exclude_paths.iter().any(|e| e == &up.path)
            });

        if need_new {
            if force_reroll {
                self.roll_nonce = self.roll_nonce.wrapping_add(1);
            }
            let salt = (glib::monotonic_time() as u64) ^ self.roll_nonce;
            self.upcoming = self.pick_next(
                current_path,
                queue_tracks,
                library_tracks,
                exclude_paths,
                salt,
            );
            // Update queue_index if path moved in queue
            if let Some(ref mut up) = self.upcoming {
                up.queue_index = queue_tracks.iter().position(|p| p == &up.path);
            }
        } else if let Some(ref mut up) = self.upcoming {
            up.queue_index = queue_tracks.iter().position(|p| p == &up.path);
        }
        self.upcoming.as_ref()
    }

    /// Generates a coherent preview list of upcoming `DjPick`s (without mutating persistent engine state).
    pub fn generate_pick_set(
        &mut self,
        current_path: Option<&Path>,
        queue_tracks: &[PathBuf],
        library_tracks: &[PathBuf],
        exclude_paths: &[PathBuf],
        count: usize,
    ) -> Vec<DjPick> {
        if !self.config.mode.is_active()
            || (queue_tracks.is_empty() && library_tracks.is_empty())
            || count == 0
        {
            return Vec::new();
        }
        let mut picks = Vec::new();
        let saved_history = self.history.clone();
        let saved_artists = self.recent_artists.clone();
        let saved_step = self.journey_step;
        let mut cur = current_path.map(Path::to_path_buf);

        let base_salt = (glib::monotonic_time() as u64) ^ self.roll_nonce;
        for step in 0..count {
            let mut excludes: Vec<PathBuf> = exclude_paths.to_vec();
            excludes.extend(picks.iter().map(|p: &DjPick| p.path.clone()));
            if let Some(ref c) = cur {
                excludes.push(c.clone());
            }
            let pick = self.pick_next(
                cur.as_deref(),
                queue_tracks,
                library_tracks,
                &excludes,
                base_salt.wrapping_add(step as u64 * 977),
            );
            let Some(chosen) = pick else {
                break;
            };
            self.record_played(&chosen.path);
            cur = Some(chosen.path.clone());
            picks.push(chosen);
        }

        self.history = saved_history;
        self.recent_artists = saved_artists;
        self.journey_step = saved_step;
        picks
    }

    /// Generates a coherent setlist of `count` tracks after `current_path` for populating the queue.
    pub fn generate_set(
        &mut self,
        current_path: Option<&Path>,
        pool_tracks: &[PathBuf],
        count: usize,
    ) -> Vec<PathBuf> {
        self.generate_pick_set(current_path, pool_tracks, pool_tracks, &[], count)
            .into_iter()
            .map(|p| p.path)
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[allow(clippy::too_many_arguments)]
    fn profile(
        name: &str,
        artist: &str,
        album: &str,
        genre: Option<&str>,
        year: Option<u32>,
        bpm: Option<u32>,
        energy: Option<f64>,
        key: Option<&str>,
    ) -> TrackProfile {
        TrackProfile {
            path: PathBuf::from(format!("/music/{name}.flac")),
            title: name.to_string(),
            artist: artist.to_string(),
            album: album.to_string(),
            genre: genre.map(str::to_string),
            year,
            bpm,
            energy,
            camelot: key.and_then(parse_camelot_key),
            folder_hint: String::new(),
        }
    }

    #[test]
    fn camelot_wheel_parsing_and_compatibility() {
        assert_eq!(parse_camelot_key("8A"), Some((8, false)));
        assert_eq!(parse_camelot_key("11b"), Some((11, true)));
        assert_eq!(parse_camelot_key("Am"), Some((8, false)));
        assert_eq!(parse_camelot_key("C major"), Some((8, true)));
        assert_eq!(parse_camelot_key("F# minor"), Some((11, false)));
        assert_eq!(parse_camelot_key("Eb"), Some((5, true)));
        assert_eq!(parse_camelot_key("invalid"), None);

        assert_eq!(camelot_compatibility((8, false), (8, false)), 1.0);
        assert!(camelot_compatibility((8, false), (9, false)) > 0.85);
        assert!(camelot_compatibility((8, false), (8, true)) > 0.80);
        assert!(camelot_compatibility((12, false), (1, false)) > 0.85);
        assert!(camelot_compatibility((8, false), (3, true)) < 0.30);
    }

    #[test]
    fn estimates_120_bpm_and_high_energy_from_pulse_envelope() {
        // 20ms interval = 50Hz. 120 BPM = 0.5s beat = 25 samples per beat.
        let mut samples = vec![-28.0_f64; 500];
        for i in (0..samples.len()).step_by(25) {
            samples[i] = -7.0;
            if i + 1 < samples.len() {
                samples[i + 1] = -12.0;
            }
        }
        let (energy, bpm) = estimate_energy_and_bpm_from_rms_db(&samples, 20.0);
        assert!(energy.is_some());
        let e = energy.unwrap();
        assert!(e > 0.10 && e < 0.50);
        assert_eq!(bpm, Some(120));
    }

    #[test]
    fn smart_flow_prefers_matching_genre_bpm_key_and_avoids_same_artist() {
        let engine = DjEngine::new(
            DjConfig {
                mode: DjMixMode::SmartFlow,
                pool: DjPoolSource::LibraryAutoFill,
                crossfade_secs: 6.0,
                curve: DjCrossfadeCurve::EqualPower,
                smart_cue: true,
                trim_intro_silence: true,
                tempo_match: true,
            },
            None,
        );

        let cur = profile(
            "Time",
            "Pink Floyd",
            "DSOTM",
            Some("Progressive Rock"),
            Some(1973),
            Some(120),
            Some(0.65),
            Some("8A"),
        );

        let candidates = vec![
            (
                profile(
                    "Speak to Me",
                    "Pink Floyd",
                    "DSOTM",
                    Some("Progressive Rock"),
                    Some(1973),
                    Some(120),
                    Some(0.60),
                    Some("8A"),
                ),
                Some(0),
            ),
            (
                profile(
                    "Roundabout",
                    "Yes",
                    "Fragile",
                    Some("Progressive Rock"),
                    Some(1971),
                    Some(122),
                    Some(0.68),
                    Some("9A"),
                ),
                Some(1),
            ),
            (
                profile(
                    "Extreme Blast",
                    "Death Band",
                    "Brutal",
                    Some("Death Metal"),
                    Some(2020),
                    Some(190),
                    Some(0.98),
                    Some("2B"),
                ),
                Some(2),
            ),
        ];

        // Over multiple salts, Yes (Roundabout) should outscore Death Metal and same-album Pink Floyd.
        let mut yes_wins = 0;
        for s in 1..20 {
            let pick = engine
                .pick_next_with_profiles(Some(&cur), None, &candidates, s * 1337)
                .unwrap();
            if pick.path.ends_with("Roundabout.flac") {
                yes_wins += 1;
            }
            assert!(!pick.path.ends_with("Extreme Blast.flac"));
        }
        assert!(
            yes_wins >= 15,
            "expected Roundabout to win cleanly, got {yes_wins}"
        );
    }

    #[test]
    fn random_modes_respect_history_and_party_favors_energy() {
        let mut engine = DjEngine::new(
            DjConfig {
                mode: DjMixMode::RandomFlow,
                ..Default::default()
            },
            None,
        );
        let cur = profile("A", "Artist 1", "Alb 1", None, None, None, Some(0.5), None);
        let c1 = profile("B", "Artist 2", "Alb 2", None, None, None, Some(0.5), None);
        let c2 = profile("C", "Artist 3", "Alb 3", None, None, None, Some(0.5), None);
        engine.record_played(&c1.path);
        let candidates = vec![(c1.clone(), Some(0)), (c2.clone(), Some(1))];

        for s in 1..10 {
            let pick = engine
                .pick_next_with_profiles(Some(&cur), None, &candidates, s * 41)
                .unwrap();
            assert_eq!(pick.path, c2.path);
        }

        engine.config.mode = DjMixMode::RandomParty;
        engine.history.clear();
        let chill = profile(
            "Chill",
            "Ambient",
            "Sky",
            None,
            None,
            Some(72),
            Some(0.15),
            None,
        );
        let hype = profile(
            "Hype",
            "Club",
            "Night",
            None,
            None,
            Some(128),
            Some(0.95),
            None,
        );
        let mut hype_count = 0;
        for s in 1..40 {
            let pick = engine
                .pick_next_with_profiles(
                    Some(&cur),
                    None,
                    &[(chill.clone(), Some(0)), (hype.clone(), Some(1))],
                    s * 911,
                )
                .unwrap();
            if pick.path == hype.path {
                hype_count += 1;
            }
        }
        assert!(
            hype_count >= 25,
            "party mode should bias toward energetic tracks"
        );
    }

    #[test]
    fn journey_mode_follows_energy_arc() {
        let mut engine = DjEngine::new(
            DjConfig {
                mode: DjMixMode::SmartJourney,
                ..Default::default()
            },
            None,
        );
        let cur = profile(
            "Intro",
            "Opener",
            "Init",
            Some("Electronic"),
            Some(2020),
            Some(115),
            Some(0.42),
            Some("8A"),
        );
        let low = profile(
            "Ambient",
            "Chill Artist",
            "Calm",
            Some("Electronic"),
            Some(2020),
            Some(110),
            Some(0.38),
            Some("8A"),
        );
        let high = profile(
            "Peak",
            "Banger Artist",
            "Peak",
            Some("Electronic"),
            Some(2020),
            Some(128),
            Some(0.92),
            Some("8A"),
        );
        let cands = vec![(low.clone(), Some(0)), (high.clone(), Some(1))];

        // Step 0 is Warm-up (0.42) -> low energy matches better
        engine.journey_step = 0;
        let p0 = engine
            .pick_next_with_profiles(Some(&cur), None, &cands, 7)
            .unwrap();
        assert_eq!(p0.journey_phase.unwrap().0, "Warm-up");

        // Step 5 is Peak Time (0.90) -> high energy wins strongly
        engine.journey_step = 5;
        let p5 = engine
            .pick_next_with_profiles(Some(&cur), None, &cands, 7)
            .unwrap();
        assert_eq!(p5.journey_phase.unwrap().0, "Peak Time");
        assert_eq!(p5.path, high.path);
    }

    #[test]
    fn mode_roundtrip_and_cache_serialization() {
        for mode in DjMixMode::ALL {
            assert_eq!(DjMixMode::from_id(mode.id()), mode);
        }
        assert_eq!(
            DjPoolSource::from_id(DjPoolSource::QueueOnly.id()),
            DjPoolSource::QueueOnly
        );
        assert_eq!(
            DjPoolSource::from_id(DjPoolSource::LibraryAutoFill.id()),
            DjPoolSource::LibraryAutoFill
        );

        let dir = std::env::temp_dir().join(format!("vinyl-dj-cache-{}", std::process::id()));
        let _ = std::fs::create_dir_all(&dir);
        let cache_file = dir.join("dj-acoustics.json");
        let mut map = HashMap::new();
        map.insert(
            PathBuf::from("/tmp/song.flac"),
            AcousticAnalysis {
                stamp: "1_100_1".into(),
                bpm: Some(124),
                energy: Some(0.78),
                genre: Some("House".into()),
                camelot: Some((8, false)),
                duration_secs: Some(312.5),
                intro_silence_ms: Some(1800),
            },
        );
        save_acoustic_cache_to(&cache_file, &map);
        let loaded = load_acoustic_cache_from(&cache_file);
        assert_eq!(loaded, map);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn crossfade_curves_and_intro_cue_behave_accurately() {
        for curve in DjCrossfadeCurve::ALL {
            assert_eq!(DjCrossfadeCurve::from_id(curve.id()), curve);
            let (a0, b0) = curve.gains(0.0);
            assert!((a0 - 1.0).abs() < 1e-6);
            assert!(b0.abs() < 1e-6);
            let (a1, b1) = curve.gains(1.0);
            assert!(a1.abs() < 1e-6);
            assert!((b1 - 1.0).abs() < 1e-6);
        }
        let (cut_out_early, cut_in_early) = DjCrossfadeCurve::HardCut.gains(0.30);
        assert!(cut_out_early > 0.85 && cut_in_early > 0.90);

        let mut engine = DjEngine::new(DjConfig::default(), None);
        let track = PathBuf::from("/tmp/intro_test.flac");
        engine.analysis_cache.insert(
            track.clone(),
            AcousticAnalysis {
                intro_silence_ms: Some(2400),
                ..Default::default()
            },
        );
        assert_eq!(engine.intro_cue_us_for(&track), Some(2_400_000));
        engine.config.trim_intro_silence = false;
        assert_eq!(engine.intro_cue_us_for(&track), None);
    }

    #[test]
    fn analyzer_cursor_sweeps_entire_library_beyond_forty_tracks() {
        let dir = std::env::temp_dir().join(format!("vinyl-dj-sweep-{}", std::process::id()));
        let _ = std::fs::create_dir_all(&dir);
        let mut tracks = Vec::new();
        for i in 0..75 {
            let p = dir.join(format!("track_{i:03}.wav"));
            std::fs::write(&p, b"stub").unwrap();
            tracks.push(p);
        }
        let mut engine = DjEngine::new(
            DjConfig {
                mode: DjMixMode::SmartFlow,
                ..Default::default()
            },
            None,
        );
        let mut seen = HashSet::new();
        for _ in 0..20 {
            let batch = engine.next_paths_to_analyze(None, &tracks, 4);
            for p in batch {
                seen.insert(p.clone());
                engine.insert_analysis(p, AcousticAnalysis::default());
            }
        }
        assert_eq!(seen.len(), 75);
        assert!(engine.next_paths_to_analyze(None, &tracks, 4).is_empty());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
