//! Um registro de referências fracas; sem temporizadores por faixa ou capa.
use crate::audio::PlaybackState;
use gtk::prelude::*;
use std::cell::RefCell;
use std::path::{Path, PathBuf};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Mode {
    None,
    Playing,
    Paused,
}

enum Target {
    Track(PathBuf),
    Album(PathBuf),
}

fn mode_for(target: &Target, current: Option<&Path>, state: PlaybackState) -> Mode {
    let matches = match (target, current) {
        (Target::Track(path), Some(current)) => path == current,
        (Target::Album(folder), Some(current)) => current.parent() == Some(folder.as_path()),
        _ => false,
    };
    match (matches, state) {
        (true, PlaybackState::Playing) => Mode::Playing,
        (true, PlaybackState::Paused) => Mode::Paused,
        _ => Mode::None,
    }
}

struct Marker {
    button: glib::WeakRef<gtk::Button>,
    icon: glib::WeakRef<gtk::Image>,
    target: Target,
    label: String,
    tooltip: String,
    mode: Mode,
}

#[derive(Default)]
struct Registry {
    entries: Vec<Marker>,
    last: Option<(String, PlaybackState)>,
    dirty: bool,
}

thread_local! { static MARKERS: RefCell<Registry> = RefCell::new(Registry::default()); }

fn register(button: &gtk::Button, icon: &gtk::Image, target: Target, label: String) {
    let tooltip = button
        .tooltip_text()
        .map(|s| s.to_string())
        .unwrap_or_else(|| label.clone());
    button.update_property(&[gtk::accessible::Property::Label(&label)]);
    MARKERS.with(|registry| {
        let mut registry = registry.borrow_mut();
        registry.entries.push(Marker {
            button: button.downgrade(),
            icon: icon.downgrade(),
            target,
            label,
            tooltip,
            mode: Mode::None,
        });
        registry.dirty = true;
    });
}

pub fn register_track(button: &gtk::Button, icon: &gtk::Image, path: &Path, title: &str) {
    register(
        button,
        icon,
        Target::Track(path.to_path_buf()),
        title.to_owned(),
    );
}

pub fn register_album(button: &gtk::Button, icon: &gtk::Image, folder: &Path, title: &str) {
    icon.set_visible(false);
    register(
        button,
        icon,
        Target::Album(folder.to_path_buf()),
        format!("Abrir álbum {title}"),
    );
}

