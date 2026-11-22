//! Fila virtualizada: só as linhas visíveis possuem widgets.
use crate::audio::{AudioEngine, PlaybackState};
use crate::playback_queue::QueueSnapshot;
use gtk::prelude::*;
use gtk::{gdk, glib};
use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::path::PathBuf;
use std::rc::Rc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc;
use std::sync::Arc;

#[derive(Clone)]
pub struct QueuePanel {
    pub widget: gtk::Box,
    shared: Rc<Shared>,
}

struct Shared {
    audio: Arc<AudioEngine>,
    model: gtk::StringList,
    search: gtk::SearchEntry,
    tags: RefCell<HashMap<PathBuf, crate::library::TrackTags>>,
    metadata_jobs: mpsc::Sender<(u64, Vec<PathBuf>)>,
    metadata_results: mpsc::Receiver<(PathBuf, crate::library::TrackTags)>,
    generation: Arc<AtomicU64>,
    snapshot: RefCell<Option<QueueSnapshot>>,
    rows: RefCell<Vec<glib::WeakRef<gtk::ListItem>>>,
    count: gtk::Label,
    empty: gtk::Label,
    error: gtk::Label,
    last: Cell<Option<(Option<usize>, PlaybackState, u64)>>,
    drag_id: u64,
}

impl QueuePanel {
    pub fn new(audio: &Arc<AudioEngine>) -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(1);
        let widget = gtk::Box::new(gtk::Orientation::Vertical, 0);
        widget.add_css_class("queue-panel");
        widget.set_widget_name("playback-queue");
        let header = gtk::Box::new(gtk::Orientation::Horizontal, 8);
        header.add_css_class("queue-heading");
        let title = gtk::Label::new(None);
        crate::i18n::bind_label(&title, |l| {
            crate::i18n::pick_for(l, "Playback queue", "Fila de reprodução")
        });
        title.add_css_class("queue-title");
        title.set_hexpand(true);
        title.set_xalign(0.0);
        header.append(&title);
        let current_button = gtk::Button::from_icon_name("go-jump-symbolic");
        current_button.add_css_class("queue-close");
        current_button.set_widget_name("queue-reveal-current");
        crate::i18n::bind_tooltip(&current_button, |l| {
            crate::i18n::pick_for(l, "Show current track", "Mostrar faixa atual")
        });
        header.append(&current_button);
        let close = gtk::Button::from_icon_name("window-close-symbolic");
        close.add_css_class("queue-close");
        crate::i18n::bind_tooltip(&close, |l| {
            crate::i18n::pick_for(l, "Close queue", "Fechar fila")
        });
        close.set_action_name(Some("win.toggle-queue"));
        header.append(&close);
        widget.append(&header);
        let search_box = gtk::Box::new(gtk::Orientation::Horizontal, 6);
        search_box.set_margin_start(20);
        search_box.set_margin_end(20);
        search_box.set_margin_bottom(10);
        let search = gtk::SearchEntry::new();
        search.add_css_class("queue-search");
        search.set_widget_name("queue-search");
        crate::i18n::bind_placeholder(&search, |l| {
            crate::i18n::pick_for(l, "Title, artist, or album", "Título, artista ou álbum")
        });
        search.set_hexpand(true);
        search.update_property(&[gtk::accessible::Property::Label(crate::i18n::pick(
            "Search queue",
            "Buscar na fila",
        ))]);
        search_box.append(&search);
        let clear = gtk::Button::new();
        crate::i18n::bind_button_label(&clear, |l| crate::i18n::pick_for(l, "Clear", "Limpar"));
        clear.add_css_class("queue-clear");
        clear.set_widget_name("queue-search-clear");
        crate::i18n::bind_tooltip(&clear, |l| {
            crate::i18n::pick_for(l, "Clear search", "Limpar busca")
        });
        search_box.append(&clear);
        widget.append(&search_box);
        let count = gtk::Label::new(None);
        count.set_xalign(0.0);
        count.add_css_class("queue-summary");
        widget.append(&count);
        let hint = gtk::Label::new(None);
        crate::i18n::bind_label(&hint, |l| {
            crate::i18n::pick_for(
                l,
                "Drag by handle to reorder. Arrows move items in the full queue, even while filtered.",
                "Arraste pela alça para ordenar. As setas movem na fila completa, mesmo com busca.",
            )
        });
        hint.set_wrap(true);
        hint.set_xalign(0.0);
        hint.add_css_class("queue-hint");
        widget.append(&hint);
        let empty = gtk::Label::new(None);
        crate::i18n::bind_label(&empty, |l| {
            crate::i18n::pick_for(
                l,
                "Your queue is empty.\nOpen a track or play an album.",
                "Sua fila está vazia.\nAbra uma música ou reproduza um álbum.",
            )
        });
        empty.set_wrap(true);
        empty.set_xalign(0.0);
        empty.add_css_class("queue-empty");
        widget.append(&empty);
        let error = gtk::Label::new(None);
        error.set_wrap(true);
        error.add_css_class("queue-error");
        error.set_visible(false);
        widget.append(&error);
        // A single worker reads tags; typing and row binding never invoke ffprobe.
        let (jobs_tx, jobs_rx) = mpsc::channel::<(u64, Vec<PathBuf>)>();
        let (results_tx, results_rx) = mpsc::channel();
        let generation = Arc::new(AtomicU64::new(0));
        let worker_generation = generation.clone();
        std::thread::spawn(move || {
            while let Ok(mut job) = jobs_rx.recv() {
                while let Ok(newer) = jobs_rx.try_recv() {
                    job = newer;
                }
                for path in job.1 {
                    if worker_generation.load(Ordering::Relaxed) != job.0 {
                        break;
                    }
                    let tags = crate::library::read_order(&path);
                    if results_tx.send((path, tags)).is_err() {
                        return;
                    }
                }
            }
        });
        let shared = Rc::new(Shared {
            audio: audio.clone(),
            model: gtk::StringList::new(&[]),
            search: search.clone(),
            tags: RefCell::new(HashMap::new()),
            metadata_jobs: jobs_tx,
            metadata_results: results_rx,
            generation,
            snapshot: RefCell::new(None),
            rows: RefCell::new(Vec::new()),
            count,
            empty,
            error,
            last: Cell::new(None),
            drag_id: NEXT.fetch_add(1, Ordering::Relaxed),
        });
        let weak = Rc::downgrade(&shared);
        search.connect_search_changed(move |_| {
            if let Some(shared) = weak.upgrade() {
                apply_filter(&shared);
            }
        });
        let weak_search = search.downgrade();
        clear.connect_clicked(move |_| {
            if let Some(search) = weak_search.upgrade() {
                search.set_text("");
            }
        });
        let factory = gtk::SignalListItemFactory::new();
        let setup = shared.clone();
        factory.connect_setup(move |_, object| {
            let Some(item) = object.downcast_ref::<gtk::ListItem>() else {
                return;
            };
            setup.rows.borrow_mut().push(item.downgrade());
            setup_row(item, &setup);
        });
        let bind = shared.clone();
        factory.connect_bind(move |_, object| {
            if let Some(item) = object.downcast_ref::<gtk::ListItem>() {
                update_row(item, &bind);
            }
        });
        let selection = gtk::NoSelection::new(Some(shared.model.clone()));
        let list = gtk::ListView::new(Some(selection), Some(factory));
        list.set_widget_name("queue-list");
        list.add_css_class("queue-list");
        list.update_property(&[gtk::accessible::Property::Label("Playback queue")]);
        let scroll = gtk::ScrolledWindow::builder()
            .hscrollbar_policy(gtk::PolicyType::Never)
            .vexpand(true)
            .child(&list)
            .build();
        widget.append(&scroll);
        let weak_list = list.downgrade();
        let current_shared = shared.clone();
        current_button.connect_clicked(move |_| reveal_current(&current_shared, &weak_list));
        let weak_list = list.downgrade();
        let map_shared = shared.clone();
        widget.connect_map(move |_| scroll_current(&map_shared, &weak_list));
        {
            let shared_i18n = shared.clone();
            crate::i18n::connect(&widget, move |_| {
                if shared_i18n.snapshot.borrow().is_some() {
                    apply_filter(&shared_i18n);
                }
            });
        }
        let panel = Self { widget, shared };
        panel.refresh();
        panel
    }

    pub fn refresh(&self) {
        refresh(&self.shared);
    }
}

