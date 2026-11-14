//! Visão gráfica de gerenciamento de Playlists (GTK4 / Libadwaita).
use crate::audio::AudioEngine;
use crate::now_playing;
use crate::playlist::{Playlist, PlaylistManager};
use gtk::prelude::*;
use gtk::{glib, Orientation};
use std::cell::RefCell;
use std::path::PathBuf;
use std::rc::Rc;
use std::sync::Arc;

/// Cria a visualização principal de Playlists para inclusão no Stack principal da aplicação.
pub fn playlist_library_view(
    audio: &Arc<AudioEngine>,
    manager: &PlaylistManager,
) -> gtk::Widget {
    let navigation = gtk::Stack::new();
    navigation.set_hexpand(true);
    navigation.set_vexpand(true);

    let browse = playlist_browse_view(audio, manager, &navigation);
    navigation.add_named(&browse, Some("browse"));
    navigation.set_visible_child_name("browse");

    navigation.upcast()
}

/// Tela de catálogo/grade de playlists existentes.
fn playlist_browse_view(
    audio: &Arc<AudioEngine>,
    manager: &PlaylistManager,
    navigation: &gtk::Stack,
) -> gtk::Widget {
    let outer = gtk::Box::new(Orientation::Vertical, 16);
    outer.set_margin_start(28);
    outer.set_margin_end(28);
    outer.set_margin_bottom(18);

    // Barra de ferramentas superior
    let toolbar = gtk::Box::new(Orientation::Horizontal, 14);
    toolbar.set_margin_top(18);

    let heading = gtk::Label::new(Some("Playlists"));
    heading.set_xalign(0.0);
    heading.set_hexpand(true);
    heading.add_css_class("page-title");
    toolbar.append(&heading);

    // Botão Nova Playlist
    let new_btn = gtk::Button::builder()
        .icon_name("list-add-symbolic")
        .label("Nova Playlist")
        .tooltip_text("Criar uma nova lista de reprodução M3U8")
        .build();
    new_btn.add_css_class("open-button");

    // Botão Importar M3U
    let import_btn = gtk::Button::builder()
        .icon_name("document-open-symbolic")
        .label("Importar M3U")
        .tooltip_text("Importar arquivo .m3u ou .m3u8 externo")
        .build();
    import_btn.add_css_class("folder-button");

    toolbar.append(&import_btn);
    toolbar.append(&new_btn);
    outer.append(&toolbar);

    let content_area = gtk::Box::new(Orientation::Vertical, 0);
    content_area.set_vexpand(true);
    outer.append(&content_area);

    let mgr_rc = Rc::new(manager.clone());
    let audio_rc = audio.clone();
    let nav_weak = navigation.downgrade();
    let area_weak = content_area.downgrade();

    // Função de recarregamento da grade de playlists
    let reload_cards: Rc<dyn Fn()> = {
        let mgr = mgr_rc.clone();
        let audio = audio_rc.clone();
        let nav_weak = nav_weak.clone();
        let area_weak = area_weak.clone();
        let reload_cards_slot: Rc<RefCell<Option<Rc<dyn Fn()>>>> = Rc::new(RefCell::new(None));
        let slot = reload_cards_slot.clone();
        let cb = Rc::new(move || {
            let Some(area) = area_weak.upgrade() else {
                return;
            };
            while let Some(child) = area.first_child() {
                area.remove(&child);
            }

            let playlists = mgr.reload();
            if playlists.is_empty() {
                let empty_box = gtk::Box::new(Orientation::Vertical, 12);
                empty_box.set_valign(gtk::Align::Center);
                empty_box.set_halign(gtk::Align::Center);
                empty_box.set_vexpand(true);

                let icon = gtk::Image::from_icon_name("view-list-symbolic");
                icon.set_pixel_size(64);
                icon.add_css_class("album-cover-placeholder");

                let lbl = gtk::Label::new(Some("Nenhuma playlist criada ainda."));
                lbl.add_css_class("bottom-track-title");

                let sub = gtk::Label::new(Some(
                    "Clique em 'Nova Playlist' acima para criar sua primeira seleção analógica.",
                ));
                sub.add_css_class("bottom-artist-title");

                empty_box.append(&icon);
                empty_box.append(&lbl);
                empty_box.append(&sub);
                area.append(&empty_box);
            } else {
                let scroll = gtk::ScrolledWindow::new();
                scroll.set_policy(gtk::PolicyType::Never, gtk::PolicyType::Automatic);
                scroll.set_vexpand(true);

                let grid = gtk::FlowBox::new();
                grid.set_selection_mode(gtk::SelectionMode::None);
                grid.set_min_children_per_line(1);
                grid.set_max_children_per_line(4);
                grid.set_column_spacing(20);
                grid.set_row_spacing(20);
                grid.set_valign(gtk::Align::Start);
                grid.set_margin_top(12);

                if let Some(self_reload) = slot.borrow().as_ref() {
                    for pl in &playlists {
                        let card = playlist_card(&audio, pl, &mgr, &nav_weak, self_reload);
                        grid.insert(&card, -1);
                    }
                }

                scroll.set_child(Some(&grid));
                area.append(&scroll);
            }
        });
        *reload_cards_slot.borrow_mut() = Some(cb.clone());
        cb
    };

    // Atualiza automaticamente as playlists sempre que a aba for exibida
    {
        let reload = reload_cards.clone();
        outer.connect_map(move |_| {
            reload();
        });
    }

    // Ação do botão Nova Playlist
    {
        let reload = reload_cards.clone();
        let mgr = mgr_rc.clone();
        new_btn.connect_clicked(move |btn| {
            let popover = gtk::Popover::new();
            let pop_box = gtk::Box::new(Orientation::Vertical, 10);
            pop_box.set_margin_top(8);
            pop_box.set_margin_bottom(8);
            pop_box.set_margin_start(10);
            pop_box.set_margin_end(10);

            let title = gtk::Label::new(Some("Nome da Playlist"));
            title.add_css_class("system-audio-title");
            title.set_xalign(0.0);

            let entry = gtk::Entry::new();
            entry.set_placeholder_text(Some("Ex: Clássicos do Vinil"));
            entry.set_width_chars(24);

            let create_btn = gtk::Button::with_label("Criar");
            create_btn.add_css_class("open-button");

            pop_box.append(&title);
            pop_box.append(&entry);
            pop_box.append(&create_btn);
            popover.set_child(Some(&pop_box));
            popover.set_parent(btn);

            let mgr = mgr.clone();
            let reload = reload.clone();
            let pop_weak = popover.downgrade();
            let entry_clone = entry.clone();

            let do_create = Rc::new(move || {
                let text = entry_clone.text();
                let trimmed = text.trim();
                if !trimmed.is_empty() {
                    if let Ok(_created) = mgr.create_playlist(trimmed) {
                        reload();
                        if let Some(pop) = pop_weak.upgrade() {
                            pop.popdown();
                        }
                    }
                }
            });

            let do_create_click = do_create.clone();
            create_btn.connect_clicked(move |_| {
                do_create_click();
            });

            entry.connect_activate(move |_| {
                do_create();
            });

            popover.popup();
        });
    }

    // Ação do botão Importar M3U
    {
        let reload = reload_cards.clone();
        let mgr = mgr_rc.clone();
        import_btn.connect_clicked(move |_| {
            let filter = gtk::FileFilter::new();
            filter.set_name(Some("Playlists M3U (*.m3u, *.m3u8)"));
            filter.add_pattern("*.m3u");
            filter.add_pattern("*.m3u8");
            filter.add_pattern("*.M3U");
            filter.add_pattern("*.M3U8");

            let dialog = gtk::FileDialog::builder()
                .title("Importar Playlist")
                .build();
            let filters = gtk::gio::ListStore::new::<gtk::FileFilter>();
            filters.append(&filter);
            dialog.set_filters(Some(&filters));

            let mgr = mgr.clone();
            let reload = reload.clone();
            glib::MainContext::default().spawn_local(async move {
                if let Ok(file) = dialog.open_future(None::<&gtk::Window>).await {
                    if let Some(path) = file.path() {
                        if mgr.import_playlist(&path).is_ok() {
                            reload();
                        }
                    }
                }
            });
        });
    }

    reload_cards();
    outer.upcast()
}

