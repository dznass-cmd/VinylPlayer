//! Entrada de arquivos locais pelo GApplication (.desktop %U e linha de comando).
use crate::audio::AudioEngine;
use gtk::prelude::*;
use libadwaita::prelude::MessageDialogExt;
use std::cell::RefCell;
use std::collections::HashSet;
use std::path::PathBuf;
use std::rc::Rc;
use std::sync::Arc;

#[derive(Default)]
struct Request {
    paths: Vec<PathBuf>,
    rejected: Vec<String>,
}

fn validate(files: &[gtk::gio::File]) -> Request {
    let mut request = Request::default();
    let mut seen = HashSet::new();
    for file in files {
        let Some(path) = file.path() else {
            request
                .rejected
                .push(format!("{}: only local files are supported", file.uri()));
            continue;
        };
        let extension = path
            .extension()
            .and_then(|s| s.to_str())
            .unwrap_or("")
            .to_ascii_lowercase();
        let lang = crate::i18n::current();
        let reason = if !path.is_file() {
            Some(crate::i18n::pick_for(
                lang,
                "file not found or not a regular file",
                "arquivo não encontrado ou inválido",
            ))
        } else if ![
            "flac", "mp3", "ogg", "oga", "wav", "m4a", "aac", "opus", "alac", "aif", "aiff", "wma",
            "m3u", "m3u8",
        ]
        .contains(&extension.as_str())
        {
            Some(crate::i18n::pick_for(
                lang,
                "unrecognized audio format",
                "formato de áudio não reconhecido",
            ))
        } else {
            None
        };
        if let Some(reason) = reason {
            request
                .rejected
                .push(format!("{}: {reason}", path.display()));
        } else if seen.insert(path.clone()) {
            request.paths.push(path);
        }
    }
    request
}

struct Session {
    audio: Arc<AudioEngine>,
    window: glib::WeakRef<gtk::Window>,
}

type Factory = Rc<dyn Fn(&gtk::Application) -> Arc<AudioEngine>>;

fn ensure_session(
    app: &gtk::Application,
    session: &RefCell<Option<Session>>,
    factory: &Factory,
) -> (Arc<AudioEngine>, gtk::Window) {
    if let Some(existing) = session.borrow().as_ref() {
        if let Some(window) = existing.window.upgrade().filter(|w| w.is_realized()) {
            return (existing.audio.clone(), window);
        }
    }
    let audio = factory(app);
    let window = app
        .active_window()
        .or_else(|| app.windows().first().cloned())
        .unwrap_or_else(|| {
            app.activate();
            app.active_window()
                .or_else(|| app.windows().first().cloned())
                .expect("janela do player após ativação")
        });
    *session.borrow_mut() = Some(Session {
        audio: audio.clone(),
        window: window.downgrade(),
    });
    (audio, window)
}

fn show_errors(window: &gtk::Window, errors: &[String]) {
    if errors.is_empty() {
        return;
    }
    let mut body = errors
        .iter()
        .take(5)
        .cloned()
        .collect::<Vec<_>>()
        .join("\n\n");
    let lang = crate::i18n::current();
    if errors.len() > 5 {
        let extra = errors.len() - 5;
        body.push_str(&match lang {
            crate::i18n::Language::En => format!("\n\nAnd {extra} more file(s)."),
            crate::i18n::Language::PtBr => format!("\n\nE mais {extra} arquivo(s)."),
        });
    }
    let dialog = libadwaita::MessageDialog::builder()
        .transient_for(window)
        .destroy_with_parent(true)
        .heading(crate::i18n::pick_for(
            lang,
            "Could not open some files",
            "Não foi possível abrir alguns arquivos",
        ))
        .body(&body)
        .build();
    dialog.add_response("ok", "OK");
    dialog.set_close_response("ok");
    dialog.present();
}

