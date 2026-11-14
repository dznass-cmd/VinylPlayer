//! Configuração de pastas e coordenação de uma única leitura em segundo plano.
use crate::{library_sources, state::StateManager};
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
        Rc::new(Self {
            state,
            report: RefCell::new(Default::default()),
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
        let weak = Rc::downgrade(self);
        glib::MainContext::default().spawn_local(async move {
            let result = gtk::gio::spawn_blocking(move || library_sources::scan(&roots)).await;
            let Some(catalog) = weak.upgrade() else {
                return;
            };
            catalog.scanning.set(false);
            // Alterações rápidas são consolidadas: só a leitura mais recente é publicada.
            if generation != catalog.generation.get() {
                catalog.launch();
                return;
            }
            *catalog.report.borrow_mut() = result.unwrap_or_else(|_| library_sources::ScanReport {
                tracks: Vec::new(),
                warnings: vec!["Could not read folders. Try refreshing again.".into()],
            });
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
        if self.scanning.get() {
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
        .title("Library Folders")
        .transient_for(parent)
        .destroy_with_parent(true)
        .default_width(580)
        .default_height(470)
        .build();
    let body = gtk::Box::new(gtk::Orientation::Vertical, 14);
    window.add_css_class("library-settings");
    body.add_css_class("app-shell");
    body.set_margin_top(22);
    body.set_margin_bottom(22);
    body.set_margin_start(22);
    body.set_margin_end(22);
    let title = gtk::Label::new(Some("Your collection, your folders"));
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
        .vexpand(true)
        .hscrollbar_policy(gtk::PolicyType::Never)
        .child(&list)
        .build();
    body.append(&scroll);

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

    let warnings = gtk::Label::new(None);
    warnings.set_wrap(true);
    warnings.set_xalign(0.0);
    warnings.add_css_class("queue-error");
    body.append(&warnings);
    window.set_child(Some(&body));
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