fn reveal_current(shared: &Rc<Shared>, list: &glib::WeakRef<gtk::ListView>) {
    // Explicit reveal restores the full queue so the current track is always reachable.
    shared.search.set_text("");
    apply_filter(shared);
    scroll_current(shared, list);
}

fn scroll_current(shared: &Rc<Shared>, list: &glib::WeakRef<gtk::ListView>) {
    refresh(shared);
    if let (Some(list), Some(current)) = (
        list.upgrade(),
        shared.snapshot.borrow().as_ref().and_then(|s| s.current),
    ) {
        if let Some(position) = (0..shared.model.n_items())
            .find(|&i| shared.model.string(i).as_deref() == Some(current.to_string().as_str()))
        {
            list.scroll_to(position, gtk::ListScrollFlags::NONE, None);
        }
    }
}

fn refresh(shared: &Rc<Shared>) {
    let (revision, current, _) = shared.audio.queue_status();
    let changed = shared
        .snapshot
        .borrow()
        .as_ref()
        .is_none_or(|s| s.revision != revision);
    if changed {
        let snapshot = shared.audio.queue_snapshot();
        let generation = shared.generation.fetch_add(1, Ordering::Relaxed) + 1;
        let paths: std::collections::HashSet<_> = snapshot.tracks.iter().cloned().collect();
        shared.tags.borrow_mut().retain(|p, _| paths.contains(p));
        let missing = paths
            .into_iter()
            .filter(|p| !shared.tags.borrow().contains_key(p))
            .collect();
        let _ = shared.metadata_jobs.send((generation, missing));
        *shared.snapshot.borrow_mut() = Some(snapshot);
    } else if let Some(snapshot) = shared.snapshot.borrow_mut().as_mut() {
        snapshot.current = current;
    }
    let mut metadata_changed = false;
    while let Ok((path, tags)) = shared.metadata_results.try_recv() {
        shared.tags.borrow_mut().insert(path, tags);
        metadata_changed = true;
    }
    let state = shared.audio.state();
    let current = shared.snapshot.borrow().as_ref().and_then(|s| s.current);
    let identity = (current, state, shared.audio.metadata_version());
    let previous = shared.last.replace(Some(identity));
    if changed || metadata_changed || previous != Some(identity) {
        apply_filter(shared);
    }
}

