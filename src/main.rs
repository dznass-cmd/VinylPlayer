#![allow(deprecated)]
use gtk::gdk;
use gtk::glib;
use gtk::prelude::*;
use gtk::{Application, ApplicationWindow, DrawingArea, Orientation};
use std::cell::{Cell, RefCell};
use std::f64::consts::TAU;
use std::path::Path;
use std::rc::Rc;
use std::sync::Arc;

fn show_track_notification(title: &str, artist: &str, album: &str, cover_path: Option<&Path>) {
    let summary = if title.trim().is_empty() {
        "VinylPlayer"
    } else {
        title.trim()
    };
    let mut body_parts = Vec::new();
    let art = artist.trim();
    if !art.is_empty() {
        body_parts.push(art);
    }
    let alb = album.trim();
    if !alb.is_empty() {
        body_parts.push(alb);
    }
    let body = if body_parts.is_empty() {
        i18n::pick("High-Fidelity Analog Audio", "Alta Fidelidade Analógica").to_string()
    } else {
        body_parts.join(" · ")
    };

    let icon = cover_path
        .and_then(|p| p.to_str())
        .filter(|s| !s.is_empty())
        .unwrap_or("vinylplayer")
        .to_string();

    let summary = summary.to_string();

    std::thread::spawn(move || {
        let _ = std::process::Command::new("notify-send")
            .arg("-a")
            .arg("VinylPlayer")
            .arg("-i")
            .arg(&icon)
            .arg("-h")
            .arg("string:x-canonical-private-synchronous:vinylplayer")
            .arg(&summary)
            .arg(&body)
            .status();
    });
}

pub(crate) fn show_system_notification(summary: &str, body: &str) {
    let summary = summary.to_string();
    let body = body.to_string();
    std::thread::spawn(move || {
        let _ = std::process::Command::new("notify-send")
            .arg("-a")
            .arg("VinylPlayer")
            .arg("-i")
            .arg("vinylplayer")
            .arg(&summary)
            .arg(&body)
            .status();
    });
}

mod audio;
mod donate;
mod file_open;
mod i18n;
mod library;
mod library_settings;
mod library_sources;
mod mpris;
mod needle_drop;
mod now_playing;
mod playback_error;
mod playback_queue;
mod playlist;
mod playlist_view;
mod queue_view;
mod scrobbler;
mod search;
mod state;
mod theme;
mod thumb_cache;
mod turntable;
mod volume_hud;
mod vu_meter;

thread_local! {
    static RAISE_WINDOWS: RefCell<Vec<glib::WeakRef<ApplicationWindow>>> = const { RefCell::new(Vec::new()) };
    static BASE_CSS_PROVIDER: RefCell<Option<gtk::CssProvider>> = const { RefCell::new(None) };
}

use audio::{AudioEngine, PlaybackState, RepeatMode};
use gstreamer::prelude::ElementExt;
pub(crate) use turntable::{
    draw_turntable, CachedCover, TurntableCache, TurntableLayout, TurntableRenderParams,
};
pub(crate) use volume_hud::VolumeHudState;

use adw::prelude::MessageDialogExt;
use libadwaita as adw;

fn main() {
    adw::init().expect("libadwaita");
    adw::StyleManager::default().set_color_scheme(adw::ColorScheme::PreferDark);
    let app = Application::builder()
        .application_id("io.github.vinylplayer")
        .flags(gtk::gio::ApplicationFlags::HANDLES_OPEN)
        .build();
    file_open::connect_handlers(&app, build_ui_with_audio);
    app.run();
}

fn build_ui_with_audio(app: &Application) -> Arc<AudioEngine> {
    build_ui_with_state(app, state::StateManager::new())
}

fn build_ui_with_state(app: &Application, state_mgr: state::StateManager) -> Arc<AudioEngine> {
    build_ui_with_catalog(app, state_mgr).0
}

