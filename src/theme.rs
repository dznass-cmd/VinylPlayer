//! Sistema de temas e paletas de cores do VinylPlayer.

use gtk::gdk;
use std::cell::RefCell;

thread_local! {
    static THEME_PROVIDER: RefCell<Option<gtk::CssProvider>> = const { RefCell::new(None) };
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Theme {
    AmberGold,
    MarantzBlue,
    EmeraldTube,
    RubySunset,
    MidnightViolet,
    ObsidianSilver,
}

impl Theme {
    pub const ALL: [Theme; 6] = [
        Theme::AmberGold,
        Theme::MarantzBlue,
        Theme::EmeraldTube,
        Theme::RubySunset,
        Theme::MidnightViolet,
        Theme::ObsidianSilver,
    ];

    pub fn id(&self) -> &'static str {
        match self {
            Theme::AmberGold => "amber",
            Theme::MarantzBlue => "blue",
            Theme::EmeraldTube => "emerald",
            Theme::RubySunset => "ruby",
            Theme::MidnightViolet => "violet",
            Theme::ObsidianSilver => "silver",
        }
    }

    pub fn label(&self) -> &'static str {
        match self {
            Theme::AmberGold => "Amber Gold",
            Theme::MarantzBlue => "Marantz Blue",
            Theme::EmeraldTube => "Emerald Tube",
            Theme::RubySunset => "Ruby Sunset",
            Theme::MidnightViolet => "Midnight Violet",
            Theme::ObsidianSilver => "Obsidian Silver",
        }
    }

    pub fn description_for(&self, lang: crate::i18n::Language) -> &'static str {
        use crate::i18n::Language;
        match (lang, self) {
            (Language::En, Theme::AmberGold) => "Vintage warm amber gold (classic)",
            (Language::PtBr, Theme::AmberGold) => "Ouro âmbar aquecido retrô (clássico)",
            (Language::En, Theme::MarantzBlue) => "70s classic stereo receiver blue",
            (Language::PtBr, Theme::MarantzBlue) => "Azul clássico de receiver estéreo dos anos 70",
            (Language::En, Theme::EmeraldTube) => "Vintage McIntosh tube amplifier green",
            (Language::PtBr, Theme::EmeraldTube) => "Verde valvulado clássico estilo McIntosh",
            (Language::En, Theme::RubySunset) => "Warm analog reel-to-reel terracotta",
            (Language::PtBr, Theme::RubySunset) => "Terracota analógico quente de rolo",
            (Language::En, Theme::MidnightViolet) => "Aesthetic deep purple & vinyl indigo",
            (Language::PtBr, Theme::MidnightViolet) => "Roxo profundo & índigo vinil",
            (Language::En, Theme::ObsidianSilver) => "Brushed silver & minimal graphite",
            (Language::PtBr, Theme::ObsidianSilver) => "Prata escovado & grafite minimalista",
        }
    }

    pub fn from_id(id: &str) -> Self {
        match id {
            "blue" => Theme::MarantzBlue,
            "emerald" => Theme::EmeraldTube,
            "ruby" => Theme::RubySunset,
            "violet" => Theme::MidnightViolet,
            "silver" => Theme::ObsidianSilver,
            _ => Theme::AmberGold,
        }
    }

    pub fn accent_color(&self) -> &'static str {
        match self {
            Theme::AmberGold => "#dfc08d",
            Theme::MarantzBlue => "#5da5db",
            Theme::EmeraldTube => "#55b685",
            Theme::RubySunset => "#e06c53",
            Theme::MidnightViolet => "#9d72d6",
            Theme::ObsidianSilver => "#dcdcdc",
        }
    }

    pub fn accent_rgb(&self) -> (f64, f64, f64) {
        match self {
            Theme::AmberGold => (223.0 / 255.0, 192.0 / 255.0, 141.0 / 255.0),
            Theme::MarantzBlue => (93.0 / 255.0, 165.0 / 255.0, 219.0 / 255.0),
            Theme::EmeraldTube => (85.0 / 255.0, 182.0 / 255.0, 133.0 / 255.0),
            Theme::RubySunset => (224.0 / 255.0, 108.0 / 255.0, 83.0 / 255.0),
            Theme::MidnightViolet => (157.0 / 255.0, 114.0 / 255.0, 214.0 / 255.0),
            Theme::ObsidianSilver => (220.0 / 255.0, 220.0 / 255.0, 220.0 / 255.0),
        }
    }

    pub fn accent_hover(&self) -> &'static str {
        match self {
            Theme::AmberGold => "#edcf9f",
            Theme::MarantzBlue => "#79b7e8",
            Theme::EmeraldTube => "#6dc59b",
            Theme::RubySunset => "#ea816a",
            Theme::MidnightViolet => "#b28ce0",
            Theme::ObsidianSilver => "#f5f5f5",
        }
    }

    pub fn accent_bg(&self) -> &'static str {
        match self {
            Theme::AmberGold => "#302b24",
            Theme::MarantzBlue => "#1b2834",
            Theme::EmeraldTube => "#1b2d22",
            Theme::RubySunset => "#331f1c",
            Theme::MidnightViolet => "#281e35",
            Theme::ObsidianSilver => "#28292c",
        }
    }

    pub fn accent_border(&self) -> &'static str {
        match self {
            Theme::AmberGold => "#494033",
            Theme::MarantzBlue => "#2d455d",
            Theme::EmeraldTube => "#2d4b38",
            Theme::RubySunset => "#54332e",
            Theme::MidnightViolet => "#493561",
            Theme::ObsidianSilver => "#42454a",
        }
    }

    pub fn accent_badge_bg(&self) -> &'static str {
        match self {
            Theme::AmberGold => "rgba(223, 192, 141, 0.16)",
            Theme::MarantzBlue => "rgba(93, 165, 219, 0.16)",
            Theme::EmeraldTube => "rgba(85, 182, 133, 0.16)",
            Theme::RubySunset => "rgba(224, 108, 83, 0.16)",
            Theme::MidnightViolet => "rgba(157, 114, 214, 0.16)",
            Theme::ObsidianSilver => "rgba(220, 220, 220, 0.16)",
        }
    }

    pub fn accent_badge_border(&self) -> &'static str {
        match self {
            Theme::AmberGold => "rgba(223, 192, 141, 0.38)",
            Theme::MarantzBlue => "rgba(93, 165, 219, 0.38)",
            Theme::EmeraldTube => "rgba(85, 182, 133, 0.38)",
            Theme::RubySunset => "rgba(224, 108, 83, 0.38)",
            Theme::MidnightViolet => "rgba(157, 114, 214, 0.38)",
            Theme::ObsidianSilver => "rgba(220, 220, 220, 0.38)",
        }
    }

    pub fn text_on_accent(&self) -> &'static str {
        match self {
            Theme::AmberGold => "#252016",
            Theme::MarantzBlue => "#11202e",
            Theme::EmeraldTube => "#11241a",
            Theme::RubySunset => "#241310",
            Theme::MidnightViolet => "#1d1427",
            Theme::ObsidianSilver => "#1a1b1d",
        }
    }

    pub fn generate_css(&self) -> String {
        let acc = self.accent_color();
        let hov = self.accent_hover();
        let bg = self.accent_bg();
        let bor = self.accent_border();
        let bbg = self.accent_badge_bg();
        let bbor = self.accent_badge_border();
        let txt = self.text_on_accent();

        format!(
            r#"
            /* Overrides dinâmicos de cor do tema: {label} */
            .brand-icon {{ color: {acc}; }}
            .nav-button.active-nav {{ background: {bg}; border-color: {bor}; color: {acc}; }}
            .version-badge {{ background: {bg}; color: {acc}; border-color: {bor}; }}
            .play-button {{ background: {acc}; color: {txt}; }}
            .play-button:hover {{ background: {hov}; }}
            .control-button.active {{ color: {acc}; background: {bbg}; }}
            .control-button.active:hover {{ color: {hov}; background: {bbor}; }}
            .queue-toggle:checked {{ background: {bg}; border-color: {bor}; }}
            .queue-toggle label, .queue-toggle image {{ color: {acc}; }}
            .queue-current {{ background: {bg}; }}
            .queue-current .queue-track-title {{ color: {acc}; }}
            .queue-summary {{ color: {acc}; }}
            .queue-search:focus-within {{ border-color: {acc}; }}
            .queue-clear {{ color: {acc}; }}
            .queue-clear:hover {{ background: {bg}; color: {hov}; }}
            .queue-drop-before {{ border-top-color: {acc}; }}
            .queue-drop-after {{ border-bottom-color: {acc}; }}
            .listening-status.is-playing {{ background: {bg}; border-color: {bor}; }}
            .listening-status.is-playing label, .listening-status.is-playing image {{ color: {acc}; }}
            .listening-format {{ color: {acc}; background: {bg}; border-color: {bor}; }}
            .rpm-button:checked {{ background: {bg}; border-color: {bor}; color: {acc}; }}
            .search-box:focus-within {{ border-color: {acc}; }}
            .open-button {{ background: {acc}; border-color: {acc}; color: {txt}; }}
            .open-button:hover {{ background: {hov}; border-color: {hov}; }}
            .mini-disc-icon {{ color: {acc}; }}
            .mini-cover-container {{ border-color: {bor}; }}
            scale highlight {{ background: {acc}; }}
            .badge-hires {{ background: {bg}; color: {acc}; }}
            .sort-dropdown:hover {{ border-color: {bor}; }}
            .album-view-switcher button:checked {{ background: {bg}; color: {acc}; }}
            .album-cover-button:hover {{ border-color: {acc}; }}
            .album-cover-button.playback-current {{ border-color: {acc}; box-shadow: 0 0 0 2px {acc}; }}
            .album-playback-indicator {{ background: {acc}; color: {txt}; }}
            .play-album-button {{ background: {bg}; color: {acc}; border-color: {bor}; }}
            .play-album-button:hover {{ background: {bor}; }}
            .track-item-button.playback-current {{ background: {bg}; }}
            .track-item-button.playback-current .track-row-name,
            .track-item-button.playback-current .track-icon {{ color: {acc}; }}
            .enqueue-menu > button:hover {{ background: {bg}; border-color: {bor}; }}
            .folder-button:hover {{ border-color: {bor}; }}
            button:focus-visible {{ outline-color: {acc}; }}
            .playlist-card:hover {{ border-color: {bor}; }}
            .playlist-badge-icon {{ color: {acc}; background: {bg}; border-color: {bor}; }}
            .playlist-detail-badge {{ background: {bg}; border-color: {bor}; color: {acc}; }}
            .popover-playlist-item:hover {{ background: {bg}; border-color: {bor}; color: {acc}; }}
            .donate-goal-num {{ color: {acc}; }}
            .donate-progress progress {{ background: {acc}; }}
            .donate-addr-entry {{ color: {acc}; }}
            .donate-copy-btn {{ background: {bbg}; border-color: {bbor}; color: {acc}; }}
            .donate-copy-btn:hover {{ background: {bbg}; color: {hov}; }}
            .donate-qr-frame {{ border-color: {acc}; }}
            .donate-github-btn:hover {{ border-color: {acc}; }}
            .donate-supporter-pill {{ background: {bbg}; border-color: {bbor}; color: {acc}; }}
            .donate-nav-button {{ color: {acc}; }}
            .donate-nav-button:hover {{ background: {bbg}; color: {hov}; }}
            .playback-error button.suggested-action {{ background: {acc}; color: {txt}; }}
            .theme-swatch.active {{ border-color: {acc}; box-shadow: 0 0 0 2px {acc}; }}
            .theme-btn {{ background: rgba(255, 255, 255, 0.04); border: 1px solid #363432; border-radius: 8px; padding: 6px 12px; }}
            .theme-btn:hover {{ background: rgba(255, 255, 255, 0.08); border-color: #55504a; }}
            .theme-btn.active {{ background: {bg}; border-color: {acc}; }}
            .theme-btn-check {{ color: {acc}; font-weight: bold; }}
            .dj-button.active, .dj-button.active > button {{ color: {acc}; background: {bbg}; }}
            .dj-button.active:hover, .dj-button.active > button:hover {{ color: {hov}; background: {bbor}; }}
            .dj-mode-card.active {{ background: {bg}; border-color: {acc}; box-shadow: 0 0 0 1px {acc}; }}
            .dj-status-pill.active {{ background: {bg}; border-color: {bor}; color: {acc}; }}
            .dj-live-badge {{ background: {bg}; border-color: {bor}; color: {acc}; }}
            .dj-live-badge:hover {{ background: {bor}; color: {hov}; }}
            .dj-pill-btn:checked {{ background: {bg}; border-color: {bor}; color: {acc}; }}
            .dj-badge-smart {{ background: {bbg}; border-color: {bbor}; color: {acc}; }}
            "#,
            label = self.label(),
            acc = acc,
            hov = hov,
            bg = bg,
            bor = bor,
            bbg = bbg,
            bbor = bbor,
            txt = txt,
        )
    }
}

pub fn apply_theme(theme: Theme) {
    let css_data = theme.generate_css();
    THEME_PROVIDER.with(|cell| {
        let mut opt = cell.borrow_mut();
        let provider = opt.get_or_insert_with(|| {
            let p = gtk::CssProvider::new();
            if let Some(display) = gdk::Display::default() {
                gtk::style_context_add_provider_for_display(
                    &display,
                    &p,
                    gtk::STYLE_PROVIDER_PRIORITY_USER + 10,
                );
            }
            p
        });
        provider.load_from_data(&css_data);
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn theme_roundtrip_ids_and_labels() {
        for theme in Theme::ALL {
            let id = theme.id();
            assert_eq!(Theme::from_id(id), theme);
            assert!(!theme.label().is_empty());
            assert!(!theme.accent_color().is_empty());
            let css = theme.generate_css();
            assert!(css.contains(theme.accent_color()));
            for selector in [
                ".control-button.active",
                ".listening-format",
                ".badge-hires",
                ".queue-clear",
                ".queue-search:focus-within",
                ".search-box:focus-within",
                ".enqueue-menu > button:hover",
                ".folder-button:hover",
                "button:focus-visible",
            ] {
                assert!(
                    css.contains(selector),
                    "missing theme override for {selector}"
                );
            }
        }
        assert_eq!(Theme::from_id("unknown"), Theme::AmberGold);
    }
}
