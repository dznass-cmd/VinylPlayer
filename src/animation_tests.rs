use super::*;
use crate::turntable::*;
use gstreamer::prelude::ElementExtManual;
use gtk::cairo;

#[test]
#[ignore = "Requer sessão gráfica; testa abertura inicial e reuso da janela com áudio silencioso"]
fn open_files_reuses_window_and_preserves_selection_order() {
    gtk::init().unwrap();
    let config = state::TestConfig::new();
    let [first, second, third] = silent_test_tracks(&config);
    let app = Application::builder()
        .application_id("io.github.vinylplayer.opentest")
        .flags(gtk::gio::ApplicationFlags::HANDLES_OPEN | gtk::gio::ApplicationFlags::NON_UNIQUE)
        .build();
    let builds = Rc::new(Cell::new(0));
    let captured = Rc::new(RefCell::new(None));
    let build_count = builds.clone();
    let captured_audio = captured.clone();
    let config_path = config.path();
    file_open::connect_handlers(&app, move |app| {
        build_count.set(build_count.get() + 1);
        let audio = build_ui_with_state(app, state::StateManager::with_path(config_path.clone()));
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

fn write_silent_wav(path: &std::path::Path) {
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
    std::fs::write(path, &wav).unwrap();
}

fn silent_test_tracks(config: &state::TestConfig) -> [std::path::PathBuf; 3] {
    let first = config.path().with_file_name("03 - Música #1.wav");
    let second = config.path().with_file_name("02 - Outra faixa.wav");
    let third = config.path().with_file_name("01 - Última.wav");
    for path in [&first, &second, &third] {
        write_silent_wav(path);
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
            .find(|w| w.widget_name() == "playback-error" && w.is_visible() && previous != Some(w))
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
    assert!(
        dialog.body().contains("not available") || dialog.body().contains("não está disponível")
    );
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
    assert!(first.body().contains("corrupted") || first.body().contains("danificado"));
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
    let _ = audio.play_files(std::slice::from_ref(&corrupt), 0);
    let obsolete = wait_playback_error(&main_loop, Some(&third));
    audio.play_files(&[fixtures[2].clone()], 0).unwrap();
    obsolete.response("skip");
    wait_for_audio(&audio, &main_loop, gstreamer::State::Playing);
    settle(&main_loop);
    assert_eq!(audio.queue_snapshot().tracks, vec![fixtures[2].clone()]);
    assert_eq!(audio.queue_snapshot().current, Some(0));
    let _ = audio.play_files(std::slice::from_ref(&corrupt), 0);
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
        .find(|w| {
            w.title().as_deref().is_some_and(|t| {
                t.contains("Settings") || t.contains("Pastas") || t.contains("Library Folders")
            })
        })
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
            && matches!(
                w.downcast::<gtk::Label>().unwrap().text().as_str(),
                "No folders added." | "Nenhuma pasta adicionada."
            )));
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
    let warning = descendants(settings.upcast_ref(), |w| w.has_css_class("queue-error"))[0].clone();
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
    assert!(descendants(window.upcast_ref(), |w| w.has_css_class("queue-empty"))[0].is_mapped());
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
    assert!(descendants(window.upcast_ref(), |w| w.has_css_class("queue-current"))[0].is_mapped());
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
        .args([
            "-metadata",
            "title=Horizonte Azul",
            "-metadata",
            "artist=João Silva",
            "-metadata",
            "album_artist=Vários Artistas",
            "-metadata",
            "album=Coleção Solar"
        ])
        .arg(&tagged)
        .status()
        .unwrap()
        .success());
    let app = Application::builder()
        .application_id("io.github.vinylplayer.queuesearchtest")
        .flags(gtk::gio::ApplicationFlags::NON_UNIQUE)
        .build();
    app.register(None::<&gtk::gio::Cancellable>).unwrap();
    let audio = build_ui_with_state(&app, state::StateManager::with_path(config.path()));
    let sink = gstreamer::ElementFactory::make("fakesink")
        .property("sync", true)
        .build()
        .unwrap();
    audio.element().set_property("audio-sink", &sink);
    let original = vec![
        tracks[0].clone(),
        tracks[1].clone(),
        tagged.clone(),
        tracks[0].clone(),
    ];
    audio.play_files(&original, 0).unwrap();
    let main_loop = glib::MainLoop::new(None, false);
    let window = app.active_window().unwrap();
    wait_for_audio(&audio, &main_loop, gstreamer::State::Playing);
    gtk::prelude::WidgetExt::activate_action(&window, "win.toggle-queue", None).unwrap();
    settle(&main_loop);
    let search = descendants(window.upcast_ref(), |w| w.widget_name() == "queue-search")
        .remove(0)
        .downcast::<gtk::SearchEntry>()
        .unwrap();
    let list = descendants(window.upcast_ref(), |w| w.widget_name() == "queue-list")
        .remove(0)
        .downcast::<gtk::ListView>()
        .unwrap();
    let indices = || -> Vec<String> {
        let model = list.model().unwrap();
        (0..model.n_items())
            .map(|i| {
                model
                    .item(i)
                    .unwrap()
                    .downcast::<gtk::StringObject>()
                    .unwrap()
                    .string()
                    .to_string()
            })
            .collect()
    };
    let uri = audio.metadata().uri;
    let attempt = audio.playback_attempt();
    let before = audio.position_us();
    for query in [
        "horizonte azul",
        "joao silva",
        "colecao solar",
        "varios artistas",
    ] {
        search.set_text(query);
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(12);
        loop {
            settle(&main_loop);
            if indices() == ["2"] {
                break;
            }
            assert!(
                std::time::Instant::now() < deadline,
                "Metadados não chegaram: {query}"
            );
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
    assert_eq!(
        audio.queue_snapshot().tracks,
        [
            tracks[0].clone(),
            tagged.clone(),
            tracks[1].clone(),
            tracks[0].clone()
        ]
    );
    assert_eq!(audio.metadata().uri, uri);
    let row = queue_row(&window, "Horizonte Azul");
    descendants(&row, |w| w.has_css_class("queue-track"))
        .remove(0)
        .downcast::<gtk::Button>()
        .unwrap()
        .emit_clicked();
    wait_for_audio(&audio, &main_loop, gstreamer::State::Playing);
    assert_eq!(audio.queue_snapshot().current, Some(1));
    audio.pause().unwrap();
    wait_for_audio(&audio, &main_loop, gstreamer::State::Paused);
    search.set_text("musica");
    settle(&main_loop);
    assert_eq!(indices(), ["0", "3"]);
    let rows = descendants(window.upcast_ref(), |w| {
        w.has_css_class("queue-row") && w.is_mapped()
    });
    queue_button(rows.last().unwrap(), "queue-remove").emit_clicked();
    settle(&main_loop);
    assert_eq!(
        audio.queue_snapshot().tracks,
        [tracks[0].clone(), tagged.clone(), tracks[1].clone()]
    );
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
        .remove(0)
        .downcast::<gtk::Label>()
        .unwrap();
    assert!(matches!(
        summary.text().as_str(),
        "0 of 4 tracks" | "0 de 4 faixas"
    ));
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
    let rows = descendants(window.upcast_ref(), |w| {
        w.has_css_class("queue-row") && w.is_mapped()
    });
    assert_eq!(rows.len(), 2);
    let controllers = rows[1].first_child().unwrap().observe_controllers();
    let source = (0..controllers.n_items())
        .find_map(|i| controllers.item(i)?.downcast::<gtk::DragSource>().ok())
        .unwrap();
    let provider = source
        .emit_by_name::<Option<gdk::ContentProvider>>("prepare", &[&0_f64, &0_f64])
        .unwrap();
    let payload = provider.value(String::static_type()).unwrap();
    let controllers = rows[0].observe_controllers();
    let target = (0..controllers.n_items())
        .find_map(|i| controllers.item(i)?.downcast::<gtk::DropTarget>().ok())
        .unwrap();
    assert!(target.emit_by_name::<bool>(
        "drop",
        &[&glib::BoxedValue(payload.clone()), &0_f64, &0_f64]
    ));
    settle(&main_loop);
    assert_eq!(
        audio.queue_snapshot().tracks,
        [
            tracks[0].clone(),
            tracks[0].clone(),
            tagged.clone(),
            tracks[1].clone()
        ]
    );
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
    let shuffle_buttons = descendants(window.upcast_ref(), |w| w.has_css_class("shuffle-button"));
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
    if filename.starts_with("docs/") && std::env::var_os("VINYLPLAYER_UPDATE_SCREENSHOTS").is_none()
    {
        return;
    }
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
    crate::i18n::set_current(crate::i18n::Language::En);
    let app = Application::builder()
        .application_id("io.github.vinylplayer.preview")
        .flags(gtk::gio::ApplicationFlags::NON_UNIQUE)
        .build();
    app.register(None::<&gtk::gio::Cancellable>).unwrap();
    let test_config = state::TestConfig::new();

    let music_dir = test_config.path().parent().unwrap().join("Music");
    let album_dir = music_dir.join("Demo Album");
    std::fs::create_dir_all(&album_dir).unwrap();
    for track_name in [
        "01 - First Track.wav",
        "02 - Second Track.wav",
        "03 - Third Track.wav",
    ] {
        write_silent_wav(&album_dir.join(track_name));
    }

    let default_roots = state::AppState::default_library_roots();
    let default_scanned = library_sources::scan(&default_roots);
    let library_roots = if default_scanned.tracks.len() >= 2 {
        default_roots
    } else {
        vec![music_dir]
    };

    let manager = state::StateManager::with_path(test_config.path());
    manager.update_library_folders(library_roots);
    manager.update_language(Some("en".to_string()));

    let (audio, catalog) = build_ui_with_catalog(&app, manager);
    crate::i18n::set_current(crate::i18n::Language::En);
    let silent_sink = gstreamer::ElementFactory::make("fakesink")
        .property("sync", true)
        .build()
        .unwrap();
    audio.element().set_property("audio-sink", &silent_sink);
    let main_loop = glib::MainLoop::new(None, false);
    wait_for_catalog(&catalog, &main_loop);
    settle(&main_loop);
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
        if view == "covers" {
            save_preview(&window, "docs/albums-grid.png");
        } else {
            save_preview(&window, "docs/albums-list.png");
        }
    }
    album_views.set_visible_child_name("covers");
    let covers = album_views
        .child_by_name("covers")
        .expect("Página de capas para a prévia");
    let grid = descendants(&covers, |widget| widget.is::<gtk::FlowBox>())
        .into_iter()
        .next()
        .expect("Grade de capas para a prévia")
        .downcast::<gtk::FlowBox>()
        .unwrap();
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
    save_preview(&window, "docs/album-detail.png");
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
    save_preview(&window, "docs/turntable-playing.png");
    save_preview(&window, "docs/toca-discos-rpm.png");
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

    pages.set_visible_child_name("overview");
    settle(&main_loop);

    // Captura o painel da fila de reprodução (Playback Queue)
    gtk::prelude::WidgetExt::activate_action(&window, "win.toggle-queue", None).unwrap();
    settle(&main_loop);
    save_preview(&window, "docs/playback-queue.png");
    save_preview(&window, "docs/fila-reproducao.png");
    gtk::prelude::WidgetExt::activate_action(&window, "win.toggle-queue", None).unwrap();
    settle(&main_loop);

    // Captura a página de playlists
    pages.set_visible_child_name("playlists");
    settle(&main_loop);
    save_preview(&window, "docs/playlists.png");
    pages.set_visible_child_name("overview");
    settle(&main_loop);

    // Captura a janela de configurações (Settings)
    gtk::prelude::WidgetExt::activate_action(&window, "win.library-settings", None).unwrap();
    settle(&main_loop);
    if let Some(settings_win) = gtk::Window::list_toplevels()
        .into_iter()
        .filter_map(|w| w.downcast::<gtk::Window>().ok())
        .find(|w| w != &window && w.is_visible())
    {
        save_preview(&settings_win, "docs/settings.png");
        save_preview(&settings_win, "docs/pastas-biblioteca.png");
        settings_win.close();
        settle(&main_loop);
    }

    // Captura a janela de suporte / doação (Support)
    gtk::prelude::WidgetExt::activate_action(&window, "win.donate", None).unwrap();
    settle(&main_loop);
    if let Some(donate_win) = gtk::Window::list_toplevels()
        .into_iter()
        .filter_map(|w| w.downcast::<gtk::Window>().ok())
        .find(|w| w != &window && w.is_visible())
    {
        save_preview(&donate_win, "docs/donate.png");
        donate_win.close();
        settle(&main_loop);
    }

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
    let p = TurntableRenderParams {
        w: 580,
        h: 360,
        disc_angle: 0.45,
        arm_angle: 1.88,
        is_playing: true,
        cache: &cache,
        scale: 1,
        rpm: 33,
        volume_level: 0.75,
        volume_alpha: 1.0,
        vu_needle_l: 0.65,
        vu_needle_r: 0.72,
        target_light_on: true,
        pitch: 0.0,
    };
    draw_turntable(&cr, &p);
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
    if std::env::var_os("VINYLPLAYER_UPDATE_SCREENSHOTS").is_some() {
        texture.save_to_png("docs/volume-hud.png").unwrap();
    }
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
    let p = TurntableRenderParams {
        w: 580,
        h: 360,
        disc_angle: 0.85,
        arm_angle: 2.15,
        is_playing: true,
        cache: &cache,
        scale: 1,
        rpm: 33,
        volume_level: 0.0,
        volume_alpha: 0.0,
        vu_needle_l: 0.62,
        vu_needle_r: 0.85,
        target_light_on: true,
        pitch: 0.0,
    };
    draw_turntable(&cr, &p);
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
    if std::env::var_os("VINYLPLAYER_UPDATE_SCREENSHOTS").is_some() {
        texture.save_to_png("docs/vu-meter.png").unwrap();
    }
    assert!(std::path::Path::new("docs/vu-meter.png").exists());
}
