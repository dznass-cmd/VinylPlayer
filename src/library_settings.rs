//! Configuração de pastas e coordenação de uma única leitura em segundo plano.
use crate::{library_sources, state::StateManager, theme::{self, Theme}};
use gtk::glib;
use gtk::prelude::*;
use std::cell::{Cell, RefCell};
use std::path::PathBuf;
use std::rc::Rc;

type Listener = (glib::WeakRef<gtk::Widget>, Rc<dyn Fn(&Catalog)>);

pub struct Catalog {
    pub state: StateManager,
    pub report: RefCell<library_sources::ScanReport>,
    pub scanning: Cell<bool>,
    generation: Cell<u64>,
    listeners: RefCell<Vec<Listener>>,
}

impl Catalog {
    pub fn new(state: StateManager) -> Rc<Self> {
        let roots = state.snapshot().library_roots();
        let cache_path = state.library_cache_path();
        let cached_tracks = library_sources::CachedLibrary::load_from(&cache_path, &roots);
        let initial_report = if let Some(tracks) = cached_tracks {
            library_sources::ScanReport {
                tracks,
                warnings: Vec::new(),
            }
        } else {
            library_sources::ScanReport::default()
        };
        Rc::new(Self {
            state,
            report: RefCell::new(initial_report),
            scanning: Cell::new(false),
            generation: Cell::new(0),
            listeners: RefCell::new(Vec::new()),
        })
    }

    pub fn connect(&self, anchor: &impl IsA<gtk::Widget>, callback: impl Fn(&Catalog) + 'static) {
        self.listeners
            .borrow_mut()
            .push((anchor.as_ref().downgrade(), Rc::new(callback)));
    }

    fn notify(&self) {
        let callbacks: Vec<_> = {
            let mut listeners = self.listeners.borrow_mut();
            listeners.retain(|(anchor, _)| anchor.upgrade().is_some());
            listeners.iter().map(|(_, cb)| cb.clone()).collect()
        };
        for callback in callbacks {
            callback(self);
        }
    }

    pub fn refresh(self: &Rc<Self>) {
        self.generation.set(self.generation.get() + 1);
        if !self.scanning.get() {
            self.launch();
        } else {
            self.notify();
        }
    }

    fn launch(self: &Rc<Self>) {
        self.scanning.set(true);
        self.notify();
        let generation = self.generation.get();
        let roots = self.state.snapshot().library_roots();
        let cache_path = self.state.library_cache_path();
        let weak = Rc::downgrade(self);
        let scan_roots = roots.clone();
        glib::MainContext::default().spawn_local(async move {
            let result = gtk::gio::spawn_blocking(move || library_sources::scan(&scan_roots)).await;
            let Some(catalog) = weak.upgrade() else {
                return;
            };
            catalog.scanning.set(false);
            // Alterações rápidas são consolidadas: só a leitura mais recente é publicada.
            if generation != catalog.generation.get() {
                catalog.launch();
                return;
            }
            let new_report = result.unwrap_or_else(|_| library_sources::ScanReport {
                tracks: Vec::new(),
                warnings: vec!["Could not read folders. Try refreshing again.".into()],
            });
            library_sources::CachedLibrary::save_to(&cache_path, &roots, &new_report.tracks);
            *catalog.report.borrow_mut() = new_report;
            catalog.notify();
        });
    }

    pub fn add_root(self: &Rc<Self>, path: PathBuf) -> Result<(), String> {
        if !path.is_absolute() || !path.is_dir() {
            return Err("Please choose an available local folder.".into());
        }
        let path = std::fs::canonicalize(path).map_err(|e| e.to_string())?;
        let mut roots = self.state.snapshot().library_roots();
        if roots
            .iter()
            .any(|p| std::fs::canonicalize(p).unwrap_or_else(|_| p.clone()) == path)
        {
            return Err("This folder is already in the library.".into());
        }
        roots.push(path);
        self.state.update_library_folders(roots);
        self.state.flush_if_dirty();
        self.refresh();
        Ok(())
    }