/// Cria o cartão visual de uma playlist na grade.
fn playlist_card(
    audio: &Arc<AudioEngine>,
    playlist: &Playlist,
    manager: &Rc<PlaylistManager>,
    nav_weak: &glib::WeakRef<gtk::Stack>,
    reload_cards: &Rc<dyn Fn()>,
) -> gtk::Widget {
    let card = gtk::Box::new(Orientation::Vertical, 12);
    card.add_css_class("playlist-card");
    card.set_size_request(260, -1);
    card.set_margin_top(4);
    card.set_margin_bottom(4);
    card.set_margin_start(4);
    card.set_margin_end(4);

    let top_row = gtk::Box::new(Orientation::Horizontal, 12);

    let icon_badge = gtk::Box::new(Orientation::Vertical, 0);
    icon_badge.add_css_class("playlist-badge-icon");
    icon_badge.set_valign(gtk::Align::Center);
    let icon = gtk::Image::from_icon_name("view-list-symbolic");
    icon.set_pixel_size(24);
    icon_badge.append(&icon);

    let meta_box = gtk::Box::new(Orientation::Vertical, 4);
    meta_box.set_hexpand(true);
    meta_box.set_valign(gtk::Align::Center);

    let title_lbl = gtk::Label::new(Some(&playlist.name));
    title_lbl.add_css_class("playlist-card-title");
    title_lbl.set_xalign(0.0);
    title_lbl.set_ellipsize(gtk::pango::EllipsizeMode::End);

    let count_str = format!("{} faixa(s)", playlist.tracks.len());
    let count_lbl = gtk::Label::new(Some(&count_str));
    count_lbl.add_css_class("playlist-card-count");
    count_lbl.set_xalign(0.0);

    meta_box.append(&title_lbl);
    meta_box.append(&count_lbl);

    top_row.append(&icon_badge);
    top_row.append(&meta_box);
    card.append(&top_row);

    // Botões de ação
    let btn_row = gtk::Box::new(Orientation::Horizontal, 8);
    btn_row.set_margin_top(6);

    let open_btn = gtk::Button::with_label("Abrir");
    open_btn.add_css_class("folder-button");
    open_btn.set_hexpand(true);

    let play_btn = gtk::Button::from_icon_name("media-playback-start-symbolic");
    play_btn.add_css_class("open-button");
    play_btn.set_tooltip_text(Some("Reproduzir Playlist"));

    let del_btn = gtk::Button::from_icon_name("user-trash-symbolic");
    del_btn.add_css_class("playlist-del-btn");
    del_btn.set_tooltip_text(Some("Excluir Playlist"));

    btn_row.append(&open_btn);
    btn_row.append(&play_btn);
    btn_row.append(&del_btn);
    card.append(&btn_row);

    // Ação Reproduzir Playlist
    {
        let engine = audio.clone();
        let file_path = playlist.file_path.clone();
        let initial_tracks = playlist.tracks.clone();
        play_btn.connect_clicked(move |_| {
            let tracks = Playlist::load_from_file(&file_path)
                .map(|p| p.tracks)
                .unwrap_or_else(|_| initial_tracks.clone());
            if !tracks.is_empty() {
                let _ = engine.play_album(&tracks, 0);
            }
        });
    }

    // Ação Abrir Detalhe da Playlist
    {
        let audio = audio.clone();
        let file_path = playlist.file_path.clone();
        let pl_initial = playlist.clone();
        let mgr = manager.clone();
        let nav_weak = nav_weak.clone();
        let reload = reload_cards.clone();
        open_btn.connect_clicked(move |_| {
            if let Some(nav) = nav_weak.upgrade() {
                if let Some(prev) = nav.child_by_name("detail") {
                    nav.remove(&prev);
                }
                // Recarrega sempre do disco para nunca abrir estado obsoleto
                let pl = Playlist::load_from_file(&file_path).unwrap_or_else(|_| pl_initial.clone());
                let detail = playlist_detail_view(&audio, &pl, &mgr, &nav, &reload);
                nav.add_named(&detail, Some("detail"));
                nav.set_visible_child_name("detail");
            }
        });
    }

    // Ação Excluir Playlist
    {
        let mgr = manager.clone();
        let file_path = playlist.file_path.clone();
        let reload = reload_cards.clone();
        del_btn.connect_clicked(move |_| {
            if mgr.delete_playlist(&file_path).is_ok() {
                reload();
            }
        });
    }

    card.upcast()
}

