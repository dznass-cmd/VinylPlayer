//! Bilingual localization module (English & Brazilian Portuguese) with live GTK widget binding.

use gtk::prelude::*;
use std::cell::RefCell;
use std::sync::atomic::{AtomicU8, Ordering};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[repr(u8)]
pub enum Language {
    #[default]
    En = 0,
    PtBr = 1,
}

impl Language {
    pub const ALL: [Language; 2] = [Language::En, Language::PtBr];

    pub fn code(self) -> &'static str {
        match self {
            Language::En => "en",
            Language::PtBr => "pt-BR",
        }
    }

    pub fn native_label(self) -> &'static str {
        match self {
            Language::En => "English",
            Language::PtBr => "Português (Brasil)",
        }
    }

    pub fn from_code(code: &str) -> Self {
        let lower = code.trim().to_ascii_lowercase();
        if lower.starts_with("pt") {
            Language::PtBr
        } else {
            Language::En
        }
    }

    pub fn detect_system() -> Self {
        for var in ["LC_ALL", "LC_MESSAGES", "LANG"] {
            if let Ok(val) = std::env::var(var) {
                let trimmed = val.trim();
                if !trimmed.is_empty() && trimmed != "C" && trimmed != "POSIX" {
                    return Self::from_code(trimmed);
                }
            }
        }
        Language::En
    }
}

static CURRENT_LANG: AtomicU8 = AtomicU8::new(0);

type Binding = Box<dyn FnMut(Language) -> bool>;

thread_local! {
    static BINDINGS: RefCell<Vec<Binding>> = RefCell::new(Vec::new());
}

pub fn init_from_state(saved: Option<&str>) {
    let lang = saved
        .map(Language::from_code)
        .unwrap_or_else(Language::detect_system);
    CURRENT_LANG.store(lang as u8, Ordering::Relaxed);
}

pub fn current() -> Language {
    match CURRENT_LANG.load(Ordering::Relaxed) {
        1 => Language::PtBr,
        _ => Language::En,
    }
}

pub fn set_current(lang: Language) {
    CURRENT_LANG.store(lang as u8, Ordering::Relaxed);
    BINDINGS.with(|cell| {
        cell.borrow_mut().retain_mut(|update| update(lang));
    });
}

pub fn bind_label(label: &gtk::Label, text: fn(Language) -> &'static str) {
    label.set_text(text(current()));
    let weak = label.downgrade();
    BINDINGS.with(|cell| {
        cell.borrow_mut().push(Box::new(move |lang| {
            if let Some(lbl) = weak.upgrade() {
                lbl.set_text(text(lang));
                true
            } else {
                false
            }
        }));
    });
}

pub fn bind_tooltip(widget: &impl IsA<gtk::Widget>, text: fn(Language) -> &'static str) {
    widget.set_tooltip_text(Some(text(current())));
    let weak = widget.as_ref().downgrade();
    BINDINGS.with(|cell| {
        cell.borrow_mut().push(Box::new(move |lang| {
            if let Some(w) = weak.upgrade() {
                w.set_tooltip_text(Some(text(lang)));
                true
            } else {
                false
            }
        }));
    });
}

pub fn bind_button_label(button: &gtk::Button, text: fn(Language) -> &'static str) {
    button.set_label(text(current()));
    let weak = button.downgrade();
    BINDINGS.with(|cell| {
        cell.borrow_mut().push(Box::new(move |lang| {
            if let Some(btn) = weak.upgrade() {
                btn.set_label(text(lang));
                true
            } else {
                false
            }
        }));
    });
}

pub fn bind_placeholder(entry: &gtk::SearchEntry, text: fn(Language) -> &'static str) {
    entry.set_placeholder_text(Some(text(current())));
    let weak = entry.downgrade();
    BINDINGS.with(|cell| {
        cell.borrow_mut().push(Box::new(move |lang| {
            if let Some(e) = weak.upgrade() {
                e.set_placeholder_text(Some(text(lang)));
                true
            } else {
                false
            }
        }));
    });
}

pub fn connect(anchor: &impl IsA<gtk::Widget>, f: impl Fn(Language) + 'static) {
    f(current());
    let weak = anchor.as_ref().downgrade();
    BINDINGS.with(|cell| {
        cell.borrow_mut().push(Box::new(move |lang| {
            if weak.upgrade().is_some() {
                f(lang);
                true
            } else {
                false
            }
        }));
    });
}