pub fn sync(uri: &str, state: PlaybackState) {
    MARKERS.with(|registry| {
        let mut registry = registry.borrow_mut();
        if !registry.dirty
            && registry
                .last
                .as_ref()
                .is_some_and(|(old_uri, old_state)| old_uri == uri && *old_state == state)
        {
            return;
        }
        registry.dirty = false;
        registry.last = Some((uri.to_owned(), state));
        let path = glib::filename_from_uri(uri).ok().map(|(path, _)| path);
        registry.entries.retain_mut(|marker| {
            let mode = mode_for(&marker.target, path.as_deref(), state);
            if mode == marker.mode && marker.mode == Mode::None {
                return true;
            }
            let (Some(button), Some(icon)) = (marker.button.upgrade(), marker.icon.upgrade())
            else {
                return false;
            };
            if mode == marker.mode {
                return true;
            }
            marker.mode = mode;
            if mode == Mode::None {
                button.remove_css_class("playback-current");
            } else {
                button.add_css_class("playback-current");
            }
            if mode == Mode::Paused {
                button.add_css_class("playback-paused");
            } else {
                button.remove_css_class("playback-paused");
            }
            let album = matches!(marker.target, Target::Album(_));
            icon.set_icon_name(Some(match mode {
                Mode::Playing => "media-playback-start-symbolic",
                Mode::Paused => "media-playback-pause-symbolic",
                Mode::None => "audio-x-generic-symbolic",
            }));
            if album {
                icon.set_visible(mode != Mode::None);
            }
            let suffix = match mode {
                Mode::Playing => " · Em reprodução",
                Mode::Paused => " · Pausado",
                Mode::None => "",
            };
            button.set_tooltip_text(Some(&format!("{}{suffix}", marker.tooltip)));
            button.update_property(&[gtk::accessible::Property::Label(&format!(
                "{}{suffix}",
                marker.label
            ))]);
            true
        });
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn matches_exact_track_and_album_without_marking_preloaded_files() {
        let track = Target::Track(PathBuf::from("/a/Álbum #1/faixa 1.flac"));
        let album = Target::Album(PathBuf::from("/a/Álbum #1"));
        let other = Target::Album(PathBuf::from("/b/Álbum #1"));
        let path = Path::new("/a/Álbum #1/faixa 1.flac");
        assert_eq!(
            mode_for(&track, Some(path), PlaybackState::Playing),
            Mode::Playing
        );
        assert_eq!(
            mode_for(&album, Some(path), PlaybackState::Paused),
            Mode::Paused
        );
        assert_eq!(
            mode_for(&other, Some(path), PlaybackState::Playing),
            Mode::None
        );
        assert_eq!(
            mode_for(&track, Some(path), PlaybackState::Stopped),
            Mode::None
        );
        assert_eq!(mode_for(&album, None, PlaybackState::Playing), Mode::None);
    }

    #[test]
    #[ignore = "Requer sessão gráfica; verifica os indicadores GTK sem emitir áudio"]
    fn gtk_markers_follow_pause_track_changes_stop_and_new_widgets() {
        gtk::init().unwrap();
        let a = Path::new("/a/Álbum #1/faixa 1.flac");
        let b = Path::new("/b/Álbum #1/faixa 2.flac");
        let uri_a = glib::filename_to_uri(a, None).unwrap();
        let uri_b = glib::filename_to_uri(b, None).unwrap();
        let row = gtk::Button::new();
        let icon = gtk::Image::new();
        let row_b = gtk::Button::new();
        let icon_b = gtk::Image::new();
        let album = gtk::Button::new();
        let badge = gtk::Image::new();
        let album_b = gtk::Button::new();
        let badge_b = gtk::Image::new();
        register_track(&row, &icon, a, "Faixa 1");
        register_track(&row_b, &icon_b, b, "Faixa 2");
        register_album(&album, &badge, a.parent().unwrap(), "Álbum #1");
        register_album(&album_b, &badge_b, b.parent().unwrap(), "Álbum #1");
        sync(&uri_a, PlaybackState::Stopped);
        assert!(!row.has_css_class("playback-current"));
        sync(&uri_a, PlaybackState::Playing);
        assert!(row.has_css_class("playback-current"));
        assert!(badge.is_visible());
        assert!(!badge_b.is_visible());
        assert_eq!(
            icon.icon_name().as_deref(),
            Some("media-playback-start-symbolic")
        );
        let detail_row = gtk::Button::new();
        let detail_icon = gtk::Image::new();
        register_track(&detail_row, &detail_icon, a, "Faixa 1");
        sync(&uri_a, PlaybackState::Playing);
        assert!(detail_row.has_css_class("playback-current"));
        sync(&uri_a, PlaybackState::Paused);
        assert!(row.has_css_class("playback-paused"));
        assert_eq!(
            icon.icon_name().as_deref(),
            Some("media-playback-pause-symbolic")
        );
        sync(&uri_b, PlaybackState::Playing);
        assert!(!row.has_css_class("playback-current"));
        assert!(!badge.is_visible());
        assert!(row_b.has_css_class("playback-current"));
        assert!(badge_b.is_visible());
        sync(&uri_b, PlaybackState::Stopped);
        assert!(!row_b.has_css_class("playback-current"));
        assert!(!badge_b.is_visible());
        drop(detail_row);
        drop(detail_icon);
        sync(&uri_a, PlaybackState::Playing);
        MARKERS.with(|registry| assert_eq!(registry.borrow().entries.len(), 4));
    }
}
