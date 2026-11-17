//! Aviso de reprodução com recuperação sem alterar a ordem da fila.
use crate::audio::AudioEngine;
use gtk::prelude::*;
use libadwaita::prelude::MessageDialogExt;
use std::cell::RefCell;
use std::rc::Rc;
use std::sync::Arc;

type FailedPlaybackIdentity = (u64, u64, Option<usize>, String);

#[derive(Clone)]
pub struct ErrorPresenter(Rc<Inner>);

struct Inner {
    audio: Arc<AudioEngine>,
    parent: glib::WeakRef<gtk::Window>,
    dialog: RefCell<glib::WeakRef<libadwaita::MessageDialog>>,
    failed: RefCell<Option<FailedPlaybackIdentity>>,
}

impl ErrorPresenter {
    pub fn new(audio: &Arc<AudioEngine>) -> Self {
        Self(Rc::new(Inner {
            audio: audio.clone(),
            parent: glib::WeakRef::new(),
            dialog: RefCell::new(glib::WeakRef::new()),
            failed: RefCell::new(None),
        }))
    }

    pub fn attach(&self, parent: &impl IsA<gtk::Window>) {
        self.0.parent.set(Some(parent.as_ref()));
    }

    pub fn show(&self, details: &str) {
        let Some(parent) = self.0.parent.upgrade() else {
            return;
        };
        let snapshot = self.0.audio.queue_snapshot();
        let metadata = self.0.audio.metadata();
        let identity = (
            self.0.audio.playback_attempt(),
            snapshot.revision,
            snapshot.current,
            metadata.uri.clone(),
        );
        if self.0.failed.borrow().as_ref() == Some(&identity) {
            return;
        }
        *self.0.failed.borrow_mut() = Some(identity.clone());
        let _ = self.0.audio.stop_after_error();
        if let Some(previous) = self.0.dialog.borrow().upgrade() {
            previous.close();
        }
        let missing = glib::filename_from_uri(&metadata.uri)
            .ok()
            .is_some_and(|(p, _)| !p.is_file());
        let reason = if missing {
            "The file is not available. Check if it was moved or if the disk is connected."
        } else {
            "Could not play audio. The file may be corrupted, the format unsupported, or the audio output unavailable."
        };
        let dialog = libadwaita::MessageDialog::builder()
            .transient_for(&parent)
            .destroy_with_parent(true)
            .heading("Unable to play this track")
            .body(format!("{}\n\n{reason}", metadata.title))
            .build();
        dialog.set_widget_name("playback-error");
        dialog.add_css_class("playback-error");
        dialog.set_tooltip_text(Some(details));
        dialog.add_response("close", "Close");
        dialog.add_response("skip", "Skip track");
        dialog.add_response("retry", "Try again");
        dialog.set_close_response("close");
        dialog.set_response_appearance("retry", libadwaita::ResponseAppearance::Suggested);
        let presenter = self.clone();
        dialog.connect_response(None, move |dialog, response| {
            dialog.destroy();
            let (revision, current, _) = presenter.0.audio.queue_status();
            let uri = presenter.0.audio.metadata().uri;
            if (presenter.0.audio.playback_attempt(), revision, current, uri) != identity {
                return;
            }
            let result = match response {
                "retry" => {
                    *presenter.0.failed.borrow_mut() = None;
                    current
                        .map(|index| presenter.0.audio.play_queue_item(revision, index))
                        .unwrap_or(Ok(()))
                }
                "skip" => presenter.0.audio.skip_failed_track().map(|_| ()),
                _ => return,
            };
            if let Err(error) = result {
                *presenter.0.failed.borrow_mut() = None;
                presenter.show(&error.to_string());
            }
        });
        self.0.dialog.borrow_mut().set(Some(&dialog));
        dialog.present();
    }

    pub fn sync(&self) {
        let (revision, current, _) = self.0.audio.queue_status();
        let identity = (
            self.0.audio.playback_attempt(),
            revision,
            current,
            self.0.audio.playback_identity().1,
        );
        let obsolete = self
            .0
            .failed
            .borrow()
            .as_ref()
            .is_some_and(|failed| failed != &identity);
        if obsolete {
            *self.0.failed.borrow_mut() = None;
            if let Some(dialog) = self.0.dialog.borrow().upgrade() {
                dialog.close();
            }
        }
    }
}