fn build_ui_with_catalog(
    app: &Application,
    state_mgr: state::StateManager,
) -> (Arc<AudioEngine>, Rc<library_settings::Catalog>) {
    let initial_state = state_mgr.snapshot();
    i18n::init_from_state(initial_state.language.as_deref());
    let initial_theme = theme::Theme::from_id(initial_state.theme());
    theme::apply_theme(initial_theme);

    let audio = Arc::new(
        match &initial_state.queue {
            Some(queue) => {
                let requested = queue.current.unwrap_or(0);
                let available: Vec<_> = queue
                    .tracks
                    .iter()
                    .enumerate()
                    .filter(|(_, path)| path.is_file())
                    .collect();
                let current = available
                    .iter()
                    .position(|(index, _)| *index >= requested)
                    .or_else(|| available.len().checked_sub(1));
                AudioEngine::with_queue(
                    available
                        .into_iter()
                        .map(|(_, path)| path.clone())
                        .collect(),
                    current,
                )
            }
            None => AudioEngine::with_queue(Vec::new(), None),
        }
        .expect("GStreamer/PipeWire"),
    );
    audio.set_volume(initial_state.volume);
    audio.set_shuffle(initial_state.shuffle);
    audio.set_repeat_mode(initial_state.repeat_mode);

    if initial_state.queue.is_none() {
        if let Some(track) = initial_state
            .last_track_path
            .as_ref()
            .filter(|p| p.is_file())
        {
            let _ = audio.load(track);
        }
    }
    let audio_mpris = audio.clone();
    let raise_cb: Arc<dyn Fn() + Send + Sync> = Arc::new(|| {
        glib::MainContext::default().invoke(|| {
            RAISE_WINDOWS.with(|cell| {
                let mut windows = cell.borrow_mut();
                windows.retain(|w| w.upgrade().is_some());
                if let Some(win) = windows.last().and_then(|w| w.upgrade()) {
                    win.present();
                }
            });
        });
    });
    let quit_cb: Arc<dyn Fn() + Send + Sync> = Arc::new(|| {
        glib::MainContext::default().invoke(|| {
            RAISE_WINDOWS.with(|cell| {
                let windows = cell.borrow();
                for win in windows.iter() {
                    if let Some(w) = win.upgrade() {
                        w.close();
                    }
                }
            });
        });
    });
    glib::MainContext::default().spawn_local(async move {
        let _ = mpris::serve(audio_mpris, raise_cb, quit_cb).await;
    });

    let playing = Rc::new(Cell::new(false));
    let angle = Rc::new(Cell::new(0.0_f64));
    let arm_angle = Rc::new(Cell::new(1.476_f64)); // Posição de repouso no cradle (84.6°)
    let volume_hud = Rc::new(RefCell::new(VolumeHudState {
        level: initial_state.volume,
        alpha: 0.0,
        last_active: None,
    }));

    let root = gtk::Box::new(Orientation::Vertical, 0);
    root.add_css_class("app-shell");

    let body = gtk::Box::new(Orientation::Horizontal, 0);
    let (nav, nav_buttons) = sidebar();

    let search_hub = search::SearchHub::new();

    let title_label = gtk::Label::new(Some(i18n::nav_title(i18n::current(), "overview")));
    title_label.add_css_class("page-title");
    title_label.set_hexpand(true);
    title_label.set_xalign(0.0);

    let stack = gtk::Stack::new();
    stack.set_widget_name("main-pages");
    stack.set_hexpand(true);
    stack.set_vexpand(true);
    {
        let title_weak = title_label.downgrade();
        let stack_weak = stack.downgrade();
        i18n::connect(&title_label, move |lang| {
            if let (Some(tl), Some(st)) = (title_weak.upgrade(), stack_weak.upgrade()) {
                let page = st.visible_child_name().unwrap_or_else(|| "overview".into());
                tl.set_text(i18n::nav_title(lang, &page));
            }
        });
    }

    let catalog = library_settings::Catalog::new(state_mgr.clone());
    let initial_tracks = catalog.report.borrow().tracks.clone();

    let queue = queue_view::QueuePanel::new(&audio);
    let queue_split = adw::OverlaySplitView::builder()
        .content(&stack)
        .sidebar(&queue.widget)
        .sidebar_position(gtk::PackType::End)
        .min_sidebar_width(350.0)
        .max_sidebar_width(390.0)
        .collapsed(true)
        .show_sidebar(false)
        .build();

    let content = gtk::Box::new(Orientation::Vertical, 0);
    content.set_hexpand(true);
    let (topbar_widget, search_entry, open_btn) = header(
        &audio,
        &search_hub,
        &stack,
        &nav_buttons,
        &title_label,
        &catalog,
    );
    let Ok(topbar) = topbar_widget.downcast::<gtk::Box>() else {
        return (audio, catalog);
    };
    let queue_toggle = gtk::ToggleButton::new();
    queue_toggle.add_css_class("queue-toggle");
    i18n::bind_tooltip(&queue_toggle, |l| {
        i18n::pick_for(l, "Toggle queue · Ctrl+J", "Alternar fila · Ctrl+J")
    });
    let queue_label = gtk::Box::new(Orientation::Horizontal, 7);
    queue_label.append(&gtk::Image::from_icon_name("view-list-symbolic"));
    let queue_btn_txt = gtk::Label::new(None);
    i18n::bind_label(&queue_btn_txt, |l| i18n::pick_for(l, "Queue", "Fila"));
    queue_label.append(&queue_btn_txt);
    queue_toggle.set_child(Some(&queue_label));
    queue_toggle
        .bind_property("active", &queue_split, "show-sidebar")
        .bidirectional()
        .sync_create()
        .build();
    topbar.append(&queue_toggle);
    content.append(&topbar);

    stack.add_named(
        &main_stage(
            &playing,
            &angle,
            &arm_angle,
            &audio,
            &state_mgr,
            &volume_hud,
        ),
        Some("overview"),
    );

    stack.add_named(
        &library_view(
            &audio,
            false,
            &search_hub,
            &state_mgr,
            initial_tracks.clone(),
        ),
        Some("tracks"),
    );
    stack.add_named(
        &library_view(
            &audio,
            true,
            &search_hub,
            &state_mgr,
            initial_tracks.clone(),
        ),
        Some("albums"),
    );
    let playlist_mgr = playlist::PlaylistManager::new(None);
    stack.add_named(
        &playlist_view::playlist_library_view(&audio, &playlist_mgr),
        Some("playlists"),
    );
    let library_status = gtk::Label::new(Some(&catalog.summary()));
    library_status.set_xalign(0.0);
    library_status.add_css_class("library-status");
    content.append(&library_status);
    let weak_status = library_status.downgrade();
    let weak_stack = stack.downgrade();
    let library_audio = audio.clone();
    let library_search = search_hub.clone();
    let initialize_queue = Cell::new(initial_state.queue.is_none());
    let seed_revision = audio.queue_status().0;
    let last_tracks = Rc::new(RefCell::new(initial_tracks));
    let last_tracks_cb = last_tracks.clone();
    {
        let cat_i18n = catalog.clone();
        let status_weak_i18n = library_status.downgrade();
        i18n::connect(&library_status, move |_| {
            if let Some(lbl) = status_weak_i18n.upgrade() {
                lbl.set_text(&cat_i18n.summary());
            }
        });
    }
    catalog.connect(&stack, move |catalog| {
        if let Some(label) = weak_status.upgrade() {
            label.set_text(&catalog.summary());
            label.set_tooltip_text(Some(&catalog.report.borrow().warnings.join("\n")));
        }
        if catalog.scanning.get() {
            return;
        }
        let Some(stack) = weak_stack.upgrade() else {
            return;
        };
        let report = catalog.report.borrow();
        if initialize_queue.replace(false) {
            let _ = library_audio.seed_library_if_unchanged(seed_revision, &report.tracks);
        }

        // Se as faixas não mudaram desde a última carga, NÃO reconstrói as views da UI
        if *last_tracks_cb.borrow() == report.tracks {
            return;
        }
        *last_tracks_cb.borrow_mut() = report.tracks.clone();

        let visible = stack.visible_child_name();
        for (page, albums) in [("tracks", false), ("albums", true)] {
            if let Some(old) = stack.child_by_name(page) {
                stack.remove(&old);
            }
            stack.add_named(
                &library_view(
                    &library_audio,
                    albums,
                    &library_search,
                    &catalog.state,
                    report.tracks.clone(),
                ),
                Some(page),
            );
        }
        if let Some(visible) = visible {
            stack.set_visible_child_name(&visible);
        }
    });
    content.append(&queue_split);

    // Suporte a arrastar e soltar arquivos, pastas e playlists na janela
    let drop = gtk::DropTarget::new(glib::types::Type::INVALID, gdk::DragAction::COPY);
    drop.set_types(&[gdk::FileList::static_type(), glib::types::Type::STRING]);
    let drop_audio = audio.clone();
    let drop_stack = stack.downgrade();
    let drop_title = title_label.downgrade();
    let drop_nav_btns: Vec<(gtk::Button, &'static str)> = nav_buttons
        .iter()
        .map(|(b, p, _)| (b.clone(), *p))
        .collect();
    drop.connect_drop(move |_, value, _, _| {
        let mut paths = Vec::new();
        if let Ok(file_list) = value.get::<gdk::FileList>() {
            for file in file_list.files() {
                if let Some(path) = file.path() {
                    paths.push(path);
                }
            }
        } else if let Ok(text) = value.get::<String>() {
            for raw in text.lines() {
                let uri = raw.trim();
                if uri.starts_with("file://") {
                    if let Ok((path, _)) = glib::filename_from_uri(uri) {
                        paths.push(path);
                    }
                }
            }
        } else {
            return false;
        }
        if paths.is_empty() {
            return false;
        }
        let audio_bg = drop_audio.clone();
        let stack_weak = drop_stack.clone();
        let title_weak = drop_title.clone();
        let nav_btns = drop_nav_btns.clone();
        glib::MainContext::default().spawn_local(async move {
            let playable = gtk::gio::spawn_blocking(move || {
                let mut collected = Vec::new();
                for path in paths {
                    if path.is_dir() {
                        let scanned = library_sources::scan(std::slice::from_ref(&path));
                        collected.extend(scanned.tracks);
                        continue;
                    }
                    if !path.is_file() {
                        continue;
                    }
                    let ext = path
                        .extension()
                        .and_then(|s| s.to_str())
                        .unwrap_or("")
                        .to_ascii_lowercase();
                    if ext == "m3u" || ext == "m3u8" {
                        if let Ok(pl) = playlist::Playlist::load_from_file(&path) {
                            collected.extend(pl.tracks.into_iter().filter(|p| p.is_file()));
                        }
                    } else if matches!(
                        ext.as_str(),
                        "flac"
                            | "mp3"
                            | "ogg"
                            | "oga"
                            | "wav"
                            | "m4a"
                            | "aac"
                            | "opus"
                            | "alac"
                            | "aif"
                            | "aiff"
                            | "wma"
                    ) {
                        collected.push(path);
                    }
                }
                collected
            })
            .await
            .unwrap_or_default();

            if !playable.is_empty() && audio_bg.play_files(&playable, 0).is_ok() {
                if let Some(st) = stack_weak.upgrade() {
                    st.set_visible_child_name("overview");
                }
                if let Some(tl) = title_weak.upgrade() {
                    tl.set_text(i18n::nav_title(i18n::current(), "overview"));
                }
                for (b, page) in &nav_btns {
                    if *page == "overview" {
                        b.add_css_class("active-nav");
                    } else {
                        b.remove_css_class("active-nav");
                    }
                }
            }
        });
        true
    });
    root.add_controller(drop);

    // Bus integrado ao main loop do GTK
    let bus = audio.element().bus().expect("bus GStreamer");
    let playback_errors = playback_error::ErrorPresenter::new(&audio);
    let bus_errors = playback_errors.clone();
    let audio_bus = audio.clone();
    let playing_bus = playing.clone();
    let bus_watch = bus
        .add_watch_local(move |_, msg| {
            use gstreamer::MessageView;
            audio_bus.handle_message(msg);
            match msg.view() {
                MessageView::StateChanged(s)
                    if msg.src().map(|s| s == audio_bus.element()).unwrap_or(false) =>
                {
                    match s.current() {
                        gstreamer::State::Playing => {
                            audio_bus.set_state(PlaybackState::Playing);
                            playing_bus.set(true);
                        }
                        gstreamer::State::Paused => {
                            audio_bus.set_state(PlaybackState::Paused);
                            playing_bus.set(false);
                        }
                        gstreamer::State::Ready | gstreamer::State::Null => {
                            audio_bus.set_state(PlaybackState::Stopped);
                            playing_bus.set(false);
                        }
                        _ => {}
                    }
                }
                MessageView::Eos(..) => match audio_bus.advance_after_eos() {
                    Ok(active) => playing_bus.set(active),
                    Err(error) => {
                        eprintln!("Reprodução do álbum: {error}");
                        let _ = audio_bus.stop();
                        playing_bus.set(false);
                        bus_errors.show(&error.to_string());
                    }
                },
                MessageView::Error(e) => {
                    eprintln!("GStreamer: {}", e.error());
                    playing_bus.set(false);
                    bus_errors.show(&e.error().to_string());
                }
                _ => {}
            }
            glib::ControlFlow::Continue
        })
        .expect("escutar eventos de áudio");

    let all_buttons: Vec<gtk::Button> = nav_buttons.iter().map(|(b, _, _)| b.clone()).collect();
    for (button, page, _title) in &nav_buttons {
        let stack = stack.clone();
        let title_label = title_label.clone();
        let all_btns = all_buttons.clone();
        let curr_btn = button.clone();
        let page_name = page.to_string();
        let page_key = *page;
        let state_nav = state_mgr.clone();
        button.connect_clicked(move |_| {
            stack.set_visible_child_name(&page_name);
            title_label.set_text(i18n::nav_title(i18n::current(), page_key));
            for b in &all_btns {
                b.remove_css_class("active-nav");
            }
            curr_btn.add_css_class("active-nav");
            state_nav.update_active_view(&page_name);
        });
    }

    // Restaura a visão ativa salva
    let saved_view = &initial_state.active_view;
    stack.set_visible_child_name(saved_view);
    for (btn, page, _title) in &nav_buttons {
        if page == saved_view {
            btn.add_css_class("active-nav");
            title_label.set_text(i18n::nav_title(i18n::current(), page));
        } else {
            btn.remove_css_class("active-nav");
        }
    }

    body.append(&nav);
    body.append(&content);
    root.append(&body);
    let (player_bar_widget, mute_btn, shuffle_btn, repeat_btn) = player_bar(
        &playing,
        &audio,
        &state_mgr,
        &queue,
        &playback_errors,
        &volume_hud,
    );
    root.append(&player_bar_widget);

    let window = ApplicationWindow::builder()
        .application(app)
        .title(i18n::window_title(
            i18n::current(),
            env!("CARGO_PKG_VERSION"),
        ))
        .default_width(1220)
        .default_height(780)
        .child(&root)
        .build();
    {
        let win_weak = window.downgrade();
        i18n::connect(&window, move |lang| {
            if let Some(win) = win_weak.upgrade() {
                win.set_title(Some(&i18n::window_title(lang, env!("CARGO_PKG_VERSION"))));
            }
        });
    }
    RAISE_WINDOWS.with(|cell| {
        let mut windows = cell.borrow_mut();
        windows.retain(|w| w.upgrade().is_some());
        windows.push(window.downgrade());
    });
    let show_turntable = gtk::gio::SimpleAction::new("show-turntable", None);
    playback_errors.attach(&window);
    if let Some((overview, _, _)) = nav_buttons.iter().find(|(_, page, _)| *page == "overview") {
        let btn = overview.clone();
        show_turntable.connect_activate(move |_, _| btn.emit_clicked());
    }
    window.add_action(&show_turntable);
    let toggle_queue = gtk::gio::SimpleAction::new("toggle-queue", None);
    let split = queue_split.clone();
    toggle_queue.connect_activate(move |_, _| split.set_show_sidebar(!split.shows_sidebar()));
    window.add_action(&toggle_queue);

    let focus_search = gtk::gio::SimpleAction::new("focus-search", None);
    let search_weak = search_entry.downgrade();
    focus_search.connect_activate(move |_, _| {
        if let Some(entry) = search_weak.upgrade() {
            entry.grab_focus();
            entry.select_region(0, -1);
        }
    });
    window.add_action(&focus_search);

    let open_track_action = gtk::gio::SimpleAction::new("open-track", None);
    let open_weak = open_btn.downgrade();
    open_track_action.connect_activate(move |_, _| {
        if let Some(btn) = open_weak.upgrade() {
            btn.emit_clicked();
        }
    });
    window.add_action(&open_track_action);

    let toggle_mute_action = gtk::gio::SimpleAction::new("toggle-mute", None);
    let mute_action_weak = mute_btn.downgrade();
    toggle_mute_action.connect_activate(move |_, _| {
        if let Some(btn) = mute_action_weak.upgrade() {
            btn.emit_clicked();
        }
    });
    window.add_action(&toggle_mute_action);

    let library_action = gtk::gio::SimpleAction::new("library-settings", None);
    let weak_window = window.downgrade();
    let settings_window = Rc::new(RefCell::new(glib::WeakRef::<gtk::Window>::new()));
    let library_catalog = catalog.clone();
    library_action.connect_activate(move |_, _| {
        if let Some(window) = settings_window.borrow().upgrade() {
            window.present();
            return;
        }
        if let Some(parent) = weak_window.upgrade() {
            let settings = library_settings::settings_window(parent.upcast_ref(), &library_catalog);
            settings_window.borrow_mut().set(Some(&settings));
            settings.present();
        }
    });
    window.add_action(&library_action);

    let donate_action = gtk::gio::SimpleAction::new("donate", None);
    let weak_window_donate = window.downgrade();
    let donate_window_ref = Rc::new(RefCell::new(glib::WeakRef::<gtk::Window>::new()));
    donate_action.connect_activate(move |_, _| {
        if let Some(win) = donate_window_ref.borrow().upgrade() {
            win.present();
            return;
        }
        if let Some(parent) = weak_window_donate.upgrade() {
            let donate_win = donate::donate_window(parent.upcast_ref());
            donate_window_ref.borrow_mut().set(Some(&donate_win));
            donate_win.present();
        }
    });
    window.add_action(&donate_action);
    app.set_accels_for_action("win.donate", &["<Control><Shift>d"]);
    app.set_accels_for_action("win.toggle-queue", &["<Control>j"]);
    app.set_accels_for_action("win.focus-search", &["<Control>f"]);
    app.set_accels_for_action("win.library-settings", &["<Control>comma"]);
    app.set_accels_for_action("win.open-track", &["<Control>o"]);
    let bus_watch = RefCell::new(Some(bus_watch));
    let state_close = state_mgr.clone();
    let audio_close = audio.clone();
    window.connect_close_request(move |window| {
        // Captura inclusive uma edição feita depois do último tick de 250 ms.
        state_close.update_queue(audio_close.queue_snapshot());
        if let Err(error) = state_close.flush_now() {
            eprintln!("Could not save preferences on exit: {error}");
            let dialog = adw::MessageDialog::builder()
                .transient_for(window)
                .destroy_with_parent(true)
                .heading(i18n::pick(
                    "Could not save preferences",
                    "Não foi possível salvar as preferências",
                ))
                .body(match i18n::current() {
                    i18n::Language::En => format!(
                        "Changes remain pending. Check disk space and permissions in the configuration directory and try closing again.\n\n{error}"
                    ),
                    i18n::Language::PtBr => format!(
                        "As alterações permanecem pendentes. Verifique o espaço em disco e as permissões no diretório de configurações e tente fechar novamente.\n\n{error}"
                    ),
                })
                .build();
            dialog.add_response("back", i18n::pick("Back to player", "Voltar ao player"));
            dialog.set_close_response("back");
            dialog.present();
            return glib::Propagation::Stop;
        }
        bus_watch.borrow_mut().take();
        glib::Propagation::Proceed
    });

    // Atalhos ergonômicos de teclado (respeitando campos de texto e controles focados)
    let key_controller = gtk::EventControllerKey::new();
    let key_audio = audio.clone();
    let key_playing = playing.clone();
    let key_state = state_mgr.clone();
    let key_hud = volume_hud.clone();
    let key_win = window.downgrade();
    let key_mute = mute_btn.downgrade();
    let key_shuffle = shuffle_btn.downgrade();
    let key_repeat = repeat_btn.downgrade();
    let key_nav_btns: Vec<gtk::Button> = nav_buttons.iter().map(|(b, _, _)| b.clone()).collect();
    key_controller.connect_key_pressed(move |_, keyval, _, modifier| {
        if modifier.contains(gdk::ModifierType::ALT_MASK) {
            let target_idx = match keyval {
                gdk::Key::_1 => Some(0),
                gdk::Key::_2 => Some(1),
                gdk::Key::_3 => Some(2),
                gdk::Key::_4 => Some(3),
                _ => None,
            };
            if let Some(idx) = target_idx {
                if let Some(btn) = key_nav_btns.get(idx) {
                    btn.emit_clicked();
                    return glib::Propagation::Stop;
                }
            }
            return glib::Propagation::Proceed;
        }

        if modifier.intersects(gdk::ModifierType::CONTROL_MASK | gdk::ModifierType::SUPER_MASK) {
            return glib::Propagation::Proceed;
        }
        if let Some(win) = key_win.upgrade() {
            if let Some(focused) = GtkWindowExt::focus(&win) {
                if focused.is::<gtk::Text>()
                    || focused.is::<gtk::Entry>()
                    || focused.is::<gtk::SearchEntry>()
                    || focused.is::<gtk::PasswordEntry>()
                    || focused.ancestor(gtk::Editable::static_type()).is_some()
                {
                    return glib::Propagation::Proceed;
                }
                if (focused.is::<gtk::Scale>() || focused.is::<gtk::DropDown>())
                    && matches!(
                        keyval,
                        gdk::Key::Left | gdk::Key::Right | gdk::Key::Up | gdk::Key::Down
                    )
                {
                    return glib::Propagation::Proceed;
                }
            }
        }
        match keyval {
            gdk::Key::space => {
                let v = !key_playing.get();
                let _ = if v {
                    key_audio.play()
                } else {
                    key_audio.pause()
                };
                key_playing.set(v);
                glib::Propagation::Stop
            }
            gdk::Key::Right => {
                if modifier.contains(gdk::ModifierType::SHIFT_MASK) {
                    let cur = key_audio.position_us();
                    let dur = key_audio.duration_us();
                    let target = (cur + 5_000_000).min(dur);
                    let _ = key_audio.seek_us(target);
                } else {
                    let _ = key_audio.next();
                }
                glib::Propagation::Stop
            }
            gdk::Key::Left => {
                if modifier.contains(gdk::ModifierType::SHIFT_MASK) {
                    let cur = key_audio.position_us();
                    let target = (cur - 5_000_000).max(0);
                    let _ = key_audio.seek_us(target);
                } else {
                    let _ = key_audio.prev();
                }
                glib::Propagation::Stop
            }
            gdk::Key::Page_Down => {
                let _ = key_audio.next();
                glib::Propagation::Stop
            }
            gdk::Key::Page_Up => {
                let _ = key_audio.prev();
                glib::Propagation::Stop
            }
            gdk::Key::Home => {
                let _ = key_audio.seek_us(0);
                glib::Propagation::Stop
            }
            gdk::Key::Up => {
                let cur = key_audio.volume();
                let new_vol = (cur + 0.05).clamp(0.0, 1.0);
                key_audio.set_volume(new_vol);
                key_state.update_volume(new_vol);
                key_hud.borrow_mut().trigger(new_vol);
                glib::Propagation::Stop
            }
            gdk::Key::Down => {
                let cur = key_audio.volume();
                let new_vol = (cur - 0.05).clamp(0.0, 1.0);
                key_audio.set_volume(new_vol);
                key_state.update_volume(new_vol);
                key_hud.borrow_mut().trigger(new_vol);
                glib::Propagation::Stop
            }
            gdk::Key::m | gdk::Key::M => {
                if let Some(btn) = key_mute.upgrade() {
                    btn.emit_clicked();
                    glib::Propagation::Stop
                } else {
                    glib::Propagation::Proceed
                }
            }
            gdk::Key::s | gdk::Key::S => {
                if let Some(btn) = key_shuffle.upgrade() {
                    btn.emit_clicked();
                    glib::Propagation::Stop
                } else {
                    glib::Propagation::Proceed
                }
            }
            gdk::Key::r | gdk::Key::R => {
                if let Some(btn) = key_repeat.upgrade() {
                    btn.emit_clicked();
                    glib::Propagation::Stop
                } else {
                    glib::Propagation::Proceed
                }
            }
            _ => glib::Propagation::Proceed,
        }
    });
    window.add_controller(key_controller);

    install_css();
    window.present();
    catalog.refresh();
    (audio, catalog)
}

fn sidebar() -> (gtk::Widget, Vec<(gtk::Button, &'static str, &'static str)>) {
    let side = gtk::Box::new(Orientation::Vertical, 6);
    side.set_width_request(212);
    side.add_css_class("sidebar");

    // Branding moderno
    let brand_box = gtk::Box::new(Orientation::Vertical, 2);
    brand_box.add_css_class("brand-box");

    let logo_row = gtk::Box::new(Orientation::Horizontal, 8);
    let icon_disc = gtk::Image::from_icon_name("media-optical-symbolic");
    icon_disc.set_pixel_size(24);
    icon_disc.add_css_class("brand-icon");
    let brand = gtk::Label::new(Some("VINYLPLAYER"));
    brand.add_css_class("brand");
    let version_badge = gtk::Label::new(Some(concat!("v", env!("CARGO_PKG_VERSION"))));
    version_badge.add_css_class("version-badge");
    version_badge.set_valign(gtk::Align::Center);
    logo_row.append(&icon_disc);
    logo_row.append(&brand);
    logo_row.append(&version_badge);
    brand_box.append(&logo_row);

    let sub = gtk::Label::new(None);
    i18n::bind_label(&sub, |l| {
        i18n::pick_for(l, "THE LISTENING RITUAL", "O RITUAL DE OUVIR")
    });
    sub.add_css_class("brand-sub");
    sub.set_xalign(0.0);
    brand_box.append(&sub);
    side.append(&brand_box);

    let sec_lbl = gtk::Label::new(None);
    i18n::bind_label(&sec_lbl, |l| i18n::pick_for(l, "COLLECTION", "COLEÇÃO"));
    sec_lbl.add_css_class("sidebar-section-title");
    sec_lbl.set_xalign(0.0);
    side.append(&sec_lbl);

    type NavSpec = (
        &'static str,
        fn(i18n::Language) -> &'static str,
        fn(i18n::Language) -> &'static str,
        &'static str,
    );
    let mut nav_buttons = Vec::new();
    let items: [NavSpec; 4] = [
        (
            "media-optical-symbolic",
            |l| i18n::nav_title(l, "overview"),
            |l| i18n::pick_for(l, "Turntable · Alt+1", "Toca-Discos · Alt+1"),
            "overview",
        ),
        (
            "audio-x-generic-symbolic",
            |l| i18n::nav_title(l, "tracks"),
            |l| i18n::pick_for(l, "Tracks · Alt+2", "Faixas · Alt+2"),
            "tracks",
        ),
        (
            "view-grid-symbolic",
            |l| i18n::nav_title(l, "albums"),
            |l| i18n::pick_for(l, "Albums · Alt+3", "Álbuns · Alt+3"),
            "albums",
        ),
        (
            "view-list-symbolic",
            |l| i18n::nav_title(l, "playlists"),
            |l| i18n::pick_for(l, "Playlists · Alt+4", "Playlists · Alt+4"),
            "playlists",
        ),
    ];

    for (idx, (icon, label_fn, tooltip_fn, page)) in items.iter().enumerate() {
        let b = labeled_icon_button_i18n(icon, *label_fn);
        b.add_css_class("nav-button");
        i18n::bind_tooltip(&b, *tooltip_fn);
        if idx == 0 {
            b.add_css_class("active-nav");
        }
        side.append(&b);
        nav_buttons.push((b, *page, *page));
    }

    let spacer = gtk::Box::new(Orientation::Vertical, 0);
    spacer.set_vexpand(true);
    side.append(&spacer);

    // Status do sistema de áudio
    let audio_pill = gtk::Box::new(Orientation::Vertical, 2);
    audio_pill.add_css_class("system-audio-pill");
    let pipe_lbl = gtk::Label::new(None);
    i18n::bind_label(&pipe_lbl, |l| {
        i18n::pick_for(l, "LOCAL LIBRARY", "BIBLIOTECA LOCAL")
    });
    pipe_lbl.add_css_class("system-audio-title");
    pipe_lbl.set_xalign(0.0);
    let sample_lbl = gtk::Label::new(None);
    i18n::bind_label(&sample_lbl, |l| {
        i18n::pick_for(
            l,
            "Drag and drop a track to play",
            "Arraste e solte uma faixa para tocar",
        )
    });
    sample_lbl.add_css_class("system-audio-sub");
    sample_lbl.set_xalign(0.0);
    audio_pill.append(&pipe_lbl);
    audio_pill.append(&sample_lbl);
    side.append(&audio_pill);

    let donate = labeled_icon_button_i18n("emblem-favorite-symbolic", |l| {
        i18n::pick_for(l, "Donate", "Apoiar")
    });
    donate.add_css_class("nav-button");
    donate.add_css_class("donate-nav-button");
    donate.set_action_name(Some("win.donate"));
    i18n::bind_tooltip(&donate, |l| {
        i18n::pick_for(
            l,
            "Support VinylPlayer (Ctrl+Shift+D)",
            "Apoiar o VinylPlayer (Ctrl+Shift+D)",
        )
    });
    side.append(&donate);

    let settings = labeled_icon_button_i18n("preferences-system-symbolic", |l| {
        i18n::pick_for(l, "Settings", "Configurações")
    });
    settings.add_css_class("nav-button");
    settings.set_action_name(Some("win.library-settings"));
    i18n::bind_tooltip(&settings, |l| {
        i18n::pick_for(
            l,
            "Settings, theme, language and folders (Ctrl+,)",
            "Configurações, tema, idioma e pastas (Ctrl+,)",
        )
    });
    side.append(&settings);

    (side.upcast(), nav_buttons)
}

fn library_view(
    audio: &Arc<AudioEngine>,
    albums: bool,
    search_hub: &search::SearchHub,
    state_mgr: &state::StateManager,
    paths: Vec<std::path::PathBuf>,
) -> gtk::Widget {
    if albums {
        return album_library_view(audio, search_hub, state_mgr, paths);
    }
    let outer = gtk::Box::new(Orientation::Vertical, 12);
    outer.set_margin_start(28);
    outer.set_margin_end(28);
    outer.set_margin_bottom(18);
    let header_box = gtk::Box::new(Orientation::Horizontal, 16);
    header_box.set_margin_top(18);
    let heading = gtk::Label::new(None);
    i18n::bind_label(&heading, |l| i18n::nav_title(l, "tracks"));
    heading.add_css_class("page-title");
    heading.set_xalign(0.0);
    heading.set_hexpand(true);
    header_box.append(&heading);

    let sort_dropdown = gtk::DropDown::from_strings(&[]);
    i18n::bind_dropdown_strings(&sort_dropdown, |l| match l {
        i18n::Language::En => &["Title (A–Z)", "Title (Z–A)", "Path (Default)"],
        i18n::Language::PtBr => &["Título (A–Z)", "Título (Z–A)", "Caminho (Padrão)"],
    });
    let initial_track_sort = state_mgr.snapshot().track_sort_mode;
    sort_dropdown.set_selected(initial_track_sort as u32);
    sort_dropdown.add_css_class("sort-dropdown");
    i18n::bind_tooltip(&sort_dropdown, |l| {
        i18n::pick_for(l, "Track sort criteria", "Ordenação das faixas")
    });
    header_box.append(&sort_dropdown);
    outer.append(&header_box);

    let list = gtk::ListBox::new();
    list.set_selection_mode(gtk::SelectionMode::None);
    list.add_css_class("library-list");

    let empty_box = gtk::Box::new(Orientation::Vertical, 10);
    empty_box.set_valign(gtk::Align::Center);
    empty_box.set_halign(gtk::Align::Center);
    empty_box.set_margin_top(48);
    empty_box.set_margin_bottom(48);
    let empty_icon = gtk::Image::from_icon_name("edit-find-symbolic");
    empty_icon.set_pixel_size(48);
    empty_icon.add_css_class("album-cover-placeholder");
    let has_tracks = !paths.is_empty();
    let empty_label = gtk::Label::new(None);
    if has_tracks {
        i18n::bind_label(&empty_label, |l| {
            i18n::pick_for(
                l,
                "No tracks match your search",
                "Nenhuma música corresponde à busca",
            )
        });
    } else {
        i18n::bind_label(&empty_label, |l| {
            i18n::pick_for(
                l,
                "Library is empty. Add a folder in Settings.",
                "Biblioteca vazia. Adicione uma pasta em Configurações.",
            )
        });
    }
    empty_label.add_css_class("bottom-artist-title");
    empty_box.append(&empty_icon);
    empty_box.append(&empty_label);
    list.set_placeholder(Some(&empty_box));

    let mut sorted_paths = paths;
    match initial_track_sort {
        0 => {
            sorted_paths.sort_by(|path_a, path_b| {
                let stem_a = path_a.file_stem().and_then(|s| s.to_str()).unwrap_or("");
                let stem_b = path_b.file_stem().and_then(|s| s.to_str()).unwrap_or("");
                library::natural_cmp(stem_a, stem_b).then_with(|| path_a.cmp(path_b))
            });
        }
        1 => {
            sorted_paths.sort_by(|path_a, path_b| {
                let stem_a = path_a.file_stem().and_then(|s| s.to_str()).unwrap_or("");
                let stem_b = path_b.file_stem().and_then(|s| s.to_str()).unwrap_or("");
                library::natural_cmp(stem_b, stem_a).then_with(|| path_b.cmp(path_a))
            });
        }
        _ => {
            sorted_paths.sort();
        }
    }

    let track_paths: Rc<Vec<std::path::PathBuf>> = Rc::new(sorted_paths);
    let scroll = gtk::ScrolledWindow::new();
    scroll.set_policy(gtk::PolicyType::Never, gtk::PolicyType::Automatic);
    scroll.set_vexpand(true);
    scroll.set_margin_top(4);
    scroll.set_child(Some(&list));
    scroll.connect_map(|s| {
        s.vadjustment().set_value(0.0);
    });

    let weak_list_fill = list.downgrade();
    let weak_scroll_fill = scroll.downgrade();
    let fill_paths = track_paths.clone();
    let fill_audio = audio.clone();
    let populated = Rc::new(std::cell::Cell::new(false));

    let fill_tracks = {
        let populated = populated.clone();
        let fill_paths = fill_paths.clone();
        let fill_audio = fill_audio.clone();
        let weak_list = weak_list_fill.clone();
        let weak_scroll = weak_scroll_fill.clone();
        Rc::new(move || {
            if populated.get() {
                return;
            }
            populated.set(true);
            let paths = fill_paths.clone();
            let audio = fill_audio.clone();
            let weak_list = weak_list.clone();
            let weak_scroll = weak_scroll.clone();
            glib::MainContext::default().spawn_local(async move {
                let chunk_size = 64;
                for (idx, path) in paths.iter().enumerate() {
                    let Some(list) = weak_list.upgrade() else {
                        break;
                    };
                    let row = track_row(&audio, path.clone(), false);
                    row.set_widget_name(&idx.to_string());
                    list.append(&row);
                    drop(list);
                    if idx % chunk_size == chunk_size - 1 {
                        glib::timeout_future_with_priority(
                            glib::Priority::LOW,
                            std::time::Duration::from_millis(2),
                        )
                        .await;
                    }
                }
                if let Some(s) = weak_scroll.upgrade() {
                    s.vadjustment().set_value(0.0);
                }
            });
        })
    };

    let ft_map = fill_tracks.clone();
    let weak_scroll_map = scroll.downgrade();
    outer.connect_map(move |_| {
        ft_map();
        if let Some(s) = weak_scroll_map.upgrade() {
            s.vadjustment().set_value(0.0);
        }
    });

    let ft_search = fill_tracks.clone();
    let search_tracks_check = search_hub.clone();
    search_hub.connect(&outer, move || {
        if !search_tracks_check.query().trim().is_empty() {
            ft_search();
        }
    });

    // Povoa imediatamente se já estiver mapeado, houver busca ativa ou lista pequena
    if outer.is_mapped() || track_paths.len() <= 60 || !search_hub.query().trim().is_empty() {
        fill_tracks();
    }

    let list_filter_weak = list.downgrade();
    search_hub.connect(&list, move || {
        if let Some(l) = list_filter_weak.upgrade() {
            l.invalidate_filter();
        }
    });

    let paths_filter = track_paths.clone();
    let search_hub_tracks = search_hub.clone();
    list.set_filter_func(move |row| {
        let q = search_hub_tracks.query();
        if q.trim().is_empty() {
            return true;
        }
        let idx = row.widget_name().parse::<usize>().ok();
        if let Some(i) = idx {
            if i < paths_filter.len() {
                return search::track_matches(&paths_filter[i], &q);
            }
        }
        true
    });

    let paths_clone = track_paths.clone();
    let track_sort_mode = Rc::new(std::cell::Cell::new(initial_track_sort));
    let mode_ref = track_sort_mode.clone();
    list.set_sort_func(move |row_a, row_b| {
        let m = mode_ref.get();
        let idx_a = row_a.widget_name().parse::<usize>().ok();
        let idx_b = row_b.widget_name().parse::<usize>().ok();
        match (idx_a, idx_b) {
            (Some(a), Some(b)) if a < paths_clone.len() && b < paths_clone.len() => {
                let path_a = &paths_clone[a];
                let path_b = &paths_clone[b];
                let ord = match m {
                    0 => {
                        let stem_a = path_a.file_stem().and_then(|s| s.to_str()).unwrap_or("");
                        let stem_b = path_b.file_stem().and_then(|s| s.to_str()).unwrap_or("");
                        library::natural_cmp(stem_a, stem_b).then_with(|| path_a.cmp(path_b))
                    }
                    1 => {
                        let stem_a = path_a.file_stem().and_then(|s| s.to_str()).unwrap_or("");
                        let stem_b = path_b.file_stem().and_then(|s| s.to_str()).unwrap_or("");
                        library::natural_cmp(stem_b, stem_a).then_with(|| path_b.cmp(path_a))
                    }
                    _ => path_a.cmp(path_b),
                };
                ord.into()
            }
            _ => gtk::Ordering::Equal,
        }
    });

    let list_weak = list.downgrade();
    let mode_store = track_sort_mode.clone();
    let state_track_sort = state_mgr.clone();
    sort_dropdown.connect_selected_notify(move |dropdown| {
        let m = dropdown.selected() as usize;
        mode_store.set(m);
        state_track_sort.update_track_sort_mode(m);
        if let Some(l) = list_weak.upgrade() {
            l.invalidate_sort();
        }
    });

    outer.append(&scroll);
    outer.upcast()
}

fn album_library_view(
    audio: &Arc<AudioEngine>,
    search_hub: &search::SearchHub,
    state_mgr: &state::StateManager,
    paths: Vec<std::path::PathBuf>,
) -> gtk::Widget {
    let navigation = gtk::Stack::new();
    navigation.set_hexpand(true);
    navigation.set_vexpand(true);
    let outer = gtk::Box::new(Orientation::Vertical, 12);
    outer.set_margin_start(28);
    outer.set_margin_end(28);
    outer.set_margin_bottom(18);
    navigation.add_named(&outer, Some("browse"));
    let toolbar = gtk::Box::new(Orientation::Horizontal, 16);
    toolbar.set_margin_top(18);
    let heading = gtk::Label::new(None);
    i18n::bind_label(&heading, |l| i18n::nav_title(l, "albums"));
    heading.set_xalign(0.0);
    heading.set_hexpand(true);
    heading.add_css_class("page-title");
    toolbar.append(&heading);

    let sort_dropdown = gtk::DropDown::from_strings(&[]);
    i18n::bind_dropdown_strings(&sort_dropdown, |l| match l {
        i18n::Language::En => &[
            "Album (A–Z)",
            "Album (Z–A)",
            "Artist (A–Z)",
            "Year (Newest)",
            "Year (Oldest)",
        ],
        i18n::Language::PtBr => &[
            "Álbum (A–Z)",
            "Álbum (Z–A)",
            "Artista (A–Z)",
            "Ano (Mais recentes)",
            "Ano (Mais antigos)",
        ],
    });
    let initial_album_sort = state_mgr.snapshot().album_sort_mode;
    sort_dropdown.set_selected(initial_album_sort as u32);
    sort_dropdown.add_css_class("sort-dropdown");
    i18n::bind_tooltip(&sort_dropdown, |l| {
        i18n::pick_for(l, "Album sort criteria", "Ordenação dos álbuns")
    });
    toolbar.append(&sort_dropdown);

    let views = gtk::Stack::new();
    views.set_hexpand(true);
    views.set_vexpand(true);
    let switcher = gtk::StackSwitcher::new();
    switcher.set_stack(Some(&views));
    switcher.add_css_class("album-view-switcher");
    toolbar.append(&switcher);
    outer.append(&toolbar);
    outer.append(&views);
    let loading = gtk::Box::new(Orientation::Vertical, 12);
    loading.set_halign(gtk::Align::Center);
    loading.set_valign(gtk::Align::Center);
    let spinner = gtk::Spinner::new();
    spinner.start();
    loading.append(&spinner);
    let status = gtk::Label::new(None);
    i18n::bind_label(&status, |l| {
        i18n::pick_for(
            l,
            "Organizing albums from metadata…",
            "Organizando álbuns pelos metadados…",
        )
    });
    status.add_css_class("bottom-artist-title");
    loading.append(&status);
    views.add_named(&loading, Some("loading"));
    let engine = audio.clone();
    let weak_views = views.downgrade();
    let weak_navigation = navigation.downgrade();
    let sort_dropdown_ref = sort_dropdown.clone();
    let search_hub_album = search_hub.clone();
    let state_mgr_album = state_mgr.clone();
    glib::MainContext::default().spawn_local(async move {
        let result = gtk::gio::spawn_blocking(move || library::load_album_metadata(paths)).await;
        if let (Some(views), Some(navigation)) = (weak_views.upgrade(), weak_navigation.upgrade()) {
            match result {
                Ok(albums) => populate_album_views(
                    &engine,
                    albums,
                    &views,
                    &navigation,
                    &sort_dropdown_ref,
                    &search_hub_album,
                    &state_mgr_album,
                ),
                Err(_) => {
                    spinner.stop();
                    i18n::bind_label(&status, |l| {
                        i18n::pick_for(
                            l,
                            "Could not organize albums.",
                            "Não foi possível organizar os álbuns.",
                        )
                    });
                }
            }
        }
    });
    navigation.upcast()
}

fn populate_album_views(
    audio: &Arc<AudioEngine>,
    albums: Vec<library::AlbumMetadata>,
    views: &gtk::Stack,
    navigation: &gtk::Stack,
    sort_dropdown: &gtk::DropDown,
    search_hub: &search::SearchHub,
    state_mgr: &state::StateManager,
) {
    let initial_album_sort = state_mgr.snapshot().album_sort_mode;
    let initial_mode = match initial_album_sort {
        0 => library::AlbumSortMode::TitleAsc,
        1 => library::AlbumSortMode::TitleDesc,
        2 => library::AlbumSortMode::ArtistAsc,
        3 => library::AlbumSortMode::YearDesc,
        4 => library::AlbumSortMode::YearAsc,
        _ => library::AlbumSortMode::TitleAsc,
    };
    let mode_cell = Rc::new(std::cell::Cell::new(initial_mode));
    let (grid_widget, grid) = album_grid(audio, &albums, navigation, &mode_cell, search_hub);
    let covers_page = views.add_titled(&grid_widget, Some("covers"), i18n::pick("Covers", "Capas"));
    i18n::connect(&grid_widget, move |l| {
        covers_page.set_title(i18n::pick_for(l, "Covers", "Capas"));
    });

    let list = gtk::ListBox::new();
    list.set_selection_mode(gtk::SelectionMode::None);
    list.add_css_class("library-list");

    let empty_box = gtk::Box::new(Orientation::Vertical, 10);
    empty_box.set_valign(gtk::Align::Center);
    empty_box.set_halign(gtk::Align::Center);
    empty_box.set_margin_top(48);
    empty_box.set_margin_bottom(48);
    let empty_icon = gtk::Image::from_icon_name("media-optical-symbolic");
    empty_icon.set_pixel_size(48);
    empty_icon.add_css_class("album-cover-placeholder");
    let empty_label = gtk::Label::new(None);
    i18n::bind_label(&empty_label, |l| {
        i18n::pick_for(
            l,
            "No albums match your search",
            "Nenhum álbum corresponde à busca",
        )
    });
    empty_label.add_css_class("bottom-artist-title");
    empty_box.append(&empty_icon);
    empty_box.append(&empty_label);
    list.set_placeholder(Some(&empty_box));

    let scrolled = gtk::ScrolledWindow::new();
    scrolled.set_vexpand(true);
    scrolled.set_policy(gtk::PolicyType::Never, gtk::PolicyType::Automatic);

    let albums_rc = Rc::new(albums);
    let fill_albums = albums_rc.clone();
    let weak_fill_list = list.downgrade();
    let fill_audio = audio.clone();
    let list_populated = Rc::new(std::cell::Cell::new(false));

    let populate_list = {
        let list_populated = list_populated.clone();
        Rc::new(move || {
            if list_populated.get() {
                return;
            }
            list_populated.set(true);
            let albums = fill_albums.clone();
            let weak_list = weak_fill_list.clone();
            let audio = fill_audio.clone();
            glib::MainContext::default().spawn_local(async move {
                for (index, album_info) in albums.iter().enumerate() {
                    let Some(list) = weak_list.upgrade() else {
                        break;
                    };
                    let row = gtk::ListBoxRow::new();
                    row.set_widget_name(&index.to_string());
                    row.add_css_class("album-card-row");
                    let box_ = gtk::Box::new(Orientation::Vertical, 6);
                    box_.set_margin_top(12);
                    box_.set_margin_bottom(12);
                    box_.set_margin_start(16);
                    box_.set_margin_end(16);

                    let header_row = gtk::Box::new(Orientation::Horizontal, 12);
                    let album_icon = gtk::Image::from_icon_name("media-optical-symbolic");
                    album_icon.set_pixel_size(24);
                    let year_str = album_info
                        .year
                        .map(|y| format!(" ({y})"))
                        .unwrap_or_default();
                    let subtitle = if !i18n::is_unknown_artist(&album_info.artist) {
                        format!("{}  ·  {}{}", album_info.artist, album_info.title, year_str)
                    } else {
                        format!("{}{}", album_info.title, year_str)
                    };
                    let label = gtk::Label::new(Some(&format!(
                        "{}  ·  {}",
                        subtitle,
                        i18n::tracks_count_inline(i18n::current(), album_info.tracks.len())
                    )));
                    label.set_xalign(0.0);
                    label.set_hexpand(true);
                    label.set_ellipsize(gtk::pango::EllipsizeMode::End);
                    label.add_css_class("album-title-header");
                    {
                        let lbl_weak = label.downgrade();
                        let track_count = album_info.tracks.len();
                        let sub_owned = subtitle.clone();
                        i18n::connect(&label, move |lang| {
                            if let Some(lbl) = lbl_weak.upgrade() {
                                lbl.set_text(&format!(
                                    "{sub_owned}  ·  {}",
                                    i18n::tracks_count_inline(lang, track_count)
                                ));
                            }
                        });
                    }

                    let tracks = Rc::new(album_info.tracks.clone());
                    let album_queue = tracks.clone();
                    let button = labeled_icon_button_i18n("media-playback-start-symbolic", |l| {
                        i18n::pick_for(l, "Play album", "Ouvir álbum")
                    });
                    button.add_css_class("play-album-button");
                    let engine = audio.clone();
                    button.connect_clicked(move |_| {
                        let _ = engine.play_album(&album_queue, 0);
                    });

                    header_row.append(&album_icon);
                    header_row.append(&label);
                    header_row.append(&button);
                    header_row.append(&queue_actions(&audio, tracks.clone(), true));
                    box_.append(&header_row);

                    row.set_child(Some(&box_));
                    list.append(&row);
                    drop(list);
                    for (track_idx, path) in album_info.tracks.iter().cloned().enumerate() {
                        if weak_list.upgrade().is_none() {
                            return;
                        }
                        let child = track_row_in_queue(
                            &audio,
                            path,
                            true,
                            Some((tracks.clone(), track_idx)),
                        );
                        box_.append(&child);
                        if track_idx % 32 == 31 {
                            glib::timeout_future_with_priority(
                                glib::Priority::LOW,
                                std::time::Duration::from_millis(4),
                            )
                            .await;
                        }
                    }
                    glib::timeout_future_with_priority(
                        glib::Priority::LOW,
                        std::time::Duration::from_millis(2),
                    )
                    .await;
                }
            });
        })
    };

    let pl = populate_list.clone();
    views.connect_visible_child_name_notify(move |v| {
        if v.visible_child_name().as_deref() == Some("list") {
            pl();
        }
    });

    let pl_search = populate_list.clone();
    let search_hub_check = search_hub.clone();
    search_hub.connect(&list, move || {
        if !search_hub_check.query().trim().is_empty() {
            pl_search();
        }
    });

    if views.visible_child_name().as_deref() == Some("list")
        || albums_rc.len() <= 10
        || !search_hub.query().trim().is_empty()
    {
        populate_list();
    }

    let albums_list = albums_rc.clone();
    let mode_list = mode_cell.clone();
    list.set_sort_func(move |row_a, row_b| {
        let m = mode_list.get();
        let idx_a = row_a.widget_name().parse::<usize>().ok();
        let idx_b = row_b.widget_name().parse::<usize>().ok();
        match (idx_a, idx_b) {
            (Some(a), Some(b)) if a < albums_list.len() && b < albums_list.len() => {
                library::compare_albums(&albums_list[a], &albums_list[b], m).into()
            }
            _ => gtk::Ordering::Equal,
        }
    });

    let list_filter_weak = list.downgrade();
    search_hub.connect(&list, move || {
        if let Some(l) = list_filter_weak.upgrade() {
            l.invalidate_filter();
        }
    });

    let search_hub_list = search_hub.clone();
    let albums_filter_list = albums_rc.clone();
    list.set_filter_func(move |row| {
        let q = search_hub_list.query();
        if q.trim().is_empty() {
            return true;
        }
        let idx = row.widget_name().parse::<usize>().ok();
        if let Some(i) = idx {
            if i < albums_filter_list.len() {
                return search::album_matches(&albums_filter_list[i], &q);
            }
        }
        true
    });

    let grid_weak = grid.downgrade();
    let list_weak = list.downgrade();
    let mode_store = mode_cell.clone();
    let state_album_sort = state_mgr.clone();
    sort_dropdown.connect_selected_notify(move |dropdown| {
        let sel = dropdown.selected() as usize;
        let mode = match sel {
            0 => library::AlbumSortMode::TitleAsc,
            1 => library::AlbumSortMode::TitleDesc,
            2 => library::AlbumSortMode::ArtistAsc,
            3 => library::AlbumSortMode::YearDesc,
            4 => library::AlbumSortMode::YearAsc,
            _ => library::AlbumSortMode::TitleAsc,
        };
        mode_store.set(mode);
        state_album_sort.update_album_sort_mode(sel);
        if let Some(g) = grid_weak.upgrade() {
            g.invalidate_sort();
        }
        if let Some(l) = list_weak.upgrade() {
            l.invalidate_sort();
        }
    });

    scrolled.set_child(Some(&list));
    let list_page = views.add_titled(&scrolled, Some("list"), i18n::pick("List", "Lista"));
    i18n::connect(&scrolled, move |l| {
        list_page.set_title(i18n::pick_for(l, "List", "Lista"));
    });
    views.set_visible_child_name("covers");
    if let Some(loading) = views.child_by_name("loading") {
        views.remove(&loading);
    }
}

fn album_grid(
    audio: &Arc<AudioEngine>,
    albums: &[library::AlbumMetadata],
    navigation: &gtk::Stack,
    mode: &Rc<std::cell::Cell<library::AlbumSortMode>>,
    search_hub: &search::SearchHub,
) -> (gtk::Widget, gtk::FlowBox) {
    let scroll = gtk::ScrolledWindow::new();
    scroll.set_policy(gtk::PolicyType::Never, gtk::PolicyType::Automatic);
    scroll.set_vexpand(true);
    let grid = gtk::FlowBox::new();
    grid.set_selection_mode(gtk::SelectionMode::None);
    grid.set_homogeneous(true);
    grid.set_min_children_per_line(1);
    grid.set_max_children_per_line(6);
    grid.set_column_spacing(20);
    grid.set_row_spacing(20);
    grid.set_valign(gtk::Align::Start);
    grid.add_css_class("album-cover-grid");

    let albums_grid: Rc<Vec<library::AlbumMetadata>> = Rc::new(albums.to_vec());
    let albums_sort = albums_grid.clone();
    let mode_grid = mode.clone();
    grid.set_sort_func(move |child_a, child_b| {
        let m = mode_grid.get();
        let idx_a = child_a
            .child()
            .and_then(|w| w.widget_name().parse::<usize>().ok());
        let idx_b = child_b
            .child()
            .and_then(|w| w.widget_name().parse::<usize>().ok());
        match (idx_a, idx_b) {
            (Some(a), Some(b)) if a < albums_sort.len() && b < albums_sort.len() => {
                library::compare_albums(&albums_sort[a], &albums_sort[b], m).into()
            }
            _ => gtk::Ordering::Equal,
        }
    });

    let grid_filter_weak = grid.downgrade();
    search_hub.connect(&grid, move || {
        if let Some(g) = grid_filter_weak.upgrade() {
            g.invalidate_filter();
        }
    });

    let search_hub_grid = search_hub.clone();
    let albums_filter = albums_grid.clone();
    grid.set_filter_func(move |child| {
        let q = search_hub_grid.query();
        if q.trim().is_empty() {
            return true;
        }
        let idx = child
            .child()
            .and_then(|w| w.widget_name().parse::<usize>().ok());
        if let Some(i) = idx {
            if i < albums_filter.len() {
                return search::album_matches(&albums_filter[i], &q);
            }
        }
        true
    });

    let nav_weak = navigation.downgrade();
    let search_hub_nav = search_hub.clone();
    search_hub.connect(navigation, move || {
        let q = search_hub_nav.query();
        if !q.trim().is_empty() {
            if let Some(nav) = nav_weak.upgrade() {
                if nav.visible_child_name().as_deref() == Some("detail") {
                    nav.set_visible_child_name("browse");
                }
            }
        }
    });

    if albums.is_empty() {
        let empty = gtk::Label::new(None);
        i18n::bind_label(&empty, |l| {
            i18n::pick_for(
                l,
                "No albums found in your library.",
                "Nenhum álbum encontrado na biblioteca.",
            )
        });
        empty.add_css_class("bottom-artist-title");
        empty.set_margin_top(40);
        scroll.set_child(Some(&empty));
        return (scroll.upcast(), grid);
    }
    scroll.set_child(Some(&grid));
    let weak_grid_fill = grid.downgrade();
    let fill_albums = albums_grid.clone();
    let fill_audio = audio.clone();
    let fill_navigation = navigation.downgrade();
    glib::MainContext::default().spawn_local(async move {
        let mut pictures = Vec::new();
        let mut sources = Vec::new();
        for (index, album_info) in fill_albums.iter().enumerate() {
            let Some(grid) = weak_grid_fill.upgrade() else {
                return;
            };
            let folder = &album_info.folder;
            let tracks = &album_info.tracks;
            let album = &album_info.title;
            let Some(first) = tracks.first() else {
                continue;
            };
            let button = gtk::Button::new();
            button.set_widget_name(&index.to_string());
            button.add_css_class("album-cover-button");
            button.set_size_request(280, 280);
            button.set_halign(gtk::Align::Center);
            button.set_valign(gtk::Align::Start);
            button.set_overflow(gtk::Overflow::Hidden);

            let tooltip = match (album_info.artist.as_str(), album_info.year) {
                (artist, Some(y)) if !i18n::is_unknown_artist(artist) => {
                    format!("{} — {} ({})", artist, album_info.title, y)
                }
                (artist, None) if !i18n::is_unknown_artist(artist) => {
                    format!("{} — {}", artist, album_info.title)
                }
                (_, Some(y)) => format!("{} ({})", album_info.title, y),
                _ => album_info.title.clone(),
            };
            button.set_tooltip_text(Some(&tooltip));
            button.update_property(&[gtk::accessible::Property::Label(&format!(
                "{} {album}",
                i18n::pick("Open album", "Abrir álbum")
            ))]);
            let square = gtk::AspectFrame::new(0.5, 0.5, 1.0, false);
            let overlay = gtk::Overlay::new();
            let placeholder = gtk::Image::from_icon_name("media-optical-symbolic");
            placeholder.set_pixel_size(64);
            placeholder.add_css_class("album-cover-placeholder");
            overlay.set_child(Some(&placeholder));
            let picture = gtk::Picture::new();
            picture.set_can_shrink(true);
            picture.set_content_fit(gtk::ContentFit::Cover);
            picture.set_hexpand(true);
            picture.set_vexpand(true);
            overlay.add_overlay(&picture);
            let indicator = gtk::Image::from_icon_name("media-playback-start-symbolic");
            indicator.set_pixel_size(16);
            indicator.set_halign(gtk::Align::End);
            indicator.set_valign(gtk::Align::Start);
            indicator.set_margin_top(12);
            indicator.set_margin_end(12);
            indicator.set_can_target(false);
            indicator.add_css_class("album-playback-indicator");
            overlay.add_overlay(&indicator);
            now_playing::register_album(&button, &indicator, folder, album);
            square.set_child(Some(&overlay));
            button.set_child(Some(&square));
            let engine = fill_audio.clone();
            let album_tracks = Rc::new(tracks.clone());
            let album_name = album.to_string();
            let navigation = fill_navigation.clone();
            let cover = picture.downgrade();
            button.connect_clicked(move |_| {
                if let Some(navigation) = navigation.upgrade() {
                    if let Some(previous) = navigation.child_by_name("detail") {
                        navigation.remove(&previous);
                    }
                    let detail = album_detail(
                        &engine,
                        &album_name,
                        album_tracks.clone(),
                        cover.upgrade().as_ref(),
                        &navigation,
                    );
                    navigation.add_named(&detail, Some("detail"));
                    navigation.set_visible_child_name("detail");
                }
            });
            grid.insert(&button, -1);
            pictures.push(picture.downgrade());
            sources.push(first.clone());
            drop(grid);
            if index % 8 == 7 {
                glib::timeout_future_with_priority(
                    glib::Priority::LOW,
                    std::time::Duration::from_millis(8),
                )
                .await;
            }
        }
        if !pictures.is_empty() {
            // Worker descarrega as capas; a UI recebe em lotes rápidos de até 8 capas por tick.
            let (sender, receiver) = std::sync::mpsc::sync_channel(16);
            std::thread::spawn(move || {
                for (index, source) in sources.into_iter().enumerate() {
                    let Some(path) = audio::find_cover(&source) else {
                        continue;
                    };
                    let Some(image) = thumb_cache::load_cached_thumbnail(&path, 256) else {
                        continue;
                    };
                    let thumbnail = (
                        index,
                        image.width(),
                        image.height(),
                        image.rowstride() as usize,
                        image.has_alpha(),
                        image.read_pixel_bytes().to_vec(),
                    );
                    if sender.send(thumbnail).is_err() {
                        break;
                    }
                }
            });
            glib::timeout_add_local(std::time::Duration::from_millis(25), move || {
                if pictures.iter().all(|picture| picture.upgrade().is_none()) {
                    return glib::ControlFlow::Break;
                }
                for _ in 0..8 {
                    match receiver.try_recv() {
                        Ok((index, width, height, stride, alpha, pixels)) => {
                            if let Some(picture) = pictures[index].upgrade() {
                                let format = if alpha {
                                    gdk::MemoryFormat::R8g8b8a8
                                } else {
                                    gdk::MemoryFormat::R8g8b8
                                };
                                let bytes = glib::Bytes::from_owned(pixels);
                                let texture =
                                    gdk::MemoryTexture::new(width, height, format, &bytes, stride);
                                picture.set_paintable(Some(&texture));
                            }
                        }
                        Err(std::sync::mpsc::TryRecvError::Empty) => break,
                        Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                            return glib::ControlFlow::Break
                        }
                    }
                }
                glib::ControlFlow::Continue
            });
        }
    });
    (scroll.upcast(), grid)
}