fn apply_filter(shared: &Rc<Shared>) {
    let query = shared.search.text();
    let indices: Vec<String> = {
        let snapshot = shared.snapshot.borrow();
        let Some(snapshot) = snapshot.as_ref() else {
            return;
        };
        let tags = shared.tags.borrow();
        snapshot
            .tracks
            .iter()
            .enumerate()
            .filter(|(_, path)| queue_matches(path, tags.get(*path), &query))
            .map(|(index, _)| index.to_string())
            .collect()
    };
    let same = shared.model.n_items() as usize == indices.len()
        && indices
            .iter()
            .enumerate()
            .all(|(i, value)| shared.model.string(i as u32).as_deref() == Some(value.as_str()));
    if !same {
        shared.model.splice(
            0,
            shared.model.n_items(),
            &indices.iter().map(String::as_str).collect::<Vec<_>>(),
        );
    }
    {
        let snapshot_guard = shared.snapshot.borrow();
        let Some(snapshot) = snapshot_guard.as_ref() else {
            return;
        };
        let len = snapshot.tracks.len();
        let pending = snapshot
            .tracks
            .iter()
            .any(|path| !shared.tags.borrow().contains_key(path));
        let lang = crate::i18n::current();
        let next = snapshot.current.map_or(len, |i| len.saturating_sub(i + 1));
        if query.trim().is_empty() {
            shared
                .count
                .set_text(&crate::i18n::queue_summary(lang, len, next));
        } else {
            shared.count.set_text(&crate::i18n::queue_search_summary(
                lang,
                indices.len(),
                len,
                pending,
            ));
        }
        shared.empty.set_text(if len == 0 {
            crate::i18n::pick_for(
                lang,
                "Your queue is empty.\nOpen a track or play an album.",
                "Sua fila está vazia.\nAbra uma música ou reproduza um álbum.",
            )
        } else if pending {
            crate::i18n::pick_for(lang, "Reading queue metadata…", "Lendo metadados da fila…")
        } else {
            crate::i18n::pick_for(
                lang,
                "No tracks found.\nTry another search term or clear.",
                "Nenhuma faixa encontrada.\nTente outro termo ou limpe a busca.",
            )
        });
        shared.empty.set_visible(indices.is_empty());
    }
    shared.rows.borrow_mut().retain(|weak| {
        if let Some(item) = weak.upgrade() {
            update_row(&item, shared);
            true
        } else {
            false
        }
    });
}