/// Tela de visualização detalhada de uma playlist selecionada.
fn playlist_detail_view(
    audio: &Arc<AudioEngine>,
    playlist: &Playlist,
    manager: &Rc<PlaylistManager>,
    navigation: &gtk::Stack,
    reload_cards: &Rc<dyn Fn()>,
) -> gtk::Widget {
    let page = gtk::Box::new(Orientation::Vertical, 18);
    page.set_margin_start(28);
    page.set_margin_end(28);
    page.set_margin_top(20);
    page.set_margin_bottom(18);

    // Botão Voltar às Playlists
    let back = gtk::Button::builder()
        .icon_name("go-previous-symbolic")
        .label("Voltar às Playlists")
        .build();
    back.add_css_class("nav-button");
    back.set_halign(gtk::Align::Start);
    let nav = navigation.downgrade();
    let reload = reload_cards.clone();
    back.connect_clicked(move |_| {
        reload();
        if let Some(nav) = nav.upgrade() {
            nav.set_visible_child_name("browse");
        }
    });
    page.append(&back);

    // Cabeçalho da Playlist
    let header = gtk::Box::new(Orientation::Horizontal, 20);

    let icon_box = gtk::Box::new(Orientation::Vertical, 0);
    icon_box.set_size_request(96, 96);
    icon_box.set_valign(gtk::Align::Center);
    icon_box.add_css_class("playlist-detail-badge");

    let icon = gtk::Image::from_icon_name("view-list-symbolic");
    icon.set_pixel_size(44);
    icon.set_vexpand(true);
    icon.set_hexpand(true);
    icon.set_halign(gtk::Align::Center);
    icon.set_valign(gtk::Align::Center);
    icon_box.append(&icon);
    header.append(&icon_box);

    let info = gtk::Box::new(Orientation::Vertical, 8);
    info.set_hexpand(true);
    info.set_valign(gtk::Align::Center);

    let title = gtk::Label::new(Some(&playlist.name));
    title.set_xalign(0.0);
    title.set_wrap(true);
    title.add_css_class("page-title");
    info.append(&title);

    let count_lbl = gtk::Label::new(Some(&format!(
        "{} faixa(s) · Arquivo M3U8",
        playlist.tracks.len()
    )));
    count_lbl.set_xalign(0.0);
    count_lbl.add_css_class("bottom-artist-title");
    info.append(&count_lbl);

    let action_row = gtk::Box::new(Orientation::Horizontal, 10);
    action_row.set_margin_top(6);

    let play_btn = gtk::Button::builder()
        .icon_name("media-playback-start-symbolic")
        .label("Reproduzir Playlist")
        .build();
    play_btn.add_css_class("open-button");

    let add_tracks_btn = gtk::Button::builder()
        .icon_name("list-add-symbolic")
        .label("Adicionar Faixas...")
        .build();
    add_tracks_btn.add_css_class("folder-button");

    let export_btn = gtk::Button::builder()
        .icon_name("document-save-symbolic")
        .label("Exportar...")
        .build();
    export_btn.add_css_class("folder-button");

    action_row.append(&play_btn);
    action_row.append(&add_tracks_btn);
    action_row.append(&export_btn);
    info.append(&action_row);
    header.append(&info);
    page.append(&header);

    // Lista de Faixas da Playlist
    let list_scroll = gtk::ScrolledWindow::new();
    list_scroll.set_policy(gtk::PolicyType::Never, gtk::PolicyType::Automatic);
    list_scroll.set_vexpand(true);

    let list = gtk::ListBox::new();
    list.set_selection_mode(gtk::SelectionMode::None);
    list.add_css_class("library-list");

    let playlist_state = Rc::new(RefCell::new(playlist.clone()));
    let list_weak = list.downgrade();
    let count_weak = count_lbl.downgrade();

    // Ação Reproduzir Playlist (lê sempre as faixas atualizadas em tempo real)
    {
        let engine = audio.clone();
        let pl_state = playlist_state.clone();
        play_btn.connect_clicked(move |_| {
            let tracks = pl_state.borrow().tracks.clone();
            if !tracks.is_empty() {
                let _ = engine.play_album(&tracks, 0);
            }
        });
    }

    // Função de renderização das faixas da playlist
    let render_rows = Rc::new({
        let audio = audio.clone();
        let pl_state = playlist_state.clone();
        let list_weak = list_weak.clone();
        let count_weak = count_weak.clone();
        let mgr = manager.clone();
        move || {
            let Some(list) = list_weak.upgrade() else { return };
            while let Some(child) = list.first_child() {
                list.remove(&child);
            }
            let pl = pl_state.borrow();
            if let Some(count_lbl) = count_weak.upgrade() {
                count_lbl.set_text(&format!("{} faixa(s) · Arquivo M3U8", pl.tracks.len()));
            }

            if pl.tracks.is_empty() {
                let empty = gtk::Label::new(Some(
                    "Esta playlist está vazia. Clique em 'Adicionar Faixas...' acima.",
                ));
                empty.add_css_class("bottom-artist-title");
                empty.set_margin_top(30);
                list.append(&empty);
                return;
            }

            let all_tracks = Rc::new(pl.tracks.clone());
            for (idx, path) in pl.tracks.iter().cloned().enumerate() {
                let row = playlist_track_row(&audio, path, idx, all_tracks.clone(), &pl_state, &list_weak, &count_weak, &mgr);
                list.append(&row);
            }
        }
    });

    // Ação Adicionar Faixas
    {
        let pl_state = playlist_state.clone();
        let render = render_rows.clone();
        let mgr = manager.clone();
        add_tracks_btn.connect_clicked(move |_| {
            let filter = gtk::FileFilter::new();
            filter.set_name(Some("Arquivos de Áudio (*.flac, *.mp3, *.wav, *.ogg, *.m4a)"));
            filter.add_pattern("*.flac");
            filter.add_pattern("*.mp3");
            filter.add_pattern("*.wav");
            filter.add_pattern("*.ogg");
            filter.add_pattern("*.m4a");
            filter.add_pattern("*.opus");
            filter.add_pattern("*.aac");

            let dialog = gtk::FileDialog::builder()
                .title("Adicionar Músicas à Playlist")
                .build();
            let filters = gtk::gio::ListStore::new::<gtk::FileFilter>();
            filters.append(&filter);
            dialog.set_filters(Some(&filters));

            let pl_state = pl_state.clone();
            let render = render.clone();
            let mgr = mgr.clone();
            glib::MainContext::default().spawn_local(async move {
                if let Ok(files) = dialog.open_multiple_future(None::<&gtk::Window>).await {
                    let mut added = false;
                    for i in 0..files.n_items() {
                        if let Some(file) = files.item(i).and_then(|obj| obj.downcast::<gtk::gio::File>().ok()) {
                            if let Some(path) = file.path() {
                                pl_state.borrow_mut().add_track(path);
                                added = true;
                            }
                        }
                    }
                    if added {
                        let _ = pl_state.borrow().save_to_file();
                        mgr.reload();
                        render();
                    }
                }
            });
        });
    }

    // Ação Exportar Playlist
    {
        let pl_state = playlist_state.clone();
        export_btn.connect_clicked(move |_| {
            let pl_name = pl_state.borrow().name.clone();
            let default_name = format!("{}.m3u8", PlaylistManager::sanitize_filename(&pl_name));
            let dialog = gtk::FileDialog::builder()
                .title("Exportar Playlist (.m3u8)")
                .initial_name(&default_name)
                .build();

            let pl_state = pl_state.clone();
            glib::MainContext::default().spawn_local(async move {
                if let Ok(file) = dialog.save_future(None::<&gtk::Window>).await {
                    if let Some(path) = file.path() {
                        let pl = pl_state.borrow();
                        let target_pl = Playlist {
                            name: pl.name.clone(),
                            file_path: path.clone(),
                            tracks: pl.tracks.clone(),
                        };
                        match target_pl.save_to_file() {
                            Ok(()) => {
                                let msg = format!("Playlist exportada com sucesso em: {}", path.display());
                                crate::show_system_notification("Exportação Concluída", &msg);
                            }
                            Err(e) => {
                                let msg = format!("Erro ao exportar playlist: {e}");
                                crate::show_system_notification("Falha na Exportação", &msg);
                            }
                        }
                    }
                }
            });
        });
    }

    render_rows();
    list_scroll.set_child(Some(&list));
    page.append(&list_scroll);
    page.upcast()
}