fn album_detail(
    audio: &Arc<AudioEngine>,
    album: &str,
    tracks: Rc<Vec<std::path::PathBuf>>,
    cover: Option<&gtk::Picture>,
    navigation: &gtk::Stack,
) -> gtk::Widget {
    let page = gtk::Box::new(Orientation::Vertical, 22);
    page.set_margin_start(28);
    page.set_margin_end(28);
    page.set_margin_top(20);
    page.set_margin_bottom(18);
    let back = labeled_icon_button_i18n("go-previous-symbolic", |l| {
        i18n::pick_for(l, "Back to albums", "Voltar aos álbuns")
    });
    back.add_css_class("nav-button");
    back.set_halign(gtk::Align::Start);
    back.set_widget_name("album-back");
    let nav = navigation.downgrade();
    back.connect_clicked(move |_| {
        if let Some(nav) = nav.upgrade() {
            nav.set_visible_child_name("browse");
        }
    });
    page.append(&back);
    let header = gtk::Box::new(Orientation::Horizontal, 24);
    let cover_box = gtk::Overlay::new();
    cover_box.set_size_request(160, 160);
    cover_box.set_halign(gtk::Align::Start);
    cover_box.set_valign(gtk::Align::Start);
    cover_box.set_overflow(gtk::Overflow::Hidden);
    cover_box.add_css_class("album-detail-cover");
    let placeholder = gtk::Image::from_icon_name("media-optical-symbolic");
    placeholder.set_pixel_size(64);
    cover_box.set_child(Some(&placeholder));
    let picture = gtk::Picture::new();
    picture.set_content_fit(gtk::ContentFit::Cover);
    picture.set_can_shrink(true);
    if let Some(cover) = cover {
        cover
            .bind_property("paintable", &picture, "paintable")
            .sync_create()
            .build();
    }
    cover_box.add_overlay(&picture);
    header.append(&cover_box);
    let info = gtk::Box::new(Orientation::Vertical, 12);
    info.set_hexpand(true);
    info.set_valign(gtk::Align::Center);
    let title = gtk::Label::new(Some(album));
    title.set_xalign(0.0);
    title.set_wrap(true);
    title.set_max_width_chars(35);
    title.add_css_class("page-title");
    info.append(&title);
    let count = gtk::Label::new(Some(&i18n::album_tracks_local(
        i18n::current(),
        tracks.len(),
    )));
    count.set_xalign(0.0);
    count.add_css_class("bottom-artist-title");
    {
        let cnt_weak = count.downgrade();
        let track_len = tracks.len();
        i18n::connect(&count, move |lang| {
            if let Some(lbl) = cnt_weak.upgrade() {
                lbl.set_text(&i18n::album_tracks_local(lang, track_len));
            }
        });
    }
    info.append(&count);
    let play = labeled_icon_button_i18n("media-playback-start-symbolic", |l| {
        i18n::pick_for(l, "Play album", "Tocar álbum")
    });
    play.add_css_class("open-button");
    play.set_widget_name("album-play");
    play.set_halign(gtk::Align::Start);
    let engine = audio.clone();
    let queue = tracks.clone();
    play.connect_clicked(move |_| {
        let _ = engine.play_album(&queue, 0);
    });
    info.append(&play);
    info.append(&queue_actions(audio, tracks.clone(), true));
    header.append(&info);
    page.append(&header);
    let list = gtk::ListBox::new();
    list.set_selection_mode(gtk::SelectionMode::None);
    list.add_css_class("library-list");
    for (index, path) in tracks.iter().cloned().enumerate() {
        list.append(&track_row_in_queue(
            audio,
            path,
            false,
            Some((tracks.clone(), index)),
        ));
    }
    let scroll = gtk::ScrolledWindow::new();
    scroll.set_vexpand(true);
    scroll.set_policy(gtk::PolicyType::Never, gtk::PolicyType::Automatic);
    scroll.set_child(Some(&list));
    page.append(&scroll);
    page.upcast()
}