fn queue_matches(
    path: &std::path::Path,
    tags: Option<&crate::library::TrackTags>,
    query: &str,
) -> bool {
    if query.trim().is_empty() {
        return true;
    }
    let mut text = path.to_string_lossy().into_owned();
    if let Some(tags) = tags {
        for value in [&tags.title, &tags.artist, &tags.performer, &tags.album]
            .into_iter()
            .flatten()
        {
            text.push(' ');
            text.push_str(value);
        }
    }
    crate::search::matches_query(&text, query)
}

fn row_index(item: &gtk::ListItem, shared: &Shared) -> Option<(u64, usize)> {
    item.item()?;
    // Model values are original ordinals, never filtered positions or path identity.
    let index = item
        .item()?
        .downcast::<gtk::StringObject>()
        .ok()?
        .string()
        .parse::<usize>()
        .ok()?;
    shared
        .snapshot
        .borrow()
        .as_ref()
        .filter(|s| index < s.tracks.len())
        .map(|s| (s.revision, index))
}

fn report(shared: &Rc<Shared>, result: anyhow::Result<()>) {
    if let Err(error) = result {
        shared.error.set_text(&error.to_string());
        shared.error.set_visible(true);
    } else {
        shared.error.set_visible(false);
    }
    refresh(shared);
}

