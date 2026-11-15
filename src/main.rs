#![allow(deprecated)]
use gtk::cairo;
use gtk::gdk;
use gtk::glib;
use gtk::prelude::*;
use gtk::{Application, ApplicationWindow, DrawingArea, Orientation};
use std::cell::{Cell, RefCell};
use std::f64::consts::{PI, TAU};
use std::path::Path;
use std::rc::Rc;
use std::sync::Arc;
use std::time::Instant;

#[derive(Clone, Debug)]
struct VolumeHudState {
    level: f64,
    alpha: f64,
    last_active: Option<Instant>,
}

impl Default for VolumeHudState {
    fn default() -> Self {
        Self {
            level: 0.5,
            alpha: 0.0,
            last_active: None,
        }
    }
}

impl VolumeHudState {
    fn trigger(&mut self, level: f64) {
        self.level = level.clamp(0.0, 1.0);
        self.alpha = 1.0;
        self.last_active = Some(Instant::now());
    }

    fn tick_fade(&mut self) -> bool {
        if self.alpha <= 0.0 {
            return false;
        }
        if let Some(active) = self.last_active {
            let elapsed = active.elapsed().as_secs_f64();
            if elapsed > 1.8 {
                self.alpha = 0.0;
                return true;
            } else if elapsed > 1.1 {
                self.alpha = ((1.8 - elapsed) / 0.7).clamp(0.0, 1.0);
                return true;
            }
        }
        true
    }
}

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
        "Alta Fidelidade Analógica".to_string()
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

struct CachedCover {
    key: String,
    original: Option<gdk_pixbuf::Pixbuf>,
    surface: Option<(i32, cairo::ImageSurface)>,
}

struct TurntableCache {
    bg_surface: Option<(i32, i32, cairo::ImageSurface)>,
    disc_surface: Option<(f64, i32, u32, cairo::ImageSurface)>,
    cover: CachedCover,
}