fn track_row(audio: &Arc<AudioEngine>, path: std::path::PathBuf, nested: bool) -> gtk::Widget {
    track_row_in_queue(audio, path, nested, None)
}

fn track_row_in_queue(
    audio: &Arc<AudioEngine>,
    path: std::path::PathBuf,
    nested: bool,
    queue: Option<(Rc<Vec<std::path::PathBuf>>, usize)>,
) -> gtk::Widget {
    let button = gtk::Button::new();
    button.set_hexpand(true);
    button.set_halign(gtk::Align::Fill);
    button.add_css_class("track-item-button");
    if nested {
        button.set_margin_start(24);
    }

    let title = path.file_stem().and_then(|s| s.to_str()).unwrap_or("Track");
    let format = path
        .extension()
        .and_then(|s| s.to_str())
        .unwrap_or("audio")
        .to_uppercase();

    let row_box = gtk::Box::new(Orientation::Horizontal, 10);
    let note = gtk::Image::from_icon_name("audio-x-generic-symbolic");
    note.add_css_class("track-icon");
    now_playing::register_track(&button, &note, &path, title);
    let name_label = gtk::Label::new(Some(title));
    name_label.set_hexpand(true);
    name_label.set_xalign(0.0);
    name_label.set_ellipsize(gtk::pango::EllipsizeMode::End);
    name_label.set_max_width_chars(56);
    name_label.add_css_class("track-row-name");

    let badge = gtk::Label::new(Some(&format));
    badge.add_css_class("track-format-tag");

    row_box.append(&note);
    row_box.append(&name_label);
    row_box.append(&badge);
    button.set_child(Some(&row_box));

    let engine = audio.clone();
    let selected = path.clone();
    button.connect_clicked(move |_| {
        if let Some((tracks, index)) = &queue {
            let _ = engine.play_album(tracks, *index);
        } else {
            let _ = engine.load(&selected);
            let _ = engine.play();
        }
    });

    let actions = gtk::Box::new(Orientation::Horizontal, 4);
    actions.append(&button);
    actions.append(&queue_actions(audio, Rc::new(vec![path]), false));
    if nested {
        actions.add_css_class("track-row-item");
        return actions.upcast();
    }
    let row = gtk::ListBoxRow::new();
    row.add_css_class("track-row-item");
    row.set_selectable(false);
    row.set_activatable(false);
    row.set_child(Some(&actions));
    row.upcast()
}

fn queue_actions(
    audio: &Arc<AudioEngine>,
    tracks: Rc<Vec<std::path::PathBuf>>,
    album: bool,
) -> gtk::MenuButton {
    let menu = gtk::MenuButton::new();
    menu.set_widget_name("queue-actions");
    menu.add_css_class("enqueue-menu");
    if album {
        menu.set_label(i18n::pick("Add to queue", "Adicionar à fila"));
        let menu_weak = menu.downgrade();
        i18n::connect(&menu, move |l| {
            if let Some(m) = menu_weak.upgrade() {
                m.set_label(i18n::pick_for(l, "Add to queue", "Adicionar à fila"));
            }
        });
        menu.set_halign(gtk::Align::Start);
        i18n::bind_tooltip(&menu, |l| {
            i18n::pick_for(
                l,
                "Queue options for this album",
                "Opções de fila para este álbum",
            )
        });
    } else {
        menu.set_icon_name("view-more-symbolic");
        i18n::bind_tooltip(&menu, |l| {
            i18n::pick_for(
                l,
                "Queue options for this track",
                "Opções de fila para esta faixa",
            )
        });
    }
    menu.set_valign(gtk::Align::Center);
    let audio = audio.clone();
    menu.set_create_popup_func(move |menu| {
        let popover = gtk::Popover::new();
        let options = gtk::Box::new(Orientation::Vertical, 6);
        let error = gtk::Label::new(None);
        error.set_wrap(true);
        error.set_max_width_chars(30);
        error.set_visible(false);
        error.add_css_class("queue-error");
        let lang = i18n::current();
        for (name, label, next) in [
            (
                "enqueue-next",
                i18n::pick_for(lang, "Play next", "Tocar a seguir"),
                true,
            ),
            (
                "enqueue-last",
                i18n::pick_for(lang, "Add to end", "Adicionar ao final"),
                false,
            ),
        ] {
            let button = gtk::Button::with_label(label);
            button.set_widget_name(name);
            button.add_css_class("folder-button");
            let audio = audio.clone();
            let tracks = tracks.clone();
            let weak_popover = popover.downgrade();
            let weak_menu = menu.downgrade();
            let error = error.clone();
            button.connect_clicked(move |_| match audio.enqueue_files(&tracks, next) {
                Ok(()) => {
                    error.set_visible(false);
                    if let Some(menu) = weak_menu.upgrade() {
                        menu.set_tooltip_text(Some(i18n::pick(
                            "Added to queue",
                            "Adicionado à fila",
                        )));
                    }
                    if let Some(popover) = weak_popover.upgrade() {
                        popover.popdown();
                    }
                }
                Err(message) => {
                    error.set_text(&message.to_string());
                    error.set_visible(true);
                }
            });
            options.append(&button);
        }

        // Listar playlists dinamicamente para adição rápida
        let pl_mgr = playlist::PlaylistManager::new(None);
        let pls = pl_mgr.playlists();
        if !pls.is_empty() {
            let sep = gtk::Separator::new(Orientation::Horizontal);
            options.append(&sep);

            let pl_lbl = gtk::Label::new(Some(i18n::pick_for(
                lang,
                "Add to playlist:",
                "Adicionar à playlist:",
            )));
            pl_lbl.set_xalign(0.0);
            pl_lbl.add_css_class("bottom-artist-title");
            options.append(&pl_lbl);

            let pl_scroll = gtk::ScrolledWindow::new();
            pl_scroll.set_policy(gtk::PolicyType::Never, gtk::PolicyType::Automatic);
            pl_scroll.set_max_content_height(180);
            pl_scroll.set_propagate_natural_height(true);

            let pl_box = gtk::Box::new(Orientation::Vertical, 4);

            for pl in pls.iter() {
                let pl_btn = gtk::Button::new();
                pl_btn.add_css_class("popover-playlist-item");

                let btn_content = gtk::Box::new(Orientation::Horizontal, 8);
                let pl_ico = gtk::Image::from_icon_name("view-list-symbolic");
                pl_ico.set_pixel_size(14);
                let pl_name_lbl = gtk::Label::new(Some(&pl.name));
                pl_name_lbl.set_ellipsize(gtk::pango::EllipsizeMode::End);
                btn_content.append(&pl_ico);
                btn_content.append(&pl_name_lbl);
                pl_btn.set_child(Some(&btn_content));

                let pl_path = pl.file_path.clone();
                let pl_mgr = pl_mgr.clone();
                let tracks = tracks.clone();
                let weak_pop = popover.downgrade();
                let weak_m = menu.downgrade();
                let pl_name = pl.name.clone();
                let error_clone = error.clone();
                pl_btn.connect_clicked(move |_| {
                    match pl_mgr.add_tracks_to_playlist(&pl_path, &tracks) {
                        Ok(()) => {
                            let cur = i18n::current();
                            let msg = i18n::playlist_added_notice(cur, tracks.len(), &pl_name);
                            if let Some(m) = weak_m.upgrade() {
                                m.set_tooltip_text(Some(&msg));
                            }
                            show_system_notification(
                                i18n::pick_for(cur, "Playlist Updated", "Playlist Atualizada"),
                                &msg,
                            );
                            if let Some(p) = weak_pop.upgrade() {
                                p.popdown();
                            }
                        }
                        Err(e) => {
                            error_clone.set_text(&format!(
                                "{}: {e}",
                                i18n::pick("Error saving", "Erro ao salvar")
                            ));
                            error_clone.set_visible(true);
                        }
                    }
                });
                pl_box.append(&pl_btn);
            }
            pl_scroll.set_child(Some(&pl_box));
            options.append(&pl_scroll);
        }

        options.append(&error);
        popover.set_child(Some(&options));
        menu.set_popover(Some(&popover));
    });
    menu
}

fn labeled_icon_button_i18n(icon: &str, text: fn(i18n::Language) -> &'static str) -> gtk::Button {
    let content = gtk::Box::new(Orientation::Horizontal, 10);
    content.append(&gtk::Image::from_icon_name(icon));
    let label = gtk::Label::new(None);
    label.set_xalign(0.0);
    i18n::bind_label(&label, text);
    content.append(&label);
    let button = gtk::Button::new();
    button.set_child(Some(&content));
    button
}

fn header(
    audio: &Arc<AudioEngine>,
    search_hub: &search::SearchHub,
    stack: &gtk::Stack,
    nav_buttons: &[(gtk::Button, &'static str, &'static str)],
    title_label: &gtk::Label,
    catalog: &Rc<library_settings::Catalog>,
) -> (gtk::Widget, gtk::SearchEntry, gtk::Button) {
    let bar = gtk::Box::new(Orientation::Horizontal, 14);
    bar.add_css_class("topbar");

    bar.append(title_label);

    let search = gtk::SearchEntry::new();
    i18n::bind_placeholder(&search, |l| {
        i18n::pick_for(
            l,
            "Search tracks, artists, or albums… (Ctrl+F)",
            "Buscar faixas, artistas ou álbuns… (Ctrl+F)",
        )
    });
    search.set_width_chars(28);
    search.add_css_class("search-box");

    let search_hub_input = search_hub.clone();
    let stack_switch = stack.clone();
    let nav_buttons_switch = nav_buttons.to_vec();
    let title_ref = title_label.clone();
    search.connect_search_changed(move |entry| {
        let text = entry.text().to_string();
        search_hub_input.set_query(&text);
        if !text.trim().is_empty()
            && stack_switch.visible_child_name().as_deref() == Some("overview")
        {
            stack_switch.set_visible_child_name("tracks");
            title_ref.set_text(i18n::nav_title(i18n::current(), "tracks"));
            for (b, page, _) in &nav_buttons_switch {
                if *page == "tracks" {
                    b.add_css_class("active-nav");
                } else {
                    b.remove_css_class("active-nav");
                }
            }
        }
    });

    search.connect_stop_search(move |entry| {
        entry.set_text("");
    });

    let search_audio = audio.clone();
    let search_catalog = catalog.clone();
    search.connect_activate(move |entry| {
        let query = entry.text().to_string();
        let trimmed = query.trim();
        if trimmed.is_empty() {
            return;
        }
        let matches: Vec<std::path::PathBuf> = search_catalog
            .report
            .borrow()
            .tracks
            .iter()
            .filter(|p| search::track_matches(p, trimmed))
            .cloned()
            .collect();
        if !matches.is_empty() {
            let _ = search_audio.play_files(&matches, 0);
        } else if search_audio.search_and_load(trimmed).is_some() {
            let _ = search_audio.play();
        }
    });
    bar.append(&search);

    let open = labeled_icon_button_i18n("folder-open-symbolic", |l| {
        i18n::pick_for(l, "Open track", "Abrir faixa")
    });
    i18n::bind_tooltip(&open, |l| {
        i18n::pick_for(l, "Open track · Ctrl+O", "Abrir faixa · Ctrl+O")
    });
    open.add_css_class("open-button");
    let open_audio = audio.clone();
    open.connect_clicked(move |btn| {
        let filter = gtk::FileFilter::new();
        filter.set_name(Some(i18n::pick(
            "Supported audio files",
            "Arquivos de áudio suportados",
        )));
        for ext in &[
            "flac", "mp3", "ogg", "oga", "wav", "m4a", "aac", "opus", "alac", "aif", "aiff", "wma",
        ] {
            filter.add_suffix(ext);
        }
        let all_filter = gtk::FileFilter::new();
        all_filter.set_name(Some(i18n::pick("All files", "Todos os arquivos")));
        all_filter.add_pattern("*");

        let filters = gtk::gio::ListStore::new::<gtk::FileFilter>();
        filters.append(&filter);
        filters.append(&all_filter);

        let dialog = gtk::FileDialog::builder()
            .title(i18n::pick("Open audio file", "Abrir arquivo de áudio"))
            .filters(&filters)
            .default_filter(&filter)
            .build();
        let parent = btn.root().and_then(|r| r.downcast::<gtk::Window>().ok());
        let engine = open_audio.clone();
        glib::MainContext::default().spawn_local(async move {
            if let Ok(files) = dialog.open_multiple_future(parent.as_ref()).await {
                let mut paths = Vec::new();
                for i in 0..files.n_items() {
                    if let Some(file) = files
                        .item(i)
                        .and_then(|obj| obj.downcast::<gtk::gio::File>().ok())
                    {
                        if let Some(path) = file.path() {
                            paths.push(path);
                        }
                    }
                }
                if !paths.is_empty() {
                    let _ = engine.play_files(&paths, 0);
                }
            }
        });
    });
    bar.append(&open);
    (bar.upcast(), search, open)
}