pub fn pick(en: &'static str, pt: &'static str) -> &'static str {
    match current() {
        Language::En => en,
        Language::PtBr => pt,
    }
}

pub fn pick_for(lang: Language, en: &'static str, pt: &'static str) -> &'static str {
    match lang {
        Language::En => en,
        Language::PtBr => pt,
    }
}

pub fn window_title(lang: Language, ver: &str) -> String {
    match lang {
        Language::En => format!("VinylPlayer v{ver} — High-Fidelity Audio"),
        Language::PtBr => format!("VinylPlayer v{ver} — Áudio de Alta Fidelidade"),
    }
}

pub fn settings_window_title(lang: Language, ver: &str) -> String {
    match lang {
        Language::En => format!("Settings & Library Folders (v{ver})"),
        Language::PtBr => format!("Configurações e Pastas da Biblioteca (v{ver})"),
    }
}

pub fn settings_heading(lang: Language, ver: &str) -> String {
    match lang {
        Language::En => format!("Your collection, your folders · v{ver}"),
        Language::PtBr => format!("Sua coleção, suas pastas · v{ver}"),
    }
}

pub fn summary_tracks(lang: Language, count: usize, warnings: usize) -> String {
    match lang {
        Language::En => {
            let noun = if count == 1 { "track" } else { "tracks" };
            match warnings {
                0 => format!("{count} {noun}"),
                1 => format!("{count} {noun} · 1 warning"),
                w => format!("{count} {noun} · {w} warnings"),
            }
        }
        Language::PtBr => {
            let noun = if count == 1 { "faixa" } else { "faixas" };
            match warnings {
                0 => format!("{count} {noun}"),
                1 => format!("{count} {noun} · 1 aviso"),
                w => format!("{count} {noun} · {w} avisos"),
            }
        }
    }
}

pub fn queue_summary(lang: Language, len: usize, next: usize) -> String {
    match lang {
        Language::En => {
            let noun = if len == 1 { "track" } else { "tracks" };
            format!("{len} {noun} · {next} upcoming")
        }
        Language::PtBr => {
            let noun = if len == 1 { "faixa" } else { "faixas" };
            format!("{len} {noun} · {next} na sequência")
        }
    }
}

pub fn queue_search_summary(lang: Language, shown: usize, len: usize, pending: bool) -> String {
    match lang {
        Language::En => {
            let loading = if pending { " · reading metadata…" } else { "" };
            format!("{shown} of {len} tracks{loading}")
        }
        Language::PtBr => {
            let loading = if pending { " · lendo metadados…" } else { "" };
            format!("{shown} de {len} faixas{loading}")
        }
    }
}

pub fn album_tracks_local(lang: Language, count: usize) -> String {
    match lang {
        Language::En => format!("{count} tracks · Local album"),
        Language::PtBr => format!("{count} faixa(s) · Álbum local"),
    }
}

pub fn playlist_tracks_count(lang: Language, count: usize) -> String {
    match lang {
        Language::En => format!("{count} track(s)"),
        Language::PtBr => format!("{count} faixa(s)"),
    }
}

pub fn playlist_detail_count(lang: Language, count: usize) -> String {
    match lang {
        Language::En => format!("{count} track(s) · M3U8 File"),
        Language::PtBr => format!("{count} faixa(s) · Arquivo M3U8"),
    }
}

pub fn nav_title(lang: Language, page: &str) -> &'static str {
    match (lang, page) {
        (Language::En, "overview") => "Turntable",
        (Language::PtBr, "overview") => "Toca-Discos",
        (Language::En, "tracks") => "Tracks",
        (Language::PtBr, "tracks") => "Faixas",
        (Language::En, "albums") => "Albums",
        (Language::PtBr, "albums") => "Álbuns",
        (_, "playlists") => "Playlists",
        (Language::En, _) => "Turntable",
        (Language::PtBr, _) => "Toca-Discos",
    }
}

pub fn bind_dropdown_strings(
    dropdown: &gtk::DropDown,
    items: fn(Language) -> &'static [&'static str],
) {
    dropdown.set_model(Some(&gtk::StringList::new(items(current()))));
    let weak = dropdown.downgrade();
    BINDINGS.with(|cell| {
        cell.borrow_mut().push(Box::new(move |lang| {
            if let Some(dd) = weak.upgrade() {
                let selected = dd.selected();
                dd.set_model(Some(&gtk::StringList::new(items(lang))));
                dd.set_selected(selected);
                true
            } else {
                false
            }
        }));
    });
}