fn setup_row(item: &gtk::ListItem, shared: &Rc<Shared>) {
    let row = gtk::Box::new(gtk::Orientation::Horizontal, 4);
    row.add_css_class("queue-row");
    let handle = gtk::Image::from_icon_name("list-drag-handle-symbolic");
    handle.add_css_class("queue-handle");
    crate::i18n::bind_tooltip(&handle, |l| {
        crate::i18n::pick_for(l, "Drag to reorder", "Arraste para reorganizar")
    });
    row.append(&handle);
    let play = gtk::Button::new();
    play.add_css_class("queue-track");
    play.set_hexpand(true);
    let labels = gtk::Box::new(gtk::Orientation::Vertical, 3);
    for class in ["queue-track-title", "queue-track-subtitle"] {
        let label = gtk::Label::new(None);
        label.set_xalign(0.0);
        label.set_ellipsize(gtk::pango::EllipsizeMode::End);
        label.set_max_width_chars(20);
        label.add_css_class(class);
        labels.append(&label);
    }
    play.set_child(Some(&labels));
    row.append(&play);
    let buttons = gtk::Box::new(gtk::Orientation::Vertical, 0);
    for (icon, tip_fn, direction) in [
        (
            "go-up-symbolic",
            (|l| crate::i18n::pick_for(l, "Move up", "Mover para cima"))
                as fn(crate::i18n::Language) -> &'static str,
            -1_isize,
        ),
        (
            "go-down-symbolic",
            (|l| crate::i18n::pick_for(l, "Move down", "Mover para baixo"))
                as fn(crate::i18n::Language) -> &'static str,
            1,
        ),
        (
            "list-remove-symbolic",
            (|l| crate::i18n::pick_for(l, "Remove from queue", "Remover da fila"))
                as fn(crate::i18n::Language) -> &'static str,
            0,
        ),
    ] {
        let button = gtk::Button::from_icon_name(icon);
        crate::i18n::bind_tooltip(&button, tip_fn);
        button.add_css_class("queue-action");
        button.set_widget_name(match direction {
            -1 => "queue-up",
            1 => "queue-down",
            _ => "queue-remove",
        });
        let weak = item.downgrade();
        let shared = shared.clone();
        button.connect_clicked(move |_| {
            let Some(item) = weak.upgrade() else {
                return;
            };
            let Some((revision, index)) = row_index(&item, &shared) else {
                return;
            };
            let result = if direction == 0 {
                shared.audio.remove_queue_item(revision, index)
            } else {
                shared.audio.move_queue_item(
                    revision,
                    index,
                    index.saturating_add_signed(direction),
                )
            };
            report(&shared, result);
        });
        buttons.append(&button);
    }
    row.append(&buttons);
    let weak = item.downgrade();
    let play_shared = shared.clone();
    play.connect_clicked(move |_| {
        let Some(item) = weak.upgrade() else {
            return;
        };
        let Some((revision, index)) = row_index(&item, &play_shared) else {
            return;
        };
        report(
            &play_shared,
            play_shared.audio.play_queue_item(revision, index),
        );
    });
    let source = gtk::DragSource::new();
    source.set_actions(gdk::DragAction::MOVE);
    let weak = item.downgrade();
    let source_shared = shared.clone();
    source.connect_prepare(move |_, _, _| {
        let item = weak.upgrade()?;
        let (revision, index) = row_index(&item, &source_shared)?;
        let payload = format!(
            "vinyl-queue:{}:{}:{revision}:{index}",
            std::process::id(),
            source_shared.drag_id
        );
        Some(gdk::ContentProvider::for_value(&payload.to_value()))
    });
    handle.add_controller(source);
    let target = gtk::DropTarget::new(String::static_type(), gdk::DragAction::MOVE);
    let weak_row = row.downgrade();
    target.connect_motion(move |_, _, y| {
        if let Some(row) = weak_row.upgrade() {
            row.remove_css_class("queue-drop-before");
            row.remove_css_class("queue-drop-after");
            row.add_css_class(if y < row.height() as f64 / 2.0 {
                "queue-drop-before"
            } else {
                "queue-drop-after"
            });
        }
        gdk::DragAction::MOVE
    });
    let weak_row = row.downgrade();
    target.connect_leave(move |_| {
        if let Some(row) = weak_row.upgrade() {
            row.remove_css_class("queue-drop-before");
            row.remove_css_class("queue-drop-after");
        }
    });
    let weak = item.downgrade();
    let weak_row = row.downgrade();
    let drop_shared = shared.clone();
    target.connect_drop(move |_, value, _, y| {
        let Some(row) = weak_row.upgrade() else {
            return false;
        };
        row.remove_css_class("queue-drop-before");
        row.remove_css_class("queue-drop-after");
        let (Some(item), Ok(payload)) = (weak.upgrade(), value.get::<String>()) else {
            return false;
        };
        let Some((revision, index)) = row_index(&item, &drop_shared) else {
            return false;
        };
        let prefix = format!(
            "vinyl-queue:{}:{}:",
            std::process::id(),
            drop_shared.drag_id
        );
        let Some((version, from)) = payload
            .strip_prefix(&prefix)
            .and_then(|s| s.split_once(':'))
        else {
            return false;
        };
        let (Ok(version), Ok(from)) = (version.parse::<u64>(), from.parse::<usize>()) else {
            return false;
        };
        if version != revision {
            report(
                &drop_shared,
                Err(anyhow::anyhow!(crate::i18n::pick(
                    "The queue changed. Drag again.",
                    "A fila mudou. Arraste novamente.",
                ))),
            );
            return false;
        }
        let slot = index + usize::from(y >= row.height() as f64 / 2.0);
        let to = slot.saturating_sub(usize::from(from < slot));
        let result = drop_shared.audio.move_queue_item(version, from, to);
        let accepted = result.is_ok();
        report(&drop_shared, result);
        accepted
    });
    row.add_controller(target);
    item.set_child(Some(&row));
}

