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
                warnings: vec![crate::i18n::pick(
                    "Could not read folders. Try refreshing again.",
                    "Não foi possível ler as pastas. Tente atualizar novamente.",
                )
                .into()],
            });
            library_sources::CachedLibrary::save_to(&cache_path, &roots, &new_report.tracks);
            *catalog.report.borrow_mut() = new_report;
            catalog.notify();
        });
    }

    pub fn add_root(self: &Rc<Self>, path: PathBuf) -> Result<(), String> {
        if !path.is_absolute() || !path.is_dir() {
            return Err(crate::i18n::pick(
                "Please choose an available local folder.",
                "Escolha uma pasta local disponível.",
            )
            .into());
        }
        let path = std::fs::canonicalize(path).map_err(|e| e.to_string())?;
        let mut roots = self.state.snapshot().library_roots();
        if roots
            .iter()
            .any(|p| std::fs::canonicalize(p).unwrap_or_else(|_| p.clone()) == path)
        {
            return Err(crate::i18n::pick(
                "This folder is already in the library.",
                "Esta pasta já está na biblioteca.",
            )
            .into());
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
        let lang = crate::i18n::current();
        if self.scanning.get() && self.report.borrow().tracks.is_empty() {
            crate::i18n::pick_for(
                lang,
                "Reading library folders…",
                "Lendo pastas da biblioteca…",
            )
            .into()
        } else {
            let report = self.report.borrow();
            crate::i18n::summary_tracks(lang, report.tracks.len(), report.warnings.len())
        }
    }
}