pub fn connect_handlers(
    app: &gtk::Application,
    factory: impl Fn(&gtk::Application) -> Arc<AudioEngine> + 'static,
) {
    let session = Rc::new(RefCell::new(None));
    let factory: Factory = Rc::new(factory);
    let active_session = session.clone();
    let active_factory = factory.clone();
    app.connect_activate(move |app| {
        let (_, window) = ensure_session(app, &active_session, &active_factory);
        window.present();
    });
    app.connect_open(move |app, files, _hint| {
        let mut request = validate(files);
        let (audio, window) = ensure_session(app, &session, &factory);
        if !request.paths.is_empty() {
            let mut playable_tracks = Vec::new();
            for p in &request.paths {
                let ext = p
                    .extension()
                    .and_then(|s| s.to_str())
                    .unwrap_or("")
                    .to_ascii_lowercase();
                if ext == "m3u" || ext == "m3u8" {
                    match crate::playlist::Playlist::load_from_file(p) {
                        Ok(pl) => playable_tracks.extend(pl.tracks),
                        Err(e) => request.rejected.push(match crate::i18n::current() {
                            crate::i18n::Language::En => {
                                format!("{}: failed to read playlist ({e})", p.display())
                            }
                            crate::i18n::Language::PtBr => {
                                format!("{}: falha ao ler playlist ({e})", p.display())
                            }
                        }),
                    }
                } else {
                    playable_tracks.push(p.clone());
                }
            }
            if !playable_tracks.is_empty() {
                match audio.play_files(&playable_tracks, 0) {
                    Ok(()) => {
                        if let Some(window) = window.downcast_ref::<gtk::ApplicationWindow>() {
                            gtk::gio::prelude::ActionGroupExt::activate_action(
                                window,
                                "show-turntable",
                                None,
                            );
                        }
                    }
                    Err(error) => request.rejected.push(match crate::i18n::current() {
                        crate::i18n::Language::En => {
                            format!("Failed to start playback: {error}")
                        }
                        crate::i18n::Language::PtBr => {
                            format!("Falha ao iniciar a reprodução: {error}")
                        }
                    }),
                }
            }
        }
        window.present();
        show_errors(&window, &request.rejected);
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepts_paths_and_encoded_uris_in_selection_order_without_duplicates() {
        let config = crate::state::TestConfig::new();
        let a = config.path().with_file_name("02 - Música #1.wav");
        let b = config.path().with_file_name("01 - Outra faixa.FLAC");
        std::fs::write(&a, b"fixture").unwrap();
        std::fs::write(&b, b"fixture").unwrap();
        let files = [
            gtk::gio::File::for_path(&a),
            gtk::gio::File::for_uri(&glib::filename_to_uri(&b, None).unwrap()),
            gtk::gio::File::for_path(&a),
        ];
        let result = validate(&files);
        assert_eq!(result.paths, [a, b]);
        assert!(result.rejected.is_empty());
    }

    #[test]
    fn rejects_directories_missing_files_remote_uris_and_non_audio_files() {
        let config = crate::state::TestConfig::new();
        let text = config.path().with_file_name("notes.txt");
        std::fs::write(&text, b"text").unwrap();
        let files = [
            gtk::gio::File::for_path(config.path()),
            gtk::gio::File::for_path(config.path().parent().unwrap()),
            gtk::gio::File::for_path(text),
            gtk::gio::File::for_uri("https://example.invalid/music.wav"),
        ];
        let result = validate(&files);
        assert!(result.paths.is_empty());
        assert_eq!(result.rejected.len(), 4);
    }

    #[test]
    fn mixed_request_keeps_valid_audio_without_reordering() {
        let config = crate::state::TestConfig::new();
        let path = config.path().with_file_name("track.wav");
        std::fs::write(&path, b"fixture").unwrap();
        let result = validate(&[
            gtk::gio::File::for_path(config.path()),
            gtk::gio::File::for_path(&path),
        ]);
        assert_eq!(result.paths, [path]);
        assert_eq!(result.rejected.len(), 1);
    }

    #[test]
    fn accepts_m3u_and_m3u8_playlists() {
        let config = crate::state::TestConfig::new();
        let m3u = config.path().with_file_name("lista.m3u");
        let m3u8 = config.path().with_file_name("lista.m3u8");
        std::fs::write(&m3u, b"#EXTM3U\n").unwrap();
        std::fs::write(&m3u8, b"#EXTM3U\n").unwrap();
        let result = validate(&[
            gtk::gio::File::for_path(&m3u),
            gtk::gio::File::for_path(&m3u8),
        ]);
        assert_eq!(result.paths, [m3u, m3u8]);
        assert!(result.rejected.is_empty());
    }
}