fn main_stage(
    playing: &Rc<Cell<bool>>,
    angle: &Rc<Cell<f64>>,
    arm_angle: &Rc<Cell<f64>>,
    audio: &Arc<AudioEngine>,
    state_mgr: &state::StateManager,
    volume_hud: &Rc<RefCell<VolumeHudState>>,
) -> gtk::Widget {
    let initial_state = state_mgr.snapshot();
    let initial_rpm = initial_state.rpm;
    let rpm = Rc::new(Cell::new(initial_rpm));

    let initial_target_light = initial_state.target_light;
    let target_light = Rc::new(Cell::new(initial_target_light));

    let initial_pitch = initial_state.pitch;
    let pitch = Rc::new(Cell::new(initial_pitch));
    audio.set_rate(1.0 + initial_pitch);

    let initial_needle_drop = initial_state.needle_drop;
    let needle_drop = Rc::new(Cell::new(initial_needle_drop));

    let stage = gtk::Box::new(Orientation::Vertical, 12);
    stage.add_css_class("listening-room");
    stage.set_vexpand(true);
    stage.set_hexpand(true);
    stage.set_halign(gtk::Align::Fill);
    stage.set_valign(gtk::Align::Fill);
    stage.set_margin_start(28);
    stage.set_margin_end(28);
    stage.set_margin_top(22);
    stage.set_margin_bottom(22);

    let heading = gtk::Box::new(Orientation::Horizontal, 12);
    let eyebrow = gtk::Label::new(None);
    i18n::bind_label(&eyebrow, |l| {
        i18n::pick_for(l, "LISTENING ROOM", "SESSÃO DE ESCUTA")
    });
    eyebrow.add_css_class("listening-eyebrow");
    eyebrow.set_xalign(0.0);
    eyebrow.set_hexpand(true);
    heading.append(&eyebrow);
    let status_box = gtk::Box::new(Orientation::Horizontal, 7);
    status_box.add_css_class("listening-status");
    let status_icon = gtk::Image::from_icon_name("media-playback-stop-symbolic");
    status_icon.set_pixel_size(12);
    let status = gtk::Label::new(Some(i18n::playback_status(i18n::current(), audio.state())));
    status_box.append(&status_icon);
    status_box.append(&status);
    heading.append(&status_box);

    let rpm_box = gtk::Box::new(Orientation::Horizontal, 0);
    rpm_box.add_css_class("rpm-selector");
    i18n::bind_tooltip(&rpm_box, |l| {
        i18n::pick_for(
            l,
            "Rotational speed: 33 ⅓ RPM or 45 RPM",
            "Velocidade de rotação: 33 ⅓ RPM ou 45 RPM",
        )
    });

    let btn_33 = gtk::ToggleButton::new();
    btn_33.set_label("33 ⅓");
    btn_33.add_css_class("rpm-button");
    btn_33.set_active(initial_rpm == 33);

    let btn_45 = gtk::ToggleButton::new();
    btn_45.set_label("45");
    btn_45.add_css_class("rpm-button");
    btn_45.set_active(initial_rpm == 45);

    rpm_box.append(&btn_33);
    rpm_box.append(&btn_45);
    heading.append(&rpm_box);
    stage.append(&heading);

    let cache = Rc::new(RefCell::new(TurntableCache {
        bg_surface: None,
        disc_surface: None,
        cover: CachedCover {
            key: String::new(),
            original: None,
            surface: None,
        },
    }));

    let vu_needles = Rc::new(Cell::new((0.0_f64, 0.0_f64)));
    let draw_vu = vu_needles.clone();

    let deck = DrawingArea::new();
    deck.set_content_width(560);
    deck.set_content_height(340);
    deck.set_hexpand(true);
    deck.set_vexpand(true);

    let draw_angle = angle.clone();
    let draw_arm = arm_angle.clone();
    let draw_playing = playing.clone();
    let draw_cache = cache.clone();
    let draw_rpm = rpm.clone();
    let draw_hud = volume_hud.clone();
    let draw_tl = target_light.clone();
    let draw_pitch = pitch.clone();
    deck.set_draw_func(move |widget, cr, w, h| {
        let (hud_level, hud_alpha) = {
            let hud = draw_hud.borrow();
            (hud.level, hud.alpha)
        };
        let (vu_l, vu_r) = draw_vu.get();
        let params = TurntableRenderParams {
            w,
            h,
            disc_angle: draw_angle.get(),
            arm_angle: draw_arm.get(),
            is_playing: draw_playing.get(),
            cache: &draw_cache,
            scale: widget.scale_factor(),
            rpm: draw_rpm.get(),
            volume_level: hud_level,
            volume_alpha: hud_alpha,
            vu_needle_l: vu_l,
            vu_needle_r: vu_r,
            target_light_on: draw_tl.get(),
            pitch: draw_pitch.get(),
        };
        draw_turntable(cr, &params);
    });

    let updating = Rc::new(Cell::new(false));

    let on_rpm_change = {
        let rpm = rpm.clone();
        let state_mgr = state_mgr.clone();
        let deck = deck.downgrade();
        let btn_33 = btn_33.downgrade();
        let btn_45 = btn_45.downgrade();
        let updating = updating.clone();
        Rc::new(move |new_rpm: u32| {
            if updating.get() {
                return;
            }
            updating.set(true);
            let target = if new_rpm == 45 { 45 } else { 33 };
            if rpm.get() != target {
                rpm.set(target);
                state_mgr.update_rpm(target);
                state_mgr.flush_if_dirty();
                if let Some(d) = deck.upgrade() {
                    d.queue_draw();
                }
            }
            if let Some(b33) = btn_33.upgrade() {
                b33.set_active(target == 33);
            }
            if let Some(b45) = btn_45.upgrade() {
                b45.set_active(target == 45);
            }
            updating.set(false);
        })
    };

    {
        let cb = on_rpm_change.clone();
        let updating = updating.clone();
        btn_33.connect_toggled(move |btn| {
            if updating.get() {
                return;
            }
            if btn.is_active() {
                cb(33);
            } else {
                updating.set(true);
                btn.set_active(true);
                updating.set(false);
            }
        });
    }

    {
        let cb = on_rpm_change.clone();
        let updating = updating.clone();
        btn_45.connect_toggled(move |btn| {
            if updating.get() {
                return;
            }
            if btn.is_active() {
                cb(45);
            } else {
                updating.set(true);
                btn.set_active(true);
                updating.set(false);
            }
        });
    }

    let click = gtk::GestureClick::new();
    let click_cb = on_rpm_change.clone();
    let click_deck = deck.downgrade();
    let click_tl = target_light.clone();
    let click_pitch = pitch.clone();
    let click_state_mgr = state_mgr.clone();
    let click_audio = audio.clone();
    let click_playing = playing.clone();
    click.connect_released(move |_, n_press, x, y| {
        let Some(deck_widget) = click_deck.upgrade() else {
            return;
        };
        let w = deck_widget.width() as f64;
        let h = deck_widget.height() as f64;
        let layout = TurntableLayout::compute(w, h);

        let d33 = ((x - layout.led_x).powi(2) + (y - layout.led_y).powi(2)).sqrt();
        let led45_x = layout.led_x + 50.0;
        let d45 = ((x - led45_x).powi(2) + (y - layout.led_y).powi(2)).sqrt();

        let in_33 = d33 <= 20.0
            || (x >= layout.led_x - 8.0
                && x <= layout.led_x + 36.0
                && (y - layout.led_y).abs() <= 14.0);
        let in_45 = d45 <= 20.0
            || (x >= led45_x - 8.0 && x <= led45_x + 36.0 && (y - layout.led_y).abs() <= 14.0);

        if in_33 {
            click_cb(33);
            return;
        } else if in_45 {
            click_cb(45);
            return;
        }

        // 2. Clique na Torre da Luz Alvo (Stylus Target Light)
        let d_tl =
            ((x - layout.target_light_x).powi(2) + (y - layout.target_light_y).powi(2)).sqrt();
        if d_tl <= layout.target_light_r + 6.0 {
            let next_tl = !click_state_mgr.snapshot().target_light;
            click_tl.set(next_tl);
            click_state_mgr.update_target_light(next_tl);
            click_state_mgr.flush_if_dirty();
            deck_widget.queue_draw();
            return;
        }

        // 3. Clique, duplo-clique (Quartz Lock 0.0%) ou trava no Pitch Fader
        let px = layout.pitch_x;
        let py = layout.pitch_y;
        let pw = layout.pitch_w;
        let ph = layout.pitch_h;
        if x >= px - 6.0 && x <= px + pw + 6.0 && y >= py - 6.0 && y <= py + ph + 6.0 {
            let center_y = py + ph / 2.0;
            let slot_h = ph - 28.0;
            let new_pitch = if n_press >= 2 || (y - center_y).abs() <= 5.0 || x <= px + 8.0 {
                0.0
            } else {
                let rel = (center_y - y) / (slot_h / 2.0);
                let p = (rel * 0.08).clamp(-0.08, 0.08);
                if p.abs() < 0.003 {
                    0.0
                } else {
                    p
                }
            };
            click_pitch.set(new_pitch);
            click_state_mgr.update_pitch(new_pitch);
            click_state_mgr.flush_if_dirty();
            click_audio.set_rate(1.0 + new_pitch);
            deck_widget.queue_draw();
            return;
        }

        // 4. Clique no Prato / Disco de Vinil para Play / Pause com Agulha
        let d_platter = ((x - layout.cx).powi(2) + (y - layout.cy).powi(2)).sqrt();
        if d_platter <= layout.r_platter {
            let next_playing = !click_playing.get();
            if next_playing {
                let _ = click_audio.play();
            } else {
                let _ = click_audio.pause();
            }
            click_playing.set(next_playing);
            deck_widget.queue_draw();
        }
    });
    deck.add_controller(click);

    let motion = gtk::EventControllerMotion::new();
    let motion_deck = deck.downgrade();
    motion.connect_motion(move |_, x, y| {
        let Some(deck_widget) = motion_deck.upgrade() else {
            return;
        };
        let w = deck_widget.width() as f64;
        let h = deck_widget.height() as f64;
        let layout = TurntableLayout::compute(w, h);

        let d33 = ((x - layout.led_x).powi(2) + (y - layout.led_y).powi(2)).sqrt();
        let led45_x = layout.led_x + 50.0;
        let d45 = ((x - led45_x).powi(2) + (y - layout.led_y).powi(2)).sqrt();
        let in_rpm = d33 <= 20.0 || d45 <= 20.0;

        let d_tl =
            ((x - layout.target_light_x).powi(2) + (y - layout.target_light_y).powi(2)).sqrt();
        let in_tl = d_tl <= layout.target_light_r + 6.0;

        let px = layout.pitch_x;
        let py = layout.pitch_y;
        let pw = layout.pitch_w;
        let ph = layout.pitch_h;
        let in_pitch = x >= px - 6.0 && x <= px + pw + 6.0 && y >= py - 6.0 && y <= py + ph + 6.0;

        let d_platter = ((x - layout.cx).powi(2) + (y - layout.cy).powi(2)).sqrt();
        let in_platter = d_platter <= layout.r_platter;

        if in_rpm || in_tl || in_pitch || in_platter {
            deck_widget.set_cursor_from_name(Some("pointer"));
        } else {
            deck_widget.set_cursor_from_name(None);
        }
    });
    let motion_deck_leave = deck.downgrade();
    motion.connect_leave(move |_| {
        if let Some(deck_widget) = motion_deck_leave.upgrade() {
            deck_widget.set_cursor_from_name(None);
        }
    });
    deck.add_controller(motion);

    let drag = gtk::GestureDrag::new();
    let drag_pitch = pitch.clone();
    let drag_state_mgr = state_mgr.clone();
    let drag_audio = audio.clone();
    let drag_deck = deck.downgrade();
    drag.connect_drag_update(move |gesture, _offset_x, _offset_y| {
        let Some((start_x, start_y)) = gesture.start_point() else {
            return;
        };
        let Some(deck_widget) = drag_deck.upgrade() else {
            return;
        };
        let w = deck_widget.width() as f64;
        let h = deck_widget.height() as f64;
        let layout = TurntableLayout::compute(w, h);
        let px = layout.pitch_x;
        let py = layout.pitch_y;
        let pw = layout.pitch_w;
        let ph = layout.pitch_h;

        if start_x >= px - 10.0
            && start_x <= px + pw + 10.0
            && start_y >= py - 8.0
            && start_y <= py + ph + 8.0
        {
            let (_, cur_offset_y) = gesture.offset().unwrap_or((0.0, 0.0));
            let cur_y = start_y + cur_offset_y;
            let center_y = py + ph / 2.0;
            let slot_h = ph - 28.0;
            let rel = (center_y - cur_y) / (slot_h / 2.0);
            let p = (rel * 0.08).clamp(-0.08, 0.08);
            let final_pitch = if p.abs() < 0.003 { 0.0 } else { p };
            if (drag_pitch.get() - final_pitch).abs() > 0.0005 {
                drag_pitch.set(final_pitch);
                drag_state_mgr.update_pitch(final_pitch);
                drag_state_mgr.flush_if_dirty();
                drag_audio.set_rate(1.0 + final_pitch);
                deck_widget.queue_draw();
            }
        }
    });
    deck.add_controller(drag);

    let scroll = gtk::EventControllerScroll::new(gtk::EventControllerScrollFlags::VERTICAL);
    let scroll_audio = audio.clone();
    let scroll_state = state_mgr.clone();
    let scroll_hud = volume_hud.clone();
    let scroll_deck = deck.downgrade();
    scroll.connect_scroll(move |_, _dx, dy| {
        let step = if dy.abs() > 0.01 {
            (-dy.signum() * 0.03).clamp(-0.10, 0.10)
        } else {
            0.0
        };
        if step.abs() > 0.0001 {
            let cur = scroll_audio.volume();
            let new_vol = (cur + step).clamp(0.0, 1.0);
            scroll_audio.set_volume(new_vol);
            scroll_state.update_volume(new_vol);
            scroll_hud.borrow_mut().trigger(new_vol);
            if let Some(d) = scroll_deck.upgrade() {
                d.queue_draw();
            }
        }
        glib::Propagation::Stop
    });
    deck.add_controller(scroll);

    stage.append(&deck);

    let information = gtk::Box::new(Orientation::Horizontal, 20);
    information.add_css_class("listening-information");
    let description = gtk::Box::new(Orientation::Vertical, 7);
    description.set_hexpand(true);
    let title = gtk::Label::new(Some(i18n::pick(
        "Choose your next track",
        "Escolha sua próxima faixa",
    )));
    title.set_xalign(0.0);
    title.set_ellipsize(gtk::pango::EllipsizeMode::End);
    title.set_max_width_chars(38);
    title.add_css_class("listening-title");
    let artist = gtk::Label::new(Some(i18n::pick(
        "Open an album or drag and drop an audio file",
        "Abra um álbum ou arraste um arquivo de áudio",
    )));
    artist.set_xalign(0.0);
    artist.set_ellipsize(gtk::pango::EllipsizeMode::End);
    artist.set_max_width_chars(55);
    artist.add_css_class("listening-artist");
    description.append(&title);
    description.append(&artist);
    information.append(&description);
    let format = gtk::Label::new(Some(i18n::pick("LOCAL AUDIO", "ÁUDIO LOCAL")));
    format.add_css_class("listening-format");
    format.set_valign(gtk::Align::Center);
    format.set_halign(gtk::Align::End);
    information.append(&format);
    stage.append(&information);

    // Loop de animação sincronizado com VSync via FrameClock e tempo delta exato
    let tick_playing = playing.clone();
    let tick_angle = angle.clone();
    let tick_arm = arm_angle.clone();
    let tick_audio = audio.clone();
    let tick_cache = cache.clone();
    let tick_rpm = rpm.clone();
    let tick_hud = volume_hud.clone();
    let tick_vu = vu_needles.clone();
    let tick_needle_drop = needle_drop.clone();
    let tick_tl = target_light.clone();
    let tick_notifications = Rc::new(Cell::new(initial_state.notifications));
    let tick_state_mgr = state_mgr.clone();
    let last_state_rev = Cell::new(state_mgr.revision());
    let tick_pitch = pitch.clone();
    let last_pitch = Rc::new(Cell::new(initial_pitch));
    let last_notif_seq = Cell::new(0_u64);
    let pending_notif: RefCell<Option<(u64, std::time::Instant)>> = RefCell::new(None);
    let last_rpm = Rc::new(Cell::new(initial_rpm));

    let last_frame_time = Rc::new(Cell::new(0_i64));
    let last_meta_ver = Rc::new(Cell::new(0_u64));
    {
        let lmv_i18n = last_meta_ver.clone();
        let status_weak = status.downgrade();
        let audio_i18n = audio.clone();
        i18n::connect(&deck, move |lang| {
            lmv_i18n.set(0);
            if let Some(st) = status_weak.upgrade() {
                st.set_text(i18n::playback_status(lang, audio_i18n.state()));
            }
        });
    }
    let last_metadata_poll = Rc::new(Cell::new(0_i64));
    let last_position_poll = Cell::new(0_i64);
    let target_arm = Cell::new(1.80_f64);
    let last_state = Cell::new(PlaybackState::Stopped);

    deck.add_tick_callback(move |deck_widget, frame_clock| {
        let frame_time = frame_clock.frame_time();
        let prev_time = last_frame_time.get();

        last_frame_time.set(frame_time);

        let dt = if prev_time == 0 {
            0.0
        } else {
            ((frame_time - prev_time) as f64 / 1_000_000.0).max(0.0)
        };
        let smoothing_dt = dt.min(0.05);

        let mut settings_changed_draw = false;
        let cur_state_rev = tick_state_mgr.revision();
        if cur_state_rev != last_state_rev.replace(cur_state_rev) {
            let snap = tick_state_mgr.snapshot();
            tick_needle_drop.set(snap.needle_drop);
            tick_notifications.set(snap.notifications);
            if tick_tl.replace(snap.target_light) != snap.target_light {
                settings_changed_draw = true;
            }
        }

        let state = tick_audio.state();
        let is_playing = state == PlaybackState::Playing;
        if state != last_state.replace(state) {
            status.set_text(i18n::playback_status(i18n::current(), state));
            status_icon.set_icon_name(Some(match state {
                PlaybackState::Playing => "media-playback-start-symbolic",
                PlaybackState::Paused => "media-playback-pause-symbolic",
                PlaybackState::Stopped => "media-playback-stop-symbolic",
            }));
            if is_playing {
                if tick_needle_drop.get() {
                    needle_drop::play_needle_drop();
                }
                status_box.add_css_class("is-playing");
            } else {
                status_box.remove_css_class("is-playing");
                *pending_notif.borrow_mut() = None;
                if state == PlaybackState::Stopped {
                    last_notif_seq.set(0);
                }
            }
        }
        let playback_changed = tick_playing.replace(is_playing) != is_playing;

        // 1. Atualização reativa de metadados e capa SOMENTE se mudaram de versão
        let should_poll_metadata =
            last_metadata_poll.get() == 0 || frame_time - last_metadata_poll.get() >= 250_000;
        if should_poll_metadata {
            last_metadata_poll.set(frame_time);
            let audio_pitch = (tick_audio.rate() - 1.0).clamp(-0.08, 0.08);
            if (audio_pitch - tick_pitch.get()).abs() > 0.001 {
                tick_pitch.set(audio_pitch);
                tick_state_mgr.update_pitch(audio_pitch);
                settings_changed_draw = true;
            }
        }
        if is_playing {
            let cur_seq = tick_audio.track_sequence();
            if cur_seq != 0 && cur_seq != last_notif_seq.get() {
                let mut pending = pending_notif.borrow_mut();
                if pending.as_ref().is_none_or(|(s, _)| *s != cur_seq) {
                    *pending = Some((cur_seq, std::time::Instant::now()));
                }
                if let Some((seq, started_at)) = *pending {
                    let m = tick_audio.metadata();
                    if m.uri.is_empty() {
                        *pending = None;
                    } else if (!m.artist.is_empty() && !m.title.is_empty())
                        || started_at.elapsed() >= std::time::Duration::from_millis(450)
                    {
                        last_notif_seq.set(seq);
                        *pending = None;
                        if tick_notifications.get() {
                            show_track_notification(
                                &m.title,
                                &m.artist,
                                &m.album,
                                m.cover_path.as_deref(),
                            );
                        }
                    }
                }
            }
        }
        let current_ver = if should_poll_metadata {
            tick_audio.metadata_version()
        } else {
            last_meta_ver.get()
        };
        if should_poll_metadata && current_ver != last_meta_ver.get() {
            last_meta_ver.set(current_ver);
            let m = tick_audio.metadata();
            title.set_text(if m.title.is_empty() {
                i18n::pick("Choose your next track", "Escolha sua próxima faixa")
            } else {
                &m.title
            });
            title.set_tooltip_text(if m.title.is_empty() {
                None
            } else {
                Some(&m.title)
            });
            let detail = [&m.artist, &m.album]
                .into_iter()
                .filter(|part| !part.is_empty())
                .map(|part| part.as_str())
                .collect::<Vec<_>>()
                .join(" · ");
            artist.set_text(if detail.is_empty() {
                i18n::pick(
                    "Open an album or drag and drop an audio file",
                    "Abra um álbum ou arraste um arquivo de áudio",
                )
            } else {
                &detail
            });
            artist.set_tooltip_text(if detail.is_empty() {
                None
            } else {
                Some(&detail)
            });
            let extension = glib::filename_from_uri(&m.uri).ok().and_then(|(path, _)| {
                path.extension()
                    .and_then(|ext| ext.to_str())
                    .map(str::to_uppercase)
            });
            let mut technical = Vec::new();
            if let Some(extension) = extension {
                technical.push(extension);
            }
            if let Some(rate) = m.sample_rate {
                technical.push(format!("{} kHz", rate as f64 / 1000.0));
            }
            if let Some(depth) = m.bit_depth {
                technical.push(format!("{depth}-bit"));
            }
            let technical_text = technical.join(" · ");
            format.set_text(if technical.is_empty() {
                i18n::pick("LOCAL AUDIO", "ÁUDIO LOCAL")
            } else {
                &technical_text
            });
            let current_key = if let Some(ref p) = m.cover_path {
                format!("path:{}", p.display())
            } else if let Some(ref b) = m.cover_bytes {
                format!("bytes:{}", b.len())
            } else if !m.uri.is_empty() {
                format!("uri:{}", m.uri)
            } else {
                "none".to_string()
            };

            let mut state = tick_cache.borrow_mut();
            if current_key != state.cover.key {
                state.cover = CachedCover {
                    key: current_key.clone(),
                    original: None,
                    surface: None,
                };
                state.disc_surface = None;
                drop(state);

                let cover_path_bg = m.cover_path.clone();
                let cover_bytes_bg = m.cover_bytes.clone();
                if cover_path_bg.is_some() || cover_bytes_bg.is_some() {
                    let cache_async = tick_cache.clone();
                    let deck_weak = deck_widget.downgrade();
                    let expected_key = current_key;
                    glib::MainContext::default().spawn_local(async move {
                        let decoded = gtk::gio::spawn_blocking(move || {
                            let pb = if let Some(ref p) = cover_path_bg {
                                thumb_cache::load_cached_thumbnail(p, 512)
                            } else if let Some(ref b) = cover_bytes_bg {
                                let loader = gdk_pixbuf::PixbufLoader::new();
                                if loader.write(b).is_ok() && loader.close().is_ok() {
                                    loader.pixbuf()
                                } else {
                                    None
                                }
                            } else {
                                None
                            }?;
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

                        if let Some((w, h, stride, alpha, bytes)) = decoded {
                            let mut st = cache_async.borrow_mut();
                            if st.cover.key == expected_key {
                                let pb = gdk_pixbuf::Pixbuf::from_bytes(
                                    &bytes,
                                    gdk_pixbuf::Colorspace::Rgb,
                                    alpha,
                                    8,
                                    w,
                                    h,
                                    stride,
                                );
                                st.cover.original = Some(pb);
                                st.cover.surface = None;
                                st.disc_surface = None;
                                drop(st);
                                if let Some(d) = deck_weak.upgrade() {
                                    d.queue_draw();
                                }
                            }
                        }
                    });
                }
            } else {
                drop(state);
            }
            deck_widget.queue_draw();
        }

        // 2. Física e Dinâmica da Rotação e do Braço
        let mut needs_draw = playback_changed || settings_changed_draw;
        if tick_rpm.get() != last_rpm.get() {
            last_rpm.set(tick_rpm.get());
            needs_draw = true;
        }
        if (tick_pitch.get() - last_pitch.get()).abs() > 0.0005 {
            last_pitch.set(tick_pitch.get());
            needs_draw = true;
        }
        let current_arm = tick_arm.get();

        if is_playing {
            // Rotação contínua calibrada modulada pelo pitch analógico
            let base_speed = if tick_rpm.get() == 45 {
                4.71238898038469_f64
            } else {
                3.490658503988659_f64
            };
            let speed_rad = base_speed * (1.0 + tick_pitch.get());
            let delta_angle = speed_rad * dt;
            tick_angle.set((tick_angle.get() + delta_angle) % TAU);

            // Rastreamento suave da posição da faixa sobre o vinil
            if last_position_poll.get() == 0
                || frame_time - last_position_poll.get() >= 250_000
                || playback_changed
            {
                last_position_poll.set(frame_time);
                let dur = tick_audio.duration_us();
                let pos = tick_audio.position_us();
                let progress = if dur > 0 {
                    (pos as f64 / dur as f64).clamp(0.0, 1.0)
                } else {
                    0.0
                };
                target_arm.set(1.80 + progress * 0.58);
            }

            // Amortecimento exponencial suave
            let arm_speed = 3.8;
            let factor = 1.0 - (-arm_speed * smoothing_dt).exp();
            let new_arm = current_arm + (target_arm.get() - current_arm) * factor;
            tick_arm.set(new_arm);

            needs_draw = true;
        } else {
            // Retorno suave e orgânico do braço para a base de repouso (1.476 rad)
            let target_arm = 1.476;
            let diff = target_arm - current_arm;
            if diff.abs() > 0.0003 {
                let arm_speed = 4.2;
                let factor = 1.0 - (-arm_speed * smoothing_dt).exp();
                let new_arm = current_arm + diff * factor;
                tick_arm.set(new_arm);
                needs_draw = true;
            }
        }

        // 3. Balística dos Medidores VU Analógicos
        let (raw_l, raw_r) = if is_playing {
            tick_audio.vu_levels()
        } else {
            (-80.0, -80.0)
        };
        let target_l = crate::audio::db_to_vu_deflection(raw_l);
        let target_r = crate::audio::db_to_vu_deflection(raw_r);

        let (cur_l, cur_r) = tick_vu.get();
        let update_needle = |cur: f64, target: f64, dt: f64| -> f64 {
            if target > cur {
                let rate = (1.0 - (-dt * 26.0).exp()).min(1.0);
                cur + (target - cur) * rate
            } else {
                let rate = (1.0 - (-dt * 7.5).exp()).min(1.0);
                cur + (target - cur) * rate
            }
        };
        let next_l = update_needle(cur_l, target_l, smoothing_dt);
        let next_r = update_needle(cur_r, target_r, smoothing_dt);
        let vu_active = (next_l - cur_l).abs() > 0.0008
            || (next_r - cur_r).abs() > 0.0008
            || next_l > 0.005
            || next_r > 0.005;
        tick_vu.set((next_l, next_r));

        let hud_active = tick_hud.borrow_mut().tick_fade();

        if needs_draw || hud_active || vu_active {
            deck_widget.queue_draw();
        }

        glib::ControlFlow::Continue
    });

    stage.upcast()
}

fn update_shuffle_btn(btn: &gtk::Button, active: bool) {
    let lang = i18n::current();
    if active {
        btn.add_css_class("active");
    } else {
        btn.remove_css_class("active");
    }
    btn.set_tooltip_text(Some(i18n::shuffle_tooltip(lang, active)));
}

fn update_repeat_btn(btn: &gtk::Button, mode: RepeatMode) {
    let lang = i18n::current();
    match mode {
        RepeatMode::Off => {
            btn.remove_css_class("active");
            btn.set_icon_name("media-playlist-repeat-symbolic");
        }
        RepeatMode::All => {
            btn.add_css_class("active");
            btn.set_icon_name("media-playlist-repeat-symbolic");
        }
        RepeatMode::Track => {
            btn.add_css_class("active");
            btn.set_icon_name("media-playlist-repeat-song-symbolic");
        }
    }
    btn.set_tooltip_text(Some(i18n::repeat_tooltip(lang, mode)));
}

fn update_volume_icon(btn: &gtk::Button, icon: &gtk::Image, vol: f64, lang: i18n::Language) {
    let name = if vol <= 0.001 {
        "audio-volume-muted-symbolic"
    } else if vol < 0.34 {
        "audio-volume-low-symbolic"
    } else if vol < 0.67 {
        "audio-volume-medium-symbolic"
    } else {
        "audio-volume-high-symbolic"
    };
    icon.set_icon_name(Some(name));
    if vol <= 0.001 {
        btn.add_css_class("active");
    } else {
        btn.remove_css_class("active");
    }
    btn.set_tooltip_text(Some(&i18n::mute_tooltip(lang, vol)));
}

fn next_mute_toggle(cur_vol: f64, pre_mute: &Cell<f64>) -> f64 {
    if cur_vol > 0.005 {
        pre_mute.set(cur_vol);
        0.0
    } else {
        let restore = pre_mute.get();
        if restore > 0.01 {
            restore
        } else {
            0.75
        }
    }
}

fn player_bar(
    playing: &Rc<Cell<bool>>,
    audio: &Arc<AudioEngine>,
    state_mgr: &state::StateManager,
    queue: &queue_view::QueuePanel,
    errors: &playback_error::ErrorPresenter,
    volume_hud: &Rc<RefCell<VolumeHudState>>,
) -> (gtk::Widget, gtk::Button, gtk::Button, gtk::Button) {
    let bar = gtk::Box::new(Orientation::Horizontal, 20);
    bar.add_css_class("player-bar");

    // Lado Esquerdo: Miniatura e Metadados da Faixa
    let left_group = gtk::Box::new(Orientation::Horizontal, 12);
    left_group.set_width_request(220);

    let cover_stack = gtk::Stack::new();
    cover_stack.set_transition_type(gtk::StackTransitionType::Crossfade);
    cover_stack.set_transition_duration(150);
    cover_stack.set_width_request(48);
    cover_stack.set_height_request(48);
    cover_stack.set_overflow(gtk::Overflow::Hidden);
    cover_stack.add_css_class("mini-cover-container");

    let mini_disc = gtk::Image::from_icon_name("media-optical-symbolic");
    mini_disc.set_pixel_size(26);
    mini_disc.add_css_class("mini-disc-icon");
    cover_stack.add_named(&mini_disc, Some("placeholder"));

    let mini_picture = gtk::Picture::new();
    mini_picture.set_content_fit(gtk::ContentFit::Cover);
    mini_picture.set_can_shrink(true);
    mini_picture.add_css_class("mini-cover-picture");
    cover_stack.add_named(&mini_picture, Some("cover"));
    cover_stack.set_visible_child_name("placeholder");

    let last_cover_key = Rc::new(RefCell::new(String::new()));
    let init_m = audio.metadata();
    if let Some(ref p) = init_m.cover_path {
        if let Some(pixbuf) = thumb_cache::load_cached_thumbnail(p, 128) {
            let texture = gdk::Texture::for_pixbuf(&pixbuf);
            mini_picture.set_paintable(Some(&texture));
            cover_stack.set_visible_child_name("cover");
            *last_cover_key.borrow_mut() = format!("path:{}", p.display());
        }
    } else if let Some(ref b) = init_m.cover_bytes {
        let loader = gdk_pixbuf::PixbufLoader::new();
        if loader.write(b).is_ok() && loader.close().is_ok() {
            if let Some(pixbuf) = loader.pixbuf() {
                let texture = gdk::Texture::for_pixbuf(&pixbuf);
                mini_picture.set_paintable(Some(&texture));
                cover_stack.set_visible_child_name("cover");
                *last_cover_key.borrow_mut() = format!("bytes:{}", b.len());
            }
        }
    }

    let text_group = gtk::Box::new(Orientation::Vertical, 2);
    text_group.set_valign(gtk::Align::Center);
    let title_lbl = gtk::Label::new(Some(i18n::pick(
        "No track loaded",
        "Nenhuma faixa carregada",
    )));
    title_lbl.add_css_class("bottom-track-title");
    title_lbl.set_xalign(0.0);
    title_lbl.set_ellipsize(gtk::pango::EllipsizeMode::End);
    title_lbl.set_max_width_chars(24);

    let artist_lbl = gtk::Label::new(Some("VinylPlayer"));
    artist_lbl.add_css_class("bottom-artist-title");
    artist_lbl.set_xalign(0.0);
    artist_lbl.set_ellipsize(gtk::pango::EllipsizeMode::End);
    artist_lbl.set_max_width_chars(24);

    text_group.append(&title_lbl);
    text_group.append(&artist_lbl);

    left_group.append(&cover_stack);
    left_group.append(&text_group);
    bar.append(&left_group);

    // Centro: Controles de Transporte e Linha de Tempo
    let center_group = gtk::Box::new(Orientation::Vertical, 6);
    center_group.set_hexpand(true);
    center_group.set_valign(gtk::Align::Center);

    // Botões de reprodução
    let controls = gtk::Box::new(Orientation::Horizontal, 14);
    controls.set_halign(gtk::Align::Center);

    let shuffle_btn = gtk::Button::from_icon_name("media-playlist-shuffle-symbolic");
    shuffle_btn.add_css_class("control-button");
    shuffle_btn.add_css_class("shuffle-button");

    let prev = gtk::Button::from_icon_name("media-skip-backward-symbolic");
    i18n::bind_tooltip(&prev, |l| {
        i18n::pick_for(l, "Previous track · ←", "Faixa anterior · ←")
    });
    prev.add_css_class("control-button");
    let toggle = gtk::Button::from_icon_name("media-playback-start-symbolic");
    i18n::bind_tooltip(&toggle, |l| {
        i18n::pick_for(l, "Play / pause · Space", "Reproduzir / pausar · Espaço")
    });
    toggle.add_css_class("play-button");
    let next = gtk::Button::from_icon_name("media-skip-forward-symbolic");
    i18n::bind_tooltip(&next, |l| {
        i18n::pick_for(l, "Next track · →", "Próxima faixa · →")
    });
    next.add_css_class("control-button");

    let repeat_btn = gtk::Button::from_icon_name("media-playlist-repeat-symbolic");
    repeat_btn.add_css_class("control-button");
    repeat_btn.add_css_class("repeat-button");

    update_shuffle_btn(&shuffle_btn, audio.shuffle());
    update_repeat_btn(&repeat_btn, audio.repeat_mode());

    controls.append(&shuffle_btn);
    controls.append(&prev);
    controls.append(&toggle);
    controls.append(&next);
    controls.append(&repeat_btn);
    center_group.append(&controls);

    let shuffle_audio = audio.clone();
    let shuffle_state = state_mgr.clone();
    let shuffle_ui = shuffle_btn.clone();
    shuffle_btn.connect_clicked(move |_| {
        let new_val = !shuffle_audio.shuffle();
        shuffle_audio.set_shuffle(new_val);
        shuffle_state.update_shuffle(new_val);
        update_shuffle_btn(&shuffle_ui, new_val);
    });

    let repeat_audio = audio.clone();
    let repeat_state = state_mgr.clone();
    let repeat_ui = repeat_btn.clone();
    repeat_btn.connect_clicked(move |_| {
        let new_mode = repeat_audio.repeat_mode().next_cycle();
        repeat_audio.set_repeat_mode(new_mode);
        repeat_state.update_repeat_mode(new_mode);
        update_repeat_btn(&repeat_ui, new_mode);
    });

    let p = playing.clone();
    let engine_toggle = audio.clone();
    toggle.connect_clicked(move |b| {
        let v = !p.get();
        let _ = if v {
            engine_toggle.play()
        } else {
            engine_toggle.pause()
        };
        p.set(v);
        b.set_icon_name(if v {
            "media-playback-pause-symbolic"
        } else {
            "media-playback-start-symbolic"
        });
    });

    let prev_audio = audio.clone();
    prev.connect_clicked(move |_| {
        let _ = prev_audio.prev();
    });

    let next_audio = audio.clone();
    next.connect_clicked(move |_| {
        let _ = next_audio.next();
    });

    // Barra de progresso (Seekbar) com tempos
    let timeline = gtk::Box::new(Orientation::Horizontal, 10);
    let elapsed = gtk::Label::new(Some("00:00"));
    elapsed.add_css_class("time-label");

    let seek = gtk::Scale::with_range(Orientation::Horizontal, 0.0, 1.0, 0.001);
    seek.set_hexpand(true);
    seek.set_draw_value(false);
    seek.add_css_class("timeline-scale");

    let total = gtk::Label::new(Some("00:00"));
    total.add_css_class("time-label");

    let is_seeking = Rc::new(Cell::new(false));

    let drag = gtk::GestureDrag::new();
    drag.set_propagation_phase(gtk::PropagationPhase::Capture);
    {
        let is_seeking_begin = is_seeking.clone();
        drag.connect_drag_begin(move |_, _, _| {
            is_seeking_begin.set(true);
        });
    }
    {
        let is_seeking_end = is_seeking.clone();
        let seek_audio = audio.clone();
        let seek_scale = seek.downgrade();
        drag.connect_drag_end(move |_, _, _| {
            if let Some(scale) = seek_scale.upgrade() {
                let upper = scale.adjustment().upper().max(0.0);
                let val = scale.value().clamp(0.0, upper);
                let _ = seek_audio.seek_us(val as i64);
            }
            is_seeking_end.set(false);
        });
    }
    seek.add_controller(drag);

    {
        let is_seeking_update = is_seeking.clone();
        let seek_elapsed = elapsed.downgrade();
        seek.connect_value_changed(move |scale| {
            if is_seeking_update.get() {
                if let Some(lbl) = seek_elapsed.upgrade() {
                    lbl.set_text(&format_time(scale.value() as i64));
                }
            }
        });
    }

    let seek_audio = audio.clone();
    let change_is_seeking = is_seeking.clone();
    seek.connect_change_value(move |scale, _, value| {
        let upper = scale.adjustment().upper().max(0.0);
        let clamped = value.clamp(0.0, upper);
        if !change_is_seeking.get() {
            let _ = seek_audio.seek_us(clamped as i64);
        }
        glib::Propagation::Proceed
    });

    timeline.append(&elapsed);
    timeline.append(&seek);
    timeline.append(&total);
    center_group.append(&timeline);

    bar.append(&center_group);

    // Lado Direito: Controle de Volume, Mudo (M), Badge de Qualidade e Saída
    let right_group = gtk::Box::new(Orientation::Horizontal, 10);
    right_group.set_width_request(210);
    right_group.set_halign(gtk::Align::End);
    right_group.set_valign(gtk::Align::Center);

    let mute_btn = gtk::Button::new();
    mute_btn.set_widget_name("mute-button");
    mute_btn.add_css_class("control-button");
    mute_btn.add_css_class("mute-button");
    let vol_icon = gtk::Image::from_icon_name("audio-volume-high-symbolic");
    vol_icon.add_css_class("vol-icon");
    mute_btn.set_child(Some(&vol_icon));

    let vol = gtk::Scale::with_range(Orientation::Horizontal, 0.0, 1.0, 0.01);
    let initial_vol = state_mgr.snapshot().volume;
    let pre_mute_vol = Rc::new(Cell::new(if initial_vol > 0.01 {
        initial_vol
    } else {
        0.75
    }));
    vol.set_value(initial_vol);
    vol.set_width_request(100);
    vol.set_draw_value(false);
    i18n::bind_tooltip(&vol, |l| i18n::pick_for(l, "Volume", "Volume"));
    vol.add_css_class("volume-scale");
    update_volume_icon(&mute_btn, &vol_icon, initial_vol, i18n::current());

    let engine_vol = audio.clone();
    let state_vol = state_mgr.clone();
    let hud_vol = volume_hud.clone();
    let pre_mute_scale = pre_mute_vol.clone();
    let mute_scale_btn = mute_btn.downgrade();
    let vol_icon_scale = vol_icon.downgrade();
    vol.connect_value_changed(move |s| {
        let val = s.value();
        if val > 0.01 {
            pre_mute_scale.set(val);
        }
        engine_vol.set_volume(val);
        state_vol.update_volume(val);
        hud_vol.borrow_mut().trigger(val);
        if let (Some(btn), Some(icon)) = (mute_scale_btn.upgrade(), vol_icon_scale.upgrade()) {
            update_volume_icon(&btn, &icon, val, i18n::current());
        }
    });

    {
        let mute_audio = audio.clone();
        let mute_state = state_mgr.clone();
        let mute_hud = volume_hud.clone();
        let mute_pre = pre_mute_vol.clone();
        let mute_scale = vol.downgrade();
        let mute_icon = vol_icon.downgrade();
        mute_btn.connect_clicked(move |btn| {
            let next = next_mute_toggle(mute_audio.volume(), &mute_pre);
            mute_audio.set_volume(next);
            mute_state.update_volume(next);
            mute_hud.borrow_mut().trigger(next);
            if let Some(icon) = mute_icon.upgrade() {
                update_volume_icon(btn, &icon, next, i18n::current());
            }
            if let Some(scale) = mute_scale.upgrade() {
                if (scale.value() - next).abs() > 0.001 {
                    scale.set_value(next);
                }
            }
        });
    }

    let quality_badge = gtk::Label::new(None);
    quality_badge.add_css_class("quality-badge");
    quality_badge.set_visible(false);

    let output_badge = gtk::Label::new(Some("LOCAL"));
    output_badge.add_css_class("output-chip");

    let vol_scroll = gtk::EventControllerScroll::new(gtk::EventControllerScrollFlags::VERTICAL);
    let vol_scroll_audio = audio.clone();
    let vol_scroll_state = state_mgr.clone();
    let vol_scroll_hud = volume_hud.clone();
    let vol_scroll_scale = vol.downgrade();
    vol_scroll.connect_scroll(move |_, _dx, dy| {
        let step = if dy.abs() > 0.01 {
            (-dy.signum() * 0.03).clamp(-0.10, 0.10)
        } else {
            0.0
        };
        if step.abs() > 0.0001 {
            let cur = vol_scroll_audio.volume();
            let new_vol = (cur + step).clamp(0.0, 1.0);
            vol_scroll_audio.set_volume(new_vol);
            vol_scroll_state.update_volume(new_vol);
            vol_scroll_hud.borrow_mut().trigger(new_vol);
            if let Some(s) = vol_scroll_scale.upgrade() {
                s.set_value(new_vol);
            }
        }
        glib::Propagation::Stop
    });
    right_group.add_controller(vol_scroll);

    right_group.append(&mute_btn);
    right_group.append(&vol);
    right_group.append(&quality_badge);
    right_group.append(&output_badge);
    bar.append(&right_group);

    // Atualização em tempo real da barra de transporte
    let tick_seek = seek.clone();
    let tick_is_seeking = is_seeking.clone();
    let tick_elapsed = elapsed.clone();
    let tick_total = total.clone();
    let tick_audio = audio.clone();
    let tick_vol = vol.clone();
    let tick_mute_btn = mute_btn.clone();
    let tick_vol_icon = vol_icon.clone();
    let tick_title = title_lbl.clone();
    let tick_artist = artist_lbl.clone();
    let tick_cover_stack = cover_stack.clone();
    let tick_mini_picture = mini_picture.clone();
    let tick_last_cover_key = last_cover_key.clone();
    let tick_toggle = toggle.clone();
    let tick_playing = playing.clone();
    let tick_badge = quality_badge.clone();
    let tick_state = state_mgr.clone();
    let tick_shuffle_btn = shuffle_btn.clone();
    let tick_repeat_btn = repeat_btn.clone();
    let last_shuffle = Cell::new(audio.shuffle());
    let last_repeat = Cell::new(audio.repeat_mode());
    let last_bar_ver = Rc::new(Cell::new(0_u64));
    {
        let lbv_i18n = last_bar_ver.clone();
        let sh_weak = shuffle_btn.downgrade();
        let rp_weak = repeat_btn.downgrade();
        let mb_weak = mute_btn.downgrade();
        let vi_weak = vol_icon.downgrade();
        let a_i18n = audio.clone();
        i18n::connect(&bar, move |lang| {
            lbv_i18n.set(0);
            if let Some(sh) = sh_weak.upgrade() {
                update_shuffle_btn(&sh, a_i18n.shuffle());
            }
            if let Some(rp) = rp_weak.upgrade() {
                update_repeat_btn(&rp, a_i18n.repeat_mode());
            }
            if let (Some(mb), Some(vi)) = (mb_weak.upgrade(), vi_weak.upgrade()) {
                update_volume_icon(&mb, &vi, a_i18n.volume(), lang);
            }
        });
    }
    let last_queue = Cell::new(None::<(u64, Option<usize>)>);
    let queue = queue.clone();
    let weak_bar = bar.downgrade();
    let errors = errors.clone();

    #[derive(Default)]
    struct ActiveScrobble {
        uri: String,
        track_seq: u64,
        artist: String,
        title: String,
        album: String,
        duration_sec: u64,
        start_ts: u64,
        listened_ms: u64,
        last_play_instant: Option<std::time::Instant>,
        now_playing_sent: bool,
        scrobbled: bool,
    }
    let current_scrobble = Rc::new(RefCell::new(ActiveScrobble::default()));

    glib::timeout_add_local(std::time::Duration::from_millis(250), move || {
        if weak_bar.upgrade().is_none_or(|bar| !bar.is_realized()) {
            return glib::ControlFlow::Break;
        }
        queue.refresh();
        errors.sync();
        let (state, uri) = tick_audio.playback_identity();
        now_playing::sync(&uri, state);
        let (revision, current, _) = tick_audio.queue_status();
        if last_queue.get().is_none_or(|(old, _)| old != revision) {
            let snapshot = tick_audio.queue_snapshot();
            last_queue.set(Some((snapshot.revision, snapshot.current)));
            tick_state.update_queue(snapshot);
        } else if last_queue.get() != Some((revision, current)) {
            tick_state.update_queue_selection(current);
            last_queue.set(Some((revision, current)));
        }
        let cur_shuffle = tick_audio.shuffle();
        if cur_shuffle != last_shuffle.get() {
            last_shuffle.set(cur_shuffle);
            tick_state.update_shuffle(cur_shuffle);
            update_shuffle_btn(&tick_shuffle_btn, cur_shuffle);
        }
        let cur_repeat = tick_audio.repeat_mode();
        if cur_repeat != last_repeat.get() {
            last_repeat.set(cur_repeat);
            tick_state.update_repeat_mode(cur_repeat);
            update_repeat_btn(&tick_repeat_btn, cur_repeat);
        }
        tick_state.flush_if_dirty();
        let cur_vol = tick_audio.volume();
        if (cur_vol - tick_vol.value()).abs() >= 0.015 {
            tick_vol.set_value(cur_vol);
            update_volume_icon(&tick_mute_btn, &tick_vol_icon, cur_vol, i18n::current());
        }
        let is_playing = state == PlaybackState::Playing;
        tick_playing.set(is_playing);
        tick_toggle.set_icon_name(if is_playing {
            "media-playback-pause-symbolic"
        } else {
            "media-playback-start-symbolic"
        });

        let current_ver = tick_audio.metadata_version();
        if current_ver != last_bar_ver.get() {
            last_bar_ver.set(current_ver);
            let m = tick_audio.metadata();
            if !m.title.is_empty() {
                tick_title.set_text(&m.title);
                let art = if !m.artist.is_empty() {
                    m.artist
                } else {
                    i18n::pick("Local Album", "Álbum Local").to_string()
                };
                tick_artist.set_text(&art);
            } else {
                tick_title.set_text(i18n::pick("No track loaded", "Nenhuma faixa carregada"));
                tick_artist.set_text("VinylPlayer");
            }

            // Atualização dinâmica da mini capa do álbum no player bar
            let cover_key = if let Some(ref p) = m.cover_path {
                format!("path:{}", p.display())
            } else if let Some(ref b) = m.cover_bytes {
                format!("bytes:{}", b.len())
            } else {
                "none".to_string()
            };

            let mut lck = tick_last_cover_key.borrow_mut();
            if *lck != cover_key {
                *lck = cover_key.clone();
                drop(lck);

                let cover_path_bg = m.cover_path.clone();
                let cover_bytes_bg = m.cover_bytes.clone();
                if cover_path_bg.is_none() && cover_bytes_bg.is_none() {
                    tick_mini_picture.set_paintable(None::<&gdk::Texture>);
                    tick_cover_stack.set_visible_child_name("placeholder");
                } else {
                    let pic_weak = tick_mini_picture.downgrade();
                    let stack_weak = tick_cover_stack.downgrade();
                    let key_ref = tick_last_cover_key.clone();
                    let expected_key = cover_key;
                    glib::MainContext::default().spawn_local(async move {
                        let decoded = gtk::gio::spawn_blocking(move || {
                            let pb = if let Some(ref p) = cover_path_bg {
                                thumb_cache::load_cached_thumbnail(p, 128)
                            } else if let Some(ref b) = cover_bytes_bg {
                                let loader = gdk_pixbuf::PixbufLoader::new();
                                if loader.write(b).is_ok() && loader.close().is_ok() {
                                    loader.pixbuf()
                                } else {
                                    None
                                }
                            } else {
                                None
                            }?;
                            Some((
                                pb.width(),
                                pb.height(),
                                pb.rowstride() as usize,
                                pb.has_alpha(),
                                pb.read_pixel_bytes(),
                            ))
                        })
                        .await
                        .ok()
                        .flatten();

                        if *key_ref.borrow() != expected_key {
                            return;
                        }
                        if let (Some(pic), Some(st)) = (pic_weak.upgrade(), stack_weak.upgrade()) {
                            if let Some((w, h, stride, alpha, bytes)) = decoded {
                                let fmt = if alpha {
                                    gdk::MemoryFormat::R8g8b8a8
                                } else {
                                    gdk::MemoryFormat::R8g8b8
                                };
                                let tex = gdk::MemoryTexture::new(w, h, fmt, &bytes, stride);
                                pic.set_paintable(Some(&tex));
                                st.set_visible_child_name("cover");
                            } else {
                                pic.set_paintable(None::<&gdk::Texture>);
                                st.set_visible_child_name("placeholder");
                            }
                        }
                    });
                }
            }

            let is_hires = m.bit_depth.map(|d| d >= 24).unwrap_or(false)
                || m.sample_rate.map(|r| r >= 88_200).unwrap_or(false);
            let codec_upper = if !m.codec.is_empty() {
                m.codec.to_uppercase()
            } else if let Some(ext) = m.uri.rsplit('.').next() {
                let clean = ext.split('?').next().unwrap_or(ext).to_uppercase();
                if ["FLAC", "MP3", "WAV", "OGG", "M4A", "AAC", "OPUS", "ALAC"]
                    .contains(&clean.as_str())
                {
                    clean
                } else {
                    String::new()
                }
            } else {
                String::new()
            };

            tick_badge.set_tooltip_text(if m.codec.is_empty() {
                None
            } else {
                Some(&m.codec)
            });
            let codec_upper = if codec_upper.is_empty() {
                String::new()
            } else {
                [
                    "FLAC", "ALAC", "OPUS", "VORBIS", "AAC", "MP3", "WAV", "OGG", "M4A", "PCM",
                ]
                .into_iter()
                .find(|name| codec_upper.contains(name))
                .unwrap_or(if codec_upper.contains("MPEG") {
                    "MP3"
                } else {
                    "ÁUDIO"
                })
                .to_owned()
            };
            if is_hires {
                tick_badge.set_text("HI-RES");
                tick_badge.set_visible(true);
                tick_badge.set_css_classes(&["quality-badge", "badge-hires"]);
            } else if codec_upper == "FLAC" || codec_upper == "WAV" || codec_upper == "ALAC" {
                tick_badge.set_text("LOSSLESS");
                tick_badge.set_visible(true);
                tick_badge.set_css_classes(&["quality-badge", "badge-hifi"]);
            } else if !codec_upper.is_empty() {
                tick_badge.set_text(&codec_upper);
                tick_badge.set_visible(true);
                tick_badge.set_css_classes(&["quality-badge", "badge-standard"]);
            } else {
                tick_badge.set_visible(false);
            }
        }

        let d = tick_audio.duration_us();
        let p = tick_audio.position_us();
        if d > 0 {
            tick_seek.set_sensitive(true);
            tick_seek.set_range(0.0, d as f64);
            if !tick_is_seeking.get() {
                tick_seek.set_value(p as f64);
            }
        } else {
            tick_seek.set_sensitive(false);
            tick_seek.set_range(0.0, 1.0);
            if !tick_is_seeking.get() {
                tick_seek.set_value(0.0);
            }
        }
        if !tick_is_seeking.get() {
            tick_elapsed.set_text(&format_time(p));
        }
        tick_total.set_text(&format_time(d));

        // Integração Last.fm: updateNowPlaying e Scrobble automático com tempo real ouvido
        let lfm_snap = tick_state.snapshot();
        if lfm_snap.lastfm_enabled && lfm_snap.is_lastfm_authenticated() {
            let is_playing = state == PlaybackState::Playing;
            let cur_seq = tick_audio.track_sequence();
            if is_playing && !uri.is_empty() {
                let mut cs = current_scrobble.borrow_mut();
                let now_inst = std::time::Instant::now();
                if cs.uri != uri || cs.track_seq != cur_seq {
                    let m = tick_audio.metadata();
                    let now_ts = std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .unwrap_or_default()
                        .as_secs();
                    let dur = (d.max(0) / 1_000_000) as u64;
                    *cs = ActiveScrobble {
                        uri: uri.clone(),
                        track_seq: cur_seq,
                        artist: m.artist,
                        title: m.title,
                        album: m.album,
                        duration_sec: dur,
                        start_ts: now_ts,
                        listened_ms: 0,
                        last_play_instant: Some(now_inst),
                        now_playing_sent: false,
                        scrobbled: false,
                    };
                } else if let Some(prev_inst) = cs.last_play_instant.replace(now_inst) {
                    let delta_ms =
                        (now_inst.duration_since(prev_inst).as_millis() as u64).min(1000);
                    cs.listened_ms = cs.listened_ms.saturating_add(delta_ms);
                }

                if cs.artist.is_empty() || cs.title.is_empty() {
                    let m = tick_audio.metadata();
                    if !m.artist.is_empty() {
                        cs.artist = m.artist;
                    }
                    if !m.title.is_empty() {
                        cs.title = m.title;
                    }
                    if !m.album.is_empty() {
                        cs.album = m.album;
                    }
                }
                if cs.duration_sec == 0 && d > 0 {
                    cs.duration_sec = (d / 1_000_000) as u64;
                }

                // 1. Envia updateNowPlaying
                if !cs.now_playing_sent && !cs.title.is_empty() && !cs.artist.is_empty() {
                    cs.now_playing_sent = true;
                    let api_key = lfm_snap.lastfm_api_key().to_string();
                    let api_secret = lfm_snap.lastfm_api_secret().to_string();
                    let sk = lfm_snap.lastfm_session_key.clone().unwrap_or_default();
                    let artist = cs.artist.clone();
                    let track = cs.title.clone();
                    let album = if cs.album.is_empty() {
                        None
                    } else {
                        Some(cs.album.clone())
                    };
                    let dur = cs.duration_sec;

                    std::thread::Builder::new()
                        .name("lastfm-now-playing".into())
                        .spawn(move || {
                            if let Err(e) = crate::scrobbler::update_now_playing(
                                &api_key,
                                &api_secret,
                                &sk,
                                &artist,
                                &track,
                                album.as_deref(),
                                Some(dur),
                            ) {
                                eprintln!("[Last.fm] updateNowPlaying error: {e}");
                            }
                        })
                        .ok();
                }

                // 2. Scrobble: faixa >= 30s e tempo REAL escutado >= min(duration / 2, 240s)
                let listened_sec = cs.listened_ms / 1000;
                let dur_sec = cs.duration_sec;
                let min_play = (dur_sec / 2).min(240);

                if !cs.scrobbled
                    && dur_sec >= 30
                    && listened_sec >= min_play
                    && !cs.title.is_empty()
                    && !cs.artist.is_empty()
                {
                    cs.scrobbled = true;
                    let api_key = lfm_snap.lastfm_api_key().to_string();
                    let api_secret = lfm_snap.lastfm_api_secret().to_string();
                    let sk = lfm_snap.lastfm_session_key.clone().unwrap_or_default();
                    let artist = cs.artist.clone();
                    let track = cs.title.clone();
                    let album = if cs.album.is_empty() {
                        None
                    } else {
                        Some(cs.album.clone())
                    };
                    let ts = cs.start_ts;
                    let dur = cs.duration_sec;

                    std::thread::Builder::new()
                        .name("lastfm-scrobbler".into())
                        .spawn(move || {
                            let payload = crate::scrobbler::ScrobblePayload {
                                artist: &artist,
                                track: &track,
                                album: album.as_deref(),
                                timestamp: ts,
                                duration_sec: Some(dur),
                            };
                            if let Err(e) = crate::scrobbler::scrobble_track(
                                &api_key,
                                &api_secret,
                                &sk,
                                &payload,
                            ) {
                                eprintln!("[Last.fm] Scrobble error: {e}");
                            }
                        })
                        .ok();
                }
            } else {
                let mut cs = current_scrobble.borrow_mut();
                if state == PlaybackState::Stopped {
                    if !cs.uri.is_empty() {
                        *cs = ActiveScrobble::default();
                    }
                } else {
                    // Em pausa: para de contar tempo escutado até retomar Playing
                    cs.last_play_instant = None;
                }
            }
        }

        glib::ControlFlow::Continue
    });

    (bar.upcast(), mute_btn, shuffle_btn, repeat_btn)
}

fn format_time(us: i64) -> String {
    let s = (us.max(0) / 1_000_000) as u64;
    format!("{:02}:{:02}", s / 60, s % 60)
}

fn install_css() {
    BASE_CSS_PROVIDER.with(|cell| {
        if cell.borrow().is_some() {
            return;
        }
        let Some(display) = gdk::Display::default() else {
            return;
        };
        let css = gtk::CssProvider::new();
        css.load_from_data(BASE_CSS);
        gtk::style_context_add_provider_for_display(
            &display,
            &css,
            gtk::STYLE_PROVIDER_PRIORITY_USER + 1,
        );
        *cell.borrow_mut() = Some(css);
    });
}

const BASE_CSS: &str = r#"
        * { color: #e7e5df; font-family: 'Inter', 'Cantarell', sans-serif; }
        .app-shell { background: #141516; }
        .enqueue-menu > button { background: transparent; border: 1px solid transparent; border-radius: 8px; padding: 8px; }
        .enqueue-menu > button:hover { background: #302b24; border-color: #494033; }
        .enqueue-menu label { font-size: 12px; }
        .enqueue-menu popover contents { background: #1b1d1f; border: 1px solid #3a3e43; border-radius: 9px; padding: 8px; }
        .playback-error { background: #1b1c1e; }
        .playback-error label { font-size: 13px; }
        .playback-error .heading { font-size: 18px; font-weight: 600; }
        .playback-error button { border-radius: 8px; padding: 12px 14px; }
        .playback-error button label { font-size: 12px; color: inherit; }
        .playback-error button.suggested-action { background: #dfc08d; color: #252016; }
        .library-status { color: #a0a5ac; font-size: 10px; padding: 7px 28px; background: #1b1d1f; }
        .folders-list { background: transparent; }
        .folders-list row { padding: 5px 8px; border-bottom: 1px solid #303337; }
        .library-settings { background: #141516; }
        .library-settings label, .library-settings button { font-size: 12px; }
        .library-settings .page-title { font-size: 21px; }
        .library-settings .bottom-artist-title { font-size: 11px; }
        .library-settings .folder-name { font-size: 13px; font-weight: 600; color: #e8e3d9; }
        .library-settings .folder-path { font-size: 10px; color: #92999f; }
        .folder-button { background: #232629; border: 1px solid #3a3e43; border-radius: 8px; padding: 8px 12px; }
        .folder-button:hover { background: #303438; border-color: #9e825c; }
        .library-settings .queue-error { margin: 0; font-size: 11px; }
        .donate-dialog { background: #141516; }
        .donate-card {
            background: #1b1d1f; border: 1px solid #303337;
            border-radius: 12px; padding: 14px 16px; margin-bottom: 4px;
        }
        .donate-card-title { font-size: 14px; font-weight: 700; color: #f0ede6; }
        .donate-card-sub { font-size: 10px; color: #8a8d93; font-family: monospace; }
        .donate-desc { font-size: 11px; color: #a0a4ab; line-height: 1.4; }
        .donate-goal-num { font-size: 15px; font-weight: 800; color: #dfc08d; }
        .donate-progress trough { background: #282a2e; border-radius: 999px; min-height: 6px; }
        .donate-progress progress { background: #dfc08d; border-radius: 999px; }
        .donate-badge-bep20 {
            background: rgba(243, 186, 47, 0.14); border: 1px solid rgba(243, 186, 47, 0.35);
            color: #f3ba2f; border-radius: 999px; padding: 3px 9px; font-size: 9px; font-weight: 700;
        }
        .donate-wallet-box {
            background: #131416; border: 1px solid #282a2d;
            border-radius: 10px; padding: 12px;
        }
        .donate-coin-title { font-size: 12px; font-weight: 700; color: #f0ede6; }
        .donate-addr-entry {
            background: #1a1b1d; border: 1px solid #32353a; border-radius: 8px;
            font-family: monospace; font-size: 11px; color: #dfc08d; padding: 4px 8px;
        }
        .donate-copy-btn {
            background: rgba(223, 192, 141, 0.16); border: 1px solid rgba(223, 192, 141, 0.38);
            color: #dfc08d; border-radius: 8px; padding: 6px 14px; font-weight: 700; font-size: 11px;
        }
        .donate-copy-btn:hover { background: rgba(223, 192, 141, 0.28); color: #f4d8aa; }
        .donate-qr-frame {
            border: 2px solid #dfc08d; border-radius: 8px; background: white; padding: 6px;
        }
        .donate-hint { font-size: 10px; color: #82868d; font-family: monospace; }
        .donate-github-btn {
            background: #131416; border: 1px solid #282a2d; border-radius: 10px;
            padding: 10px 12px; transition: border-color 140ms ease;
        }
        .donate-github-btn:hover { border-color: #dfc08d; background: #1a1c1e; }
        .donate-supporter-pill {
            background: rgba(223, 192, 141, 0.16); border: 1px solid rgba(223, 192, 141, 0.38);
            color: #dfc08d; border-radius: 999px; padding: 5px 12px; font-size: 10px; font-weight: 700;
        }
        .donate-nav-button {
            color: #dfc08d;
        }
        .donate-nav-button:hover {
            background: rgba(223, 192, 141, 0.12); color: #f4d8aa;
        }
        .queue-toggle { background: #242629; border: 1px solid #393c40; border-radius: 999px; padding: 9px 13px; }
        .queue-toggle:checked { background: #302b24; border-color: #8b7352; }
        .queue-toggle label, .queue-toggle image { color: #dfc08d; }
        .queue-panel { background: #1b1d1f; border-left: 1px solid #383a3e; }
        .queue-heading { padding: 18px 14px 8px 20px; }
        .queue-close { background: transparent; border: 1px solid transparent; border-radius: 8px; padding: 8px; }
        .queue-close:hover { background: #303337; }
        .queue-close image { color: #a7adb4; }
        .queue-title { font-size: 16px; font-weight: 600; color: #f0ede6; }
        .queue-summary { color: #d2b889; font-size: 11px; margin: 0 20px 7px; }
        .queue-search { background: #232527; color: #ece8e1; border: 1px solid #393b3e; border-radius: 8px; min-height: 34px; box-shadow: none; }
        .queue-search:focus-within { border-color: #d2b889; }
        .queue-clear { background: #292a2b; color: #d2b889; border-radius: 8px; padding: 4px 9px; box-shadow: none; }
        .queue-hint { color: #8d939a; font-size: 10px; margin: 0 20px 16px; }
        .queue-list { background: transparent; padding: 0 8px 12px; }
        .queue-list row { background: transparent; padding: 0; }
        .queue-row { border-radius: 8px; border-top: 2px solid transparent; border-bottom: 2px solid transparent; padding: 5px 6px; }
        .queue-current { background: #302b24; }
        .queue-current .queue-track-title { color: #e5c591; }
        .queue-drop-before { border-top-color: #dfc08d; }
        .queue-drop-after { border-bottom-color: #dfc08d; }
        .queue-handle { color: #777e87; min-width: 18px; }
        .queue-track { background: transparent; border: none; padding: 9px 5px; }
        .queue-track:hover { background: #ffffff08; }
        .queue-track-title { font-size: 12px; font-weight: 600; color: #dddedc; }
        .queue-track-subtitle { font-size: 10px; color: #92999f; }
        .queue-action { background: transparent; border: none; padding: 2px 5px; min-width: 18px; min-height: 18px; color: #a7adb4; }
        .queue-action image { -gtk-icon-size: 12px; color: inherit; }
        .queue-action:hover { background: #373a3d; color: #f0ede6; }
        .queue-action:disabled { opacity: 0.22; }
        .queue-empty { color: #9ca1a7; margin: 30px 22px; font-size: 13px; }
        .queue-error { color: #f2b0a4; margin: 8px 20px; font-size: 11px; }
        .listening-eyebrow { color: #99978f; font-size: 10px; font-weight: 600; letter-spacing: 2px; }
        .listening-status { background: #212224; border: 1px solid #333538; border-radius: 999px; padding: 7px 11px; }
        .listening-status label, .listening-status image { color: #9fa2a5; font-size: 9px; font-weight: 600; letter-spacing: 0.7px; }
        .listening-status.is-playing { background: #302a20; border-color: #534630; }
        .listening-status.is-playing label, .listening-status.is-playing image { color: #dfc08d; }
        .rpm-selector { background: #1b1c1e; border: 1px solid #333538; border-radius: 999px; padding: 2px; }
        .rpm-button { background: transparent; border: 1px solid transparent; border-radius: 999px; padding: 4px 10px; font-size: 10px; font-weight: 600; color: #8b8e93; min-height: 0; min-width: 0; }
        .rpm-button:checked { background: #302a20; border-color: #534630; color: #dfc08d; }
        .rpm-button:hover:not(:checked) { background: #25272a; color: #d0d2d6; }
        .listening-information { border-top: 1px solid #2e3032; padding: 19px 2px 2px; }
        .listening-title { color: #f1ede4; font-size: 23px; font-weight: 600; letter-spacing: -0.5px; }
        .listening-artist { color: #a5a5a1; font-size: 12px; }
        .listening-format { color: #cbb18a; background: #26231e; border: 1px solid #42392d; border-radius: 6px; padding: 7px 10px; font-size: 10px; font-weight: 600; }
        button { box-shadow: none; text-shadow: none; }
        button:focus-visible { outline: 2px solid #d5b17b; outline-offset: 3px; }
        .sidebar {
            background: #191a1c; padding: 28px 14px 18px;
            border-right: 1px solid #2c2d30;
        }
        .brand-box { padding: 0 10px; margin-bottom: 32px; }
        .brand-icon { color: #d5b17b; }
        .brand { color: #f0ede6; font-size: 14px; font-weight: 800; letter-spacing: 1.5px; }
        .brand-sub { color: #8a8b8e; font-size: 9px; letter-spacing: 1.6px; margin-top: 8px; }
        .sidebar-section-title {
            color: #828489; font-size: 10px; font-weight: 600;
            letter-spacing: 1.5px; margin: 0 12px 12px;
        }
        .nav-button {
            background: transparent; border: 1px solid transparent; border-radius: 9px;
            color: #a3a5aa; font-size: 13px; font-weight: 500; padding: 12px;
            transition: background 140ms ease, color 140ms ease;
        }
        .nav-button box, .nav-button label, .nav-button image { color: inherit; }
        .nav-button:hover { background: #25272a; color: #f0ede6; }
        .nav-button.active-nav { background: #302b24; border-color: #494033; color: #e0bd86; }
        .nav-button:disabled { opacity: 0.4; }
        .system-audio-pill {
            background: #202123; border: 1px solid #303134; border-radius: 10px;
            padding: 14px 12px; margin-bottom: 14px;
        }
        .system-audio-title { color: #c3c3c0; font-size: 9px; font-weight: 700; letter-spacing: 1px; }
        .system-audio-sub { color: #909297; font-size: 10px; margin-top: 5px; }
        .topbar { padding: 22px 28px; border-bottom: 1px solid #292a2d; }
        .page-title { color: #f0ede6; font-size: 21px; font-weight: 600; letter-spacing: -0.4px; }
        .search-box {
            background: #202123; border: 1px solid #323438; border-radius: 9px;
            color: #bfc1c5; font-size: 12px; min-height: 38px;
        }
        .search-box:focus-within { border-color: #a88c63; }
        .open-button {
            background: #d5b17b; border: 1px solid #d5b17b; color: #201c16;
            border-radius: 9px; font-size: 12px; font-weight: 700; padding: 10px 14px;
        }
        .open-button box, .open-button label, .open-button image { color: inherit; }
        .open-button:hover { background: #e3c391; border-color: #e3c391; }
        .player-bar { background: #1b1c1e; border-top: 1px solid #353639; padding: 14px 24px; }
        .mini-cover-container {
            border-radius: 10px;
            border: 1px solid #494033; background: #22201d;
            box-shadow: 0 2px 6px rgba(0, 0, 0, 0.35);
        }
        .mini-cover-picture {
            border-radius: 9px;
        }
        .mini-disc-icon {
            color: #d5b17b; background: #292722;
            border-radius: 9px; padding: 11px;
        }
        .bottom-track-title { font-size: 13px; font-weight: 600; color: #f0ede6; }
        .bottom-artist-title { font-size: 11px; color: #989a9e; margin-top: 4px; }
        .control-button {
            background: transparent; color: #b3b5b9; border: none;
            border-radius: 999px; min-width: 36px; min-height: 36px; padding: 4px;
        }
        .control-button:hover { background: #2c2e31; color: #f4eee3; }
        .control-button.active {
            color: #e5a93b; background: rgba(229, 169, 59, 0.16);
        }
        .control-button.active:hover {
            color: #f7be58; background: rgba(229, 169, 59, 0.28);
        }
        .play-button {
            background: #dfc08d; color: #252016; border: none;
            border-radius: 999px; min-width: 44px; min-height: 44px; padding: 4px;
        }
        .play-button:hover { background: #edcf9f; }
        .play-button image, .control-button image { color: inherit; }
        .time-label { font-family: monospace; font-size: 10px; color: #92959a; min-width: 38px; }
        scale { padding: 6px 0; }
        scale trough { background: #383a3e; border-radius: 999px; min-height: 3px; }
        scale highlight { background: #d5b17b; border-radius: 999px; }
        scale slider {
            background: #eee4d3; border: none; border-radius: 999px;
            min-width: 10px; min-height: 10px; margin: -4px 0; box-shadow: none;
        }
        scale:hover slider { background: #ffffff; }
        .vol-icon { color: #a0a3a9; }
        .output-chip {
            color: #92959a; font-size: 9px; font-weight: 600; letter-spacing: 1px; padding: 4px;
        }
        .quality-badge {
            font-size: 9px; font-weight: 700; letter-spacing: 0.5px; padding: 5px 7px; border-radius: 5px;
        }
        .badge-hires { background: #393123; color: #e1bd7f; }
        .badge-hifi { background: #25352f; color: #9ccbb6; }
        .badge-standard { background: #303237; color: #babdc3; }
        .library-list { background: transparent; padding-top: 2px; }
        .version-badge {
            background: #302b24; color: #dfc08d; font-size: 10px; font-weight: 700;
            letter-spacing: 0.5px; padding: 2px 7px; border-radius: 6px; border: 1px solid #4a3f31;
        }
        .sort-dropdown {
            background: #202123; border: 1px solid #34363a; border-radius: 9px;
            color: #d9dadc; font-size: 12px; font-weight: 500;
        }
        .sort-dropdown:hover { border-color: #a88c63; }
        .sort-dropdown button { background: transparent; border: none; color: inherit; padding: 6px 12px; }
        .album-view-switcher { background: #202123; border-radius: 9px; padding: 3px; }
        .album-view-switcher button {
            background: transparent; color: #a3a5aa; border: none;
            border-radius: 7px; padding: 8px 14px; font-size: 12px;
        }
        .album-view-switcher button:checked { background: #393125; color: #e0bd86; }
        .album-view-switcher label { color: inherit; }
        .album-cover-grid { padding: 12px 0 20px; }
        .album-cover-grid flowboxchild { padding: 0; background: transparent; }
        .album-cover-button {
            background: #222427; border: 1px solid #35383d;
            border-radius: 12px; padding: 0; }
        .album-cover-button:hover { border-color: #d5b17b; }
        .album-cover-placeholder { color: #686b71; }
        .album-detail-cover { background: #222427; border: 1px solid #35383d; border-radius: 10px; }
        .album-card-row {
            background: #202123; border: 1px solid #34363a; border-radius: 10px; margin: 16px 0 6px;
        }
        .album-title-header { color: #e7e5df; font-size: 14px; font-weight: 600; }
        .play-album-button {
            background: #302b24; color: #dfc08d; border: 1px solid #494033;
            border-radius: 7px; padding: 7px 12px; font-size: 11px; font-weight: 600;
        }
        .play-album-button box, .play-album-button label, .play-album-button image { color: inherit; }
        .play-album-button:hover { background: #443a2c; }
        .track-row-item { background: transparent; border-bottom: 1px solid #25272a; }
        .track-item-button { background: transparent; border: none; border-radius: 7px; padding: 13px 14px; }
        .track-item-button:hover { background: #222427; }
        .track-icon { color: #81858c; }
        .track-row-name { color: #d9dadc; font-size: 12px; font-weight: 500; }
        .track-item-button.playback-current { background: #302b24; }
        .track-item-button.playback-current .track-row-name,
        .track-item-button.playback-current .track-icon { color: #e0bd86; }
        .track-item-button.playback-paused { background: #262729; }
        .track-item-button.playback-paused .track-row-name,
        .track-item-button.playback-paused .track-icon { color: #b9b8b3; }
        .album-cover-button.playback-current { border-color: #d5b17b; box-shadow: 0 0 0 2px #d5b17b; }
        .album-cover-button.playback-paused { border-color: #9b9d9f; box-shadow: 0 0 0 2px #9b9d9f; }
        .album-playback-indicator { background: #dfc08d; color: #252016; border-radius: 999px; padding: 9px; }
        .playback-paused .album-playback-indicator { background: #c3c3bf; color: #252627; }
        .track-format-tag {
            background: #2b2d31; color: #a1a5ac; border-radius: 4px;
            padding: 4px 7px; font-size: 9px; font-weight: 600;
        }
        .playlist-card {
            background: #1d1e21; border: 1px solid #32353a; border-radius: 12px;
            padding: 16px; transition: background 150ms ease, border-color 150ms ease, box-shadow 150ms ease;
        }
        .playlist-card:hover {
            background: #232529; border-color: #5a4b35;
            box-shadow: 0 4px 16px rgba(0, 0, 0, 0.35);
        }
        .playlist-badge-icon {
            color: #dfc08d; background: #2a2620; border: 1px solid #4a3e2e;
            border-radius: 8px; padding: 8px;
        }
        .playlist-card-title {
            color: #f1ede4; font-size: 14px; font-weight: 600; letter-spacing: -0.2px;
        }
        .playlist-card-count {
            color: #92959a; font-size: 11px; font-weight: 500;
        }
        .playlist-del-btn {
            background: transparent; border: 1px solid transparent; border-radius: 8px;
            padding: 7px 10px; color: #92959a; transition: all 120ms ease;
        }
        .playlist-del-btn:hover {
            background: rgba(230, 80, 80, 0.15); border-color: rgba(230, 80, 80, 0.3);
            color: #ff7878;
        }
        .playlist-detail-badge {
            background: #25221b; border: 1px solid #483b28; border-radius: 14px;
            color: #dfc08d;
        }
        .playlist-track-action-btn {
            background: transparent; border: none; border-radius: 6px; padding: 6px;
            min-width: 28px; min-height: 28px; color: #8a8e95; transition: all 120ms ease;
        }
        .playlist-track-action-btn:hover {
            background: #2f3237; color: #f0ede6;
        }
        .playlist-track-action-btn:disabled {
            opacity: 0.25;
        }
        .playlist-track-delete-btn:hover {
            background: rgba(230, 80, 80, 0.16); color: #ff8585;
        }
        .popover-playlist-item {
            background: transparent; border: 1px solid transparent; border-radius: 6px;
            padding: 6px 10px; font-size: 11px; font-weight: 500; color: #d8d9dc;
            transition: all 120ms ease;
        }
        .popover-playlist-item:hover {
            background: #2d2922; border-color: #55442e; color: #dfc08d;
        }
        .mute-button {
            min-width: 30px; min-height: 30px; padding: 4px 6px;
        }
    "#;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mute_toggle_remembers_and_restores_previous_volume() {
        let pre_mute = Cell::new(0.8);
        assert_eq!(next_mute_toggle(0.62, &pre_mute), 0.0);
        assert!((pre_mute.get() - 0.62).abs() < 1e-6);
        assert!((next_mute_toggle(0.0, &pre_mute) - 0.62).abs() < 1e-6);

        let fallback_pre = Cell::new(0.0);
        assert!((next_mute_toggle(0.0, &fallback_pre) - 0.75).abs() < 1e-6);
    }
}

#[cfg(test)]
mod animation_tests;