mod audio;
mod donate;
mod file_open;
mod library;
mod library_settings;
mod library_sources;
mod mpris;
mod now_playing;
mod playback_error;
mod playback_queue;
mod queue_view;
mod search;
mod state;
mod needle_drop;
mod playlist;
mod playlist_view;
use audio::{AudioEngine, PlaybackState, RepeatMode};
use gstreamer::prelude::ElementExt;

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
    glib::MainContext::default().spawn_local(async move {
        let _ = mpris::serve(audio_mpris).await;
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

    let title_label = gtk::Label::new(Some("Turntable"));
    title_label.add_css_class("page-title");
    title_label.set_hexpand(true);
    title_label.set_xalign(0.0);

    let stack = gtk::Stack::new();
    stack.set_widget_name("main-pages");
    stack.set_hexpand(true);
    stack.set_vexpand(true);

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
    let topbar = header(&audio, &search_hub, &stack, &nav_buttons, &title_label)
        .downcast::<gtk::Box>()
        .unwrap();
    let queue_toggle = gtk::ToggleButton::new();
    queue_toggle.add_css_class("queue-toggle");
    queue_toggle.set_tooltip_text(Some("Toggle queue · Ctrl+J"));
    let queue_label = gtk::Box::new(Orientation::Horizontal, 7);
    queue_label.append(&gtk::Image::from_icon_name("view-list-symbolic"));
    queue_label.append(&gtk::Label::new(Some("Queue")));
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
        &library_view(&audio, false, &search_hub, &state_mgr, Vec::new()),
        Some("tracks"),
    );
    stack.add_named(
        &library_view(&audio, true, &search_hub, &state_mgr, Vec::new()),
        Some("albums"),
    );
    let playlist_mgr = playlist::PlaylistManager::new(None);
    stack.add_named(
        &playlist_view::playlist_library_view(&audio, &playlist_mgr),
        Some("playlists"),
    );
    let catalog = library_settings::Catalog::new(state_mgr.clone());
    let library_status = gtk::Label::new(Some("Lendo pastas da biblioteca…"));
    library_status.set_xalign(0.0);
    library_status.add_css_class("library-status");
    content.append(&library_status);
    let weak_status = library_status.downgrade();
    let weak_stack = stack.downgrade();
    let library_audio = audio.clone();
    let library_search = search_hub.clone();
    let initialize_queue = Cell::new(initial_state.queue.is_none());
    let seed_revision = audio.queue_status().0;
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

    // Suporte a arrastar e soltar arquivos na janela
    let drop = gtk::DropTarget::new(glib::types::Type::STRING, gdk::DragAction::COPY);
    let drop_audio = audio.clone();
    drop.connect_drop(move |_, value, _, _| {
        let Ok(text) = value.get::<String>() else {
            return false;
        };
        let uri = text.lines().next().unwrap_or_default().trim();
        if !uri.starts_with("file://") {
            return false;
        }
        let path = glib::filename_from_uri(uri).ok().map(|(path, _)| path);
        if let Some(path) = path {
            let ext = path
                .extension()
                .and_then(|s| s.to_str())
                .unwrap_or("")
                .to_ascii_lowercase();
            if ext == "m3u" || ext == "m3u8" {
                if let Ok(pl) = playlist::Playlist::load_from_file(&path) {
                    if !pl.tracks.is_empty() {
                        let _ = drop_audio.play_album(&pl.tracks, 0);
                        return true;
                    }
                }
            }
            if drop_audio.load(path).is_ok() {
                let _ = drop_audio.play();
                return true;
            }
        }
        false
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
    for (button, page, title) in &nav_buttons {
        let stack = stack.clone();
        let title_label = title_label.clone();
        let all_btns = all_buttons.clone();
        let curr_btn = button.clone();
        let page_name = page.to_string();
        let title_text = *title;
        let state_nav = state_mgr.clone();
        button.connect_clicked(move |_| {
            stack.set_visible_child_name(&page_name);
            title_label.set_text(title_text);
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
    for (btn, page, title) in &nav_buttons {
        if page == saved_view {
            btn.add_css_class("active-nav");
            title_label.set_text(title);
        } else {
            btn.remove_css_class("active-nav");
        }
    }

    body.append(&nav);
    body.append(&content);
    root.append(&body);
    root.append(&player_bar(
        &playing,
        &audio,
        &state_mgr,
        &queue,
        &playback_errors,
        &volume_hud,
    ));

    let window = ApplicationWindow::builder()
        .application(app)
        .title("VinylPlayer — High-Fidelity Audio")
        .default_width(1220)
        .default_height(780)
        .child(&root)
        .build();
    let show_turntable = gtk::gio::SimpleAction::new("show-turntable", None);
    playback_errors.attach(&window);
    let overview = nav_buttons
        .iter()
        .find(|(_, page, _)| *page == "overview")
        .unwrap()
        .0
        .clone();
    show_turntable.connect_activate(move |_, _| overview.emit_clicked());
    window.add_action(&show_turntable);
    let toggle_queue = gtk::gio::SimpleAction::new("toggle-queue", None);
    let split = queue_split.clone();
    toggle_queue.connect_activate(move |_, _| split.set_show_sidebar(!split.shows_sidebar()));
    window.add_action(&toggle_queue);
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
                .heading("Could not save preferences")
                .body(format!("Changes remain pending. Check disk space and permissions in the configuration directory and try closing again.\n\n{error}"))
                .build();
            dialog.add_response("back", "Back to player");
            dialog.set_close_response("back");
            dialog.present();
            return glib::Propagation::Stop;
        }
        bus_watch.borrow_mut().take();
        glib::Propagation::Proceed
    });

    // Atalhos ergonômicos de teclado
    let key_controller = gtk::EventControllerKey::new();
    let key_audio = audio.clone();
    let key_playing = playing.clone();
    let key_state = state_mgr.clone();
    let key_hud = volume_hud.clone();
    key_controller.connect_key_pressed(move |_, keyval, _, _| match keyval {
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
            let _ = key_audio.next();
            glib::Propagation::Stop
        }
        gdk::Key::Left => {
            let _ = key_audio.prev();
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
        _ => glib::Propagation::Proceed,
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
    logo_row.append(&icon_disc);
    logo_row.append(&brand);
    brand_box.append(&logo_row);

    let sub = gtk::Label::new(Some("O RITUAL DE OUVIR"));
    sub.add_css_class("brand-sub");
    sub.set_xalign(0.0);
    brand_box.append(&sub);
    side.append(&brand_box);

    let sec_lbl = gtk::Label::new(Some("COLEÇÃO"));
    sec_lbl.add_css_class("sidebar-section-title");
    sec_lbl.set_xalign(0.0);
    side.append(&sec_lbl);

    let mut nav_buttons = Vec::new();
    let items = [
        ("media-optical-symbolic", "Turntable", "overview"),
        ("audio-x-generic-symbolic", "Tracks", "tracks"),
        ("view-grid-symbolic", "Albums", "albums"),
        ("view-list-symbolic", "Playlists", "playlists"),
    ];

    for (idx, (icon, text, page)) in items.iter().enumerate() {
        let b = labeled_icon_button(icon, text);
        b.add_css_class("nav-button");
        if idx == 0 {
            b.add_css_class("active-nav");
        }
        side.append(&b);
        nav_buttons.push((b, *page, *text));
    }

    let spacer = gtk::Box::new(Orientation::Vertical, 0);
    spacer.set_vexpand(true);
    side.append(&spacer);

    // Status do sistema de áudio
    let audio_pill = gtk::Box::new(Orientation::Vertical, 2);
    audio_pill.add_css_class("system-audio-pill");
    let pipe_lbl = gtk::Label::new(Some("LOCAL LIBRARY"));
    pipe_lbl.add_css_class("system-audio-title");
    pipe_lbl.set_xalign(0.0);
    let sample_lbl = gtk::Label::new(Some("Drag and drop a track to play"));
    sample_lbl.add_css_class("system-audio-sub");
    sample_lbl.set_xalign(0.0);
    audio_pill.append(&pipe_lbl);
    audio_pill.append(&sample_lbl);
    side.append(&audio_pill);

    let donate = labeled_icon_button("emblem-favorite-symbolic", "Donate");
    donate.add_css_class("nav-button");
    donate.add_css_class("donate-nav-button");
    donate.set_action_name(Some("win.donate"));
    donate.set_tooltip_text(Some("Support VinylPlayer (Ctrl+Shift+D)"));
    side.append(&donate);

    let settings = labeled_icon_button("preferences-system-symbolic", "Settings");
    settings.add_css_class("nav-button");
    settings.set_action_name(Some("win.library-settings"));
    settings.set_tooltip_text(Some("Choose library folders"));
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
    let heading = gtk::Label::new(Some("Tracks"));
    heading.add_css_class("page-title");
    heading.set_xalign(0.0);
    heading.set_hexpand(true);
    header_box.append(&heading);

    let sort_options = ["Title (A–Z)", "Title (Z–A)", "Path (Default)"];
    let sort_dropdown = gtk::DropDown::from_strings(&sort_options);
    let initial_track_sort = state_mgr.snapshot().track_sort_mode;
    sort_dropdown.set_selected(initial_track_sort as u32);
    sort_dropdown.add_css_class("sort-dropdown");
    sort_dropdown.set_tooltip_text(Some("Track sort criteria"));
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
    let empty_label = gtk::Label::new(Some(if paths.is_empty() {
        "Biblioteca vazia. Adicione uma pasta em Configurações."
    } else {
        "Nenhuma música corresponde à busca"
    }));
    empty_label.add_css_class("bottom-artist-title");
    empty_box.append(&empty_icon);
    empty_box.append(&empty_label);
    list.set_placeholder(Some(&empty_box));

    let track_paths: Rc<Vec<std::path::PathBuf>> = Rc::new(paths);
    let weak_list_fill = list.downgrade();
    let fill_paths = track_paths.clone();
    let fill_audio = audio.clone();
    glib::MainContext::default().spawn_local(async move {
        for (idx, path) in fill_paths.iter().enumerate() {
            let Some(list) = weak_list_fill.upgrade() else {
                break;
            };
            let row = track_row(&fill_audio, path.clone(), false);
            row.set_widget_name(&idx.to_string());
            list.append(&row);
            drop(list);
            if idx % 24 == 23 {
                glib::timeout_future_with_priority(
                    glib::Priority::LOW,
                    std::time::Duration::from_millis(8),
                )
                .await;
            }
        }
    });

    let list_filter_weak = list.downgrade();
    search_hub.on_query_changed(move || {
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

    let scroll = gtk::ScrolledWindow::new();
    scroll.set_policy(gtk::PolicyType::Never, gtk::PolicyType::Automatic);
    scroll.set_vexpand(true);
    scroll.set_child(Some(&list));
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
    let heading = gtk::Label::new(Some("Albums"));
    heading.set_xalign(0.0);
    heading.set_hexpand(true);
    heading.add_css_class("page-title");
    toolbar.append(&heading);

    let sort_options = [
        "Album (A–Z)",
        "Album (Z–A)",
        "Artist (A–Z)",
        "Year (Newest)",
        "Year (Oldest)",
    ];
    let sort_dropdown = gtk::DropDown::from_strings(&sort_options);
    let initial_album_sort = state_mgr.snapshot().album_sort_mode;
    sort_dropdown.set_selected(initial_album_sort as u32);
    sort_dropdown.add_css_class("sort-dropdown");
    sort_dropdown.set_tooltip_text(Some("Album sort criteria"));
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
    let status = gtk::Label::new(Some("Organizando álbuns pelos metadados…"));
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
                    status.set_text("Could not organize albums.");
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
    views.add_titled(&grid_widget, Some("covers"), "Covers");

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
    let empty_label = gtk::Label::new(Some("Nenhum álbum corresponde à busca"));
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
    glib::MainContext::default().spawn_local(async move {
        for (index, album_info) in fill_albums.iter().enumerate() {
            let Some(list) = weak_fill_list.upgrade() else {
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
            let subtitle = if album_info.artist != "Artista desconhecido" {
                format!("{}  ·  {}{}", album_info.artist, album_info.title, year_str)
            } else {
                format!("{}{}", album_info.title, year_str)
            };
            let label = gtk::Label::new(Some(&format!(
                "{}  ·  {} faixa(s)",
                subtitle,
                album_info.tracks.len()
            )));
            label.set_xalign(0.0);
            label.set_hexpand(true);
            label.set_ellipsize(gtk::pango::EllipsizeMode::End);
            label.add_css_class("album-title-header");

            let tracks = Rc::new(album_info.tracks.clone());
            let album_queue = tracks.clone();
            let button = labeled_icon_button("media-playback-start-symbolic", "Ouvir álbum");
            button.add_css_class("play-album-button");
            let engine = fill_audio.clone();
            button.connect_clicked(move |_| {
                let _ = engine.play_album(&album_queue, 0);
            });

            header_row.append(&album_icon);
            header_row.append(&label);
            header_row.append(&button);
            header_row.append(&queue_actions(&fill_audio, tracks.clone(), true));
            box_.append(&header_row);

            row.set_child(Some(&box_));
            list.append(&row);
            drop(list);
            for (track_idx, path) in album_info.tracks.iter().cloned().enumerate() {
                if weak_fill_list.upgrade().is_none() {
                    return;
                }
                let child =
                    track_row_in_queue(&fill_audio, path, true, Some((tracks.clone(), track_idx)));
                box_.append(&child);
                if track_idx % 24 == 23 {
                    glib::timeout_future_with_priority(
                        glib::Priority::LOW,
                        std::time::Duration::from_millis(8),
                    )
                    .await;
                }
            }
            glib::timeout_future_with_priority(
                glib::Priority::LOW,
                std::time::Duration::from_millis(4),
            )
            .await;
        }
    });

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
    search_hub.on_query_changed(move || {
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
    views.add_titled(&scrolled, Some("list"), "List");
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
    search_hub.on_query_changed(move || {
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
    search_hub.on_query_changed(move || {
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
        let empty = gtk::Label::new(Some("Nenhum álbum encontrado na biblioteca."));
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
                (artist, Some(y)) if artist != "Artista desconhecido" => {
                    format!("{} — {} ({})", artist, album_info.title, y)
                }
                (artist, None) if artist != "Artista desconhecido" => {
                    format!("{} — {}", artist, album_info.title)
                }
                (_, Some(y)) => format!("{} ({})", album_info.title, y),
                _ => album_info.title.clone(),
            };
            button.set_tooltip_text(Some(&tooltip));
            button.update_property(&[gtk::accessible::Property::Label(&format!(
                "Abrir álbum {album}"
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
            // Um único worker decodifica as capas; a UI recebe no máximo duas por rodada.
            let (sender, receiver) = std::sync::mpsc::sync_channel(4);
            std::thread::spawn(move || {
                for (index, source) in sources.into_iter().enumerate() {
                    let Some(path) = audio::find_cover(&source) else {
                        continue;
                    };
                    let Ok(image) = gdk_pixbuf::Pixbuf::from_file_at_scale(path, 640, 640, true)
                    else {
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
            glib::timeout_add_local(std::time::Duration::from_millis(50), move || {
                if pictures.iter().all(|picture| picture.upgrade().is_none()) {
                    return glib::ControlFlow::Break;
                }
                for _ in 0..2 {
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
    let back = labeled_icon_button("go-previous-symbolic", "Back to albums");
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
    let count = gtk::Label::new(Some(&format!("{} tracks · Local album", tracks.len())));
    count.set_xalign(0.0);
    count.add_css_class("bottom-artist-title");
    info.append(&count);
    let play = labeled_icon_button("media-playback-start-symbolic", "Play album");
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
        menu.set_label("Add to queue");
        menu.set_halign(gtk::Align::Start);
    } else {
        menu.set_icon_name("view-more-symbolic");
    }
    menu.set_tooltip_text(Some(if album {
        "Queue options for this album"
    } else {
        "Queue options for this track"
    }));
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
        for (name, label, next) in [
            ("enqueue-next", "Play next", true),
            ("enqueue-last", "Add to end", false),
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
                        menu.set_tooltip_text(Some("Added to queue"));
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

            let pl_lbl = gtk::Label::new(Some("Add to playlist:"));
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
                            let msg = if tracks.len() == 1 {
                                format!("Track added to \"{pl_name}\"!")
                            } else {
                                format!("{} tracks added to \"{pl_name}\"!", tracks.len())
                            };
                            if let Some(m) = weak_m.upgrade() {
                                m.set_tooltip_text(Some(&msg));
                            }
                            show_system_notification("Playlist Updated", &msg);
                            if let Some(p) = weak_pop.upgrade() {
                                p.popdown();
                            }
                        }
                        Err(e) => {
                            error_clone.set_text(&format!("Error saving: {e}"));
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

fn labeled_icon_button(icon: &str, text: &str) -> gtk::Button {
    let content = gtk::Box::new(Orientation::Horizontal, 10);
    content.append(&gtk::Image::from_icon_name(icon));
    let label = gtk::Label::new(Some(text));
    label.set_xalign(0.0);
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
) -> gtk::Widget {
    let bar = gtk::Box::new(Orientation::Horizontal, 14);
    bar.add_css_class("topbar");

    bar.append(title_label);

    let search = gtk::SearchEntry::new();
    search.set_placeholder_text(Some("Search tracks, artists, or albums..."));
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
            title_ref.set_text("Tracks");
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
    search.connect_activate(move |entry| {
        let text = entry.text();
        let _ = search_audio.search_and_load(text.as_str());
    });
    bar.append(&search);

    let open = labeled_icon_button("folder-open-symbolic", "Open track");
    open.add_css_class("open-button");
    let open_audio = audio.clone();
    open.connect_clicked(move |_| {
        let dialog = gtk::FileDialog::builder()
            .title("Open audio file")
            .build();
        let engine = open_audio.clone();
        glib::MainContext::default().spawn_local(async move {
            if let Ok(file) = dialog.open_future(None::<&gtk::Window>).await {
                if let Some(path) = file.path() {
                    if engine.load(path).is_ok() {
                        let _ = engine.play();
                    }
                }
            }
        });
    });
    bar.append(&open);
    bar.upcast()
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
    let eyebrow = gtk::Label::new(Some("SESSÃO DE ESCUTA"));
    eyebrow.add_css_class("listening-eyebrow");
    eyebrow.set_xalign(0.0);
    eyebrow.set_hexpand(true);
    heading.append(&eyebrow);
    let status_box = gtk::Box::new(Orientation::Horizontal, 7);
    status_box.add_css_class("listening-status");
    let status_icon = gtk::Image::from_icon_name("media-playback-stop-symbolic");
    status_icon.set_pixel_size(12);
    let status = gtk::Label::new(Some("PRONTO"));
    status_box.append(&status_icon);
    status_box.append(&status);
    heading.append(&status_box);

    let rpm_box = gtk::Box::new(Orientation::Horizontal, 0);
    rpm_box.add_css_class("rpm-selector");
    rpm_box.set_tooltip_text(Some("Rotational speed: 33 ⅓ RPM or 45 RPM"));

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
        draw_turntable(
            cr,
            w,
            h,
            draw_angle.get(),
            draw_arm.get(),
            draw_playing.get(),
            &draw_cache,
            widget.scale_factor(),
            draw_rpm.get(),
            hud_level,
            hud_alpha,
            vu_l,
            vu_r,
            draw_tl.get(),
            draw_pitch.get(),
        );
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
    click.connect_released(move |_, _, x, y| {
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
        let d_tl = ((x - layout.target_light_x).powi(2) + (y - layout.target_light_y).powi(2)).sqrt();
        if d_tl <= layout.target_light_r + 6.0 {
            let next_tl = !click_tl.get();
            click_tl.set(next_tl);
            click_state_mgr.update_target_light(next_tl);
            click_state_mgr.flush_if_dirty();
            deck_widget.queue_draw();
            return;
        }

        // 3. Clique ou trava no Pitch Fader
        let px = layout.pitch_x;
        let py = layout.pitch_y;
        let pw = layout.pitch_w;
        let ph = layout.pitch_h;
        if x >= px - 6.0 && x <= px + pw + 6.0 && y >= py - 6.0 && y <= py + ph + 6.0 {
            let center_y = py + ph / 2.0;
            let slot_h = ph - 28.0;
            let new_pitch = if (y - center_y).abs() <= 5.0 || x <= px + 8.0 {
                0.0
            } else {
                let rel = (center_y - y) / (slot_h / 2.0);
                let p = (rel * 0.08).clamp(-0.08, 0.08);
                if p.abs() < 0.003 { 0.0 } else { p }
            };
            click_pitch.set(new_pitch);
            click_state_mgr.update_pitch(new_pitch);
            click_state_mgr.flush_if_dirty();
            click_audio.set_rate(1.0 + new_pitch);
            deck_widget.queue_draw();
        }
    });
    deck.add_controller(click);

    let drag = gtk::GestureDrag::new();
    let drag_pitch = pitch.clone();
    let drag_state_mgr = state_mgr.clone();
    let drag_audio = audio.clone();
    let drag_deck = deck.downgrade();
    drag.connect_drag_update(move |gesture, _offset_x, _offset_y| {
        let Some((start_x, start_y)) = gesture.start_point() else { return; };
        let Some(deck_widget) = drag_deck.upgrade() else { return; };
        let w = deck_widget.width() as f64;
        let h = deck_widget.height() as f64;
        let layout = TurntableLayout::compute(w, h);
        let px = layout.pitch_x;
        let py = layout.pitch_y;
        let pw = layout.pitch_w;
        let ph = layout.pitch_h;

        if start_x >= px - 10.0 && start_x <= px + pw + 10.0 && start_y >= py - 8.0 && start_y <= py + ph + 8.0 {
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
    let title = gtk::Label::new(Some("Escolha sua próxima faixa"));
    title.set_xalign(0.0);
    title.set_ellipsize(gtk::pango::EllipsizeMode::End);
    title.set_max_width_chars(38);
    title.add_css_class("listening-title");
    let artist = gtk::Label::new(Some("Abra um álbum ou arraste um arquivo de áudio"));
    artist.set_xalign(0.0);
    artist.set_ellipsize(gtk::pango::EllipsizeMode::End);
    artist.set_max_width_chars(55);
    artist.add_css_class("listening-artist");
    description.append(&title);
    description.append(&artist);
    information.append(&description);
    let format = gtk::Label::new(Some("ÁUDIO LOCAL"));
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
    let tick_pitch = pitch.clone();
    let last_pitch = Rc::new(Cell::new(initial_pitch));
    let tick_last_notif = Rc::new(RefCell::new(String::new()));
    let last_rpm = Rc::new(Cell::new(initial_rpm));

    let last_frame_time = Rc::new(Cell::new(0_i64));
    let last_meta_ver = Rc::new(Cell::new(0_u64));
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

        let state = tick_audio.state();
        let is_playing = state == PlaybackState::Playing;
        if state != last_state.replace(state) {
            status.set_text(match state {
                PlaybackState::Playing => "EM REPRODUÇÃO",
                PlaybackState::Paused => "PAUSADO",
                PlaybackState::Stopped => "PRONTO",
            });
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
                let m = tick_audio.metadata();
                if !m.uri.is_empty() {
                    let mut last_notif = tick_last_notif.borrow_mut();
                    if *last_notif != m.uri {
                        *last_notif = m.uri.clone();
                        show_track_notification(
                            &m.title,
                            &m.artist,
                            &m.album,
                            m.cover_path.as_deref(),
                        );
                    }
                }
            } else {
                status_box.remove_css_class("is-playing");
                if state == PlaybackState::Stopped {
                    tick_last_notif.borrow_mut().clear();
                }
            }
        }
        let playback_changed = tick_playing.replace(is_playing) != is_playing;

        // 1. Atualização reativa de metadados e capa SOMENTE se mudaram de versão
        let should_poll_metadata =
            last_metadata_poll.get() == 0 || frame_time - last_metadata_poll.get() >= 250_000;
        if should_poll_metadata {
            last_metadata_poll.set(frame_time);
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
                "Choose your next track"
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
                "Local audio file"
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
                "ÁUDIO LOCAL"
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
                let pb = if let Some(ref p) = m.cover_path {
                    gdk_pixbuf::Pixbuf::from_file(p).ok()
                } else if let Some(ref b) = m.cover_bytes {
                    let loader = gdk_pixbuf::PixbufLoader::new();
                    if loader.write(b).is_ok() && loader.close().is_ok() {
                        loader.pixbuf()
                    } else {
                        None
                    }
                } else {
                    None
                };
                state.cover = CachedCover {
                    key: current_key,
                    original: pb,
                    surface: None,
                };
                state.disc_surface = None;
            }
            if is_playing && !m.uri.is_empty() {
                let mut last_notif = tick_last_notif.borrow_mut();
                if *last_notif != m.uri {
                    *last_notif = m.uri.clone();
                    show_track_notification(&m.title, &m.artist, &m.album, m.cover_path.as_deref());
                }
            }
            deck_widget.queue_draw();
        }

        // 2. Física e Dinâmica da Rotação e do Braço
        let mut needs_draw = playback_changed;
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

struct TurntableLayout {
    mx: f64,
    my: f64,
    dw: f64,
    dh: f64,
    cx: f64,
    cy: f64,
    r_disc: f64,
    r_platter: f64,
    pivot_x: f64,
    pivot_y: f64,
    arm_len: f64,
    led_x: f64,
    led_y: f64,
    vu_x: f64,
    vu_y: f64,
    vu_w: f64,
    vu_h: f64,
    target_light_x: f64,
    target_light_y: f64,
    target_light_r: f64,
    strobe_x: f64,
    strobe_y: f64,
    pitch_x: f64,
    pitch_y: f64,
    pitch_w: f64,
    pitch_h: f64,
}

impl TurntableLayout {
    fn compute(w: f64, h: f64) -> Self {
        let margin = 14.0;
        let max_w = (w - 2.0 * margin).max(200.0);
        let max_h = (h - 2.0 * margin).max(200.0);
        let aspect = 1.34_f64;
        let dh = max_h.min(max_w / aspect);
        let dw = dh * aspect;
        let mx = (w - dw) / 2.0;
        let my = (h - dh) / 2.0;

        let cx = mx + dw * 0.46;
        let cy = my + dh * 0.50;
        let r_disc = dh * 0.445;
        let r_platter = r_disc + 7.5;

        let pivot_x = mx + dw * 0.855;
        let pivot_y = my + dh * 0.17;
        let arm_len = dh * 0.515;

        let led_x = mx + 26.0;
        let led_y = my + dh - 26.0;

        let vu_w = 96.0;
        let vu_h = 35.0;
        let vu_x = mx + dw - vu_w - 10.0;
        let vu_y = my + dh - vu_h - 10.0;

        let target_light_x = mx + 38.0;
        let target_light_y = my + dh * 0.74;
        let target_light_r = 11.0;

        let strobe_x = mx + 20.0;
        let strobe_y = my + dh * 0.38;

        let pitch_w = 26.0;
        let pitch_h = (dh * 0.38).clamp(100.0, 160.0);
        let pitch_x = mx + dw - pitch_w - 22.0;
        let pitch_y = my + dh * 0.35;

        Self {
            mx,
            my,
            dw,
            dh,
            cx,
            cy,
            r_disc,
            r_platter,
            pivot_x,
            pivot_y,
            arm_len,
            led_x,
            led_y,
            vu_x,
            vu_y,
            vu_w,
            vu_h,
            target_light_x,
            target_light_y,
            target_light_r,
            strobe_x,
            strobe_y,
            pitch_x,
            pitch_y,
            pitch_w,
            pitch_h,
        }
    }
}

fn render_background(layout: &TurntableLayout, w: i32, h: i32) -> Option<cairo::ImageSurface> {
    let surface = cairo::ImageSurface::create(cairo::Format::ARgb32, w, h).ok()?;
    let cr = cairo::Context::new(&surface).ok()?;

    let mx = layout.mx;
    let my = layout.my;
    let dw = layout.dw;
    let dh = layout.dh;
    let corner_r = 16.0;

    // Sombra suave sob o chassi
    cr.set_source_rgba(0.0, 0.0, 0.0, 0.45);
    draw_rounded_rect(&cr, mx + 3.0, my + 5.0, dw, dh, corner_r);
    cr.fill().ok();

    // Base de alumínio escovado escuro (chassi)
    let deck_grad = cairo::LinearGradient::new(mx, my, mx + dw, my + dh);
    deck_grad.add_color_stop_rgba(0.0, 0.12, 0.125, 0.135, 1.0);
    deck_grad.add_color_stop_rgba(0.5, 0.09, 0.095, 0.105, 1.0);
    deck_grad.add_color_stop_rgba(1.0, 0.055, 0.06, 0.065, 1.0);
    cr.set_source(&deck_grad).ok();
    draw_rounded_rect(&cr, mx, my, dw, dh, corner_r);
    cr.fill().ok();

    // Borda metálica fina do chassi
    cr.set_source_rgba(1.0, 1.0, 1.0, 0.08);
    cr.set_line_width(1.5);
    draw_rounded_rect(&cr, mx, my, dw, dh, corner_r);
    cr.stroke().ok();

    // Painel usinado e textura sutil: tudo permanece no cache do fundo.
    cr.save().ok();
    draw_rounded_rect(&cr, mx + 8.0, my + 8.0, dw - 16.0, dh - 16.0, 11.0);
    cr.clip();
    cr.set_line_width(0.5);
    cr.set_source_rgba(1.0, 1.0, 1.0, 0.018);
    for y in (0..dh as i32).step_by(4) {
        cr.move_to(mx, my + y as f64);
        cr.line_to(mx + dw, my + y as f64);
    }
    cr.stroke().ok();
    cr.restore().ok();
    cr.set_source_rgba(1.0, 1.0, 1.0, 0.055);
    cr.set_line_width(0.8);
    draw_rounded_rect(&cr, mx + 7.0, my + 7.0, dw - 14.0, dh - 14.0, 11.0);
    cr.stroke().ok();

    // Parafusos metálicos hexagonais nos 4 cantos do chassi
    let bolt_inset = 22.0;
    let bolts = [
        (mx + bolt_inset, my + bolt_inset),
        (mx + dw - bolt_inset, my + bolt_inset),
        (mx + bolt_inset, my + dh - bolt_inset),
        (mx + dw - bolt_inset, my + dh - bolt_inset),
    ];
    for (bx, by) in bolts {
        cr.set_source_rgba(0.04, 0.03, 0.05, 0.8);
        cr.arc(bx, by, 4.5, 0.0, TAU);
        cr.fill().ok();

        cr.set_source_rgba(0.45, 0.42, 0.50, 0.9);
        cr.arc(bx, by, 3.5, 0.0, TAU);
        cr.fill().ok();

        cr.set_source_rgba(0.1, 0.1, 0.12, 0.9);
        cr.move_to(bx - 2.0, by);
        cr.line_to(bx + 2.0, by);
        cr.move_to(bx, by - 2.0);
        cr.line_to(bx, by + 2.0);
        cr.set_line_width(0.9);
        cr.stroke().ok();
    }

    // 2. Prato do Toca-Discos (Platter)
    let cx = layout.cx;
    let cy = layout.cy;
    let r_disc = layout.r_disc;
    let r_platter = layout.r_platter;

    // Poço rebaixado do prato (sombra circular)
    let well_shadow = cairo::RadialGradient::new(cx, cy, r_platter, cx, cy, r_platter + 8.0);
    well_shadow.add_color_stop_rgba(0.0, 0.0, 0.0, 0.0, 0.6);
    well_shadow.add_color_stop_rgba(1.0, 0.0, 0.0, 0.0, 0.0);
    cr.set_source(&well_shadow).ok();
    cr.arc(cx, cy, r_platter + 8.0, 0.0, TAU);
    cr.fill().ok();

    // Aro de alumínio maciço usinado (platter rim) com frisos chanfrados
    let platter_grad = cairo::LinearGradient::new(cx - r_platter, cy, cx + r_platter, cy);
    platter_grad.add_color_stop_rgba(0.0, 0.28, 0.29, 0.30, 1.0);
    platter_grad.add_color_stop_rgba(0.3, 0.63, 0.63, 0.61, 1.0);
    platter_grad.add_color_stop_rgba(0.7, 0.22, 0.23, 0.24, 1.0);
    platter_grad.add_color_stop_rgba(1.0, 0.43, 0.44, 0.44, 1.0);
    cr.set_source(&platter_grad).ok();
    cr.arc(cx, cy, r_platter, 0.0, TAU);
    cr.fill().ok();

    // Ranhuras circulares usinadas no aro do prato
    cr.set_source_rgba(0.12, 0.12, 0.14, 0.55);
    cr.set_line_width(0.7);
    cr.arc(cx, cy, r_platter - 2.5, 0.0, TAU);
    cr.stroke().ok();
    cr.arc(cx, cy, r_platter - 5.5, 0.0, TAU);
    cr.stroke().ok();

    // Tapete antiderrapante (Slipmat)
    cr.set_source_rgb(0.08, 0.07, 0.09);
    cr.arc(cx, cy, r_disc + 2.0, 0.0, TAU);
    cr.fill().ok();

    // Base dos indicadores e botões de velocidade 33 ⅓ e 45 RPM
    let led_x = layout.led_x;
    let led_y = layout.led_y;
    let led45_x = led_x + 50.0;

    for &lx in &[led_x, led45_x] {
        cr.set_source_rgba(0.04, 0.04, 0.05, 0.9);
        cr.arc(lx, led_y, 6.0, 0.0, TAU);
        cr.fill().ok();
        cr.set_source_rgba(1.0, 1.0, 1.0, 0.06);
        cr.set_line_width(0.8);
        cr.arc(lx, led_y, 6.0, 0.0, TAU);
        cr.stroke().ok();
    }

    cr.select_font_face(
        "Cantarell, Sans",
        cairo::FontSlant::Normal,
        cairo::FontWeight::Bold,
    );
    cr.set_font_size(9.5);
    cr.set_source_rgba(0.85, 0.80, 0.74, 0.85);
    cr.move_to(led_x + 9.0, led_y + 3.5);
    cr.show_text("33 ⅓").ok();

    cr.move_to(led45_x + 9.0, led_y + 3.5);
    cr.show_text("45").ok();

    cr.select_font_face("Sans", cairo::FontSlant::Normal, cairo::FontWeight::Normal);
    cr.set_font_size(8.0);
    cr.set_source_rgba(0.80, 0.80, 0.76, 0.50);
    cr.move_to(led45_x + 28.0, led_y + 3.5);
    cr.show_text("RPM").ok();

    // Medidores VU Analógicos Estéreo Hi-Fi (Chassi, mostrador e escala dB pré-renderizados)
    draw_vu_background(&cr, layout.vu_x, layout.vu_y, layout.vu_w, layout.vu_h);

    // Base usinada da Luz Alvo (Stylus Target Light)
    let tlx = layout.target_light_x;
    let tly = layout.target_light_y;
    let tlr = layout.target_light_r;
    cr.set_source_rgba(0.0, 0.0, 0.0, 0.40);
    cr.arc(tlx + 1.5, tly + 2.5, tlr + 3.0, 0.0, TAU);
    cr.fill().ok();
    let tl_ring = cairo::RadialGradient::new(tlx - 2.0, tly - 2.0, 2.0, tlx, tly, tlr + 2.0);
    tl_ring.add_color_stop_rgba(0.0, 0.55, 0.54, 0.58, 1.0);
    tl_ring.add_color_stop_rgba(0.7, 0.22, 0.21, 0.25, 1.0);
    tl_ring.add_color_stop_rgba(1.0, 0.12, 0.11, 0.14, 1.0);
    cr.set_source(&tl_ring).ok();
    cr.arc(tlx, tly, tlr + 2.0, 0.0, TAU);
    cr.fill().ok();
    cr.set_source_rgba(1.0, 1.0, 1.0, 0.15);
    cr.set_line_width(0.8);
    cr.arc(tlx, tly, tlr + 2.0, 0.0, TAU);
    cr.stroke().ok();

    // Carcaça metálica do Prisma Estroboscópico (Strobe Prism Housing)
    let sx = layout.strobe_x;
    let sy = layout.strobe_y;
    cr.set_source_rgba(0.02, 0.02, 0.03, 0.85);
    draw_rounded_rect(&cr, sx - 2.0, sy - 14.0, 16.0, 28.0, 4.0);
    cr.fill().ok();
    let prism_grad = cairo::LinearGradient::new(sx, sy - 14.0, sx + 14.0, sy + 14.0);
    prism_grad.add_color_stop_rgba(0.0, 0.42, 0.41, 0.46, 1.0);
    prism_grad.add_color_stop_rgba(0.6, 0.18, 0.17, 0.20, 1.0);
    prism_grad.add_color_stop_rgba(1.0, 0.28, 0.27, 0.30, 1.0);
    cr.set_source(&prism_grad).ok();
    draw_rounded_rect(&cr, sx, sy - 12.0, 12.0, 24.0, 3.0);
    cr.fill().ok();
    // Lente óptica vermelha do prisma estroboscópico
    cr.set_source_rgb(0.65, 0.08, 0.05);
    draw_rounded_rect(&cr, sx + 7.0, sy - 8.0, 4.0, 16.0, 1.5);
    cr.fill().ok();

    // Painel do Pitch Fader (Controle Analógico de Afinação ±8%)
    let px = layout.pitch_x;
    let py = layout.pitch_y;
    let pw = layout.pitch_w;
    let ph = layout.pitch_h;
    cr.set_source_rgba(0.02, 0.02, 0.03, 0.75);
    draw_rounded_rect(&cr, px - 1.0, py - 1.0, pw + 2.0, ph + 2.0, 4.0);
    cr.fill().ok();
    let fader_plate = cairo::LinearGradient::new(px, py, px, py + ph);
    fader_plate.add_color_stop_rgba(0.0, 0.18, 0.18, 0.20, 1.0);
    fader_plate.add_color_stop_rgba(0.5, 0.12, 0.12, 0.14, 1.0);
    fader_plate.add_color_stop_rgba(1.0, 0.16, 0.16, 0.18, 1.0);
    cr.set_source(&fader_plate).ok();
    draw_rounded_rect(&cr, px, py, pw, ph, 3.0);
    cr.fill().ok();
    cr.set_source_rgba(1.0, 1.0, 1.0, 0.08);
    cr.set_line_width(0.8);
    draw_rounded_rect(&cr, px, py, pw, ph, 3.0);
    cr.stroke().ok();

    // Ranhura central (slot)
    let slot_x = px + pw * 0.58;
    let slot_top = py + 14.0;
    let slot_h = ph - 28.0;
    cr.set_source_rgb(0.02, 0.02, 0.03);
    draw_rounded_rect(&cr, slot_x - 1.5, slot_top, 3.0, slot_h, 1.5);
    cr.fill().ok();

    // Marcações da escala serigrafada
    let center_y = py + ph / 2.0;
    for i in -4..=4 {
        let frac = i as f64 / 4.0;
        let y = center_y - frac * (slot_h / 2.0 - 2.0);
        let is_center = i == 0;
        let tick_w = if is_center { 7.0 } else if i % 2 == 0 { 5.0 } else { 3.0 };
        if is_center {
            cr.set_source_rgba(0.95, 0.85, 0.40, 0.90);
            cr.set_line_width(1.2);
        } else {
            cr.set_source_rgba(0.80, 0.80, 0.75, 0.45);
            cr.set_line_width(0.7);
        }
        cr.move_to(slot_x - 4.0 - tick_w, y);
        cr.line_to(slot_x - 4.0, y);
        cr.stroke().ok();
    }

    // Base do LED Quartz Lock
    let led_q_x = px + 6.0;
    let led_q_y = center_y;
    cr.set_source_rgba(0.04, 0.04, 0.05, 0.9);
    cr.arc(led_q_x, led_q_y, 3.0, 0.0, TAU);
    cr.fill().ok();
    cr.set_source_rgba(1.0, 1.0, 1.0, 0.06);
    cr.set_line_width(0.6);
    cr.arc(led_q_x, led_q_y, 3.0, 0.0, TAU);
    cr.stroke().ok();

    // Suporte / Descanso do braço (Rest Cradle) na posição de repouso exata
    let pivot_x = layout.pivot_x;
    let pivot_y = layout.pivot_y;
    let arm_len = layout.arm_len;
    let rest_angle = 1.476_f64;
    let rest_x = pivot_x + (arm_len * 0.95) * rest_angle.cos();
    let rest_y = pivot_y + (arm_len * 0.95) * rest_angle.sin();
    cr.set_source_rgba(0.08, 0.07, 0.10, 0.95);
    cr.rectangle(rest_x - 4.0, rest_y - 2.0, 8.0, 16.0);
    cr.fill().ok();
    cr.set_source_rgba(0.55, 0.52, 0.60, 0.85);
    cr.set_line_width(1.5);
    cr.arc(rest_x, rest_y, 4.5, PI, 0.0);
    cr.stroke().ok();

    // Base do Cardan / Pivô (Gimbal assembly)
    cr.set_source_rgba(0.0, 0.0, 0.0, 0.4);
    cr.arc(pivot_x + 2.0, pivot_y + 3.0, 22.0, 0.0, TAU);
    cr.fill().ok();

    let gimbal_outer =
        cairo::RadialGradient::new(pivot_x - 3.0, pivot_y - 3.0, 2.0, pivot_x, pivot_y, 22.0);
    gimbal_outer.add_color_stop_rgba(0.0, 0.35, 0.33, 0.40, 1.0);
    gimbal_outer.add_color_stop_rgba(1.0, 0.14, 0.13, 0.16, 1.0);
    cr.set_source(&gimbal_outer).ok();
    cr.arc(pivot_x, pivot_y, 21.0, 0.0, TAU);
    cr.fill().ok();
    cr.set_source_rgba(1.0, 1.0, 1.0, 0.12);
    cr.set_line_width(1.2);
    cr.stroke().ok();

    let gimbal_inner =
        cairo::RadialGradient::new(pivot_x - 2.0, pivot_y - 2.0, 1.0, pivot_x, pivot_y, 14.0);
    gimbal_inner.add_color_stop_rgba(0.0, 0.85, 0.87, 0.95, 1.0);
    gimbal_inner.add_color_stop_rgba(0.7, 0.45, 0.44, 0.52, 1.0);
    gimbal_inner.add_color_stop_rgba(1.0, 0.20, 0.18, 0.24, 1.0);
    cr.set_source(&gimbal_inner).ok();
    cr.arc(pivot_x, pivot_y, 14.0, 0.0, TAU);
    cr.fill().ok();

    Some(surface)
}

// Rasteriza a arte que gira somente quando a capa ou o tamanho mudam.
fn render_rotating_disc(
    r_disc: f64,
    target_dim: i32,
    surface_to_draw: Option<cairo::ImageSurface>,
    scale: i32,
    rpm: u32,
) -> Option<cairo::ImageSurface> {
    // Margem transparente para preservar a borda durante a interpolação.
    let side = (2.0 * r_disc + 4.0).ceil() as i32;
    let surface =
        cairo::ImageSurface::create(cairo::Format::ARgb32, side * scale, side * scale).ok()?;
    surface.set_device_scale(scale as f64, scale as f64);
    let cr = cairo::Context::new(&surface).ok()?;
    cr.translate(side as f64 / 2.0, side as f64 / 2.0);
    // Recorta estritamente ao formato circular do disco de vinil
    cr.arc(0.0, 0.0, r_disc, 0.0, TAU);
    cr.clip();

    if let Some(ref surf) = surface_to_draw {
        // Capa em superfície Cairo nativa em rotação fluida
        let half = target_dim as f64 / 2.0;
        cr.set_source_surface(surf, -half, -half).ok();
        cr.paint().ok();

        // Vinheta escura elegante nas bordas do vinil para profundidade ótica
        let edge_shade = cairo::RadialGradient::new(0.0, 0.0, r_disc * 0.76, 0.0, 0.0, r_disc);
        edge_shade.add_color_stop_rgba(0.0, 0.0, 0.0, 0.0, 0.0);
        edge_shade.add_color_stop_rgba(0.90, 0.0, 0.0, 0.0, 0.28);
        edge_shade.add_color_stop_rgba(1.0, 0.0, 0.0, 0.0, 0.58);
        cr.set_source(&edge_shade).ok();
        cr.arc(0.0, 0.0, r_disc, 0.0, TAU);
        cr.fill().ok();
    } else {
        // Arte geométrica padrão quando nenhuma capa está carregada
        cr.set_source_rgb(0.16, 0.13, 0.20);
        cr.paint().ok();
        let default_grad = cairo::RadialGradient::new(0.0, 0.0, 8.0, 0.0, 0.0, r_disc);
        default_grad.set_extend(cairo::Extend::Pad);
        default_grad.add_color_stop_rgba(0.0, 0.16, 0.13, 0.20, 1.0);
        default_grad.add_color_stop_rgba(0.65, 0.08, 0.065, 0.10, 1.0);
        default_grad.add_color_stop_rgba(1.0, 0.03, 0.025, 0.04, 1.0);
        cr.set_source(&default_grad).ok();
        cr.arc(0.0, 0.0, r_disc, 0.0, TAU);
        cr.fill().ok();

        let num_rays = 16;
        for i in 0..num_rays {
            if i % 2 == 0 {
                let a1 = i as f64 * TAU / num_rays as f64;
                let a2 = (i + 1) as f64 * TAU / num_rays as f64;
                cr.new_sub_path();
                cr.arc(0.0, 0.0, r_disc, a1, a2);
                cr.line_to(0.0, 0.0);
                cr.close_path();
                cr.set_source_rgba(0.96, 0.72, 0.22, 0.06);
                cr.fill().ok();
            }
        }

        cr.set_source_rgba(0.96, 0.75, 0.25, 0.25);
        cr.set_line_width(1.2);
        cr.arc(0.0, 0.0, r_disc * 0.72, 0.0, TAU);
        cr.stroke().ok();

        cr.set_source_rgba(0.96, 0.75, 0.25, 0.15);
        cr.set_line_width(0.8);
        cr.arc(0.0, 0.0, r_disc * 0.44, 0.0, TAU);
        cr.stroke().ok();

        cr.select_font_face(
            "Cantarell, Sans",
            cairo::FontSlant::Normal,
            cairo::FontWeight::Bold,
        );
        cr.set_font_size(11.0);
        cr.set_source_rgba(1.0, 0.88, 0.50, 0.85);
        let brand = "VINYLPLAYER";
        let bw = cr.text_extents(brand).map(|e| e.width()).unwrap_or(60.0);
        cr.move_to(-bw / 2.0, -r_disc * 0.32);
        cr.show_text(brand).ok();

        cr.set_font_size(7.5);
        cr.set_source_rgba(0.90, 0.78, 0.45, 0.70);
        let sub = if rpm == 45 {
            "HI-FI AUDIOPHILE · 45 RPM"
        } else {
            "HI-FI AUDIOPHILE · 33 ⅓ RPM"
        };
        let sw = cr.text_extents(sub).map(|e| e.width()).unwrap_or(80.0);
        cr.move_to(-sw / 2.0, r_disc * 0.35);
        cr.show_text(sub).ok();
    }

    // Micro-sulcos em lote (batching: dezenas de círculos desenhados em 3 strokes rápidos)
    // 1. Sulcos regulares
    cr.new_sub_path();
    for i in 1..32 {
        if i % 7 != 0 {
            let gr = r_disc - (i as f64 * (r_disc * 0.028));
            if gr > 14.0 {
                cr.arc(0.0, 0.0, gr, 0.0, TAU);
            }
        }
    }
    cr.set_line_width(0.7);
    cr.set_source_rgba(0.0, 0.0, 0.0, 0.08);
    cr.stroke().ok();

    // 2. Sulcos de intervalo entre faixas
    cr.new_sub_path();
    for i in (7..32).step_by(7) {
        let gr = r_disc - (i as f64 * (r_disc * 0.028));
        if gr > 14.0 {
            cr.arc(0.0, 0.0, gr, 0.0, TAU);
        }
    }
    cr.set_line_width(1.3);
    cr.set_source_rgba(0.0, 0.0, 0.0, 0.22);
    cr.stroke().ok();

    // 3. Reflexos das bordas dos sulcos
    cr.new_sub_path();
    for i in 1..32 {
        let gr = r_disc - (i as f64 * (r_disc * 0.028));
        if gr > 14.0 {
            cr.arc(0.0, 0.0, gr - 0.5, 0.0, TAU);
        }
    }
    cr.set_line_width(0.6);
    cr.set_source_rgba(1.0, 1.0, 1.0, 0.05);
    cr.stroke().ok();

    // Espiral de runout no centro gravada
    cr.set_line_width(0.8);
    cr.set_source_rgba(1.0, 1.0, 1.0, 0.12);
    cr.new_sub_path();
    for step in 0..100 {
        let t = step as f64 / 100.0;
        let r = (r_disc * 0.18) * (1.0 - t * 0.50);
        let a = t * TAU * 2.0;
        let gx = r * a.cos();
        let gy = r * a.sin();
        if step == 0 {
            cr.move_to(gx, gy);
        } else {
            cr.line_to(gx, gy);
        }
    }
    cr.stroke().ok();

    Some(surface)
}

#[cfg(test)]
mod animation_tests {
    use super::*;
    use gstreamer::prelude::ElementExtManual;

    #[test]
    #[ignore = "Requer sessão gráfica; testa abertura inicial e reuso da janela com áudio silencioso"]
    fn open_files_reuses_window_and_preserves_selection_order() {
        gtk::init().unwrap();
        let config = state::TestConfig::new();
        let [first, second, third] = silent_test_tracks(&config);
        let app = Application::builder()
            .application_id("io.github.vinylplayer.opentest")
            .flags(
                gtk::gio::ApplicationFlags::HANDLES_OPEN | gtk::gio::ApplicationFlags::NON_UNIQUE,
            )
            .build();
        let builds = Rc::new(Cell::new(0));
        let captured = Rc::new(RefCell::new(None));
        let build_count = builds.clone();
        let captured_audio = captured.clone();
        let config_path = config.path();
        file_open::connect_handlers(&app, move |app| {
            build_count.set(build_count.get() + 1);
            let audio =
                build_ui_with_state(app, state::StateManager::with_path(config_path.clone()));
            let sink = gstreamer::ElementFactory::make("fakesink")
                .property("sync", true)
                .build()
                .unwrap();
            audio.element().set_property("audio-sink", &sink);
            *captured_audio.borrow_mut() = Some(audio.clone());
            audio
        });
        app.register(None::<&gtk::gio::Cancellable>).unwrap();
        let main_loop = glib::MainLoop::new(None, false);
        app.open(&[gtk::gio::File::for_path(&first)], "");
        settle(&main_loop);
        let audio = captured.borrow().as_ref().unwrap().clone();
        let window = app.active_window().unwrap();
        let assert_track = |path: &std::path::Path| {
            wait_for_audio(&audio, &main_loop, gstreamer::State::Playing);
            assert_eq!(
                audio.metadata().uri,
                glib::filename_to_uri(path, None).unwrap().as_str()
            );
            assert_eq!(audio.state(), PlaybackState::Playing);
        };
        assert_track(&first);
        app.activate();
        assert_eq!(app.active_window().unwrap(), window);
        app.open(
            &[
                gtk::gio::File::for_uri(&glib::filename_to_uri(&second, None).unwrap()),
                gtk::gio::File::for_path(&third),
            ],
            "",
        );
        settle(&main_loop);
        assert_track(&second);
        audio.next().unwrap();
        settle(&main_loop);
        assert_track(&third);
        assert_eq!(builds.get(), 1);
        assert_eq!(app.windows().len(), 1);
        assert_eq!(app.active_window().unwrap(), window);
        app.open(
            &[gtk::gio::File::for_path(
                config.path().with_file_name("missing.wav"),
            )],
            "",
        );
        assert_track(&third);
        let dialog = gtk::Window::list_toplevels()
            .into_iter()
            .find_map(|w| w.downcast::<adw::MessageDialog>().ok())
            .unwrap();
        dialog.response("ok");
        assert_eq!(builds.get(), 1);
        audio.stop().unwrap();
        window.close();
    }

    fn silent_test_tracks(config: &state::TestConfig) -> [std::path::PathBuf; 3] {
        let first = config.path().with_file_name("03 - Música #1.wav");
        let second = config.path().with_file_name("02 - Outra faixa.wav");
        let third = config.path().with_file_name("01 - Última.wav");
        // PCM mono de 30 segundos: tempo suficiente para verificar a fila sem EOS.
        let samples = 8000_u32 * 30;
        let bytes = samples * 2;
        let mut wav = Vec::new();
        wav.extend_from_slice(b"RIFF");
        wav.extend_from_slice(&(36 + bytes).to_le_bytes());
        wav.extend_from_slice(b"WAVEfmt ");
        wav.extend_from_slice(&16_u32.to_le_bytes());
        wav.extend_from_slice(&1_u16.to_le_bytes());
        wav.extend_from_slice(&1_u16.to_le_bytes());
        wav.extend_from_slice(&8000_u32.to_le_bytes());
        wav.extend_from_slice(&16000_u32.to_le_bytes());
        wav.extend_from_slice(&2_u16.to_le_bytes());
        wav.extend_from_slice(&16_u16.to_le_bytes());
        wav.extend_from_slice(b"data");
        wav.extend_from_slice(&bytes.to_le_bytes());
        wav.resize(44 + bytes as usize, 0);
        for path in [&first, &second, &third] {
            std::fs::write(path, &wav).unwrap();
        }
        [first, second, third]
    }

    fn wait_playback_error(
        main_loop: &glib::MainLoop,
        previous: Option<&adw::MessageDialog>,
    ) -> adw::MessageDialog {
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(8);
        loop {
            if let Some(dialog) = gtk::Window::list_toplevels()
                .into_iter()
                .filter_map(|w| w.downcast::<adw::MessageDialog>().ok())
                .find(|w| {
                    w.widget_name() == "playback-error" && w.is_visible() && previous != Some(w)
                })
            {
                return dialog;
            }
            assert!(
                std::time::Instant::now() < deadline,
                "Aviso de erro não apareceu"
            );
            let done = main_loop.clone();
            glib::timeout_add_local_once(std::time::Duration::from_millis(50), move || done.quit());
            main_loop.run();
        }
    }

    #[test]
    #[ignore = "Requer sessão gráfica; testa erros reais, retry, skip, avisos repetidos e fim da fila"]
    fn playback_errors_recover_missing_corrupt_and_stale_selections() {
        gtk::init().unwrap();
        let config = state::TestConfig::new();
        let fixtures = silent_test_tracks(&config);
        let manager = state::StateManager::with_path(config.path());
        manager.update_library_folders(Vec::new());
        manager.update_queue(playback_queue::QueueSnapshot::default());
        let app = Application::builder()
            .application_id("io.github.vinylplayer.errortest")
            .flags(gtk::gio::ApplicationFlags::NON_UNIQUE)
            .build();
        app.register(None::<&gtk::gio::Cancellable>).unwrap();
        let audio = build_ui_with_state(&app, manager);
        let sink = gstreamer::ElementFactory::make("fakesink")
            .property("sync", true)
            .build()
            .unwrap();
        audio.element().set_property("audio-sink", &sink);
        let main_loop = glib::MainLoop::new(None, false);
        std::fs::remove_file(&fixtures[1]).unwrap();
        let _ = audio.play_files(&[fixtures[1].clone(), fixtures[0].clone()], 0);
        let dialog = wait_playback_error(&main_loop, None);
        assert!(dialog.body().contains("não está disponível"));
        assert_eq!(audio.state(), PlaybackState::Stopped);
        save_preview(dialog.upcast_ref(), "/tmp/vinylplayer-error-missing.png");
        silent_test_tracks(&config);
        dialog.response("retry");
        wait_for_audio(&audio, &main_loop, gstreamer::State::Playing);
        assert_eq!(audio.queue_snapshot().current, Some(0));
        assert_eq!(audio.queue_snapshot().tracks.len(), 2);
        let corrupt = config.path().with_file_name("Arquivo danificado.wav");
        std::fs::write(&corrupt, b"this is not a valid audio file").unwrap();
        let queue = vec![corrupt.clone(), fixtures[0].clone()];
        let _ = audio.play_files(&queue, 0);
        let first = wait_playback_error(&main_loop, Some(&dialog));
        assert!(first.body().contains("danificado"));
        save_preview(first.upcast_ref(), "/tmp/vinylplayer-error-corrupt.png");
        first.response("retry");
        let second = wait_playback_error(&main_loop, Some(&first));
        assert_ne!(first, second);
        settle(&main_loop);
        assert_eq!(
            gtk::Window::list_toplevels()
                .into_iter()
                .filter(|w| w.widget_name() == "playback-error" && w.is_visible())
                .count(),
            1
        );
        second.response("close");
        let _ = audio.play();
        let third = wait_playback_error(&main_loop, Some(&second));
        audio.set_repeat_mode(RepeatMode::Track);
        audio.set_shuffle(true);
        third.response("skip");
        wait_for_audio(&audio, &main_loop, gstreamer::State::Playing);
        assert_eq!(audio.queue_snapshot().tracks, queue);
        assert_eq!(audio.queue_snapshot().current, Some(1));
        assert_eq!(audio.repeat_mode(), RepeatMode::Track);
        assert!(audio.shuffle());
        assert_eq!(
            audio.metadata().uri,
            glib::filename_to_uri(&fixtures[0], None).unwrap().as_str()
        );
        let _ = audio.play_files(&[corrupt.clone()], 0);
        let obsolete = wait_playback_error(&main_loop, Some(&third));
        audio.play_files(&[fixtures[2].clone()], 0).unwrap();
        obsolete.response("skip");
        wait_for_audio(&audio, &main_loop, gstreamer::State::Playing);
        settle(&main_loop);
        assert_eq!(audio.queue_snapshot().tracks, vec![fixtures[2].clone()]);
        assert_eq!(audio.queue_snapshot().current, Some(0));
        let _ = audio.play_files(&[corrupt.clone()], 0);
        audio.set_repeat_mode(RepeatMode::All);
        wait_playback_error(&main_loop, Some(&obsolete)).response("skip");
        settle(&main_loop);
        assert_eq!(audio.state(), PlaybackState::Stopped);
        assert_eq!(audio.queue_snapshot().tracks, vec![corrupt]);
        assert!(!gtk::Window::list_toplevels()
            .into_iter()
            .any(|w| w.widget_name() == "playback-error" && w.is_visible()));
        app.active_window().unwrap().close();
        audio.element().set_state(gstreamer::State::Null).unwrap();
    }

    fn wait_for_audio(audio: &AudioEngine, main_loop: &glib::MainLoop, expected: gstreamer::State) {
        let playback = match expected {
            gstreamer::State::Playing => PlaybackState::Playing,
            gstreamer::State::Paused => PlaybackState::Paused,
            _ => PlaybackState::Stopped,
        };
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(8);
        while audio.state() != playback || audio.element().current_state() != expected {
            assert!(
                std::time::Instant::now() < deadline,
                "Transição para {expected:?} não concluiu"
            );
            let done = main_loop.clone();
            glib::timeout_add_local_once(std::time::Duration::from_millis(50), move || done.quit());
            main_loop.run();
        }
    }

    fn descendants(
        widget: &gtk::Widget,
        matches: impl Fn(&gtk::Widget) -> bool + Copy,
    ) -> Vec<gtk::Widget> {
        let mut result = Vec::new();
        if matches(widget) {
            result.push(widget.clone());
        }
        let mut child = widget.first_child();
        while let Some(widget) = child {
            child = widget.next_sibling();
            result.extend(descendants(&widget, matches));
        }
        result
    }

    fn queue_row(window: &gtk::Window, title: &str) -> gtk::Widget {
        descendants(window.upcast_ref(), |w| w.has_css_class("queue-row"))
            .into_iter()
            .find(|row| {
                descendants(row, |w| w.has_css_class("queue-track-title"))
                    .into_iter()
                    .any(|label| label.downcast::<gtk::Label>().unwrap().text() == title)
            })
            .expect("faixa visível na fila")
    }

    fn queue_button(row: &gtk::Widget, name: &str) -> gtk::Button {
        descendants(row, |_| true)
            .into_iter()
            .find(|w| w.widget_name() == name)
            .unwrap()
            .downcast()
            .unwrap()
    }

    fn wait_for_catalog(catalog: &library_settings::Catalog, main_loop: &glib::MainLoop) {
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(20);
        while catalog.scanning.get() {
            assert!(
                std::time::Instant::now() < deadline,
                "Leitura da biblioteca não concluiu"
            );
            let done = main_loop.clone();
            glib::timeout_add_local_once(std::time::Duration::from_millis(20), move || done.quit());
            main_loop.run();
        }
    }

    fn menu_in(root: &gtk::Widget) -> gtk::MenuButton {
        descendants(root, |w| w.widget_name() == "queue-actions")[0]
            .clone()
            .downcast()
            .unwrap()
    }

    fn enqueue_click(menu: &gtk::MenuButton, action: &str, main_loop: &glib::MainLoop) {
        settle(main_loop);
        menu.popup();
        settle(main_loop);
        let popover = menu.popover().unwrap();
        assert!(
            popover.is_mapped(),
            "{action}: menu mapeado={}, visível={}",
            menu.is_mapped(),
            menu.is_visible()
        );
        queue_button(popover.upcast_ref(), action).emit_clicked();
    }

    fn track_menu(page: &gtk::Widget, title: &str) -> gtk::MenuButton {
        let row = descendants(page, |w| w.has_css_class("track-row-item"))
            .into_iter()
            .find(|row| {
                descendants(row, |w| w.has_css_class("track-row-name"))
                    .into_iter()
                    .any(|w| w.downcast::<gtk::Label>().unwrap().text() == title)
            })
            .unwrap();
        menu_in(&row)
    }

    #[test]
    #[ignore = "Requer sessão gráfica; testa menus de adicionar faixa/álbum sem interrupção e persistência"]
    fn enqueue_menus_preserve_stream_order_pause_and_saved_queue() {
        gtk::init().unwrap();
        let config = state::TestConfig::new();
        let files = silent_test_tracks(&config);
        let manager = state::StateManager::with_path(config.path());
        manager.update_library_folders(vec![config.path().parent().unwrap().to_path_buf()]);
        manager.update_queue(playback_queue::QueueSnapshot::default());
        let app = Application::builder()
            .application_id("io.github.vinylplayer.enqueuetest")
            .flags(gtk::gio::ApplicationFlags::NON_UNIQUE)
            .build();
        app.register(None::<&gtk::gio::Cancellable>).unwrap();
        let (audio, catalog) = build_ui_with_catalog(&app, manager);
        let sink = gstreamer::ElementFactory::make("fakesink")
            .property("sync", true)
            .build()
            .unwrap();
        audio.element().set_property("audio-sink", &sink);
        let main_loop = glib::MainLoop::new(None, false);
        wait_for_catalog(&catalog, &main_loop);
        let window = app.active_window().unwrap();
        let pages = descendants(window.upcast_ref(), |w| w.widget_name() == "main-pages")[0]
            .clone()
            .downcast::<gtk::Stack>()
            .unwrap();
        let navigation = pages
            .child_by_name("albums")
            .unwrap()
            .downcast::<gtk::Stack>()
            .unwrap();
        let browse = navigation.child_by_name("browse").unwrap();
        let views = browse
            .last_child()
            .unwrap()
            .downcast::<gtk::Stack>()
            .unwrap();
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(20);
        while views.child_by_name("covers").is_none() {
            assert!(std::time::Instant::now() < deadline);
            settle(&main_loop);
        }
        settle(&main_loop);
        audio.play_files(&[files[0].clone()], 0).unwrap();
        wait_for_audio(&audio, &main_loop, gstreamer::State::Playing);
        let attempt = audio.playback_attempt();
        let uri = audio.metadata().uri;
        let before = audio.position_us();
        pages.set_visible_child_name("tracks");
        let track_page = pages.child_by_name("tracks").unwrap();
        let next = track_menu(&track_page, "02 - Outra faixa");
        next.popup();
        settle(&main_loop);
        save_enqueue_popover(&next, "/tmp/vinylplayer-enqueue-options.png");
        save_preview(&window, "/tmp/vinylplayer-enqueue-track.png");
        queue_button(next.popover().unwrap().upcast_ref(), "enqueue-next").emit_clicked();
        let last = track_menu(&track_page, "01 - Última");
        enqueue_click(&last, "enqueue-last", &main_loop);
        let mut expected = vec![files[0].clone(), files[1].clone(), files[2].clone()];
        assert_eq!(audio.queue_snapshot().tracks, expected);
        pages.set_visible_child_name("albums");
        views.set_visible_child_name("list");
        let album_list = views.child_by_name("list").unwrap();
        let album_menu = menu_in(&album_list);
        enqueue_click(&album_menu, "enqueue-next", &main_loop);
        let album_tracks = vec![files[2].clone(), files[1].clone(), files[0].clone()];
        expected.splice(1..1, album_tracks.clone());
        assert_eq!(audio.queue_snapshot().tracks, expected);
        views.set_visible_child_name("covers");
        let covers = views.child_by_name("covers").unwrap();
        let grid = descendants(&covers, |w| w.is::<gtk::FlowBox>())[0]
            .clone()
            .downcast::<gtk::FlowBox>()
            .unwrap();
        grid.child_at_index(0)
            .unwrap()
            .child()
            .unwrap()
            .downcast::<gtk::Button>()
            .unwrap()
            .emit_clicked();
        let detail = navigation.child_by_name("detail").unwrap();
        let detail_menu = menu_in(&detail);
        detail_menu.popup();
        settle(&main_loop);
        save_preview(&window, "/tmp/vinylplayer-enqueue-album.png");
        queue_button(detail_menu.popover().unwrap().upcast_ref(), "enqueue-last").emit_clicked();
        expected.extend(album_tracks);
        assert_eq!(audio.queue_snapshot().tracks, expected);
        assert_eq!(audio.queue_snapshot().current, Some(0));
        assert_eq!(audio.metadata().uri, uri);
        assert_eq!(
            audio.playback_attempt(),
            attempt,
            "Adicionar não pode reiniciar o stream"
        );
        assert_eq!(audio.element().current_state(), gstreamer::State::Playing);
        assert!(audio.position_us() >= before);
        std::fs::remove_file(&files[2]).unwrap();
        enqueue_click(&detail_menu, "enqueue-last", &main_loop);
        assert_eq!(
            audio.queue_snapshot().tracks,
            expected,
            "Álbum indisponível não é inserido parcialmente"
        );
        assert!(
            descendants(detail_menu.popover().unwrap().upcast_ref(), |w| w
                .has_css_class("queue-error"))[0]
                .is_visible()
        );
        detail_menu.popdown();
        pages.set_visible_child_name("tracks");
        enqueue_click(&last, "enqueue-next", &main_loop);
        assert_eq!(audio.queue_snapshot().tracks, expected);
        last.popdown();
        silent_test_tracks(&config);
        audio.pause().unwrap();
        wait_for_audio(&audio, &main_loop, gstreamer::State::Paused);
        enqueue_click(&next, "enqueue-last", &main_loop);
        expected.push(files[1].clone());
        assert_eq!(audio.state(), PlaybackState::Paused);
        assert_eq!(audio.playback_attempt(), attempt);
        assert_eq!(audio.metadata().uri, uri);
        window.close();
        audio.element().set_state(gstreamer::State::Null).unwrap();
        let restored = state::StateManager::with_path(config.path());
        assert_eq!(
            restored.snapshot().queue.as_ref().unwrap().tracks.as_ref(),
            &expected
        );
        let (reopened, reopened_catalog) = build_ui_with_catalog(&app, restored);
        assert_eq!(reopened.queue_snapshot().tracks, expected);
        assert_eq!(reopened.queue_snapshot().current, Some(0));
        assert_eq!(reopened.state(), PlaybackState::Stopped);
        wait_for_catalog(&reopened_catalog, &main_loop);
        while !reopened.queue_snapshot().tracks.is_empty() {
            let snapshot = reopened.queue_snapshot();
            reopened
                .remove_queue_item(snapshot.revision, snapshot.current.unwrap())
                .unwrap();
        }
        reopened
            .enqueue_files(&[files[1].clone(), files[2].clone()], true)
            .unwrap();
        assert_eq!(
            reopened.queue_snapshot().tracks,
            vec![files[1].clone(), files[2].clone()]
        );
        assert_eq!(reopened.queue_snapshot().current, Some(0));
        assert_eq!(reopened.state(), PlaybackState::Stopped);
        assert_ne!(
            reopened.element().current_state(),
            gstreamer::State::Playing
        );
        app.active_window().unwrap().close();
        reopened
            .element()
            .set_state(gstreamer::State::Null)
            .unwrap();
    }

    #[test]
    #[ignore = "Requer sessão gráfica; testa pastas, atualização, persistência, avisos e continuidade do áudio"]
    fn library_folders_update_ui_and_restore_without_interrupting_audio() {
        gtk::init().unwrap();
        let config = state::TestConfig::new();
        let fixtures = silent_test_tracks(&config);
        let parent = config.path().parent().unwrap().to_path_buf();
        let folder_a = parent.join("Coleção A");
        let folder_b = parent.join("Coleção B");
        std::fs::create_dir(&folder_a).unwrap();
        std::fs::create_dir(&folder_b).unwrap();
        let track_a = folder_a.join("01 - Canção.wav");
        let track_b = folder_b.join("02 - Horizonte.wav");
        std::fs::copy(&fixtures[0], &track_a).unwrap();
        std::fs::copy(&fixtures[1], &track_b).unwrap();
        let manager = state::StateManager::with_path(config.path());
        manager.update_library_folders(Vec::new());
        manager.update_queue(playback_queue::QueueSnapshot::default());
        let app = Application::builder()
            .application_id("io.github.vinylplayer.foldertest")
            .flags(gtk::gio::ApplicationFlags::NON_UNIQUE)
            .build();
        app.register(None::<&gtk::gio::Cancellable>).unwrap();
        let (audio, catalog) = build_ui_with_catalog(&app, manager.clone());
        let sink = gstreamer::ElementFactory::make("fakesink")
            .property("sync", true)
            .build()
            .unwrap();
        audio.element().set_property("audio-sink", &sink);
        audio.play_files(&[fixtures[0].clone()], 0).unwrap();
        let main_loop = glib::MainLoop::new(None, false);
        wait_for_catalog(&catalog, &main_loop);
        settle(&main_loop);
        wait_for_audio(&audio, &main_loop, gstreamer::State::Playing);
        let window = app.active_window().unwrap();
        let frames = Rc::new(Cell::new(0));
        let counter = frames.clone();
        descendants(window.upcast_ref(), |w| w.is::<gtk::DrawingArea>())[0].add_tick_callback(
            move |_, _| {
                counter.set(counter.get() + 1);
                glib::ControlFlow::Continue
            },
        );
        let uri = audio.metadata().uri;
        let position = audio.position_us();
        catalog.add_root(folder_a.clone()).unwrap();
        catalog.add_root(folder_b.clone()).unwrap();
        assert!(catalog.add_root(folder_a.clone()).is_err());
        wait_for_catalog(&catalog, &main_loop);
        settle(&main_loop);
        assert_eq!(
            catalog.report.borrow().tracks,
            vec![track_a.clone(), track_b.clone()]
        );
        assert_eq!(audio.metadata().uri, uri);
        assert_eq!(audio.state(), PlaybackState::Playing);
        assert!(audio.position_us() >= position);
        assert_eq!(audio.queue_snapshot().tracks, vec![fixtures[0].clone()]);
        let pages = descendants(window.upcast_ref(), |w| w.widget_name() == "main-pages")[0]
            .clone()
            .downcast::<gtk::Stack>()
            .unwrap();
        let tracks_page = pages.child_by_name("tracks").unwrap();
        assert_eq!(
            descendants(&tracks_page, |w| w.has_css_class("track-row-name")).len(),
            2
        );
        gtk::prelude::WidgetExt::activate_action(&window, "win.library-settings", None).unwrap();
        settle(&main_loop);
        let settings = gtk::Window::list_toplevels()
            .into_iter()
            .filter_map(|w| w.downcast::<gtk::Window>().ok())
            .find(|w| w.title().as_deref() == Some("Pastas da biblioteca"))
            .unwrap();
        save_preview(&settings, "/tmp/vinylplayer-library-folders.png");
        queue_button(settings.upcast_ref(), "folder-remove").emit_clicked();
        wait_for_catalog(&catalog, &main_loop);
        settle(&main_loop);
        assert_eq!(
            catalog.state.snapshot().library_roots(),
            vec![folder_b.clone()]
        );
        assert_eq!(catalog.report.borrow().tracks, vec![track_b.clone()]);
        assert!(
            track_a.is_file(),
            "Remover pasta não deve excluir os arquivos"
        );
        queue_button(settings.upcast_ref(), "folder-remove").emit_clicked();
        wait_for_catalog(&catalog, &main_loop);
        assert!(catalog.state.snapshot().library_roots().is_empty());
        assert!(catalog.report.borrow().tracks.is_empty());
        assert!(descendants(settings.upcast_ref(), |w| w.is::<gtk::Label>())
            .into_iter()
            .any(|w| w.is_mapped()
                && w.downcast::<gtk::Label>().unwrap().text() == "Nenhuma pasta adicionada."));
        assert!(catalog.add_root(parent.join("não existe")).is_err());
        catalog.add_root(folder_b.clone()).unwrap();
        wait_for_catalog(&catalog, &main_loop);
        let disconnected = parent.join("disco desconectado");
        std::fs::rename(&folder_b, &disconnected).unwrap();
        queue_button(settings.upcast_ref(), "folder-refresh").emit_clicked();
        wait_for_catalog(&catalog, &main_loop);
        settle(&main_loop);
        assert!(catalog.report.borrow().tracks.is_empty());
        assert_eq!(catalog.report.borrow().warnings.len(), 1);
        let warning =
            descendants(settings.upcast_ref(), |w| w.has_css_class("queue-error"))[0].clone();
        assert!(warning.is_mapped());
        assert!(warning
            .downcast::<gtk::Label>()
            .unwrap()
            .text()
            .contains("Coleção B"));
        save_preview(&settings, "/tmp/vinylplayer-library-unavailable.png");
        assert_eq!(audio.metadata().uri, uri);
        assert_eq!(audio.state(), PlaybackState::Playing);
        assert!(
            frames.get() > 2,
            "A animação deve continuar recebendo frames durante as atualizações"
        );
        queue_button(settings.upcast_ref(), "folder-close").emit_clicked();
        assert!(!settings.is_visible());
        assert!(window.is_visible());
        window.close();
        audio.element().set_state(gstreamer::State::Null).unwrap();
        let restored = state::StateManager::with_path(config.path());
        assert_eq!(restored.snapshot().library_roots(), vec![folder_b.clone()]);
        let (reopened, restored_catalog) = build_ui_with_catalog(&app, restored);
        wait_for_catalog(&restored_catalog, &main_loop);
        assert_eq!(restored_catalog.report.borrow().warnings.len(), 1);
        assert_eq!(reopened.state(), PlaybackState::Stopped);
        assert_eq!(reopened.queue_snapshot().tracks, vec![fixtures[0].clone()]);
        std::fs::rename(&disconnected, &folder_b).unwrap();
        restored_catalog.refresh();
        wait_for_catalog(&restored_catalog, &main_loop);
        assert_eq!(restored_catalog.report.borrow().tracks, vec![track_b]);
        assert!(restored_catalog.report.borrow().warnings.is_empty());
        assert_eq!(reopened.state(), PlaybackState::Stopped);
        app.active_window().unwrap().close();
        reopened
            .element()
            .set_state(gstreamer::State::Null)
            .unwrap();
    }

    #[test]
    #[ignore = "Requer sessão gráfica; valida controles, arraste, fila vazia e renderiza /tmp/vinylplayer-queue.png"]
    fn queue_panel_edits_preserve_playback_and_render() {
        gtk::init().unwrap();
        let config = state::TestConfig::new();
        let tracks = silent_test_tracks(&config);
        let app = Application::builder()
            .application_id("io.github.vinylplayer.queuetest")
            .flags(gtk::gio::ApplicationFlags::NON_UNIQUE)
            .build();
        app.register(None::<&gtk::gio::Cancellable>).unwrap();
        let audio = build_ui_with_state(&app, state::StateManager::with_path(config.path()));
        let sink = gstreamer::ElementFactory::make("fakesink")
            .property("sync", true)
            .build()
            .unwrap();
        audio.element().set_property("audio-sink", &sink);
        audio.play_files(&tracks, 1).unwrap();
        let main_loop = glib::MainLoop::new(None, false);
        let window = app.active_window().unwrap();
        settle(&main_loop);
        wait_for_audio(&audio, &main_loop, gstreamer::State::Playing);
        let deck = descendants(window.upcast_ref(), |w| w.is::<gtk::DrawingArea>()).remove(0);
        let width = deck.width();
        gtk::prelude::WidgetExt::activate_action(&window, "win.toggle-queue", None).unwrap();
        settle(&main_loop);
        assert_eq!(
            deck.width(),
            width,
            "O painel sobreposto preserva o tamanho do vinil"
        );
        assert!(queue_row(&window, "02 - Outra faixa").is_mapped());
        save_preview(&window, "/tmp/vinylplayer-queue.png");
        let uri = audio.metadata().uri;
        let before = audio.position_us();
        queue_button(&queue_row(&window, "02 - Outra faixa"), "queue-down").emit_clicked();
        assert_eq!(
            audio.queue_snapshot().tracks,
            [tracks[0].clone(), tracks[2].clone(), tracks[1].clone()]
        );
        assert_eq!(audio.queue_snapshot().current, Some(2));
        assert_eq!(audio.metadata().uri, uri);
        assert!(audio.position_us() >= before);
        settle(&main_loop);

        // Exercita os sinais reais do DragSource e DropTarget, incluindo o payload privado.
        let source_row = queue_row(&window, "03 - Música #1");
        let controllers = source_row.first_child().unwrap().observe_controllers();
        let source = (0..controllers.n_items())
            .find_map(|i| controllers.item(i)?.downcast::<gtk::DragSource>().ok())
            .unwrap();
        let provider = source
            .emit_by_name::<Option<gdk::ContentProvider>>("prepare", &[&0_f64, &0_f64])
            .unwrap();
        let payload = provider.value(String::static_type()).unwrap();
        let target_row = queue_row(&window, "02 - Outra faixa");
        let controllers = target_row.observe_controllers();
        let target = (0..controllers.n_items())
            .find_map(|i| controllers.item(i)?.downcast::<gtk::DropTarget>().ok())
            .unwrap();
        assert!(target.emit_by_name::<bool>(
            "drop",
            &[
                &glib::BoxedValue(payload.clone()),
                &0_f64,
                &(target_row.height() as f64)
            ]
        ));
        assert_eq!(
            audio.queue_snapshot().tracks,
            [tracks[2].clone(), tracks[1].clone(), tracks[0].clone()]
        );
        assert_eq!(audio.metadata().uri, uri);
        assert_eq!(audio.state(), PlaybackState::Playing);
        settle(&main_loop);
        let target_row = queue_row(&window, "02 - Outra faixa");
        let controllers = target_row.observe_controllers();
        let target = (0..controllers.n_items())
            .find_map(|i| controllers.item(i)?.downcast::<gtk::DropTarget>().ok())
            .unwrap();
        assert!(!target.emit_by_name::<bool>("drop", &[&glib::BoxedValue(payload), &0_f64, &0_f64]));
        queue_button(&queue_row(&window, "03 - Música #1"), "queue-remove").emit_clicked();
        assert_eq!(audio.metadata().uri, uri);
        assert_eq!(audio.queue_snapshot().tracks.len(), 2);
        settle(&main_loop);

        // Seleciona uma faixa pela fila, pausa e remove a atual sem retomar o áudio.
        let row = queue_row(&window, "01 - Última");
        row.first_child()
            .unwrap()
            .next_sibling()
            .unwrap()
            .downcast::<gtk::Button>()
            .unwrap()
            .emit_clicked();
        wait_for_audio(&audio, &main_loop, gstreamer::State::Playing);
        audio.pause().unwrap();
        wait_for_audio(&audio, &main_loop, gstreamer::State::Paused);
        settle(&main_loop);
        queue_button(&queue_row(&window, "01 - Última"), "queue-remove").emit_clicked();
        wait_for_audio(&audio, &main_loop, gstreamer::State::Paused);
        assert_eq!(audio.metadata().uri, uri);
        settle(&main_loop);
        queue_button(&queue_row(&window, "02 - Outra faixa"), "queue-remove").emit_clicked();
        settle(&main_loop);
        assert!(audio.queue_snapshot().tracks.is_empty());
        assert!(audio.metadata().uri.is_empty());
        assert_eq!(audio.state(), PlaybackState::Stopped);
        assert!(
            descendants(window.upcast_ref(), |w| w.has_css_class("queue-empty"))[0].is_mapped()
        );
        assert!(descendants(window.upcast_ref(), |w| w.has_css_class("queue-row")).is_empty());
        save_preview(&window, "/tmp/vinylplayer-queue-empty.png");

        // Uma fila extensa não cria um widget por arquivo.
        audio
            .play_files(&vec![tracks[0].clone(); 2_000], 1_900)
            .unwrap();
        settle(&main_loop);
        let rendered = descendants(window.upcast_ref(), |w| w.has_css_class("queue-row")).len();
        assert!(
            rendered > 0 && rendered < 500,
            "Virtualização criou {rendered} linhas"
        );
        queue_button(window.upcast_ref(), "queue-reveal-current").emit_clicked();
        settle(&main_loop);
        assert!(
            descendants(window.upcast_ref(), |w| w.has_css_class("queue-current"))[0].is_mapped()
        );
        gtk::prelude::WidgetExt::activate_action(&window, "win.toggle-queue", None).unwrap();
        settle(&main_loop);
        assert!(!queue_row(&window, "03 - Música #1").is_mapped());
        audio.stop().unwrap();
        window.close();
    }

    #[test]
    #[ignore = "Requer sessão gráfica e ffmpeg; busca por tags, ações filtradas, duplicatas e captura"]
    fn queue_search_preserves_original_indices_and_playback() {
        gtk::init().unwrap();
        let config = state::TestConfig::new();
        let tracks = silent_test_tracks(&config);
        let tagged = tracks[2].with_file_name("metadata.flac");
        assert!(std::process::Command::new("ffmpeg")
            .args(["-v", "error", "-y", "-i"])
            .arg(&tracks[2])
            .args(["-metadata", "title=Horizonte Azul", "-metadata", "artist=João Silva",
                "-metadata", "album_artist=Vários Artistas", "-metadata", "album=Coleção Solar"])
            .arg(&tagged).status().unwrap().success());
        let app = Application::builder()
            .application_id("io.github.vinylplayer.queuesearchtest")
            .flags(gtk::gio::ApplicationFlags::NON_UNIQUE).build();
        app.register(None::<&gtk::gio::Cancellable>).unwrap();
        let audio = build_ui_with_state(&app, state::StateManager::with_path(config.path()));
        let sink = gstreamer::ElementFactory::make("fakesink").property("sync", true).build().unwrap();
        audio.element().set_property("audio-sink", &sink);
        let original = vec![tracks[0].clone(), tracks[1].clone(), tagged.clone(), tracks[0].clone()];
        audio.play_files(&original, 0).unwrap();
        let main_loop = glib::MainLoop::new(None, false);
        let window = app.active_window().unwrap();
        wait_for_audio(&audio, &main_loop, gstreamer::State::Playing);
        gtk::prelude::WidgetExt::activate_action(&window, "win.toggle-queue", None).unwrap();
        settle(&main_loop);
        let search = descendants(window.upcast_ref(), |w| w.widget_name() == "queue-search")
            .remove(0).downcast::<gtk::SearchEntry>().unwrap();
        let list = descendants(window.upcast_ref(), |w| w.widget_name() == "queue-list")
            .remove(0).downcast::<gtk::ListView>().unwrap();
        let indices = || -> Vec<String> {
            let model = list.model().unwrap();
            (0..model.n_items()).map(|i| model.item(i).unwrap().downcast::<gtk::StringObject>().unwrap().string().to_string()).collect()
        };
        let uri = audio.metadata().uri;
        let attempt = audio.playback_attempt();
        let before = audio.position_us();
        for query in ["horizonte azul", "joao silva", "colecao solar", "varios artistas"] {
            search.set_text(query);
            let deadline = std::time::Instant::now() + std::time::Duration::from_secs(12);
            loop {
                settle(&main_loop);
                if indices() == ["2"] { break; }
                assert!(std::time::Instant::now() < deadline, "Metadados não chegaram: {query}");
            }
            assert_eq!(audio.queue_snapshot().tracks, original);
            assert_eq!(audio.metadata().uri, uri);
            assert_eq!(audio.playback_attempt(), attempt);
        }
        assert!(audio.position_us() >= before);
        assert_eq!(audio.state(), PlaybackState::Playing);
        save_preview(&window, "/tmp/vinylplayer-queue-search.png");
        queue_button(&queue_row(&window, "Horizonte Azul"), "queue-up").emit_clicked();
        settle(&main_loop);
        assert_eq!(indices(), ["1"]);
        assert_eq!(audio.queue_snapshot().tracks, [tracks[0].clone(), tagged.clone(), tracks[1].clone(), tracks[0].clone()]);
        assert_eq!(audio.metadata().uri, uri);
        let row = queue_row(&window, "Horizonte Azul");
        descendants(&row, |w| w.has_css_class("queue-track")).remove(0)
            .downcast::<gtk::Button>().unwrap().emit_clicked();
        wait_for_audio(&audio, &main_loop, gstreamer::State::Playing);
        assert_eq!(audio.queue_snapshot().current, Some(1));
        audio.pause().unwrap();
        wait_for_audio(&audio, &main_loop, gstreamer::State::Paused);
        search.set_text("musica");
        settle(&main_loop);
        assert_eq!(indices(), ["0", "3"]);
        let rows = descendants(window.upcast_ref(), |w| w.has_css_class("queue-row") && w.is_mapped());
        queue_button(rows.last().unwrap(), "queue-remove").emit_clicked();
        settle(&main_loop);
        assert_eq!(audio.queue_snapshot().tracks, [tracks[0].clone(), tagged.clone(), tracks[1].clone()]);
        assert_eq!(audio.queue_snapshot().current, Some(1));
        assert_eq!(audio.state(), PlaybackState::Paused);
        assert_eq!(indices(), ["0"]);
        audio.enqueue_files(&[tracks[0].clone()], false).unwrap();
        settle(&main_loop);
        assert_eq!(indices(), ["0", "3"]);
        search.set_text("sem resultado xyz");
        settle(&main_loop);
        assert!(indices().is_empty());
        let summary = descendants(window.upcast_ref(), |w| w.has_css_class("queue-summary"))
            .remove(0).downcast::<gtk::Label>().unwrap();
        assert_eq!(summary.text(), "0 de 4 faixas");
        assert!(descendants(window.upcast_ref(), |w| w.has_css_class("queue-empty"))[0].is_mapped());
        save_preview(&window, "/tmp/vinylplayer-queue-search-empty.png");
        queue_button(window.upcast_ref(), "queue-search-clear").emit_clicked();
        settle(&main_loop);
        assert_eq!(indices(), ["0", "1", "2", "3"]);
        search.set_text("sem resultado");
        settle(&main_loop);
        queue_button(window.upcast_ref(), "queue-reveal-current").emit_clicked();
        settle(&main_loop);
        assert!(search.text().is_empty());
        assert_eq!(indices(), ["0", "1", "2", "3"]);
        assert_eq!(audio.queue_snapshot().current, Some(1));
        assert_eq!(audio.state(), PlaybackState::Paused);
        search.set_text("musica");
        settle(&main_loop);
        let rows = descendants(window.upcast_ref(), |w| w.has_css_class("queue-row") && w.is_mapped());
        assert_eq!(rows.len(), 2);
        let controllers = rows[1].first_child().unwrap().observe_controllers();
        let source = (0..controllers.n_items()).find_map(|i| controllers.item(i)?.downcast::<gtk::DragSource>().ok()).unwrap();
        let provider = source.emit_by_name::<Option<gdk::ContentProvider>>("prepare", &[&0_f64, &0_f64]).unwrap();
        let payload = provider.value(String::static_type()).unwrap();
        let controllers = rows[0].observe_controllers();
        let target = (0..controllers.n_items()).find_map(|i| controllers.item(i)?.downcast::<gtk::DropTarget>().ok()).unwrap();
        assert!(target.emit_by_name::<bool>("drop", &[&glib::BoxedValue(payload.clone()), &0_f64, &0_f64]));
        settle(&main_loop);
        assert_eq!(audio.queue_snapshot().tracks, [tracks[0].clone(), tracks[0].clone(), tagged.clone(), tracks[1].clone()]);
        assert_eq!(audio.queue_snapshot().current, Some(2));
        assert_eq!(indices(), ["0", "1"]);
        assert_eq!(audio.state(), PlaybackState::Paused);
        assert!(!target.emit_by_name::<bool>("drop", &[&glib::BoxedValue(payload), &0_f64, &0_f64]));
        audio.stop().unwrap();
        window.close();
    }

    #[test]
    #[ignore = "Requer sessão gráfica; fecha/reabre fila ordenada, duplicatas, arquivo ausente e fila vazia sem autoplay"]
    fn queue_restores_after_immediate_close_without_autoplay() {
        gtk::init().unwrap();
        let config = state::TestConfig::new();
        let tracks = silent_test_tracks(&config);
        let manager = state::StateManager::with_path(config.path());
        manager.update_volume(0.37);
        manager.update_active_view("albums");
        let app = Application::builder()
            .application_id("io.github.vinylplayer.queuerestore")
            .flags(gtk::gio::ApplicationFlags::NON_UNIQUE)
            .build();
        app.register(None::<&gtk::gio::Cancellable>).unwrap();
        let audio = build_ui_with_state(&app, manager);
        let sink = gstreamer::ElementFactory::make("fakesink")
            .property("sync", true)
            .build()
            .unwrap();
        audio.element().set_property("audio-sink", &sink);
        audio
            .play_files(
                &[
                    tracks[0].clone(),
                    tracks[1].clone(),
                    tracks[2].clone(),
                    tracks[1].clone(),
                ],
                3,
            )
            .unwrap();
        let main_loop = glib::MainLoop::new(None, false);
        settle(&main_loop);
        wait_for_audio(&audio, &main_loop, gstreamer::State::Playing);
        let revision = audio.queue_snapshot().revision;
        audio.move_queue_item(revision, 0, 2).unwrap();
        let revision = audio.queue_snapshot().revision;
        audio.remove_queue_item(revision, 1).unwrap();
        let expected = audio.queue_snapshot();
        assert_eq!(expected.current, Some(2));
        // Sem tick entre a última edição e o fechamento.
        app.active_window().unwrap().close();
        audio.element().set_state(gstreamer::State::Null).unwrap();
        let restored = state::StateManager::with_path(config.path());
        let saved = restored.snapshot();
        assert_eq!(
            saved.queue.as_ref().unwrap().tracks.as_ref(),
            &expected.tracks
        );
        assert_eq!(saved.queue.as_ref().unwrap().current, Some(2));
        assert_eq!(saved.volume, 0.37);
        assert_eq!(saved.active_view, "albums");
        let intact = build_ui_with_state(&app, restored.clone());
        settle(&main_loop);
        assert_eq!(intact.queue_snapshot().tracks, expected.tracks);
        assert_eq!(intact.queue_snapshot().current, Some(2));
        assert_eq!(intact.state(), PlaybackState::Stopped);
        assert_eq!(intact.element().current_state(), gstreamer::State::Ready);
        app.active_window().unwrap().close();
        intact.element().set_state(gstreamer::State::Null).unwrap();
        // Arquivos indisponíveis são ignorados ao restaurar; seleciona a entrada válida anterior.
        std::fs::remove_file(&tracks[1]).unwrap();
        let reopened = build_ui_with_state(&app, restored);
        settle(&main_loop);
        assert_eq!(reopened.queue_snapshot().tracks, vec![tracks[0].clone()]);
        assert_eq!(reopened.queue_snapshot().current, Some(0));
        assert_eq!(reopened.state(), PlaybackState::Stopped);
        assert_ne!(
            reopened.element().current_state(),
            gstreamer::State::Playing
        );
        assert_eq!(reopened.position_us(), 0);
        assert_eq!(
            reopened.metadata().uri,
            glib::filename_to_uri(&tracks[0], None).unwrap().as_str()
        );
        // A fila filtrada é persistida; devolver o arquivo não o reinsere automaticamente.
        app.active_window().unwrap().close();
        reopened
            .element()
            .set_state(gstreamer::State::Null)
            .unwrap();
        silent_test_tracks(&config);
        let reopened = build_ui_with_state(&app, state::StateManager::with_path(config.path()));
        settle(&main_loop);
        assert_eq!(reopened.queue_snapshot().tracks, vec![tracks[0].clone()]);
        assert_eq!(reopened.queue_snapshot().current, Some(0));
        assert_eq!(reopened.state(), PlaybackState::Stopped);
        assert_eq!(reopened.element().current_state(), gstreamer::State::Ready);
        while !reopened.queue_snapshot().tracks.is_empty() {
            let snapshot = reopened.queue_snapshot();
            reopened
                .remove_queue_item(snapshot.revision, snapshot.current.unwrap())
                .unwrap();
        }
        app.active_window().unwrap().close();
        let final_manager = state::StateManager::with_path(config.path());
        assert!(final_manager.snapshot().queue.unwrap().tracks.is_empty());
        let empty = build_ui_with_state(&app, final_manager);
        settle(&main_loop);
        assert!(empty.queue_snapshot().tracks.is_empty());
        assert!(empty.metadata().uri.is_empty());
        assert_eq!(empty.state(), PlaybackState::Stopped);
        assert_eq!(empty.element().current_state(), gstreamer::State::Null);
        app.active_window().unwrap().close();
        // Seleção ausente com sucessora válida; depois, nenhuma entrada disponível.
        let manager = state::StateManager::with_path(config.path());
        let missing = config.path().with_file_name("missing.wav");
        manager.update_queue(playback_queue::QueueSnapshot {
            revision: 1,
            tracks: vec![tracks[0].clone(), missing.clone(), tracks[2].clone()],
            current: Some(1),
        });
        manager.flush_now().unwrap();
        let successor = build_ui_with_state(&app, manager.clone());
        settle(&main_loop);
        assert_eq!(
            successor.queue_snapshot().tracks,
            vec![tracks[0].clone(), tracks[2].clone()]
        );
        assert_eq!(successor.queue_snapshot().current, Some(1));
        assert_eq!(successor.state(), PlaybackState::Stopped);
        app.active_window().unwrap().close();
        successor
            .element()
            .set_state(gstreamer::State::Null)
            .unwrap();
        manager.update_queue(playback_queue::QueueSnapshot {
            revision: 1,
            tracks: vec![missing],
            current: Some(0),
        });
        manager.flush_now().unwrap();
        let unavailable = build_ui_with_state(&app, manager);
        settle(&main_loop);
        assert!(unavailable.queue_snapshot().tracks.is_empty());
        assert_eq!(unavailable.queue_snapshot().current, None);
        assert!(unavailable.metadata().uri.is_empty());
        assert_eq!(unavailable.state(), PlaybackState::Stopped);
        app.active_window().unwrap().close();
    }

    #[test]
    #[ignore = "Requer sessão gráfica; testa erro de gravação usando pasta temporária"]
    fn close_error_keeps_preferences_and_retry_closes_window() {
        gtk::init().unwrap();
        let config = state::TestConfig::new();
        let blocker = config.path();
        std::fs::write(&blocker, b"not a directory").unwrap();
        let path = blocker.join("state.json");
        let manager = state::StateManager::with_path(path.clone());
        let app = Application::builder()
            .application_id("io.github.vinylplayer.savepreview")
            .flags(gtk::gio::ApplicationFlags::NON_UNIQUE)
            .build();
        app.register(None::<&gtk::gio::Cancellable>).unwrap();
        let _audio = build_ui_with_state(&app, manager.clone());
        let main_loop = glib::MainLoop::new(None, false);
        settle(&main_loop);
        let window = app.active_window().unwrap();
        manager.update_volume(0.42);
        window.close();
        assert!(
            window.is_visible(),
            "Uma falha de gravação não deve descartar a janela"
        );
        assert_eq!(manager.snapshot().volume, 0.42);
        let dialog = gtk::Window::list_toplevels()
            .into_iter()
            .find_map(|widget| widget.downcast::<adw::MessageDialog>().ok())
            .expect("Aviso de erro");
        dialog.response("back");
        std::fs::remove_file(&blocker).unwrap();
        window.close();
        assert!(!window.is_visible());
        let restored = state::StateManager::with_path(path).snapshot();
        assert_eq!(restored.volume, 0.42);
    }

    #[test]
    #[ignore = "Requer sessão gráfica; testa alternância 33/45 RPM na interface e persistência"]
    fn rpm_speed_toggles_and_persists() {
        gtk::init().unwrap();
        let config = state::TestConfig::new();
        let manager = state::StateManager::with_path(config.path());
        assert_eq!(manager.snapshot().rpm, 33);

        let app = Application::builder()
            .application_id("io.github.vinylplayer.rpmtest")
            .flags(gtk::gio::ApplicationFlags::NON_UNIQUE)
            .build();
        app.register(None::<&gtk::gio::Cancellable>).unwrap();
        let _audio = build_ui_with_state(&app, manager.clone());
        let main_loop = glib::MainLoop::new(None, false);
        settle(&main_loop);

        let window = app.active_window().unwrap();
        let rpm_buttons = descendants(window.upcast_ref(), |w| w.has_css_class("rpm-button"));
        assert_eq!(rpm_buttons.len(), 2);
        let btn_33 = rpm_buttons[0].downcast_ref::<gtk::ToggleButton>().unwrap();
        let btn_45 = rpm_buttons[1].downcast_ref::<gtk::ToggleButton>().unwrap();

        assert!(btn_33.is_active());
        assert!(!btn_45.is_active());

        // Alterna para 45 RPM
        btn_45.set_active(true);
        settle(&main_loop);

        assert!(!btn_33.is_active());
        assert!(btn_45.is_active());
        assert_eq!(manager.snapshot().rpm, 45);

        // Desmarcar não desativa
        btn_45.set_active(false);
        settle(&main_loop);
        assert!(btn_45.is_active());

        // Alterna de volta para 33 RPM
        btn_33.set_active(true);
        settle(&main_loop);

        assert!(btn_33.is_active());
        assert!(!btn_45.is_active());
        assert_eq!(manager.snapshot().rpm, 33);

        window.close();
        settle(&main_loop);
    }

    #[test]
    #[ignore = "Requer sessão gráfica; testa botões de shuffle e repeat na interface e persistência"]
    fn shuffle_and_repeat_buttons_toggle_and_persist() {
        gtk::init().unwrap();
        let config = state::TestConfig::new();
        let manager = state::StateManager::with_path(config.path());
        assert!(!manager.snapshot().shuffle);
        assert_eq!(manager.snapshot().repeat_mode, RepeatMode::Off);

        let app = Application::builder()
            .application_id("io.github.vinylplayer.shuffletest")
            .flags(gtk::gio::ApplicationFlags::NON_UNIQUE)
            .build();
        app.register(None::<&gtk::gio::Cancellable>).unwrap();
        let audio = build_ui_with_state(&app, manager.clone());
        let main_loop = glib::MainLoop::new(None, false);
        settle(&main_loop);

        let window = app.active_window().unwrap();
        let shuffle_buttons =
            descendants(window.upcast_ref(), |w| w.has_css_class("shuffle-button"));
        assert_eq!(shuffle_buttons.len(), 1);
        let repeat_buttons = descendants(window.upcast_ref(), |w| w.has_css_class("repeat-button"));
        assert_eq!(repeat_buttons.len(), 1);

        let s_btn = shuffle_buttons[0].downcast_ref::<gtk::Button>().unwrap();
        let r_btn = repeat_buttons[0].downcast_ref::<gtk::Button>().unwrap();

        assert!(!s_btn.has_css_class("active"));
        assert!(!r_btn.has_css_class("active"));

        // Clica no botão de shuffle
        s_btn.emit_clicked();
        settle(&main_loop);
        assert!(audio.shuffle());
        assert!(s_btn.has_css_class("active"));
        assert!(manager.snapshot().shuffle);

        // Clica no botão de repeat (Off -> All)
        r_btn.emit_clicked();
        settle(&main_loop);
        assert_eq!(audio.repeat_mode(), RepeatMode::All);
        assert!(r_btn.has_css_class("active"));
        assert_eq!(manager.snapshot().repeat_mode, RepeatMode::All);

        // Clica novamente (All -> Track)
        r_btn.emit_clicked();
        settle(&main_loop);
        assert_eq!(audio.repeat_mode(), RepeatMode::Track);
        assert!(r_btn.has_css_class("active"));
        assert_eq!(manager.snapshot().repeat_mode, RepeatMode::Track);

        // Clica novamente (Track -> Off)
        r_btn.emit_clicked();
        settle(&main_loop);
        assert_eq!(audio.repeat_mode(), RepeatMode::Off);
        assert!(!r_btn.has_css_class("active"));
        assert_eq!(manager.snapshot().repeat_mode, RepeatMode::Off);

        // Desativa shuffle
        s_btn.emit_clicked();
        settle(&main_loop);
        assert!(!audio.shuffle());
        assert!(!s_btn.has_css_class("active"));
        assert!(!manager.snapshot().shuffle);

        window.close();
        settle(&main_loop);
    }

    fn settle(main_loop: &glib::MainLoop) {
        let done = main_loop.clone();
        glib::timeout_add_local_once(std::time::Duration::from_millis(500), move || done.quit());
        main_loop.run();
    }

    fn save_preview(window: &gtk::Window, filename: &str) {
        window.present();
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(8);
        loop {
            let snapshot = gtk::Snapshot::new();
            gtk::WidgetPaintable::new(Some(window)).snapshot(
                &snapshot,
                window.width() as f64,
                window.height() as f64,
            );
            if let Some(node) = snapshot.to_node() {
                let renderer = gtk::gsk::CairoRenderer::new();
                renderer.realize(None::<&gdk::Surface>).unwrap();
                renderer
                    .render_texture(&node, None)
                    .save_to_png(filename)
                    .unwrap();
                renderer.unrealize();
                return;
            }
            if std::time::Instant::now() >= deadline {
                let layout: Vec<_> = descendants(window.upcast_ref(), |w| {
                    w.is::<gtk::Stack>() || w.is::<gtk::ScrolledWindow>() || w.is::<gtk::ListBox>()
                })
                .into_iter()
                .filter(|w| w.is_mapped())
                .take(12)
                .map(|w| (w.type_().name().to_string(), w.width(), w.height()))
                .collect();
                eprintln!("Layout da captura {filename}: {layout:?}");
            }
            assert!(
                std::time::Instant::now() < deadline,
                "Janela não renderizou: {filename}; visível={}, mapeada={}, realizada={}, tamanho={}x{}",
                window.is_visible(), window.is_mapped(), window.is_realized(), window.width(), window.height()
            );
            window.queue_draw();
            let wait = glib::MainLoop::new(None, false);
            let done = wait.clone();
            glib::timeout_add_local_full(
                std::time::Duration::from_millis(50),
                glib::Priority::LOW,
                move || {
                    done.quit();
                    glib::ControlFlow::Break
                },
            );
            wait.run();
        }
    }

    fn save_enqueue_popover(menu: &gtk::MenuButton, filename: &str) {
        let popover = menu.popover().unwrap();
        let snapshot = gtk::Snapshot::new();
        gtk::WidgetPaintable::new(Some(&popover)).snapshot(
            &snapshot,
            popover.width() as f64,
            popover.height() as f64,
        );
        let node = snapshot.to_node().expect("Popover renderizado");
        let renderer = gtk::gsk::CairoRenderer::new();
        renderer.realize(None::<&gdk::Surface>).unwrap();
        renderer
            .render_texture(&node, None)
            .save_to_png(filename)
            .unwrap();
        renderer.unrealize();
    }

    #[test]
    #[ignore = "Requer sessão gráfica; gera a prévia visual em /tmp"]
    fn render_professional_preview() {
        gtk::init().unwrap();
        let app = Application::builder()
            .application_id("io.github.vinylplayer.preview")
            .flags(gtk::gio::ApplicationFlags::NON_UNIQUE)
            .build();
        app.register(None::<&gtk::gio::Cancellable>).unwrap();
        let test_config = state::TestConfig::new();
        let audio = build_ui_with_state(&app, state::StateManager::with_path(test_config.path()));
        let silent_sink = gstreamer::ElementFactory::make("fakesink")
            .property("sync", true)
            .build()
            .unwrap();
        audio.element().set_property("audio-sink", &silent_sink);
        let main_loop = glib::MainLoop::new(None, false);
        let exit_loop = main_loop.clone();
        glib::timeout_add_local_once(std::time::Duration::from_millis(700), move || {
            exit_loop.quit()
        });
        main_loop.run();
        let window = app.active_window().unwrap();
        save_preview(&window, "/tmp/vinylplayer-professional.png");
        let body = window.child().unwrap().first_child().unwrap();
        let content = body.last_child().unwrap();
        let pages = content
            .last_child()
            .unwrap()
            .downcast::<adw::OverlaySplitView>()
            .unwrap()
            .content()
            .unwrap()
            .downcast::<gtk::Stack>()
            .unwrap();
        pages.set_visible_child_name("albums");
        let album_navigation = pages
            .child_by_name("albums")
            .unwrap()
            .downcast::<gtk::Stack>()
            .unwrap();
        let album_views = album_navigation
            .child_by_name("browse")
            .unwrap()
            .last_child()
            .unwrap()
            .downcast::<gtk::Stack>()
            .unwrap();
        let indexed_deadline = std::time::Instant::now() + std::time::Duration::from_secs(120);
        while album_views.child_by_name("covers").is_none() {
            assert!(
                std::time::Instant::now() < indexed_deadline,
                "Indexação dos álbuns não concluiu"
            );
            let done = main_loop.clone();
            glib::timeout_add_local_once(std::time::Duration::from_millis(50), move || done.quit());
            main_loop.run();
        }
        for (view, filename) in [
            ("covers", "/tmp/vinylplayer-album-grid.png"),
            ("list", "/tmp/vinylplayer-album-list.png"),
        ] {
            album_views.set_visible_child_name(view);
            let exit_loop = main_loop.clone();
            glib::timeout_add_local_once(std::time::Duration::from_millis(1000), move || {
                exit_loop.quit()
            });
            main_loop.run();
            assert_eq!(album_views.visible_child_name().as_deref(), Some(view));
            save_preview(&window, filename);
        }
        album_views.set_visible_child_name("covers");
        let grid_widget = album_views
            .child_by_name("covers")
            .unwrap()
            .downcast::<gtk::ScrolledWindow>()
            .unwrap()
            .child()
            .unwrap();
        let grid = grid_widget
            .clone()
            .downcast::<gtk::FlowBox>()
            .unwrap_or_else(|_| {
                grid_widget
                    .first_child()
                    .unwrap()
                    .downcast::<gtk::FlowBox>()
                    .unwrap()
            });
        let button = grid
            .child_at_index(0)
            .expect("Biblioteca para a prévia")
            .child()
            .unwrap()
            .downcast::<gtk::Button>()
            .unwrap();
        button.emit_clicked();
        assert_eq!(
            album_navigation.visible_child_name().as_deref(),
            Some("detail")
        );
        let exit_loop = main_loop.clone();
        glib::timeout_add_local_once(std::time::Duration::from_millis(500), move || {
            exit_loop.quit()
        });
        main_loop.run();
        save_preview(&window, "/tmp/vinylplayer-album-detail.png");
        let detail = album_navigation.child_by_name("detail").unwrap();
        assert_eq!(
            audio.state(),
            PlaybackState::Stopped,
            "Abrir uma capa não inicia áudio"
        );
        let scroll = detail
            .last_child()
            .unwrap()
            .downcast::<gtk::ScrolledWindow>()
            .unwrap();
        let content = scroll.child().unwrap();
        let list = content
            .clone()
            .downcast::<gtk::ListBox>()
            .unwrap_or_else(|_| {
                content
                    .first_child()
                    .unwrap()
                    .downcast::<gtk::ListBox>()
                    .unwrap()
            });
        let first = list
            .row_at_index(0)
            .unwrap()
            .child()
            .unwrap()
            .first_child()
            .unwrap()
            .downcast::<gtk::Button>()
            .unwrap();
        let second = list
            .row_at_index(1)
            .unwrap()
            .child()
            .unwrap()
            .first_child()
            .unwrap()
            .downcast::<gtk::Button>()
            .unwrap();
        first.emit_clicked();
        settle(&main_loop);
        assert!(first.has_css_class("playback-current"));
        assert!(!second.has_css_class("playback-current"));
        assert!(button.has_css_class("playback-current"));
        save_preview(&window, "/tmp/vinylplayer-playing-track.png");
        pages.set_visible_child_name("overview");
        settle(&main_loop);
        save_preview(&window, "/tmp/vinylplayer-turntable-playing.png");
        pages.set_visible_child_name("albums");
        album_navigation.set_visible_child_name("browse");
        settle(&main_loop);
        save_preview(&window, "/tmp/vinylplayer-playing-album.png");
        album_navigation.set_visible_child_name("detail");
        audio.pause().unwrap();
        settle(&main_loop);
        assert!(first.has_css_class("playback-paused"));
        assert!(button.has_css_class("playback-paused"));
        save_preview(&window, "/tmp/vinylplayer-paused-track.png");
        audio.play().unwrap();
        settle(&main_loop);
        assert!(!first.has_css_class("playback-paused"));
        audio.next().unwrap();
        settle(&main_loop);
        assert!(!first.has_css_class("playback-current"));
        assert!(second.has_css_class("playback-current"));
        assert!(button.has_css_class("playback-current"));
        audio.stop().unwrap();
        settle(&main_loop);
        assert!(!second.has_css_class("playback-current"));
        assert!(!button.has_css_class("playback-current"));
        detail
            .first_child()
            .unwrap()
            .downcast::<gtk::Button>()
            .unwrap()
            .emit_clicked();
        assert_eq!(
            album_navigation.visible_child_name().as_deref(),
            Some("browse")
        );
        assert_eq!(album_views.visible_child_name().as_deref(), Some("covers"));
        window.close();
    }

    #[test]
    fn rotating_disc_preserves_transparency_and_display_scale() {
        for scale in [1, 2] {
            let mut surface = render_rotating_disc(120.25, 241, None, scale, 33).unwrap();
            assert_eq!(surface.width(), 245 * scale);
            assert_eq!(surface.height(), 245 * scale);
            assert_eq!(surface.device_scale(), (scale as f64, scale as f64));
            let stride = surface.stride() as usize;
            let middle = surface.width() as usize / 2;
            let data = surface.data().unwrap();
            let pixel = |x: usize, y: usize| {
                u32::from_ne_bytes(
                    data[y * stride + x * 4..y * stride + x * 4 + 4]
                        .try_into()
                        .unwrap(),
                )
            };
            assert_eq!(pixel(0, 0) >> 24, 0);
            assert_eq!(pixel(middle, middle) >> 24, 255);
        }
    }

    #[test]
    fn rotating_disc_includes_the_cover() {
        let cover = cairo::ImageSurface::create(cairo::Format::ARgb32, 240, 240).unwrap();
        let cr = cairo::Context::new(&cover).unwrap();
        cr.set_source_rgb(0.9, 0.05, 0.05);
        cr.paint().unwrap();
        drop(cr);
        let mut disc = render_rotating_disc(120.0, 240, Some(cover), 1, 33).unwrap();
        let stride = disc.stride() as usize;
        let data = disc.data().unwrap();
        let offset = 122 * stride + 122 * 4;
        let pixel = u32::from_ne_bytes(data[offset..offset + 4].try_into().unwrap());
        assert!((pixel >> 16) & 255 > (pixel >> 8) & 255);
    }

    #[test]
    fn rotating_disc_supports_45_rpm() {
        let disc = render_rotating_disc(120.0, 240, None, 1, 45).unwrap();
        assert_eq!(disc.width(), 244);
    }

    #[test]
    fn volume_hud_gauge_and_fade_calculation() {
        let mut hud = VolumeHudState::default();
        assert_eq!(hud.alpha, 0.0);
        assert!(!hud.tick_fade());

        hud.trigger(0.75);
        assert_eq!(hud.level, 0.75);
        assert_eq!(hud.alpha, 1.0);
        assert!(hud.last_active.is_some());
        assert!(hud.tick_fade());

        // Clamping do volume
        hud.trigger(1.5);
        assert_eq!(hud.level, 1.0);
        hud.trigger(-0.5);
        assert_eq!(hud.level, 0.0);
    }

    #[test]
    fn draw_volume_hud_renders_without_panic() {
        let surface = cairo::ImageSurface::create(cairo::Format::ARgb32, 200, 200).unwrap();
        let cr = cairo::Context::new(&surface).unwrap();
        // Alpha zero não deve desenhar nada
        draw_volume_hud(&cr, 100.0, 100.0, 0.5, 0.0);
        // Alpha ativo desenha o HUD analógico completo
        draw_volume_hud(&cr, 100.0, 100.0, 0.75, 1.0);
        drop(cr);
        assert_eq!(surface.width(), 200);
        assert_eq!(surface.height(), 200);
    }

    #[test]
    fn render_volume_hud_preview_to_file() {
        let mut surface = cairo::ImageSurface::create(cairo::Format::ARgb32, 580, 360).unwrap();
        let cr = cairo::Context::new(&surface).unwrap();
        let cache = Rc::new(RefCell::new(TurntableCache {
            bg_surface: None,
            disc_surface: None,
            cover: CachedCover {
                key: String::new(),
                original: None,
                surface: None,
            },
        }));
        draw_turntable(
            &cr, 580, 360, 0.45, 1.88, true, &cache, 1, 33, 0.75, 1.0, 0.65, 0.72, true, 0.0,
        );
        drop(cr);

        let stride = surface.stride() as usize;
        let data = surface.data().unwrap();
        let bytes = glib::Bytes::from(&data[..]);
        let texture = gdk::MemoryTexture::new(
            580,
            360,
            gdk::MemoryFormat::B8g8r8a8Premultiplied,
            &bytes,
            stride,
        );
        texture.save_to_png("docs/volume-hud.png").unwrap();
        assert!(std::path::Path::new("docs/volume-hud.png").exists());
    }

    #[test]
    fn vu_layout_coordinates_and_clearances() {
        let layout = TurntableLayout::compute(560.0, 340.0);
        assert_eq!(layout.vu_w, 96.0);
        assert_eq!(layout.vu_h, 35.0);

        // Deve estar contido dentro do chassi
        assert!(layout.vu_x >= layout.mx);
        assert!(layout.vu_x + layout.vu_w <= layout.mx + layout.dw);
        assert!(layout.vu_y >= layout.my);
        assert!(layout.vu_y + layout.vu_h <= layout.my + layout.dh);

        // Folga em relação à borda do prato giratório
        let dy_top = (layout.vu_y - layout.cy).abs();
        if dy_top < layout.r_platter {
            let platter_edge_top = layout.cx + (layout.r_platter.powi(2) - dy_top.powi(2)).sqrt();
            let gap = layout.vu_x - platter_edge_top;
            assert!(
                gap > 12.0,
                "folga do prato deve ser > 12px (obtido {gap:.1}px)"
            );
        }

        // Folga abaixo do descanso do braço (rest cradle)
        let arm_len = layout.arm_len;
        let rest_y = layout.pivot_y + (arm_len * 0.95) * (1.476_f64).sin();
        let gap_cradle = layout.vu_y - (rest_y + 16.0);
        assert!(
            gap_cradle > 30.0,
            "folga abaixo do descanso deve ser > 30px (obtido {gap_cradle:.1}px)"
        );
    }

    #[test]
    fn vu_meter_ballistics_attack_decay_behavior() {
        let update_needle = |cur: f64, target: f64, dt: f64| -> f64 {
            if target > cur {
                let rate = (1.0 - (-dt * 26.0_f64).exp()).min(1.0);
                cur + (target - cur) * rate
            } else {
                let rate = (1.0 - (-dt * 7.5_f64).exp()).min(1.0);
                cur + (target - cur) * rate
            }
        };

        let dt = 0.050; // 50ms
                        // Ataque: deve subir rápido em 50ms
        let rise = update_needle(0.0, 0.80, dt);
        assert!(
            rise > 0.50,
            "ataque deve subir > 50% em 50ms (obtido {rise:.2})"
        );

        // Decaimento: deve cair suavemente
        let fall = update_needle(0.80, 0.0, dt);
        assert!(
            fall > 0.40,
            "decaimento deve reter carga balística (obtido {fall:.2})"
        );
        assert!(fall < 0.80);
    }

    #[test]
    fn vu_meter_renders_all_states_without_panic() {
        let surface = cairo::ImageSurface::create(cairo::Format::ARgb32, 200, 100).unwrap();
        let cr = cairo::Context::new(&surface).unwrap();
        let layout = TurntableLayout::compute(200.0, 100.0);

        draw_vu_background(&cr, layout.vu_x, layout.vu_y, layout.vu_w, layout.vu_h);

        // Repouso (silêncio)
        draw_vu_meters(&cr, &layout, 0.0, 0.0, false);
        // Nominal (0 VU) tocando
        draw_vu_meters(&cr, &layout, 0.708, 0.708, true);
        // Pico (+3 dB VU) e Sobrecarga (> +3 dB)
        draw_vu_meters(&cr, &layout, 1.0, 1.15, true);
        drop(cr);
    }

    #[test]
    fn render_vu_meter_preview_to_file() {
        let mut surface = cairo::ImageSurface::create(cairo::Format::ARgb32, 580, 360).unwrap();
        let cr = cairo::Context::new(&surface).unwrap();
        let cache = Rc::new(RefCell::new(TurntableCache {
            bg_surface: None,
            disc_surface: None,
            cover: CachedCover {
                key: String::new(),
                original: None,
                surface: None,
            },
        }));
        // Renderiza com L em 0.62 (-1.5 dB VU) e R em 0.85 (+1.5 dB VU, agulha em pico vermelho!)
        draw_turntable(
            &cr, 580, 360, 0.85, 2.15, true, &cache, 1, 33, 0.0, 0.0, 0.62, 0.85, true, 0.0,
        );
        drop(cr);

        let stride = surface.stride() as usize;
        let data = surface.data().unwrap();
        let bytes = glib::Bytes::from(&data[..]);
        let texture = gdk::MemoryTexture::new(
            580,
            360,
            gdk::MemoryFormat::B8g8r8a8Premultiplied,
            &bytes,
            stride,
        );
        texture.save_to_png("docs/vu-meter.png").unwrap();
        assert!(std::path::Path::new("docs/vu-meter.png").exists());
    }

    #[test]
    fn analog_controls_layout_and_clearances() {
        let layout = TurntableLayout::compute(560.0, 340.0);

        // 1. Luz Alvo (Stylus Target Light)
        assert!(layout.target_light_x >= layout.mx);
        assert!(layout.target_light_x <= layout.mx + layout.dw);
        assert!(layout.target_light_y >= layout.my);
        assert!(layout.target_light_y <= layout.my + layout.dh);
        let dist_tl_center = ((layout.target_light_x - layout.cx).powi(2)
            + (layout.target_light_y - layout.cy).powi(2))
        .sqrt();
        assert!(
            dist_tl_center > layout.r_platter,
            "A torre da luz alvo deve ficar do lado de fora do prato giratório"
        );

        // 2. Prisma Estroboscópico (Strobe Prism)
        assert!(layout.strobe_x >= layout.mx);
        assert!(layout.strobe_x <= layout.mx + layout.dw);
        assert!(layout.strobe_y >= layout.my);
        assert!(layout.strobe_y <= layout.my + layout.dh);
        let dist_strobe_center = ((layout.strobe_x - layout.cx).powi(2)
            + (layout.strobe_y - layout.cy).powi(2))
        .sqrt();
        assert!(
            dist_strobe_center > layout.r_platter,
            "O prisma estroboscópico deve ficar fora do prato giratório"
        );

        // 3. Pitch Fader (Controle de Afinação)
        assert!(layout.pitch_x >= layout.mx);
        assert!(layout.pitch_x + layout.pitch_w <= layout.mx + layout.dw);
        assert!(layout.pitch_y >= layout.my);
        assert!(layout.pitch_y + layout.pitch_h <= layout.mx + layout.dh);

        // Folga acima dos medidores VU
        let gap_to_vu = layout.vu_y - (layout.pitch_y + layout.pitch_h);
        assert!(
            gap_to_vu >= 15.0,
            "Folga entre Pitch Fader e VU Meters deve ser >= 15px (obtido {gap_to_vu:.1}px)"
        );
    }

    #[test]
    fn analog_turntable_renders_all_states_without_panic() {
        let surface = cairo::ImageSurface::create(cairo::Format::ARgb32, 580, 360).unwrap();
        let cr = cairo::Context::new(&surface).unwrap();
        let cache = Rc::new(RefCell::new(TurntableCache {
            bg_surface: None,
            disc_surface: None,
            cover: CachedCover {
                key: String::new(),
                original: None,
                surface: None,
            },
        }));

        // Estado 1: Luz Alvo ativa, Pitch em +4.5% (LED Quartz desligado)
        draw_turntable(
            &cr, 580, 360, 0.45, 1.88, true, &cache, 1, 33, 0.75, 1.0, 0.65, 0.72, true, 0.045,
        );

        // Estado 2: Luz Alvo desligada, Pitch em 0.0% (LED Quartz travado e iluminado)
        draw_turntable(
            &cr, 580, 360, 0.85, 2.15, true, &cache, 1, 45, 0.0, 0.0, 0.62, 0.85, false, 0.0,
        );
        drop(cr);
    }
}

fn draw_turntable(
    cr: &gtk::cairo::Context,
    w: i32,
    h: i32,
    disc_angle: f64,
    arm_angle: f64,
    is_playing: bool,
    cache: &Rc<RefCell<TurntableCache>>,
    scale: i32,
    rpm: u32,
    volume_level: f64,
    volume_alpha: f64,
    vu_needle_l: f64,
    vu_needle_r: f64,
    target_light_on: bool,
    pitch: f64,
) {
    let layout = TurntableLayout::compute(w as f64, h as f64);

    // 1. Pintura ultrarrápida do fundo pré-renderizado (Chassi, Platter Rim, Gimbal Base, Strobe Dots)
    {
        let mut state = cache.borrow_mut();
        let bg_matches = match state.bg_surface {
            Some((bw, bh, _)) => bw == w && bh == h,
            None => false,
        };
        if !bg_matches {
            if let Some(bg) = render_background(&layout, w, h) {
                state.bg_surface = Some((w, h, bg));
            }
        }
    }

    if let Some((_, _, ref bg)) = cache.borrow().bg_surface {
        cr.set_source_surface(bg, 0.0, 0.0).ok();
        cr.paint().ok();
    }

    // 2. Pontos Estroboscópicos Dinâmicos em Rotação com Feixe Neon Vermelho
    draw_strobe_dots(cr, &layout, disc_angle, is_playing);

    // 2. LEDs Indicadores de velocidade (33 e 45 RPM)
    let led33_x = layout.led_x;
    let led33_y = layout.led_y;
    let led45_x = layout.led_x + 50.0;
    let led45_y = layout.led_y;

    let draw_led = |cr: &gtk::cairo::Context, x: f64, y: f64, active: bool| {
        if active {
            if is_playing {
                let led_glow = cairo::RadialGradient::new(x, y, 1.0, x, y, 9.0);
                led_glow.add_color_stop_rgba(0.0, 0.98, 0.75, 0.14, 0.7);
                led_glow.add_color_stop_rgba(1.0, 0.98, 0.75, 0.14, 0.0);
                cr.set_source(&led_glow).ok();
                cr.arc(x, y, 9.0, 0.0, TAU);
                cr.fill().ok();
                cr.set_source_rgb(0.98, 0.78, 0.20);
            } else {
                cr.set_source_rgb(0.55, 0.38, 0.12);
            }
        } else {
            cr.set_source_rgb(0.20, 0.18, 0.20);
        }
        cr.arc(x, y, 3.5, 0.0, TAU);
        cr.fill().ok();
    };

    draw_led(cr, led33_x, led33_y, rpm != 45);
    draw_led(cr, led45_x, led45_y, rpm == 45);

    // 3. Disco de Vinil com Capa Inteira (Picture Disc em Rotação Contínua)
    let cx = layout.cx;
    let cy = layout.cy;
    let r_disc = layout.r_disc;
    let target_dim = (2.0 * r_disc).round() as i32;

    // Obtém ou prepara superfície escalada em cache (executado 1 única vez por imagem!)
    let surface_to_draw = {
        let mut state = cache.borrow_mut();
        let mut matching = None;
        if let Some((dim, ref surf)) = state.cover.surface {
            if dim == target_dim {
                matching = Some(surf.clone());
            }
        }
        if matching.is_some() {
            matching
        } else if let Some(ref orig) = state.cover.original {
            let pw = orig.width();
            let ph = orig.height();
            let size = pw.min(ph);
            if size > 0 && target_dim > 0 {
                let sub = orig.new_subpixbuf((pw - size) / 2, (ph - size) / 2, size, size);
                if let Some(scaled) =
                    sub.scale_simple(target_dim, target_dim, gdk_pixbuf::InterpType::Hyper)
                {
                    if let Ok(surf) =
                        cairo::ImageSurface::create(cairo::Format::ARgb32, target_dim, target_dim)
                    {
                        if let Ok(surf_cr) = cairo::Context::new(&surf) {
                            surf_cr.set_source_pixbuf(&scaled, 0.0, 0.0);
                            surf_cr.paint().ok();
                            state.cover.surface = Some((target_dim, surf.clone()));
                            Some(surf)
                        } else {
                            None
                        }
                    } else {
                        None
                    }
                } else {
                    None
                }
            } else {
                None
            }
        } else {
            None
        }
    };

    // Reutiliza o disco completo: sulcos, espiral, texto, capa e vinheta.
    let rotating_disc = {
        let mut state = cache.borrow_mut();
        let matches = state
            .disc_surface
            .as_ref()
            .map(|(radius, cached_scale, cached_rpm, _)| {
                *radius == r_disc && *cached_scale == scale && *cached_rpm == rpm
            })
            .unwrap_or(false);
        if !matches {
            state.disc_surface =
                render_rotating_disc(r_disc, target_dim, surface_to_draw, scale, rpm)
                    .map(|surface| (r_disc, scale, rpm, surface));
        }
        state
            .disc_surface
            .as_ref()
            .map(|(_, _, _, surface)| surface.clone())
    };
    if let Some(surface) = rotating_disc {
        let half = surface.width() as f64 / (2.0 * scale as f64);
        cr.save().ok();
        cr.translate(cx, cy);
        cr.rotate(disc_angle);
        cr.set_source_surface(&surface, -half, -half).ok();
        cr.source().set_filter(cairo::Filter::Bilinear);
        cr.paint().ok();
        cr.restore().ok();
    }

    // 4. Detalhes de Superfície Fixos (Iluminação de Estúdio & Borda Externa)
    // Borda chanfrada externa de vinil maciço
    cr.set_source_rgba(0.0, 0.0, 0.0, 0.50);
    cr.set_line_width(2.6);
    cr.arc(cx, cy, r_disc - 1.3, 0.0, TAU);
    cr.stroke().ok();

    cr.set_source_rgba(1.0, 1.0, 1.0, 0.26);
    cr.set_line_width(1.0);
    cr.arc(cx, cy, r_disc - 0.5, 0.0, TAU);
    cr.stroke().ok();

    // Brilho Anisotrópico de Luz no Vinil (Reflexo borboleta de estúdio estático)
    let sheen_angles = [0.75, 0.75 + PI];
    for &sa in &sheen_angles {
        let sheen = cairo::RadialGradient::new(cx, cy, 14.0, cx, cy, r_disc);
        sheen.add_color_stop_rgba(0.0, 1.0, 1.0, 1.0, 0.18);
        sheen.add_color_stop_rgba(0.5, 1.0, 1.0, 1.0, 0.07);
        sheen.add_color_stop_rgba(1.0, 1.0, 1.0, 1.0, 0.0);
        cr.set_source(&sheen).ok();

        cr.new_sub_path();
        cr.arc(cx, cy, r_disc - 1.0, sa - 0.28, sa + 0.28);
        cr.line_to(cx, cy);
        cr.close_path();
        cr.fill().ok();
    }

    // Colar metálico cromado do furo central (Grommet)
    cr.set_source_rgba(0.88, 0.90, 0.96, 0.95);
    cr.set_line_width(2.0);
    cr.arc(cx, cy, 9.0, 0.0, TAU);
    cr.stroke().ok();

    // Furo central preto (Spindle hole)
    cr.set_source_rgb(0.03, 0.02, 0.04);
    cr.arc(cx, cy, 6.0, 0.0, TAU);
    cr.fill().ok();

    // Pino central usinado (Spindle pin) em latão polido dourado
    let pin_grad = cairo::RadialGradient::new(cx - 1.2, cy - 1.2, 0.5, cx, cy, 4.2);
    pin_grad.add_color_stop_rgba(0.0, 1.0, 0.96, 0.70, 1.0);
    pin_grad.add_color_stop_rgba(0.8, 0.88, 0.65, 0.22, 1.0);
    pin_grad.add_color_stop_rgba(1.0, 0.45, 0.30, 0.08, 1.0);
    cr.set_source(&pin_grad).ok();
    cr.arc(cx, cy, 4.0, 0.0, TAU);
    cr.fill().ok();

    // 5. Braço do Toca-Discos (Tonearm) de Alta Fidelidade
    let pivot_x = layout.pivot_x;
    let pivot_y = layout.pivot_y;
    let arm_len = layout.arm_len;

    // Desenho do Braço Articulado rotacionado pelo arm_angle
    cr.save().ok();
    cr.translate(pivot_x, pivot_y);
    cr.rotate(arm_angle);

    // Contrapeso Traseiro (Counterweight) no lado oposto do braço
    cr.set_source_rgba(0.05, 0.04, 0.06, 0.95);
    cr.rectangle(-32.0, -9.0, 22.0, 18.0);
    cr.fill().ok();

    // Anel de ajuste de peso graduado
    cr.set_source_rgba(0.85, 0.88, 0.95, 0.85);
    cr.set_line_width(1.0);
    cr.rectangle(-32.0, -9.0, 22.0, 18.0);
    cr.stroke().ok();

    cr.move_to(-21.0, -9.0);
    cr.line_to(-21.0, 9.0);
    cr.stroke().ok();

    // Eixo de fixação do contrapeso
    cr.set_source_rgba(0.7, 0.72, 0.8, 1.0);
    cr.rectangle(-38.0, -3.0, 8.0, 6.0);
    cr.fill().ok();

    // Tubo Metálico do Braço (Curva S clássica para redução de erro angular)
    // Sombra projetada do braço
    cr.set_source_rgba(0.0, 0.0, 0.0, 0.35);
    cr.set_line_width(4.5);
    cr.move_to(2.0, 4.0);
    cr.curve_to(
        arm_len * 0.35,
        -12.0 + 4.0,
        arm_len * 0.68,
        14.0 + 4.0,
        arm_len,
        4.0,
    );
    cr.stroke().ok();

    // Tubo de alumínio cromado
    let tube_grad = cairo::LinearGradient::new(0.0, -4.0, 0.0, 4.0);
    tube_grad.add_color_stop_rgba(0.0, 0.92, 0.93, 0.98, 1.0);
    tube_grad.add_color_stop_rgba(0.5, 0.65, 0.66, 0.75, 1.0);
    tube_grad.add_color_stop_rgba(1.0, 0.28, 0.26, 0.32, 1.0);
    cr.set_source(&tube_grad).ok();
    cr.set_line_width(3.6);
    cr.move_to(0.0, 0.0);
    cr.curve_to(arm_len * 0.35, -12.0, arm_len * 0.68, 14.0, arm_len, 0.0);
    cr.stroke().ok();

    // Brilho especular central no tubo metálico
    cr.set_source_rgba(1.0, 1.0, 1.0, 0.75);
    cr.set_line_width(1.1);
    cr.move_to(0.0, 0.0);
    cr.curve_to(arm_len * 0.35, -12.0, arm_len * 0.68, 14.0, arm_len, 0.0);
    cr.stroke().ok();

    // Cápsula Fonográfica e Agulha (Headshell & Cartridge)
    cr.save().ok();
    cr.translate(arm_len, 0.0);
    cr.rotate(-0.25);

    // Corpo da cápsula (Headshell preto acetinado)
    cr.set_source_rgb(0.09, 0.08, 0.11);
    cr.rectangle(-2.0, -5.5, 20.0, 11.0);
    cr.fill().ok();

    // Detalhe dourado da cápsula (Audiophile Cartridge)
    cr.set_source_rgb(0.95, 0.72, 0.22);
    cr.rectangle(10.0, -5.5, 4.0, 11.0);
    cr.fill().ok();

    // Indicador LED de leitura na agulha
    if is_playing {
        let needle_glow = cairo::RadialGradient::new(17.0, 0.0, 0.5, 17.0, 0.0, 7.0);
        needle_glow.add_color_stop_rgba(0.0, 1.0, 0.85, 0.3, 0.9);
        needle_glow.add_color_stop_rgba(1.0, 1.0, 0.85, 0.3, 0.0);
        cr.set_source(&needle_glow).ok();
        cr.arc(17.0, 0.0, 7.0, 0.0, TAU);
        cr.fill().ok();

        cr.set_source_rgb(1.0, 0.95, 0.65);
    } else {
        cr.set_source_rgb(0.40, 0.30, 0.15);
    }
    cr.arc(17.0, 0.0, 2.0, 0.0, TAU);
    cr.fill().ok();

    cr.restore().ok();
    cr.restore().ok();

    // Tampa central do rolamento do pivô
    let cap_grad =
        cairo::RadialGradient::new(pivot_x - 1.5, pivot_y - 1.5, 0.5, pivot_x, pivot_y, 7.0);
    cap_grad.add_color_stop_rgba(0.0, 1.0, 1.0, 1.0, 0.9);
    cap_grad.add_color_stop_rgba(0.5, 0.65, 0.68, 0.75, 1.0);
    cap_grad.add_color_stop_rgba(1.0, 0.25, 0.24, 0.30, 1.0);
    cr.set_source(&cap_grad).ok();
    cr.arc(pivot_x, pivot_y, 6.5, 0.0, TAU);
    cr.fill().ok();

    // 5. Luz Alvo Retrátil da Agulha (Stylus Target Light)
    draw_target_light(cr, &layout, target_light_on, is_playing);

    // 6. Controle Analógico de Pitch Fader (±8% com Quartz Lock)
    draw_pitch_fader(cr, &layout, pitch);

    // 7. HUD Analógico de Volume (Overlay estroboscópico/vintage ao rolar o scroll)
    draw_volume_hud(cr, layout.cx, layout.cy, volume_level, volume_alpha);

    // 8. Medidores VU Analógicos Estéreo Dinâmicos (Ponteiros Balísticos Hi-Fi)
    draw_vu_meters(cr, &layout, vu_needle_l, vu_needle_r, is_playing);
}

fn draw_strobe_dots(
    cr: &gtk::cairo::Context,
    layout: &TurntableLayout,
    disc_angle: f64,
    is_playing: bool,
) {
    let cx = layout.cx;
    let cy = layout.cy;
    let r_platter = layout.r_platter;
    let sx = layout.strobe_x;
    let sy = layout.strobe_y;

    // 1. Feixe de luz estroboscópica neon vermelha emitida pelo prisma óptico
    let strobe_glow = cairo::RadialGradient::new(sx + 8.0, sy, 1.0, sx + 14.0, sy, 32.0);
    strobe_glow.add_color_stop_rgba(0.0, 1.0, 0.22, 0.12, if is_playing { 0.85 } else { 0.35 });
    strobe_glow.add_color_stop_rgba(0.4, 0.95, 0.14, 0.08, if is_playing { 0.45 } else { 0.12 });
    strobe_glow.add_color_stop_rgba(1.0, 0.85, 0.08, 0.04, 0.0);
    cr.set_source(&strobe_glow).ok();
    cr.arc(sx + 14.0, sy, 32.0, 0.0, TAU);
    cr.fill().ok();

    // 2. Duas fileiras de calibração estroboscópica usinadas na lateral chanfrada
    let rows: &[(f64, usize, f64)] = &[
        (r_platter - 2.2, 72, 1.15),
        (r_platter - 5.0, 53, 1.05),
    ];

    for &(radius, count, dot_r) in rows {
        for i in 0..count {
            let base_ang = (i as f64 * TAU / count as f64) + disc_angle;
            let px = cx + radius * base_ang.cos();
            let py = cy + radius * base_ang.sin();

            let dist_to_strobe = ((px - (sx + 8.0)).powi(2) + (py - sy).powi(2)).sqrt();
            if dist_to_strobe < 28.0 {
                let intensity = (1.0 - dist_to_strobe / 28.0).max(0.0);
                if is_playing {
                    cr.set_source_rgba(1.0, 0.25 + 0.5 * intensity, 0.20, 0.95);
                } else {
                    cr.set_source_rgba(0.95, 0.35, 0.25, 0.75);
                }
            } else {
                cr.set_source_rgba(0.82, 0.85, 0.92, 0.65);
            }
            cr.arc(px, py, dot_r, 0.0, TAU);
            cr.fill().ok();
        }
    }
}

fn draw_target_light(
    cr: &gtk::cairo::Context,
    layout: &TurntableLayout,
    active: bool,
    _is_playing: bool,
) {
    let tlx = layout.target_light_x;
    let tly = layout.target_light_y;
    let tlr = layout.target_light_r;

    let lift = if active { 4.0 } else { 0.0 };

    let body_grad = cairo::LinearGradient::new(tlx - tlr, tly, tlx + tlr, tly);
    body_grad.add_color_stop_rgba(0.0, 0.35, 0.34, 0.38, 1.0);
    body_grad.add_color_stop_rgba(0.3, 0.75, 0.74, 0.78, 1.0);
    body_grad.add_color_stop_rgba(0.7, 0.25, 0.24, 0.28, 1.0);
    body_grad.add_color_stop_rgba(1.0, 0.45, 0.44, 0.48, 1.0);
    cr.set_source(&body_grad).ok();
    cr.arc(tlx, tly - lift, tlr - 1.5, 0.0, TAU);
    cr.fill().ok();

    cr.set_source_rgba(0.85, 0.86, 0.92, 0.85);
    cr.set_line_width(0.8);
    cr.arc(tlx, tly - lift, tlr - 1.5, 0.0, TAU);
    cr.stroke().ok();

    let lens_x = tlx + 3.0;
    let lens_y = tly - lift - 2.0;

    if active {
        let beam_target_x = layout.cx + layout.r_disc * 0.68;
        let beam_target_y = layout.cy + layout.r_disc * 0.32;
        let beam_len = 190.0;

        let beam = cairo::RadialGradient::new(lens_x, lens_y, 2.0, beam_target_x, beam_target_y, beam_len);
        beam.add_color_stop_rgba(0.0, 1.0, 0.94, 0.75, 0.38);
        beam.add_color_stop_rgba(0.3, 1.0, 0.90, 0.60, 0.16);
        beam.add_color_stop_rgba(0.7, 0.95, 0.82, 0.45, 0.05);
        beam.add_color_stop_rgba(1.0, 0.90, 0.70, 0.30, 0.0);
        cr.set_source(&beam).ok();

        cr.new_sub_path();
        cr.arc(lens_x, lens_y, beam_len, -0.98, -0.32);
        cr.line_to(lens_x, lens_y);
        cr.close_path();
        cr.fill().ok();

        let lens_glow = cairo::RadialGradient::new(lens_x, lens_y, 0.5, lens_x, lens_y, 7.0);
        lens_glow.add_color_stop_rgba(0.0, 1.0, 0.98, 0.88, 1.0);
        lens_glow.add_color_stop_rgba(0.5, 1.0, 0.92, 0.55, 0.85);
        lens_glow.add_color_stop_rgba(1.0, 1.0, 0.85, 0.30, 0.0);
        cr.set_source(&lens_glow).ok();
        cr.arc(lens_x, lens_y, 7.0, 0.0, TAU);
        cr.fill().ok();

        cr.set_source_rgb(1.0, 0.98, 0.85);
        cr.arc(lens_x, lens_y, 2.2, 0.0, TAU);
        cr.fill().ok();
    } else {
        cr.set_source_rgba(0.25, 0.18, 0.10, 0.90);
        cr.arc(lens_x, lens_y, 2.2, 0.0, TAU);
        cr.fill().ok();
    }
}

fn draw_pitch_fader(cr: &gtk::cairo::Context, layout: &TurntableLayout, pitch: f64) {
    let px = layout.pitch_x;
    let py = layout.pitch_y;
    let pw = layout.pitch_w;
    let ph = layout.pitch_h;
    let center_y = py + ph / 2.0;
    let slot_x = px + pw * 0.58;
    let slot_h = ph - 28.0;

    let led_x = px + 6.0;
    let led_y = center_y;
    let is_locked = pitch.abs() < 0.003;

    if is_locked {
        let led_glow = cairo::RadialGradient::new(led_x, led_y, 0.5, led_x, led_y, 6.5);
        led_glow.add_color_stop_rgba(0.0, 0.30, 1.0, 0.55, 0.95);
        led_glow.add_color_stop_rgba(0.5, 0.15, 0.90, 0.35, 0.50);
        led_glow.add_color_stop_rgba(1.0, 0.10, 0.80, 0.25, 0.0);
        cr.set_source(&led_glow).ok();
        cr.arc(led_x, led_y, 6.5, 0.0, TAU);
        cr.fill().ok();

        cr.set_source_rgb(0.40, 1.0, 0.65);
    } else {
        cr.set_source_rgb(0.08, 0.25, 0.12);
    }
    cr.arc(led_x, led_y, 2.2, 0.0, TAU);
    cr.fill().ok();

    let knob_y = center_y - (pitch / 0.08) * (slot_h / 2.0 - 2.0);
    let knob_w = 15.0;
    let knob_h = 10.0;
    let knob_x = slot_x - knob_w / 2.0;

    cr.set_source_rgba(0.0, 0.0, 0.0, 0.45);
    draw_rounded_rect(cr, knob_x + 1.0, knob_y - knob_h / 2.0 + 2.0, knob_w, knob_h, 1.5);
    cr.fill().ok();

    let knob_grad = cairo::LinearGradient::new(knob_x, knob_y - knob_h / 2.0, knob_x + knob_w, knob_y + knob_h / 2.0);
    knob_grad.add_color_stop_rgba(0.0, 0.85, 0.86, 0.92, 1.0);
    knob_grad.add_color_stop_rgba(0.4, 0.55, 0.54, 0.58, 1.0);
    knob_grad.add_color_stop_rgba(0.8, 0.28, 0.27, 0.32, 1.0);
    knob_grad.add_color_stop_rgba(1.0, 0.75, 0.74, 0.80, 1.0);
    cr.set_source(&knob_grad).ok();
    draw_rounded_rect(cr, knob_x, knob_y - knob_h / 2.0, knob_w, knob_h, 1.5);
    cr.fill().ok();

    cr.set_source_rgba(1.0, 1.0, 1.0, 0.35);
    cr.set_line_width(0.7);
    draw_rounded_rect(cr, knob_x, knob_y - knob_h / 2.0, knob_w, knob_h, 1.5);
    cr.stroke().ok();

    cr.set_source_rgba(0.1, 0.1, 0.12, 0.8);
    cr.set_line_width(0.6);
    cr.move_to(knob_x + 2.0, knob_y - 2.5);
    cr.line_to(knob_x + knob_w - 2.0, knob_y - 2.5);
    cr.move_to(knob_x + 2.0, knob_y + 2.5);
    cr.line_to(knob_x + knob_w - 2.0, knob_y + 2.5);
    cr.stroke().ok();

    cr.set_source_rgb(1.0, 1.0, 1.0);
    cr.set_line_width(1.3);
    cr.move_to(knob_x + 1.5, knob_y);
    cr.line_to(knob_x + knob_w - 1.5, knob_y);
    cr.stroke().ok();
}

fn draw_volume_hud(cr: &gtk::cairo::Context, cx: f64, cy: f64, level: f64, alpha: f64) {
    if alpha <= 0.005 {
        return;
    }

    cr.save().ok();

    // 1. Sombra ambiente suave / Ambient Glow em volta do HUD
    let hud_r = 68.0;
    let shadow = cairo::RadialGradient::new(cx, cy, hud_r - 4.0, cx, cy, hud_r + 16.0);
    shadow.add_color_stop_rgba(0.0, 0.0, 0.0, 0.0, 0.55 * alpha);
    shadow.add_color_stop_rgba(1.0, 0.0, 0.0, 0.0, 0.0);
    cr.set_source(&shadow).ok();
    cr.arc(cx, cy, hud_r + 16.0, 0.0, TAU);
    cr.fill().ok();

    // 2. Base de vidro escuro / Smoked Glass
    let bg = cairo::RadialGradient::new(cx - 10.0, cy - 10.0, 2.0, cx, cy, hud_r);
    bg.add_color_stop_rgba(0.0, 0.12, 0.10, 0.14, 0.92 * alpha);
    bg.add_color_stop_rgba(0.7, 0.06, 0.05, 0.08, 0.94 * alpha);
    bg.add_color_stop_rgba(1.0, 0.03, 0.02, 0.04, 0.96 * alpha);
    cr.set_source(&bg).ok();
    cr.arc(cx, cy, hud_r, 0.0, TAU);
    cr.fill_preserve().ok();

    // Borda metálica chanfrada com reflexo dourado
    let border = cairo::LinearGradient::new(cx - hud_r, cy - hud_r, cx + hud_r, cy + hud_r);
    border.add_color_stop_rgba(0.0, 0.95, 0.76, 0.22, 0.85 * alpha);
    border.add_color_stop_rgba(0.5, 0.45, 0.35, 0.15, 0.40 * alpha);
    border.add_color_stop_rgba(1.0, 0.95, 0.76, 0.22, 0.70 * alpha);
    cr.set_source(&border).ok();
    cr.set_line_width(1.8);
    cr.stroke().ok();

    // 3. Mostrador em Arco (Analog Dial Gauge de 270 graus: de 135° a 405°)
    let start_angle = 0.75 * PI;
    let total_arc = 1.5 * PI;
    let track_r = 50.0;
    let arc_width = 4.5;

    // Trilho de fundo (apagado)
    cr.set_source_rgba(0.24, 0.22, 0.28, 0.55 * alpha);
    cr.set_line_width(arc_width);
    cr.arc(cx, cy, track_r, start_angle, start_angle + total_arc);
    cr.stroke().ok();

    // Arco ativo (preenchido com gradiente âmbar/ouro)
    let fill_angle = (level.clamp(0.0, 1.0) * total_arc).max(0.001);
    let arc_grad = cairo::LinearGradient::new(cx - track_r, cy, cx + track_r, cy);
    arc_grad.add_color_stop_rgba(0.0, 0.95, 0.70, 0.15, 0.95 * alpha);
    arc_grad.add_color_stop_rgba(1.0, 1.00, 0.88, 0.35, 1.00 * alpha);
    cr.set_source(&arc_grad).ok();
    cr.set_line_width(arc_width + 0.5);
    cr.set_line_cap(cairo::LineCap::Round);
    cr.arc(cx, cy, track_r, start_angle, start_angle + fill_angle);
    cr.stroke().ok();

    // Marcador da ponta do arco (agulha / ponteiro luminoso)
    let needle_rad = start_angle + fill_angle;
    let tip_x = cx + track_r * needle_rad.cos();
    let tip_y = cy + track_r * needle_rad.sin();
    let tip_glow = cairo::RadialGradient::new(tip_x, tip_y, 0.5, tip_x, tip_y, 5.0);
    tip_glow.add_color_stop_rgba(0.0, 1.0, 0.9, 0.4, 0.95 * alpha);
    tip_glow.add_color_stop_rgba(1.0, 0.95, 0.7, 0.1, 0.0);
    cr.set_source(&tip_glow).ok();
    cr.arc(tip_x, tip_y, 5.0, 0.0, TAU);
    cr.fill().ok();

    // 4. Marcações analógicas (Notches / Ticks a cada 25% e no meio)
    for i in 0..=10 {
        let frac = i as f64 / 10.0;
        let tick_ang = start_angle + frac * total_arc;
        let is_major = i % 5 == 0;
        let inner_r = if is_major {
            track_r - 9.0
        } else {
            track_r - 6.0
        };
        let outer_r = track_r - 3.0;
        let x1 = cx + inner_r * tick_ang.cos();
        let y1 = cy + inner_r * tick_ang.sin();
        let x2 = cx + outer_r * tick_ang.cos();
        let y2 = cy + outer_r * tick_ang.sin();
        if frac <= level {
            cr.set_source_rgba(0.95, 0.80, 0.30, 0.85 * alpha);
        } else {
            cr.set_source_rgba(0.40, 0.36, 0.45, 0.40 * alpha);
        }
        cr.set_line_width(if is_major { 1.5 } else { 1.0 });
        cr.move_to(x1, y1);
        cr.line_to(x2, y2);
        cr.stroke().ok();
    }

    // 5. Tipografia Vintage Hi-Fi
    // Topo: "VOLUME" em dourado
    cr.select_font_face("Sans", cairo::FontSlant::Normal, cairo::FontWeight::Bold);
    cr.set_font_size(8.5);
    cr.set_source_rgba(0.95, 0.78, 0.25, 0.90 * alpha);
    let vol_text = "VOLUME";
    let vol_ext = cr.text_extents(vol_text).map(|e| e.width()).unwrap_or(40.0);
    cr.move_to(cx - vol_ext / 2.0, cy - 14.0);
    cr.show_text(vol_text).ok();

    // Centro: Porcentagem destacada (ex: "75%")
    cr.set_font_size(21.0);
    cr.set_source_rgba(1.0, 0.98, 0.95, 0.96 * alpha);
    let percent = (level.clamp(0.0, 1.0) * 100.0).round() as i32;
    let pct_text = format!("{percent}%");
    let pct_ext = cr
        .text_extents(&pct_text)
        .map(|e| e.width())
        .unwrap_or(44.0);
    cr.move_to(cx - pct_ext / 2.0, cy + 8.0);
    cr.show_text(&pct_text).ok();

    // Base: Decibéis / Status (ex: "-3.2 dB" ou "MUTE")
    cr.set_font_size(8.0);
    cr.select_font_face("Sans", cairo::FontSlant::Normal, cairo::FontWeight::Normal);
    let db_text = if percent == 0 {
        "MUTE".to_string()
    } else {
        let db = 20.0 * level.max(0.001).log10();
        format!("{db:+.1} dB")
    };
    cr.set_source_rgba(0.72, 0.70, 0.78, 0.80 * alpha);
    let db_ext = cr.text_extents(&db_text).map(|e| e.width()).unwrap_or(36.0);
    cr.move_to(cx - db_ext / 2.0, cy + 24.0);
    cr.show_text(&db_text).ok();

    cr.restore().ok();
}

fn draw_vu_background(cr: &gtk::cairo::Context, x: f64, y: f64, w: f64, h: f64) {
    let r = 4.0;
    // 1. Rebaixo metálico no chassi (sombra interna do recorte)
    cr.set_source_rgba(0.02, 0.02, 0.03, 0.85);
    draw_rounded_rect(cr, x - 1.0, y - 1.0, w + 2.0, h + 2.0, r + 1.0);
    cr.fill().ok();

    // 2. Borda chanfrada de alumínio escovado (bezel externo)
    let bezel = cairo::LinearGradient::new(x, y, x, y + h);
    bezel.add_color_stop_rgba(0.0, 0.35, 0.34, 0.38, 1.0);
    bezel.add_color_stop_rgba(0.5, 0.18, 0.17, 0.20, 1.0);
    bezel.add_color_stop_rgba(1.0, 0.28, 0.27, 0.30, 1.0);
    cr.set_source(&bezel).ok();
    draw_rounded_rect(cr, x, y, w, h, r);
    cr.fill().ok();

    // Friso metálico fino em volta do bezel
    cr.set_source_rgba(1.0, 1.0, 1.0, 0.12);
    cr.set_line_width(0.8);
    draw_rounded_rect(cr, x, y, w, h, r);
    cr.stroke().ok();

    // 3. Mostradores Estéreo (Canal L e Canal R)
    let dial_pad = 2.5;
    let divider_w = 2.0;
    let dial_w = (w - dial_pad * 2.0 - divider_w) / 2.0;
    let dial_h = h - dial_pad * 2.0;
    let dial_y = y + dial_pad;

    for (ch_idx, &label) in ["L", "R"].iter().enumerate() {
        let dial_x = x + dial_pad + ch_idx as f64 * (dial_w + divider_w);

        // Fundo do mostrador: tom de pergaminho vintage / âmbar escuro analógico
        let dial_bg = cairo::LinearGradient::new(dial_x, dial_y, dial_x, dial_y + dial_h);
        dial_bg.add_color_stop_rgba(0.0, 0.15, 0.14, 0.13, 1.0);
        dial_bg.add_color_stop_rgba(0.4, 0.21, 0.19, 0.16, 1.0);
        dial_bg.add_color_stop_rgba(1.0, 0.11, 0.10, 0.09, 1.0);
        cr.set_source(&dial_bg).ok();
        draw_rounded_rect(cr, dial_x, dial_y, dial_w, dial_h, 2.0);
        cr.fill().ok();

        // Sombra suave na parte superior do vidro do mostrador
        let glass_shadow = cairo::LinearGradient::new(dial_x, dial_y, dial_x, dial_y + 7.0);
        glass_shadow.add_color_stop_rgba(0.0, 0.0, 0.0, 0.0, 0.45);
        glass_shadow.add_color_stop_rgba(1.0, 0.0, 0.0, 0.0, 0.0);
        cr.set_source(&glass_shadow).ok();
        draw_rounded_rect(cr, dial_x, dial_y, dial_w, dial_h, 2.0);
        cr.fill().ok();

        // Escala em arco e divisões de decibéis
        let pivot_x = dial_x + dial_w * 0.5;
        let pivot_y = dial_y + dial_h + 8.0;
        let arc_r = dial_h + 3.0;

        let angle_start = -0.733; // -42 graus
        let angle_0vu = -0.733 + 0.7079 * 1.466; // ~+17.5 graus
        let angle_max = -0.733 + 1.466; // +42 graus

        // Trilha normal (-20 a 0 VU)
        cr.set_source_rgba(0.75, 0.70, 0.60, 0.75);
        cr.set_line_width(0.9);
        cr.arc(
            pivot_x,
            pivot_y,
            arc_r,
            -PI * 0.5 + angle_start,
            -PI * 0.5 + angle_0vu,
        );
        cr.stroke().ok();

        // Trilha de pico (0 a +3 dB VU) em vermelho vivo
        cr.set_source_rgba(0.95, 0.22, 0.18, 0.90);
        cr.set_line_width(1.3);
        cr.arc(
            pivot_x,
            pivot_y,
            arc_r,
            -PI * 0.5 + angle_0vu,
            -PI * 0.5 + angle_max,
        );
        cr.stroke().ok();

        // Marcações (ticks) principais da escala
        let ticks: &[(f64, bool)] = &[
            (-80.0, true),
            (-23.0, true),
            (-13.0, true),
            (-8.0, false),
            (-6.0, true),
            (-4.0, false),
            (-3.0, true),
            (-1.5, false),
            (-0.8, false),
            (0.0, true),
        ];

        for &(db, is_major) in ticks {
            let def = crate::audio::db_to_vu_deflection(db);
            let a = angle_start + def * 1.466;
            let tick_len = if is_major { 3.2 } else { 1.8 };
            let is_red = db >= -3.0;

            if is_red {
                cr.set_source_rgba(0.95, 0.22, 0.18, 0.90);
                cr.set_line_width(if is_major { 1.0 } else { 0.7 });
            } else {
                cr.set_source_rgba(0.80, 0.75, 0.65, 0.75);
                cr.set_line_width(if is_major { 0.8 } else { 0.5 });
            }

            let x1 = pivot_x + (arc_r - tick_len) * a.sin();
            let y1 = pivot_y - (arc_r - tick_len) * a.cos();
            let x2 = pivot_x + arc_r * a.sin();
            let y2 = pivot_y - arc_r * a.cos();
            cr.move_to(x1, y1);
            cr.line_to(x2, y2);
            cr.stroke().ok();
        }

        // Rótulo do Canal ("L" ou "R") no canto superior
        cr.select_font_face("Sans", cairo::FontSlant::Normal, cairo::FontWeight::Bold);
        cr.set_font_size(6.8);
        cr.set_source_rgba(0.90, 0.85, 0.70, 0.75);
        cr.move_to(dial_x + 3.5, dial_y + 8.5);
        cr.show_text(label).ok();

        // Rótulo "VU" no centro inferior
        cr.set_font_size(5.6);
        cr.set_source_rgba(0.80, 0.75, 0.65, 0.55);
        cr.move_to(pivot_x - 4.5, dial_y + dial_h - 2.5);
        cr.show_text("VU").ok();
    }

    // Divisor metálico central
    let div_x = x + dial_pad + dial_w;
    cr.set_source_rgba(0.08, 0.08, 0.10, 0.95);
    cr.rectangle(div_x, dial_y, divider_w, dial_h);
    cr.fill().ok();
    cr.set_source_rgba(1.0, 1.0, 1.0, 0.08);
    cr.move_to(div_x + 1.0, dial_y);
    cr.line_to(div_x + 1.0, dial_y + dial_h);
    cr.set_line_width(0.6);
    cr.stroke().ok();
}

fn draw_vu_meters(
    cr: &gtk::cairo::Context,
    layout: &TurntableLayout,
    needle_l: f64,
    needle_r: f64,
    is_playing: bool,
) {
    let x = layout.vu_x;
    let y = layout.vu_y;
    let w = layout.vu_w;
    let h = layout.vu_h;
    let dial_pad = 2.5;
    let divider_w = 2.0;
    let dial_w = (w - dial_pad * 2.0 - divider_w) / 2.0;
    let dial_h = h - dial_pad * 2.0;
    let dial_y = y + dial_pad;

    for (ch_idx, &deflection) in [needle_l, needle_r].iter().enumerate() {
        let dial_x = x + dial_pad + ch_idx as f64 * (dial_w + divider_w);
        let pivot_x = dial_x + dial_w * 0.5;
        let pivot_y = dial_y + dial_h + 8.0;
        let needle_len = dial_h + 4.5;

        // 1. Backlight dinâmico incandescente vintage
        if is_playing {
            let glow = cairo::RadialGradient::new(
                pivot_x,
                dial_y + 4.0,
                2.0,
                pivot_x,
                dial_y + 12.0,
                dial_w * 0.7,
            );
            glow.add_color_stop_rgba(0.0, 1.00, 0.88, 0.45, 0.35);
            glow.add_color_stop_rgba(0.6, 0.95, 0.70, 0.20, 0.15);
            glow.add_color_stop_rgba(1.0, 0.85, 0.50, 0.10, 0.0);
            cr.set_source(&glow).ok();
            draw_rounded_rect(cr, dial_x, dial_y, dial_w, dial_h, 2.0);
            cr.fill().ok();
        }

        // 2. Ponteiro Balístico Móvel
        let angle_start = -0.733; // -42 graus
        let a = angle_start + deflection.clamp(0.0, 1.08) * 1.466;

        let tip_x = pivot_x + needle_len * a.sin();
        let tip_y = pivot_y - needle_len * a.cos();

        // Sombra projetada da agulha no mostrador
        cr.set_source_rgba(0.0, 0.0, 0.0, 0.35);
        cr.set_line_width(0.9);
        cr.move_to(pivot_x + 0.8, pivot_y + 0.8);
        cr.line_to(tip_x + 0.8, tip_y + 0.8);
        cr.stroke().ok();

        // Corpo da agulha
        let is_overload = deflection >= 0.708; // Acima de 0 VU entra na zona vermelha
        if is_overload {
            cr.set_source_rgb(0.95, 0.20, 0.18);
        } else {
            cr.set_source_rgb(0.12, 0.11, 0.14);
        }
        cr.set_line_width(0.9);
        cr.move_to(pivot_x, pivot_y);
        cr.line_to(tip_x, tip_y);
        cr.stroke().ok();

        // Calota metálica preta/latão do pivô inferior
        cr.set_source_rgb(0.08, 0.08, 0.10);
        cr.arc(pivot_x, dial_y + dial_h - 1.0, 3.2, 0.0, TAU);
        cr.fill().ok();

        cr.set_source_rgba(0.85, 0.75, 0.35, 0.80);
        cr.arc(pivot_x, dial_y + dial_h - 1.0, 1.5, 0.0, TAU);
        cr.fill().ok();
    }
}

fn draw_rounded_rect(cr: &gtk::cairo::Context, x: f64, y: f64, w: f64, h: f64, r: f64) {
    cr.new_sub_path();
    cr.arc(x + w - r, y + r, r, -PI / 2.0, 0.0);
    cr.arc(x + w - r, y + h - r, r, 0.0, PI / 2.0);
    cr.arc(x + r, y + h - r, r, PI / 2.0, PI);
    cr.arc(x + r, y + r, r, PI, 3.0 * PI / 2.0);
    cr.close_path();
}

fn update_shuffle_btn(btn: &gtk::Button, active: bool) {
    if active {
        btn.add_css_class("active");
        btn.set_tooltip_text(Some("Shuffle · Enabled"));
    } else {
        btn.remove_css_class("active");
        btn.set_tooltip_text(Some("Shuffle · Disabled"));
    }
}

fn update_repeat_btn(btn: &gtk::Button, mode: RepeatMode) {
    match mode {
        RepeatMode::Off => {
            btn.remove_css_class("active");
            btn.set_icon_name("media-playlist-repeat-symbolic");
            btn.set_tooltip_text(Some(
                "Repeat · Off (click to repeat entire queue)",
            ));
        }
        RepeatMode::All => {
            btn.add_css_class("active");
            btn.set_icon_name("media-playlist-repeat-symbolic");
            btn.set_tooltip_text(Some(
                "Repeat · All (click to repeat current track)",
            ));
        }
        RepeatMode::Track => {
            btn.add_css_class("active");
            btn.set_icon_name("media-playlist-repeat-song-symbolic");
            btn.set_tooltip_text(Some(
                "Repeat · Track (click to turn off)",
            ));
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
) -> gtk::Widget {
    let bar = gtk::Box::new(Orientation::Horizontal, 20);
    bar.add_css_class("player-bar");

    // Lado Esquerdo: Miniatura e Metadados da Faixa
    let left_group = gtk::Box::new(Orientation::Horizontal, 12);
    left_group.set_width_request(220);

    let mini_disc = gtk::Image::from_icon_name("media-optical-symbolic");
    mini_disc.set_pixel_size(28);
    mini_disc.add_css_class("mini-disc-icon");

    let text_group = gtk::Box::new(Orientation::Vertical, 2);
    text_group.set_valign(gtk::Align::Center);
    let title_lbl = gtk::Label::new(Some("No track loaded"));
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

    left_group.append(&mini_disc);
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
    prev.set_tooltip_text(Some("Previous track · ←"));
    prev.add_css_class("control-button");
    let toggle = gtk::Button::from_icon_name("media-playback-start-symbolic");
    toggle.set_tooltip_text(Some("Play / pause · Space"));
    toggle.add_css_class("play-button");
    let next = gtk::Button::from_icon_name("media-skip-forward-symbolic");
    next.set_tooltip_text(Some("Next track · →"));
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

    let seek_audio = audio.clone();
    seek.connect_change_value(move |_, _, value| {
        let _ = seek_audio.seek_us(value as i64);
        glib::Propagation::Proceed
    });

    timeline.append(&elapsed);
    timeline.append(&seek);
    timeline.append(&total);
    center_group.append(&timeline);

    bar.append(&center_group);

    // Lado Direito: Controle de Volume, Badge de Qualidade e Saída
    let right_group = gtk::Box::new(Orientation::Horizontal, 10);
    right_group.set_width_request(210);
    right_group.set_halign(gtk::Align::End);
    right_group.set_valign(gtk::Align::Center);

    let vol_icon = gtk::Image::from_icon_name("audio-volume-high-symbolic");
    vol_icon.add_css_class("vol-icon");

    let vol = gtk::Scale::with_range(Orientation::Horizontal, 0.0, 1.0, 0.01);
    let initial_vol = state_mgr.snapshot().volume;
    vol.set_value(initial_vol);
    vol.set_width_request(100);
    vol.set_draw_value(false);
    vol.set_tooltip_text(Some("Volume"));
    vol.add_css_class("volume-scale");

    let engine_vol = audio.clone();
    let state_vol = state_mgr.clone();
    let hud_vol = volume_hud.clone();
    vol.connect_value_changed(move |s| {
        let val = s.value();
        engine_vol.set_volume(val);
        state_vol.update_volume(val);
        hud_vol.borrow_mut().trigger(val);
    });

    let quality_badge = gtk::Label::new(None);
    quality_badge.add_css_class("quality-badge");
    quality_badge.set_visible(false);

    let output_badge = gtk::Label::new(Some("LOCAL"));
    output_badge.add_css_class("output-chip");

    right_group.append(&vol_icon);
    right_group.append(&vol);
    right_group.append(&quality_badge);
    right_group.append(&output_badge);
    bar.append(&right_group);

    // Atualização em tempo real da barra de transporte
    let tick_seek = seek.clone();
    let tick_elapsed = elapsed.clone();
    let tick_total = total.clone();
    let tick_audio = audio.clone();
    let tick_vol = vol.clone();
    let tick_title = title_lbl.clone();
    let tick_artist = artist_lbl.clone();
    let tick_toggle = toggle.clone();
    let tick_playing = playing.clone();
    let tick_badge = quality_badge.clone();
    let tick_state = state_mgr.clone();
    let tick_shuffle_btn = shuffle_btn.clone();
    let tick_repeat_btn = repeat_btn.clone();
    let last_shuffle = Cell::new(audio.shuffle());
    let last_repeat = Cell::new(audio.repeat_mode());
    let last_bar_ver = Cell::new(0_u64);
    let last_queue = Cell::new(None::<(u64, Option<usize>)>);
    let queue = queue.clone();
    let weak_bar = bar.downgrade();
    let errors = errors.clone();

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
                    "Álbum Local".to_string()
                };
                tick_artist.set_text(&art);
            } else {
                tick_title.set_text("Nenhuma faixa carregada");
                tick_artist.set_text("VinylPlayer");
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
            tick_seek.set_value(p as f64);
        } else {
            tick_seek.set_sensitive(false);
            tick_seek.set_range(0.0, 1.0);
            tick_seek.set_value(0.0);
        }
        tick_elapsed.set_text(&format_time(p));
        tick_total.set_text(&format_time(d));

        glib::ControlFlow::Continue
    });

    bar.upcast()
}

fn format_time(us: i64) -> String {
    let s = (us.max(0) / 1_000_000) as u64;
    format!("{:02}:{:02}", s / 60, s % 60)
}

fn install_css() {
    let css = gtk::CssProvider::new();
    css.load_from_data(r#"
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
        .mini-disc-icon {
            color: #d5b17b; background: #292722; border: 1px solid #494033;
            border-radius: 10px; padding: 11px;
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
        .library-list { background: transparent; margin-top: 12px; }
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
    "#);

    gtk::style_context_add_provider_for_display(
        &gdk::Display::default().unwrap(),
        &css,
        gtk::STYLE_PROVIDER_PRIORITY_USER + 1,
    );
}
