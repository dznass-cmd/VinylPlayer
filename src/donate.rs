//! Support and Donate modal window for VinylPlayer with Binance BEP20 wallet.
use gtk::glib;
use gtk::prelude::*;

pub const WALLET_ADDRESS: &str = "0xbc1ce0b7d0e5bc3332502bb82cf34125e3062b86";
const QR_BYTES: &[u8] = include_bytes!("../assets/binance_qr.png");

pub fn donate_window(parent: &gtk::Window) -> gtk::Window {
    use crate::i18n;
    let lang = i18n::current();
    let window = gtk::Window::builder()
        .title(i18n::pick_for(
            lang,
            "Support VinylPlayer",
            "Apoiar o VinylPlayer",
        ))
        .transient_for(parent)
        .destroy_with_parent(true)
        .default_width(520)
        .default_height(620)
        .build();

    {
        let win_weak = window.downgrade();
        i18n::connect(&window, move |l| {
            if let Some(win) = win_weak.upgrade() {
                win.set_title(Some(i18n::pick_for(
                    l,
                    "Support VinylPlayer",
                    "Apoiar o VinylPlayer",
                )));
            }
        });
    }

    window.add_css_class("donate-dialog");
    window.add_css_class("library-settings");

    let root_box = gtk::Box::new(gtk::Orientation::Vertical, 12);
    root_box.set_margin_top(20);
    root_box.set_margin_bottom(20);
    root_box.set_margin_start(22);
    root_box.set_margin_end(22);

    // --- Header ---
    let header = gtk::Box::new(gtk::Orientation::Horizontal, 12);
    let title = gtk::Label::new(None);
    i18n::bind_label(&title, |l| {
        i18n::pick_for(l, "Support VinylPlayer", "Apoiar o VinylPlayer")
    });
    title.add_css_class("page-title");
    title.set_xalign(0.0);
    title.set_hexpand(true);
    header.append(&title);

    let close = gtk::Button::with_label(i18n::pick_for(lang, "Close", "Fechar"));
    i18n::bind_button_label(&close, |l| i18n::pick_for(l, "Close", "Fechar"));
    close.set_widget_name("donate-close");
    close.add_css_class("folder-button");
    i18n::bind_tooltip(&close, |l| {
        i18n::pick_for(l, "Close support window", "Fechar janela de apoio")
    });
    let weak_window = window.downgrade();
    close.connect_clicked(move |_| {
        if let Some(win) = weak_window.upgrade() {
            win.close();
        }
    });
    header.append(&close);
    root_box.append(&header);

    // --- Scrollable Content ---
    let scroll = gtk::ScrolledWindow::builder()
        .vexpand(true)
        .hscrollbar_policy(gtk::PolicyType::Never)
        .build();

    let content = gtk::Box::new(gtk::Orientation::Vertical, 14);
    content.set_margin_top(6);
    content.set_margin_bottom(12);
    content.set_margin_end(6);

    // ==========================================
    // Card 1: Monthly Development Goal
    // ==========================================
    let goal_card = gtk::Box::new(gtk::Orientation::Vertical, 8);
    goal_card.add_css_class("donate-card");

    let goal_title = gtk::Label::new(None);
    i18n::bind_label(&goal_title, |l| {
        i18n::pick_for(
            l,
            "November VinylPlayer development goal",
            "Meta mensal de desenvolvimento do VinylPlayer",
        )
    });
    goal_title.add_css_class("donate-card-title");
    goal_title.set_xalign(0.0);
    goal_card.append(&goal_title);

    let goal_sub = gtk::Label::new(None);
    i18n::bind_label(&goal_sub, |l| {
        i18n::pick_for(l, "November 2026", "Novembro de 2026")
    });
    goal_sub.add_css_class("donate-card-sub");
    goal_sub.set_xalign(0.0);
    goal_card.append(&goal_sub);

    let goal_desc = gtk::Label::new(None);
    i18n::bind_label(&goal_desc, |l| {
        i18n::pick_for(
            l,
            "Monthly support covers server capacity, development tools, audio testing gear, and release packaging.",
            "O apoio mensal cobre custos de servidor, ferramentas de desenvolvimento, equipamentos de teste de áudio e empacotamento.",
        )
    });
    goal_desc.add_css_class("donate-desc");
    goal_desc.set_wrap(true);
    goal_desc.set_xalign(0.0);
    goal_desc.set_margin_top(4);
    goal_desc.set_margin_bottom(6);
    goal_card.append(&goal_desc);

    // Progress stats
    let stats_box = gtk::Box::new(gtk::Orientation::Horizontal, 10);
    let pct_lbl = gtk::Label::new(Some("0%"));
    pct_lbl.add_css_class("donate-goal-num");
    pct_lbl.set_xalign(0.0);
    pct_lbl.set_hexpand(true);

    let target_lbl = gtk::Label::new(None);
    i18n::bind_label(&target_lbl, |l| {
        i18n::pick_for(l, "$0 / $250 mo", "$0 / $250 mês")
    });
    target_lbl.add_css_class("donate-card-sub");
    target_lbl.set_xalign(1.0);

    stats_box.append(&pct_lbl);
    stats_box.append(&target_lbl);
    goal_card.append(&stats_box);

    // Progress bar
    let progress = gtk::ProgressBar::new();
    progress.add_css_class("donate-progress");
    progress.set_fraction(0.0);
    goal_card.append(&progress);

    content.append(&goal_card);

    // ==========================================
    // Card 2: Support VinylPlayer (Binance Wallet)
    // ==========================================
    let wallet_card = gtk::Box::new(gtk::Orientation::Vertical, 10);
    wallet_card.add_css_class("donate-card");

    let wallet_hdr = gtk::Box::new(gtk::Orientation::Horizontal, 10);
    let wallet_hdr_title = gtk::Label::new(None);
    i18n::bind_label(&wallet_hdr_title, |l| {
        i18n::pick_for(l, "Support VinylPlayer", "Apoiar o VinylPlayer")
    });
    wallet_hdr_title.add_css_class("donate-card-title");
    wallet_hdr_title.set_xalign(0.0);
    wallet_hdr_title.set_hexpand(true);

    let badge = gtk::Label::new(Some("Binance / Web3"));
    badge.add_css_class("donate-badge-bep20");
    wallet_hdr.append(&wallet_hdr_title);
    wallet_hdr.append(&badge);
    wallet_card.append(&wallet_hdr);

    // Inner wallet box
    let inner_box = gtk::Box::new(gtk::Orientation::Vertical, 8);
    inner_box.add_css_class("donate-wallet-box");

    let row_info = gtk::Box::new(gtk::Orientation::Horizontal, 10);
    let icon_coins = gtk::Image::from_icon_name("starred-symbolic");
    icon_coins.add_css_class("brand-icon");
    row_info.append(&icon_coins);

    let labels_box = gtk::Box::new(gtk::Orientation::Vertical, 2);
    labels_box.set_hexpand(true);
    let coin_title = gtk::Label::new(Some("USDT / BNB (BEP20)"));
    coin_title.add_css_class("donate-coin-title");
    coin_title.set_xalign(0.0);
    let chain_sub = gtk::Label::new(Some("Binance Smart Chain (BSC)"));
    chain_sub.add_css_class("donate-card-sub");
    chain_sub.set_xalign(0.0);
    labels_box.append(&coin_title);
    labels_box.append(&chain_sub);
    row_info.append(&labels_box);

    inner_box.append(&row_info);

    // QR Code Container (toggled by button)
    let qr_box = gtk::Box::new(gtk::Orientation::Vertical, 6);
    qr_box.set_visible(false);
    qr_box.set_halign(gtk::Align::Center);
    qr_box.set_margin_top(8);
    qr_box.set_margin_bottom(8);

    // Load QR Pixbuf from embedded memory
    let loader = gdk_pixbuf::PixbufLoader::new();
    let _ = loader.write(QR_BYTES);
    let _ = loader.close();
    if let Some(pixbuf) = loader.pixbuf() {
        let texture = gtk::gdk::Texture::for_pixbuf(&pixbuf);
        let qr_pic = gtk::Picture::for_paintable(&texture);
        qr_pic.set_can_shrink(false);
        qr_pic.add_css_class("donate-qr-frame");
        qr_box.append(&qr_pic);
    }

    let qr_hint = gtk::Label::new(None);
    i18n::bind_label(&qr_hint, |l| {
        i18n::pick_for(
            l,
            "Scan with Binance App (BEP20)",
            "Escaneie com o app Binance (BEP20)",
        )
    });
    qr_hint.add_css_class("donate-card-sub");
    qr_box.append(&qr_hint);
    inner_box.append(&qr_box);

    // Address & Copy Row
    let addr_row = gtk::Box::new(gtk::Orientation::Horizontal, 8);
    addr_row.set_margin_top(4);

    let addr_entry = gtk::Entry::builder()
        .text(WALLET_ADDRESS)
        .editable(false)
        .hexpand(true)
        .build();
    addr_entry.add_css_class("donate-addr-entry");
    addr_row.append(&addr_entry);

    let copy_btn = gtk::Button::with_label(i18n::pick_for(lang, "Copy", "Copiar"));
    i18n::bind_button_label(&copy_btn, |l| i18n::pick_for(l, "Copy", "Copiar"));
    copy_btn.set_widget_name("donate-copy-btn");
    copy_btn.add_css_class("donate-copy-btn");
    i18n::bind_tooltip(&copy_btn, |l| {
        i18n::pick_for(
            l,
            "Copy Binance deposit address",
            "Copiar endereço de depósito Binance",
        )
    });

    let weak_window_for_clip = window.downgrade();
    let copy_btn_weak = copy_btn.downgrade();
    copy_btn.connect_clicked(move |_| {
        if let Some(win) = weak_window_for_clip.upgrade() {
            win.clipboard().set_text(WALLET_ADDRESS);
        }
        if let Some(btn) = copy_btn_weak.upgrade() {
            btn.set_label(i18n::pick("✓ Copied!", "✓ Copiado!"));
            let btn_reset = btn.downgrade();
            glib::timeout_add_local_once(std::time::Duration::from_millis(2500), move || {
                if let Some(b) = btn_reset.upgrade() {
                    b.set_label(i18n::pick("Copy", "Copiar"));
                }
            });
        }
    });
    addr_row.append(&copy_btn);

    // QR toggle button
    let qr_btn = gtk::Button::with_label("QR Code");
    qr_btn.add_css_class("folder-button");
    let qr_box_clone = qr_box.clone();
    let qr_btn_clone = qr_btn.clone();
    qr_btn.connect_clicked(move |_| {
        let is_vis = qr_box_clone.is_visible();
        qr_box_clone.set_visible(!is_vis);
        qr_btn_clone.set_label(if !is_vis {
            i18n::pick("Close QR", "Fechar QR")
        } else {
            "QR Code"
        });
    });
    addr_row.append(&qr_btn);

    inner_box.append(&addr_row);

    // Network compatibility note
    let note = gtk::Label::new(None);
    i18n::bind_label(&note, |l| {
        i18n::pick_for(
            l,
            "💡 Compatible with USDT, USDC, BNB, ETH on BEP-20, ERC-20, and Polygon networks.",
            "💡 Compatível com USDT, USDC, BNB e ETH nas redes BEP-20, ERC-20 e Polygon.",
        )
    });
    note.set_wrap(true);
    note.set_xalign(0.0);
    note.add_css_class("donate-hint");
    inner_box.append(&note);

    wallet_card.append(&inner_box);

    // GitHub Repo Link
    let github_btn = gtk::Button::builder().build();
    i18n::bind_tooltip(&github_btn, |l| {
        i18n::pick_for(
            l,
            "Open GitHub repository in browser",
            "Abrir repositório no GitHub pelo navegador",
        )
    });
    github_btn.add_css_class("donate-github-btn");
    let gh_content = gtk::Box::new(gtk::Orientation::Horizontal, 10);
    let gh_icon = gtk::Image::from_icon_name("starred-symbolic");
    gh_icon.add_css_class("brand-icon");
    gh_content.append(&gh_icon);

    let gh_labels = gtk::Box::new(gtk::Orientation::Vertical, 2);
    gh_labels.set_hexpand(true);
    let gh_title = gtk::Label::new(None);
    i18n::bind_label(&gh_title, |l| {
        i18n::pick_for(
            l,
            "GitHub Repository & Star",
            "Repositório GitHub & Estrela",
        )
    });
    gh_title.add_css_class("donate-coin-title");
    gh_title.set_xalign(0.0);
    let gh_sub = gtk::Label::new(Some("github.com/dznass-cmd/VinylPlayer"));
    gh_sub.add_css_class("donate-card-sub");
    gh_sub.set_xalign(0.0);
    gh_labels.append(&gh_title);
    gh_labels.append(&gh_sub);
    gh_content.append(&gh_labels);

    let ext_icon = gtk::Image::from_icon_name("go-next-symbolic");
    gh_content.append(&ext_icon);
    github_btn.set_child(Some(&gh_content));

    github_btn.connect_clicked(|_| {
        let _ = std::process::Command::new("xdg-open")
            .arg("https://github.com/dznass-cmd/VinylPlayer")
            .spawn();
    });
    wallet_card.append(&github_btn);

    content.append(&wallet_card);

    // ==========================================
    // Card 3: Recent Supporters
    // ==========================================
    let supporters_card = gtk::Box::new(gtk::Orientation::Vertical, 8);
    supporters_card.add_css_class("donate-card");

    let supp_hdr = gtk::Box::new(gtk::Orientation::Horizontal, 8);
    let star_icon = gtk::Image::from_icon_name("starred-symbolic");
    star_icon.add_css_class("brand-icon");
    supp_hdr.append(&star_icon);

    let supp_title = gtk::Label::new(None);
    i18n::bind_label(&supp_title, |l| {
        i18n::pick_for(l, "Recent Supporters", "Apoiadores Recentes")
    });
    supp_title.add_css_class("donate-card-title");
    supp_title.set_xalign(0.0);
    supp_hdr.append(&supp_title);
    supporters_card.append(&supp_hdr);

    let supp_desc = gtk::Label::new(None);
    i18n::bind_label(&supp_desc, |l| {
        i18n::pick_for(
            l,
            "Thank you for your generosity! Every contribution keeps VinylPlayer free and open-source.",
            "Obrigado pela generosidade! Cada contribuição mantém o VinylPlayer gratuito e de código aberto.",
        )
    });
    supp_desc.add_css_class("donate-desc");
    supp_desc.set_wrap(true);
    supp_desc.set_xalign(0.0);
    supporters_card.append(&supp_desc);

    let badges_box = gtk::Box::new(gtk::Orientation::Horizontal, 8);
    badges_box.set_margin_top(4);
    let first_badge = gtk::Label::new(None);
    i18n::bind_label(&first_badge, |l| {
        i18n::pick_for(
            l,
            "Be the first supporter! ❤️",
            "Seja o primeiro apoiador! ❤️",
        )
    });
    first_badge.add_css_class("donate-supporter-pill");
    badges_box.append(&first_badge);
    supporters_card.append(&badges_box);

    content.append(&supporters_card);

    scroll.set_child(Some(&content));
    root_box.append(&scroll);
    window.set_child(Some(&root_box));

    // Handle Escape key to close window
    let key_controller = gtk::EventControllerKey::new();
    let weak_window_esc = window.downgrade();
    key_controller.connect_key_pressed(move |_, keyval, _, _| {
        if keyval == gtk::gdk::Key::Escape {
            if let Some(win) = weak_window_esc.upgrade() {
                win.close();
                return glib::Propagation::Stop;
            }
        }
        glib::Propagation::Proceed
    });
    window.add_controller(key_controller);

    window
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_binance_wallet_address_and_qr() {
        assert_eq!(
            WALLET_ADDRESS,
            "0xbc1ce0b7d0e5bc3332502bb82cf34125e3062b86",
            "Wallet address must match the user's Binance deposit address"
        );
        // Verify QR PNG magic bytes
        assert!(
            QR_BYTES.starts_with(&[0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A]),
            "Embedded QR image must be a valid PNG"
        );
    }
}