fn update_row(item: &gtk::ListItem, shared: &Shared) {
    let Some((_, index)) = row_index(item, shared) else {
        return;
    };
    let snapshot_guard = shared.snapshot.borrow();
    let Some(snapshot) = snapshot_guard.as_ref() else {
        return;
    };
    let Some(path) = snapshot.tracks.get(index) else {
        return;
    };
    let Some(child) = item.child() else {
        return;
    };
    let Ok(row) = child.downcast::<gtk::Box>() else {
        return;
    };
    let Some(play) = row
        .first_child()
        .and_then(|c| c.next_sibling())
        .and_then(|c| c.downcast::<gtk::Button>().ok())
    else {
        return;
    };
    let Some(labels) = play.child() else {
        return;
    };
    let Some(title) = labels
        .first_child()
        .and_then(|c| c.downcast::<gtk::Label>().ok())
    else {
        return;
    };
    let Some(subtitle) = labels
        .last_child()
        .and_then(|c| c.downcast::<gtk::Label>().ok())
    else {
        return;
    };
    let current = snapshot.current == Some(index);
    let state = shared.audio.state();
    let name = path.file_stem().unwrap_or_default().to_string_lossy();
    if current {
        let metadata = shared.audio.metadata();
        title.set_text(if metadata.title.is_empty() {
            &name
        } else {
            &metadata.title
        });
        subtitle.set_text(match (crate::i18n::current(), state) {
            (crate::i18n::Language::En, PlaybackState::Playing) => "Now playing",
            (crate::i18n::Language::PtBr, PlaybackState::Playing) => "Em reprodução",
            (crate::i18n::Language::En, PlaybackState::Paused) => "Paused",
            (crate::i18n::Language::PtBr, PlaybackState::Paused) => "Pausada",
            (crate::i18n::Language::En, PlaybackState::Stopped) => "Selected",
            (crate::i18n::Language::PtBr, PlaybackState::Stopped) => "Selecionada",
        });
        row.add_css_class("queue-current");
    } else {
        let tags = shared.tags.borrow();
        title.set_text(
            tags.get(path)
                .and_then(|tags| tags.title.as_deref())
                .unwrap_or(&name),
        );
        let details = tags
            .get(path)
            .map(|tags| {
                [
                    tags.performer.as_deref().or(tags.artist.as_deref()),
                    tags.album.as_deref(),
                ]
                .into_iter()
                .flatten()
                .collect::<Vec<_>>()
                .join(" · ")
            })
            .filter(|text| !text.is_empty());
        let folder = path
            .parent()
            .and_then(|p| p.file_name())
            .unwrap_or_default()
            .to_string_lossy();
        subtitle.set_text(details.as_deref().unwrap_or(&folder));
        row.remove_css_class("queue-current");
    }
    play.set_tooltip_text(Some(&path.to_string_lossy()));
    let play_label = format!("{} {}", crate::i18n::pick("Play", "Tocar"), title.text());
    play.update_property(&[gtk::accessible::Property::Label(&play_label)]);
    if let Some(buttons) = row.last_child() {
        if let Some(up) = buttons.first_child() {
            up.set_sensitive(index > 0);
            if let Some(down) = up.next_sibling() {
                down.set_sensitive(index + 1 < snapshot.tracks.len());
            }
        }
    }
}