/// Cria a linha de uma faixa dentro da tela de detalhes da playlist.
fn playlist_track_row(
    audio: &Arc<AudioEngine>,
    path: PathBuf,
    idx: usize,
    queue: Rc<Vec<PathBuf>>,
    pl_state: &Rc<RefCell<Playlist>>,
    list_weak: &glib::WeakRef<gtk::ListBox>,
    count_weak: &glib::WeakRef<gtk::Label>,
    manager: &Rc<PlaylistManager>,
) -> gtk::Widget {
    let row = gtk::ListBoxRow::new();
    row.add_css_class("track-row-item");

    let button = gtk::Button::new();
    button.set_hexpand(true);
    button.set_halign(gtk::Align::Fill);
    button.add_css_class("track-item-button");

    let title = path.file_stem().and_then(|s| s.to_str()).unwrap_or("Faixa");
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
    name_label.add_css_class("track-row-name");

    let badge = gtk::Label::new(Some(&format));
    badge.add_css_class("track-format-tag");

    row_box.append(&note);
    row_box.append(&name_label);
    row_box.append(&badge);
    button.set_child(Some(&row_box));

    // Ação Clicar para Tocar a partir desta faixa na playlist
    {
        let engine = audio.clone();
        let q = queue.clone();
        button.connect_clicked(move |_| {
            let _ = engine.play_album(&q, idx);
        });
    }

    // Botões de Mover e Remover
    let actions = gtk::Box::new(Orientation::Horizontal, 4);
    actions.append(&button);

    let up_btn = gtk::Button::from_icon_name("go-up-symbolic");
    up_btn.add_css_class("playlist-track-action-btn");
    up_btn.set_tooltip_text(Some("Mover para cima"));
    up_btn.set_sensitive(idx > 0);

    let down_btn = gtk::Button::from_icon_name("go-down-symbolic");
    down_btn.add_css_class("playlist-track-action-btn");
    down_btn.set_tooltip_text(Some("Mover para baixo"));
    down_btn.set_sensitive(idx + 1 < queue.len());

    let del_btn = gtk::Button::from_icon_name("edit-delete-symbolic");
    del_btn.add_css_class("playlist-track-action-btn");
    del_btn.add_css_class("playlist-track-delete-btn");
    del_btn.set_tooltip_text(Some("Remover da Playlist"));

    actions.append(&up_btn);
    actions.append(&down_btn);
    actions.append(&del_btn);

    // Ação Mover para cima
    {
        let pl_state = pl_state.clone();
        let list_weak = list_weak.clone();
        let count_weak = count_weak.clone();
        let audio = audio.clone();
        let mgr = manager.clone();
        up_btn.connect_clicked(move |_| {
            if idx > 0 {
                pl_state.borrow_mut().move_track(idx, idx - 1);
                let _ = pl_state.borrow().save_to_file();
                mgr.reload();
                refresh_playlist_list(&audio, &pl_state, &list_weak, &count_weak, &mgr);
            }
        });
    }

    // Ação Mover para baixo
    {
        let pl_state = pl_state.clone();
        let list_weak = list_weak.clone();
        let count_weak = count_weak.clone();
        let audio = audio.clone();
        let mgr = manager.clone();
        down_btn.connect_clicked(move |_| {
            pl_state.borrow_mut().move_track(idx, idx + 1);
            let _ = pl_state.borrow().save_to_file();
            mgr.reload();
            refresh_playlist_list(&audio, &pl_state, &list_weak, &count_weak, &mgr);
        });
    }

    // Ação Remover
    {
        let pl_state = pl_state.clone();
        let list_weak = list_weak.clone();
        let count_weak = count_weak.clone();
        let audio = audio.clone();
        let mgr = manager.clone();
        del_btn.connect_clicked(move |_| {
            pl_state.borrow_mut().remove_track(idx);
            let _ = pl_state.borrow().save_to_file();
            mgr.reload();
            refresh_playlist_list(&audio, &pl_state, &list_weak, &count_weak, &mgr);
        });
    }

    row.set_child(Some(&actions));
    row.upcast()
}