pub fn playback_status(lang: Language, state: crate::audio::PlaybackState) -> &'static str {
    use crate::audio::PlaybackState;
    match (lang, state) {
        (Language::En, PlaybackState::Playing) => "PLAYING",
        (Language::PtBr, PlaybackState::Playing) => "EM REPRODUÇÃO",
        (Language::En, PlaybackState::Paused) => "PAUSED",
        (Language::PtBr, PlaybackState::Paused) => "PAUSADO",
        (Language::En, PlaybackState::Stopped) => "READY",
        (Language::PtBr, PlaybackState::Stopped) => "PRONTO",
    }
}

pub fn shuffle_tooltip(lang: Language, active: bool) -> &'static str {
    match (lang, active) {
        (Language::En, true) => "Shuffle · Enabled (S)",
        (Language::PtBr, true) => "Aleatório · Ativado (S)",
        (Language::En, false) => "Shuffle · Disabled (S)",
        (Language::PtBr, false) => "Aleatório · Desativado (S)",
    }
}

pub fn repeat_tooltip(lang: Language, mode: crate::audio::RepeatMode) -> &'static str {
    use crate::audio::RepeatMode;
    match (lang, mode) {
        (Language::En, RepeatMode::Off) => "Repeat · Off (R — click to repeat entire queue)",
        (Language::PtBr, RepeatMode::Off) => "Repetir · Desativado (R — clique para repetir a fila)",
        (Language::En, RepeatMode::All) => "Repeat · All (R — click to repeat current track)",
        (Language::PtBr, RepeatMode::All) => {
            "Repetir · Tudo (R — clique para repetir a faixa atual)"
        }
        (Language::En, RepeatMode::Track) => "Repeat · Track (R — click to turn off)",
        (Language::PtBr, RepeatMode::Track) => "Repetir · Faixa (R — clique para desativar)",
    }
}

pub fn mute_tooltip(lang: Language, vol: f64) -> String {
    if vol <= 0.001 {
        pick_for(lang, "Muted · Unmute (M)", "Mudo · Ativar som (M)").to_string()
    } else {
        let pct = (vol * 100.0).round() as u32;
        match lang {
            Language::En => format!("Volume {pct}% · Mute (M)"),
            Language::PtBr => format!("Volume {pct}% · Silenciar (M)"),
        }
    }
}

pub fn playlist_added_notice(lang: Language, count: usize, pl_name: &str) -> String {
    match lang {
        Language::En => {
            if count == 1 {
                format!("Track added to \"{pl_name}\"!")
            } else {
                format!("{count} tracks added to \"{pl_name}\"!")
            }
        }
        Language::PtBr => {
            if count == 1 {
                format!("Faixa adicionada a \"{pl_name}\"!")
            } else {
                format!("{count} faixas adicionadas a \"{pl_name}\"!")
            }
        }
    }
}

pub fn is_unknown_artist(artist: &str) -> bool {
    let trimmed = artist.trim();
    trimmed.is_empty() || trimmed == "Unknown Artist" || trimmed == "Artista desconhecido"
}

pub fn tracks_count_inline(lang: Language, count: usize) -> String {
    match lang {
        Language::En => format!("{count} track(s)"),
        Language::PtBr => format!("{count} faixa(s)"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn language_roundtrip_and_parsing() {
        assert_eq!(Language::from_code("pt_BR.UTF-8"), Language::PtBr);
        assert_eq!(Language::from_code("pt-BR"), Language::PtBr);
        assert_eq!(Language::from_code("pt"), Language::PtBr);
        assert_eq!(Language::from_code("en_US.UTF-8"), Language::En);
        assert_eq!(Language::from_code("unknown"), Language::En);
        assert_eq!(Language::En.code(), "en");
        assert_eq!(Language::PtBr.code(), "pt-BR");
    }

    #[test]
    fn localized_summaries_format_both_languages() {
        assert_eq!(summary_tracks(Language::En, 1, 0), "1 track");
        assert_eq!(summary_tracks(Language::En, 4, 2), "4 tracks · 2 warnings");
        assert_eq!(summary_tracks(Language::PtBr, 1, 0), "1 faixa");
        assert_eq!(summary_tracks(Language::PtBr, 4, 2), "4 faixas · 2 avisos");
        assert_eq!(queue_search_summary(Language::En, 0, 4, false), "0 of 4 tracks");
        assert_eq!(queue_search_summary(Language::PtBr, 0, 4, false), "0 de 4 faixas");
    }
}