    pub fn remove_root(self: &Rc<Self>, path: &std::path::Path) {
        let mut roots = self.state.snapshot().library_roots();
        roots.retain(|root| root != path);
        self.state.update_library_folders(roots);
        self.state.flush_if_dirty();
        self.refresh();
    }

    pub fn summary(&self) -> String {
        if self.scanning.get() && self.report.borrow().tracks.is_empty() {
            "Reading library folders…".into()
        } else {
            let report = self.report.borrow();
            let count = report.tracks.len();
            let noun = if count == 1 { "track" } else { "tracks" };
            match report.warnings.len() {
                0 => format!("{count} {noun}"),
                1 => format!("{count} {noun} · 1 warning"),
                warnings => format!("{count} {noun} · {warnings} warnings"),
            }
        }
    }
}

pub fn settings_window(parent: &gtk::Window, catalog: &Rc<Catalog>) -> gtk::Window {
    let window = gtk::Window::builder()
        .title(format!("Settings & Library Folders (v{})", env!("CARGO_PKG_VERSION")))
        .transient_for(parent)
        .destroy_with_parent(true)
        .default_width(620)
        .default_height(640)
        .build();
    let body = gtk::Box::new(gtk::Orientation::Vertical, 14);
    window.add_css_class("library-settings");
    body.add_css_class("app-shell");
    body.set_margin_top(22);
    body.set_margin_bottom(22);
    body.set_margin_start(22);
    body.set_margin_end(22);
    let title = gtk::Label::new(Some(&format!("Your collection, your folders · v{}", env!("CARGO_PKG_VERSION"))));
    title.add_css_class("page-title");
    title.set_xalign(0.0);
    title.set_hexpand(true);
    let heading = gtk::Box::new(gtk::Orientation::Horizontal, 12);
    heading.append(&title);
    let close = gtk::Button::with_label("Close");
    close.set_widget_name("folder-close");
    close.add_css_class("folder-button");
    close.set_tooltip_text(Some("Close library settings"));
    let weak_window = window.downgrade();
    close.connect_clicked(move |_| {
        if let Some(window) = weak_window.upgrade() {
            window.close();
        }
    });
    heading.append(&close);
    body.append(&heading);
    let hint = gtk::Label::new(Some("Adding or removing folders updates the library. Your files and current queue are preserved."));
    hint.set_wrap(true);
    hint.set_xalign(0.0);
    hint.add_css_class("bottom-artist-title");
    body.append(&hint);
    let toolbar = gtk::Box::new(gtk::Orientation::Horizontal, 10);
    let add = gtk::Button::with_label("Add Folder");
    add.set_widget_name("folder-add");
    add.add_css_class("open-button");
    let refresh = gtk::Button::with_label("Refresh Library");
    refresh.add_css_class("folder-button");
    refresh.set_widget_name("folder-refresh");
    toolbar.append(&add);
    toolbar.append(&refresh);
    body.append(&toolbar);
    let status = gtk::Label::new(None);
    status.set_xalign(0.0);
    status.add_css_class("bottom-artist-title");
    body.append(&status);
    let list = gtk::ListBox::new();
    list.set_selection_mode(gtk::SelectionMode::None);
    list.add_css_class("folders-list");
    let empty = gtk::Label::new(Some("No folders added."));
    list.set_placeholder(Some(&empty));
    let scroll = gtk::ScrolledWindow::builder()
        .vexpand(false)
        .min_content_height(100)
        .max_content_height(140)
        .hscrollbar_policy(gtk::PolicyType::Never)
        .child(&list)
        .build();
    body.append(&scroll);

    // Seção de Tema de Cores / Interface Theme
    let theme_box = gtk::Box::new(gtk::Orientation::Vertical, 8);
    theme_box.set_margin_top(12);
    theme_box.set_margin_bottom(6);

    let theme_title = gtk::Label::new(Some("Interface Theme · Tema de Cores"));
    theme_title.add_css_class("system-audio-title");
    theme_title.set_xalign(0.0);
    theme_box.append(&theme_title);

    let theme_hint = gtk::Label::new(Some("Choose an analog accent palette for controls, dials, and highlights:"));
    theme_hint.set_xalign(0.0);
    theme_hint.add_css_class("bottom-artist-title");
    theme_box.append(&theme_hint);

    let theme_grid = gtk::Grid::builder()
        .column_spacing(10)
        .row_spacing(8)
        .hexpand(true)
        .build();

    let current_theme_id = catalog.state.snapshot().theme().to_string();
    let theme_buttons: Rc<RefCell<Vec<(Theme, gtk::Button, gtk::Label)>>> = Rc::new(RefCell::new(Vec::new()));

    for (idx, &theme) in Theme::ALL.iter().enumerate() {
        let btn = gtk::Button::new();
        btn.add_css_class("theme-btn");
        btn.set_hexpand(true);
        btn.set_tooltip_text(Some(theme.description()));

        let row_content = gtk::Box::new(gtk::Orientation::Horizontal, 8);

        let swatch = gtk::DrawingArea::new();
        swatch.set_content_width(16);
        swatch.set_content_height(16);
        swatch.set_valign(gtk::Align::Center);
        let (r, g, b) = theme.accent_rgb();
        swatch.set_draw_func(move |_, cr, w, h| {
            let radius = (w.min(h) as f64 / 2.0) - 1.0;
            cr.arc(w as f64 / 2.0, h as f64 / 2.0, radius, 0.0, 2.0 * std::f64::consts::PI);
            cr.set_source_rgb(r, g, b);
            let _ = cr.fill_preserve();
            cr.set_source_rgba(1.0, 1.0, 1.0, 0.25);
            cr.set_line_width(1.0);
            let _ = cr.stroke();
        });
        row_content.append(&swatch);

        let label = gtk::Label::new(Some(theme.label()));
        label.set_xalign(0.0);
        label.set_hexpand(true);
        row_content.append(&label);

        let check = gtk::Label::new(Some("✓"));
        check.add_css_class("theme-btn-check");
        let is_active = theme.id() == current_theme_id;
        check.set_visible(is_active);
        if is_active {
            btn.add_css_class("active");
        }
        row_content.append(&check);

        btn.set_child(Some(&row_content));

        let col = (idx % 3) as i32;
        let row = (idx / 3) as i32;
        theme_grid.attach(&btn, col, row, 1, 1);

        theme_buttons.borrow_mut().push((theme, btn.clone(), check));

        let state_mgr = catalog.state.clone();
        let all_btns = theme_buttons.clone();
        btn.connect_clicked(move |_| {
            theme::apply_theme(theme);
            state_mgr.update_theme(theme.id().to_string());
            state_mgr.flush_if_dirty();

            for (t, b, chk) in all_btns.borrow().iter() {
                let active = *t == theme;
                if active {
                    b.add_css_class("active");
                    chk.set_visible(true);
                } else {
                    b.remove_css_class("active");
                    chk.set_visible(false);
                }
            }
        });
    }

    theme_box.append(&theme_grid);
    body.append(&theme_box);

    let analog_box = gtk::Box::new(gtk::Orientation::Vertical, 6);
    analog_box.set_margin_top(10);
    analog_box.set_margin_bottom(6);

    let analog_title = gtk::Label::new(Some("Analog Experience"));
    analog_title.add_css_class("system-audio-title");
    analog_title.set_xalign(0.0);
    analog_box.append(&analog_title);

    let chk_needle = gtk::CheckButton::with_label("Needle Drop acoustic effect on playback start");
    chk_needle.set_active(catalog.state.snapshot().needle_drop);
    let state_needle = catalog.state.clone();
    chk_needle.connect_toggled(move |btn| {
        state_needle.update_needle_drop(btn.is_active());
        state_needle.flush_if_dirty();
    });
    analog_box.append(&chk_needle);

    let chk_light = gtk::CheckButton::with_label("Stylus Target Light on platter");
    chk_light.set_active(catalog.state.snapshot().target_light);
    let state_light = catalog.state.clone();
    chk_light.connect_toggled(move |btn| {
        state_light.update_target_light(btn.is_active());
        state_light.flush_if_dirty();
    });
    analog_box.append(&chk_light);

    body.append(&analog_box);

    // Seção Last.fm Scrobbler nativo
    let lastfm_box = gtk::Box::new(gtk::Orientation::Vertical, 6);
    lastfm_box.set_margin_top(10);
    lastfm_box.set_margin_bottom(6);

    let lastfm_title = gtk::Label::new(Some("Last.fm Scrobbler"));
    lastfm_title.add_css_class("system-audio-title");
    lastfm_title.set_xalign(0.0);
    lastfm_box.append(&lastfm_title);

    let chk_lastfm = gtk::CheckButton::with_label("Scrobble tracks and update Now Playing on Last.fm");
    chk_lastfm.set_active(catalog.state.snapshot().lastfm_enabled);
    let state_lfm = catalog.state.clone();
    chk_lastfm.connect_toggled(move |btn| {
        state_lfm.update_lastfm_enabled(btn.is_active());
        state_lfm.flush_if_dirty();
    });
    lastfm_box.append(&chk_lastfm);

    let lfm_row = gtk::Box::new(gtk::Orientation::Horizontal, 10);
    lfm_row.set_margin_top(4);
    let lfm_status_lbl = gtk::Label::new(None);
    lfm_status_lbl.set_xalign(0.0);
    lfm_status_lbl.set_hexpand(true);
    lfm_status_lbl.add_css_class("folder-path");
    let lfm_btn = gtk::Button::new();
    lfm_btn.set_valign(gtk::Align::Center);
    lfm_row.append(&lfm_status_lbl);
    lfm_row.append(&lfm_btn);
    lastfm_box.append(&lfm_row);

    let pending_token: Rc<RefCell<Option<String>>> = Rc::new(RefCell::new(None));
    let update_lfm_view = {
        let state = catalog.state.clone();
        let status_lbl = lfm_status_lbl.clone();
        let btn = lfm_btn.clone();
        let token_cell = pending_token.clone();
        Rc::new(move || {
            let snap = state.snapshot();
            if let Some(user) = snap.lastfm_username.as_deref() {
                status_lbl.set_text(&format!("Connected as: {user}"));
                btn.set_label("Disconnect");
                btn.remove_css_class("open-button");
                btn.add_css_class("folder-button");
                *token_cell.borrow_mut() = None;
            } else if token_cell.borrow().is_some() {
                status_lbl.set_text("Awaiting authorization in browser…");
                btn.set_label("Complete Connection");
                btn.remove_css_class("folder-button");
                btn.add_css_class("open-button");
            } else {
                status_lbl.set_text("Not connected to Last.fm");
                btn.set_label("Connect to Last.fm");
                btn.remove_css_class("folder-button");
                btn.add_css_class("open-button");
            }
        })
    };
    (update_lfm_view)();

    let weak_w = window.downgrade();
    let btn_state = catalog.state.clone();
    let btn_token = pending_token.clone();
    let btn_update = update_lfm_view.clone();
    let btn_status = lfm_status_lbl.clone();
    let btn_action = lfm_btn.clone();

    lfm_btn.connect_clicked(move |_| {
        let snap = btn_state.snapshot();
        if snap.is_lastfm_authenticated() {
            btn_state.update_lastfm_credentials(None, None);
            btn_state.flush_if_dirty();
            btn_update();
            return;
        }

        if let Some(token) = btn_token.borrow().clone() {
            btn_status.set_text("Verifying authorization…");
            btn_action.set_sensitive(false);
            let api_key = snap.lastfm_api_key().to_string();
            let api_secret = snap.lastfm_api_secret().to_string();
            let state_cb = btn_state.clone();
            let update_cb = btn_update.clone();
            let status_cb = btn_status.clone();
            let btn_cb = btn_action.clone();
            let token_cb = btn_token.clone();

            glib::MainContext::default().spawn_local(async move {
                let res = gtk::gio::spawn_blocking(move || {
                    crate::scrobbler::fetch_session(&api_key, &api_secret, &token)
                })
                .await;
                btn_cb.set_sensitive(true);
                match res {
                    Ok(Ok((sk, user))) => {
                        state_cb.update_lastfm_credentials(Some(sk), Some(user));
                        state_cb.flush_if_dirty();
                        *token_cb.borrow_mut() = None;
                        update_cb();
                    }
                    Ok(Err(err)) => {
                        status_cb.set_text(&format!("Authorization pending: {err}"));
                    }
                    Err(_) => {
                        status_cb.set_text("Network thread error.");
                    }
                }
            });
        } else {
            btn_status.set_text("Requesting Last.fm authorization token…");
            btn_action.set_sensitive(false);
            let api_key = snap.lastfm_api_key().to_string();
            let update_cb = btn_update.clone();
            let status_cb = btn_status.clone();
            let btn_cb = btn_action.clone();
            let token_cb = btn_token.clone();
            let weak_w_clone = weak_w.clone();

            glib::MainContext::default().spawn_local(async move {
                let key_call = api_key.clone();
                let res = gtk::gio::spawn_blocking(move || {
                    crate::scrobbler::request_token(&key_call)
                })
                .await;
                btn_cb.set_sensitive(true);
                match res {
                    Ok(Ok(token)) => {
                        let auth_url = crate::scrobbler::authorization_url(&api_key, &token);
                        if let Some(win) = weak_w_clone.upgrade() {
                            gtk::show_uri(Some(&win), &auth_url, gtk::gdk::CURRENT_TIME);
                        } else {
                            gtk::show_uri(None::<&gtk::Window>, &auth_url, gtk::gdk::CURRENT_TIME);
                        }
                        *token_cb.borrow_mut() = Some(token);
                        update_cb();
                    }
                    Ok(Err(err)) => {
                        status_cb.set_text(&format!("Connection error: {err}"));
                    }
                    Err(_) => {
                        status_cb.set_text("Network thread error.");
                    }
                }
            });
        }
    });

    let api_expander = gtk::Expander::new(Some("Custom API Credentials (Optional)"));
    api_expander.set_margin_top(4);
    let api_box = gtk::Box::new(gtk::Orientation::Vertical, 6);
    api_box.set_margin_top(4);
    api_box.set_margin_bottom(4);

    let key_entry = gtk::Entry::new();
    key_entry.set_placeholder_text(Some("Custom Last.fm API Key (leave empty for default)"));
    if let Some(k) = catalog.state.snapshot().lastfm_api_key.as_deref() {
        key_entry.set_text(k);
    }
    api_box.append(&key_entry);

    let sec_entry = gtk::PasswordEntry::new();
    sec_entry.set_placeholder_text(Some("Custom Shared Secret (leave empty for default)"));
    if let Some(s) = catalog.state.snapshot().lastfm_api_secret.as_deref() {
        sec_entry.set_text(s);
    }
    api_box.append(&sec_entry);

    let save_api_btn = gtk::Button::with_label("Save API Keys");
    save_api_btn.add_css_class("folder-button");
    save_api_btn.set_halign(gtk::Align::Start);

    let key_entry_clone = key_entry.clone();
    let sec_entry_clone = sec_entry.clone();
    let state_custom = catalog.state.clone();
    let update_lfm_after_keys = update_lfm_view.clone();
    save_api_btn.connect_clicked(move |_| {
        let k = key_entry_clone.text().trim().to_string();
        let s = sec_entry_clone.text().trim().to_string();
        let opt_k = if k.is_empty() { None } else { Some(k) };
        let opt_s = if s.is_empty() { None } else { Some(s) };
        state_custom.update_lastfm_custom_api(opt_k, opt_s);
        state_custom.flush_if_dirty();
        update_lfm_after_keys();
    });
    api_box.append(&save_api_btn);
    api_expander.set_child(Some(&api_box));
    lastfm_box.append(&api_expander);

    body.append(&lastfm_box);

    let warnings = gtk::Label::new(None);
    warnings.set_wrap(true);
    warnings.set_xalign(0.0);
    warnings.add_css_class("queue-error");
    body.append(&warnings);
    let scroll_body = gtk::ScrolledWindow::builder()
        .hscrollbar_policy(gtk::PolicyType::Never)
        .vscrollbar_policy(gtk::PolicyType::Automatic)
        .child(&body)
        .build();
    window.set_child(Some(&scroll_body));
    let weak_list = list.downgrade();
    let weak_status = status.downgrade();
    let weak_warnings = warnings.downgrade();
    let weak_catalog = Rc::downgrade(catalog);
    let render = move |catalog: &Catalog| {
        let (Some(list), Some(status), Some(warnings)) = (
            weak_list.upgrade(),
            weak_status.upgrade(),
            weak_warnings.upgrade(),
        ) else {
            return;
        };
        while let Some(row) = list.row_at_index(0) {
            list.remove(&row);
        }
        for path in catalog.state.snapshot().library_roots() {
            let row = gtk::Box::new(gtk::Orientation::Horizontal, 10);
            row.set_margin_top(8);
            row.set_margin_bottom(8);
            let labels = gtk::Box::new(gtk::Orientation::Vertical, 4);
            labels.set_hexpand(true);
            let name = gtk::Label::new(Some(
                &path.file_name().unwrap_or_default().to_string_lossy(),
            ));
            name.add_css_class("folder-name");
            name.set_xalign(0.0);
            name.set_ellipsize(gtk::pango::EllipsizeMode::End);
            labels.append(&name);
            let label = gtk::Label::new(Some(&path.to_string_lossy()));
            label.add_css_class("folder-path");
            label.set_xalign(0.0);
            label.set_hexpand(true);
            label.set_ellipsize(gtk::pango::EllipsizeMode::Middle);
            label.set_max_width_chars(45);
            label.set_tooltip_text(Some(&path.to_string_lossy()));
            labels.append(&label);
            row.append(&labels);
            let remove = gtk::Button::from_icon_name("user-trash-symbolic");
            remove.set_widget_name("folder-remove");
            remove.add_css_class("folder-button");
            remove.set_tooltip_text(Some("Remove folder from library"));
            remove.set_valign(gtk::Align::Center);
            let weak = weak_catalog.clone();
            remove.connect_clicked(move |_| {
                if let Some(catalog) = weak.upgrade() {
                    catalog.remove_root(&path);
                }
            });
            row.append(&remove);
            list.append(&row);
        }
        status.set_text(&catalog.summary());
        let report = catalog.report.borrow();
        warnings.set_text(
            &report
                .warnings
                .iter()
                .take(5)
                .cloned()
                .collect::<Vec<_>>()
                .join("\n"),
        );
        warnings.set_tooltip_text(Some(&report.warnings.join("\n")));
        warnings.set_visible(!report.warnings.is_empty());
    };
    render(catalog);
    catalog.connect(&window, render);
    let update_catalog = catalog.clone();
    refresh.connect_clicked(move |_| update_catalog.refresh());
    let weak_window = window.downgrade();
    let choose_catalog = catalog.clone();
    let weak_error = warnings.downgrade();
    add.connect_clicked(move |_| {
        let Some(window) = weak_window.upgrade() else {
            return;
        };
        let catalog = choose_catalog.clone();
        let weak_error = weak_error.clone();
        let weak_window = window.downgrade();
        gtk::FileDialog::builder()
            .title("Add music folder")
            .build()
            .select_folder(
                Some(&window),
                None::<&gtk::gio::Cancellable>,
                move |result| {
                    if weak_window.upgrade().is_none() {
                        return;
                    }
                    let result = match result {
                        Ok(file) => file
                            .path()
                            .ok_or_else(|| "Choose a local folder.".to_string())
                            .and_then(|path| catalog.add_root(path)),
                        Err(error)
                            if error.matches(gtk::DialogError::Dismissed)
                                || error.matches(gtk::DialogError::Cancelled) =>
                        {
                            return
                        }
                        Err(error) => Err(error.to_string()),
                    };
                    if let (Err(error), Some(label)) = (result, weak_error.upgrade()) {
                        label.set_text(&error);
                        label.set_visible(true);
                    }
                },
            );
    });
    window
}