fn refresh_playlist_list(
    audio: &Arc<AudioEngine>,
    pl_state: &Rc<RefCell<Playlist>>,
    list_weak: &glib::WeakRef<gtk::ListBox>,
    count_weak: &glib::WeakRef<gtk::Label>,
    manager: &Rc<PlaylistManager>,
) {
    let Some(list) = list_weak.upgrade() else { return };
    while let Some(child) = list.first_child() {
        list.remove(&child);
    }
    let pl = pl_state.borrow();
    if let Some(count_lbl) = count_weak.upgrade() {
        count_lbl.set_text(&format!("{} faixa(s) · Arquivo M3U8", pl.tracks.len()));
    }
    if pl.tracks.is_empty() {
        let empty = gtk::Label::new(Some(
            "Esta playlist está vazia. Clique em 'Adicionar Faixas...' acima.",
        ));
        empty.add_css_class("bottom-artist-title");
        empty.set_margin_top(30);
        list.append(&empty);
        return;
    }
    let all_tracks = Rc::new(pl.tracks.clone());
    for (idx, path) in pl.tracks.iter().cloned().enumerate() {
        let row = playlist_track_row(audio, path, idx, all_tracks.clone(), pl_state, list_weak, count_weak, manager);
        list.append(&row);
    }
}