pub fn settings_window(parent: &gtk::Window, catalog: &Rc<Catalog>) -> gtk::Window {
    use crate::i18n;
    let window = gtk::Window::builder()
        .title(i18n::settings_window_title(
            i18n::current(),
            env!("CARGO_PKG_VERSION"),
        ))
        .transient_for(parent)
        .destroy_with_parent(true)
        .default_width(620)
        .default_height(680)
        .build();
    {
        let win_weak = window.downgrade();
        i18n::connect(&window, move |lang| {
            if let Some(win) = win_weak.upgrade() {
                win.set_title(Some(&i18n::settings_window_title(
                    lang,
                    env!("CARGO_PKG_VERSION"),
                )));
            }
        });
    }
    {
        let key_ctrl = gtk::EventControllerKey::new();
        let win_esc = window.downgrade();
        key_ctrl.connect_key_pressed(move |_, keyval, _, _| {
            if keyval == gtk::gdk::Key::Escape {
                if let Some(win) = win_esc.upgrade() {
                    win.close();
                }
                return glib::Propagation::Stop;
            }
            glib::Propagation::Proceed
        });
        window.add_controller(key_ctrl);
    }
    let body = gtk::Box::new(gtk::Orientation::Vertical, 14);
    window.add_css_class("library-settings");
    body.add_css_class("app-shell");
    body.set_margin_top(22);
    body.set_margin_bottom(22);
    body.set_margin_start(22);
    body.set_margin_end(22);
    let title = gtk::Label::new(None);
    {
        let title_weak = title.downgrade();
        i18n::connect(&title, move |lang| {
            if let Some(lbl) = title_weak.upgrade() {
                lbl.set_text(&i18n::settings_heading(lang, env!("CARGO_PKG_VERSION")));
            }
        });
    }
    title.add_css_class("page-title");
    title.set_xalign(0.0);
    title.set_hexpand(true);
    let heading = gtk::Box::new(gtk::Orientation::Horizontal, 12);
    heading.append(&title);
    let close = gtk::Button::new();
    i18n::bind_button_label(&close, |l| i18n::pick_for(l, "Close", "Fechar"));
    close.set_widget_name("folder-close");
    close.add_css_class("folder-button");
    i18n::bind_tooltip(&close, |l| {
        i18n::pick_for(
            l,
            "Close library settings (Esc)",
            "Fechar configurações da biblioteca (Esc)",
        )
    });
    let weak_window = window.downgrade();
    close.connect_clicked(move |_| {
        if let Some(window) = weak_window.upgrade() {
            window.close();
        }
    });
    heading.append(&close);
    body.append(&heading);
    let hint = gtk::Label::new(None);
    i18n::bind_label(&hint, |l| {
        i18n::pick_for(
            l,
            "Adding or removing folders updates the library. Your files and current queue are preserved.",
            "Adicionar ou remover pastas atualiza a biblioteca. Seus arquivos e a fila atual são preservados.",
        )
    });
    hint.set_wrap(true);
    hint.set_xalign(0.0);
    hint.add_css_class("bottom-artist-title");
    body.append(&hint);
    let toolbar = gtk::Box::new(gtk::Orientation::Horizontal, 10);
    let add = gtk::Button::new();
    i18n::bind_button_label(&add, |l| i18n::pick_for(l, "Add Folder", "Adicionar Pasta"));
    add.set_widget_name("folder-add");
    add.add_css_class("open-button");
    let refresh = gtk::Button::new();
    i18n::bind_button_label(&refresh, |l| {
        i18n::pick_for(l, "Refresh Library", "Atualizar Biblioteca")
    });
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
    let empty = gtk::Label::new(None);
    i18n::bind_label(&empty, |l| {
        i18n::pick_for(l, "No folders added.", "Nenhuma pasta adicionada.")
    });
    list.set_placeholder(Some(&empty));
    let scroll = gtk::ScrolledWindow::builder()
        .vexpand(false)
        .min_content_height(100)
        .max_content_height(140)
        .hscrollbar_policy(gtk::PolicyType::Never)
        .child(&list)
        .build();
    body.append(&scroll);

    // Seção de Idioma / Language
    let lang_box = gtk::Box::new(gtk::Orientation::Vertical, 8);
    lang_box.set_margin_top(10);
    lang_box.set_margin_bottom(4);
    let lang_title = gtk::Label::new(None);
    i18n::bind_label(&lang_title, |l| i18n::pick_for(l, "Language · Idioma", "Idioma · Language"));
    lang_title.add_css_class("system-audio-title");
    lang_title.set_xalign(0.0);
    lang_box.append(&lang_title);

    let lang_row = gtk::Grid::builder()
        .column_spacing(10)
        .row_spacing(6)
        .hexpand(true)
        .build();
    let lang_buttons: Rc<RefCell<Vec<(i18n::Language, gtk::Button, gtk::Label)>>> =
        Rc::new(RefCell::new(Vec::new()));
    for (col_idx, &lang) in i18n::Language::ALL.iter().enumerate() {
        let btn = gtk::Button::new();
        btn.add_css_class("theme-btn");
        btn.set_hexpand(true);
        let row_content = gtk::Box::new(gtk::Orientation::Horizontal, 8);
        let lbl = gtk::Label::new(Some(lang.native_label()));
        lbl.set_xalign(0.0);
        lbl.set_hexpand(true);
        let check = gtk::Label::new(Some("✓"));
        check.add_css_class("theme-btn-check");
        let is_active = lang == i18n::current();
        check.set_visible(is_active);
        if is_active {
            btn.add_css_class("active");
        }
        row_content.append(&lbl);
        row_content.append(&check);
        btn.set_child(Some(&row_content));
        lang_row.attach(&btn, col_idx as i32, 0, 1, 1);
        lang_buttons.borrow_mut().push((lang, btn.clone(), check));

        let state_lang = catalog.state.clone();
        btn.connect_clicked(move |_| {
            state_lang.update_language(Some(lang.code().to_string()));
            state_lang.flush_if_dirty();
            i18n::set_current(lang);
        });
    }
    {
        let lang_btns_sync = lang_buttons.clone();
        i18n::connect(&lang_row, move |active_lang| {
            for (l, b, chk) in lang_btns_sync.borrow().iter() {
                let active = *l == active_lang;
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
    lang_box.append(&lang_row);
    body.append(&lang_box);

    // Seção de Tema de Cores / Interface Theme
    let theme_box = gtk::Box::new(gtk::Orientation::Vertical, 8);
    theme_box.set_margin_top(12);
    theme_box.set_margin_bottom(6);

    let theme_title = gtk::Label::new(None);
    i18n::bind_label(&theme_title, |l| {
        i18n::pick_for(l, "Interface Theme", "Tema da Interface")
    });
    theme_title.add_css_class("system-audio-title");
    theme_title.set_xalign(0.0);
    theme_box.append(&theme_title);

    let theme_hint = gtk::Label::new(None);
    i18n::bind_label(&theme_hint, |l| {
        i18n::pick_for(
            l,
            "Choose an analog accent palette for controls, dials, and highlights:",
            "Escolha uma paleta analógica de destaque para controles, dials e realces:",
        )
    });
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
        btn.set_tooltip_text(Some(theme.description_for(i18n::current())));
        {
            let btn_weak = btn.downgrade();
            i18n::connect(&btn, move |lang| {
                if let Some(b) = btn_weak.upgrade() {
                    b.set_tooltip_text(Some(theme.description_for(lang)));
                }
            });
        }

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

    let analog_title = gtk::Label::new(None);
    i18n::bind_label(&analog_title, |l| {
        i18n::pick_for(
            l,
            "Analog Experience & Notifications",
            "Experiência Analógica e Notificações",
        )
    });
    analog_title.add_css_class("system-audio-title");
    analog_title.set_xalign(0.0);
    analog_box.append(&analog_title);

    let chk_needle = gtk::CheckButton::with_label(i18n::pick(
        "Needle Drop acoustic effect on playback start",
        "Efeito acústico de agulha ao iniciar a reprodução",
    ));
    {
        let c = chk_needle.downgrade();
        i18n::connect(&chk_needle, move |l| {
            if let Some(b) = c.upgrade() {
                b.set_label(Some(i18n::pick_for(
                    l,
                    "Needle Drop acoustic effect on playback start",
                    "Efeito acústico de agulha ao iniciar a reprodução",
                )));
            }
        });
    }
    chk_needle.set_active(catalog.state.snapshot().needle_drop);
    let state_needle = catalog.state.clone();
    chk_needle.connect_toggled(move |btn| {
        state_needle.update_needle_drop(btn.is_active());
        state_needle.flush_if_dirty();
    });
    analog_box.append(&chk_needle);

    let chk_light = gtk::CheckButton::with_label(i18n::pick(
        "Stylus Target Light on platter",
        "Luz alvo na agulha sobre o prato",
    ));
    {
        let c = chk_light.downgrade();
        i18n::connect(&chk_light, move |l| {
            if let Some(b) = c.upgrade() {
                b.set_label(Some(i18n::pick_for(
                    l,
                    "Stylus Target Light on platter",
                    "Luz alvo na agulha sobre o prato",
                )));
            }
        });
    }
    chk_light.set_active(catalog.state.snapshot().target_light);
    let state_light = catalog.state.clone();
    chk_light.connect_toggled(move |btn| {
        state_light.update_target_light(btn.is_active());
        state_light.flush_if_dirty();
    });
    analog_box.append(&chk_light);

    let chk_notifications = gtk::CheckButton::with_label(i18n::pick(
        "Show desktop notifications on track change",
        "Mostrar notificações na área de trabalho ao trocar de faixa",
    ));
    {
        let c = chk_notifications.downgrade();
        i18n::connect(&chk_notifications, move |l| {
            if let Some(b) = c.upgrade() {
                b.set_label(Some(i18n::pick_for(
                    l,
                    "Show desktop notifications on track change",
                    "Mostrar notificações na área de trabalho ao trocar de faixa",
                )));
            }
        });
    }
    chk_notifications.set_active(catalog.state.snapshot().notifications);
    let state_notif = catalog.state.clone();
    chk_notifications.connect_toggled(move |btn| {
        state_notif.update_notifications(btn.is_active());
        state_notif.flush_if_dirty();
    });
    analog_box.append(&chk_notifications);

    {
        let state_sync = catalog.state.clone();
        let chk_needle_sync = chk_needle.downgrade();
        let chk_light_sync = chk_light.downgrade();
        let chk_notif_sync = chk_notifications.downgrade();
        window.connect_is_active_notify(move |win| {
            if !win.is_active() {
                return;
            }
            let snap = state_sync.snapshot();
            if let Some(chk) = chk_needle_sync.upgrade() {
                if chk.is_active() != snap.needle_drop {
                    chk.set_active(snap.needle_drop);
                }
            }
            if let Some(chk) = chk_light_sync.upgrade() {
                if chk.is_active() != snap.target_light {
                    chk.set_active(snap.target_light);
                }
            }
            if let Some(chk) = chk_notif_sync.upgrade() {
                if chk.is_active() != snap.notifications {
                    chk.set_active(snap.notifications);
                }
            }
        });
    }

    body.append(&analog_box);

    // Seção Last.fm Scrobbler nativo
    let lastfm_box = gtk::Box::new(gtk::Orientation::Vertical, 6);
    lastfm_box.set_margin_top(10);
    lastfm_box.set_margin_bottom(6);

    let lastfm_title = gtk::Label::new(Some("Last.fm Scrobbler"));
    lastfm_title.add_css_class("system-audio-title");
    lastfm_title.set_xalign(0.0);
    lastfm_box.append(&lastfm_title);

    let chk_lastfm = gtk::CheckButton::with_label(i18n::pick(
        "Scrobble tracks and update Now Playing on Last.fm",
        "Enviar scrobbles e atualizar \"Ouvindo agora\" no Last.fm",
    ));
    {
        let c = chk_lastfm.downgrade();
        i18n::connect(&chk_lastfm, move |l| {
            if let Some(b) = c.upgrade() {
                b.set_label(Some(i18n::pick_for(
                    l,
                    "Scrobble tracks and update Now Playing on Last.fm",
                    "Enviar scrobbles e atualizar \"Ouvindo agora\" no Last.fm",
                )));
            }
        });
    }
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
            let lang = i18n::current();
            let snap = state.snapshot();
            if let Some(user) = snap.lastfm_username.as_deref() {
                status_lbl.set_text(&match lang {
                    i18n::Language::En => format!("Connected as: {user}"),
                    i18n::Language::PtBr => format!("Conectado como: {user}"),
                });
                btn.set_label(i18n::pick_for(lang, "Disconnect", "Desconectar"));
                btn.remove_css_class("open-button");
                btn.add_css_class("folder-button");
                *token_cell.borrow_mut() = None;
            } else if token_cell.borrow().is_some() {
                status_lbl.set_text(i18n::pick_for(
                    lang,
                    "Awaiting authorization in browser…",
                    "Aguardando autorização no navegador…",
                ));
                btn.set_label(i18n::pick_for(
                    lang,
                    "Complete Connection",
                    "Concluir Conexão",
                ));
                btn.remove_css_class("folder-button");
                btn.add_css_class("open-button");
            } else {
                status_lbl.set_text(i18n::pick_for(
                    lang,
                    "Not connected to Last.fm",
                    "Não conectado ao Last.fm",
                ));
                btn.set_label(i18n::pick_for(
                    lang,
                    "Connect to Last.fm",
                    "Conectar ao Last.fm",
                ));
                btn.remove_css_class("folder-button");
                btn.add_css_class("open-button");
            }
        })
    };
    (update_lfm_view)();
    {
        let upd = update_lfm_view.clone();
        i18n::connect(&lfm_row, move |_| {
            upd();
        });
    }

    let api_expander = gtk::Expander::new(Some(i18n::pick(
        "Custom API Credentials (Optional)",
        "Credenciais de API Personalizadas (Opcional)",
    )));
    {
        let exp = api_expander.downgrade();
        i18n::connect(&api_expander, move |l| {
            if let Some(e) = exp.upgrade() {
                e.set_label(Some(i18n::pick_for(
                    l,
                    "Custom API Credentials (Optional)",
                    "Credenciais de API Personalizadas (Opcional)",
                )));
            }
        });
    }
    api_expander.set_margin_top(4);

    let weak_w = window.downgrade();
    let btn_state = catalog.state.clone();
    let btn_token = pending_token.clone();
    let btn_update = update_lfm_view.clone();
    let btn_status = lfm_status_lbl.clone();
    let btn_action = lfm_btn.clone();
    let expander_for_connect = api_expander.clone();

    lfm_btn.connect_clicked(move |_| {
        let snap = btn_state.snapshot();
        if snap.is_lastfm_authenticated() {
            btn_state.update_lastfm_credentials(None, None);
            btn_state.flush_if_dirty();
            btn_update();
            return;
        }

        if snap.lastfm_api_key().trim().is_empty() {
            btn_status.set_text(i18n::pick(
                "Configure your Last.fm API Key below first.",
                "Configure sua API Key da Last.fm abaixo primeiro.",
            ));
            expander_for_connect.set_expanded(true);
            return;
        }

        if let Some(token) = btn_token.borrow().clone() {
            if snap.lastfm_api_secret().trim().is_empty() {
                btn_status.set_text(i18n::pick(
                    "Configure your Last.fm Shared Secret below first.",
                    "Configure seu Shared Secret da Last.fm abaixo primeiro.",
                ));
                expander_for_connect.set_expanded(true);
                return;
            }

            btn_status.set_text(i18n::pick(
                "Verifying authorization…",
                "Verificando autorização…",
            ));
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
                        status_cb.set_text(&match i18n::current() {
                            i18n::Language::En => format!("Authorization pending: {err}"),
                            i18n::Language::PtBr => format!("Autorização pendente: {err}"),
                        });
                    }
                    Err(_) => {
                        status_cb.set_text(i18n::pick(
                            "Network thread error.",
                            "Erro na thread de rede.",
                        ));
                    }
                }
            });
        } else {
            btn_status.set_text(i18n::pick(
                "Requesting Last.fm authorization token…",
                "Solicitando token de autorização da Last.fm…",
            ));
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
                        status_cb.set_text(&match i18n::current() {
                            i18n::Language::En => format!("Connection error: {err}"),
                            i18n::Language::PtBr => format!("Erro de conexão: {err}"),
                        });
                    }
                    Err(_) => {
                        status_cb.set_text(i18n::pick(
                            "Network thread error.",
                            "Erro na thread de rede.",
                        ));
                    }
                }
            });
        }
    });

    let api_box = gtk::Box::new(gtk::Orientation::Vertical, 6);
    api_box.set_margin_top(4);
    api_box.set_margin_bottom(4);

    let key_entry = gtk::Entry::new();
    {
        let ke = key_entry.downgrade();
        i18n::connect(&key_entry, move |l| {
            if let Some(e) = ke.upgrade() {
                e.set_placeholder_text(Some(i18n::pick_for(
                    l,
                    "Custom Last.fm API Key (leave empty for default)",
                    "API Key personalizada da Last.fm (vazio usa padrão)",
                )));
            }
        });
    }
    if let Some(k) = catalog.state.snapshot().lastfm_api_key.as_deref() {
        key_entry.set_text(k);
    }
    api_box.append(&key_entry);

    let sec_entry = gtk::PasswordEntry::new();
    {
        let se = sec_entry.downgrade();
        i18n::connect(&sec_entry, move |l| {
            if let Some(e) = se.upgrade() {
                e.set_placeholder_text(Some(i18n::pick_for(
                    l,
                    "Custom Shared Secret (leave empty for default)",
                    "Shared Secret personalizado (vazio usa padrão)",
                )));
            }
        });
    }
    if let Some(s) = catalog.state.snapshot().lastfm_api_secret.as_deref() {
        sec_entry.set_text(s);
    }
    api_box.append(&sec_entry);

    let save_api_btn = gtk::Button::new();
    i18n::bind_button_label(&save_api_btn, |l| {
        i18n::pick_for(l, "Save API Keys", "Salvar Chaves de API")
    });
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
            i18n::bind_tooltip(&remove, |l| {
                i18n::pick_for(
                    l,
                    "Remove folder from library",
                    "Remover pasta da biblioteca",
                )
            });
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
    {
        let weak_cat_i18n = Rc::downgrade(catalog);
        let weak_status_i18n = status.downgrade();
        i18n::connect(&status, move |_| {
            if let (Some(cat), Some(st)) = (weak_cat_i18n.upgrade(), weak_status_i18n.upgrade()) {
                st.set_text(&cat.summary());
            }
        });
    }
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
            .title(i18n::pick(
                "Add music folder",
                "Adicionar pasta de músicas",
            ))
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
                            .ok_or_else(|| {
                                i18n::pick("Choose a local folder.", "Escolha uma pasta local.")
                                    .to_string()
                            })
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
