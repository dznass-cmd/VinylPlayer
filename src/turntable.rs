//! Mecanismo de renderização vetorial e animação analógica do Toca-Discos (Turntable).

use crate::volume_hud;
use crate::vu_meter::{self, draw_rounded_rect};
use gtk::cairo;
use gtk::gdk_pixbuf;
use gtk::prelude::*;
use std::cell::RefCell;
use std::f64::consts::{PI, TAU};
use std::rc::Rc;

#[derive(Default)]
pub struct CachedCover {
    pub key: String,
    pub original: Option<gdk_pixbuf::Pixbuf>,
    pub surface: Option<(i32, cairo::ImageSurface)>,
}

#[derive(Default)]
pub struct TurntableCache {
    pub bg_surface: Option<(i32, i32, cairo::ImageSurface)>,
    pub disc_surface: Option<(f64, i32, u32, cairo::ImageSurface)>,
    pub cover: CachedCover,
}

pub struct TurntableLayout {
    pub mx: f64,
    pub my: f64,
    pub dw: f64,
    pub dh: f64,
    pub cx: f64,
    pub cy: f64,
    pub r_disc: f64,
    pub r_platter: f64,
    pub pivot_x: f64,
    pub pivot_y: f64,
    pub arm_len: f64,
    pub led_x: f64,
    pub led_y: f64,
    pub vu_x: f64,
    pub vu_y: f64,
    pub vu_w: f64,
    pub vu_h: f64,
    pub target_light_x: f64,
    pub target_light_y: f64,
    pub target_light_r: f64,
    pub strobe_x: f64,
    pub strobe_y: f64,
    pub pitch_x: f64,
    pub pitch_y: f64,
    pub pitch_w: f64,
    pub pitch_h: f64,
}

impl TurntableLayout {
    pub fn compute(w: f64, h: f64) -> Self {
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

pub struct TurntableRenderParams<'a> {
    pub w: i32,
    pub h: i32,
    pub disc_angle: f64,
    pub arm_angle: f64,
    pub is_playing: bool,
    pub cache: &'a Rc<RefCell<TurntableCache>>,
    pub scale: i32,
    pub rpm: u32,
    pub volume_level: f64,
    pub volume_alpha: f64,
    pub vu_needle_l: f64,
    pub vu_needle_r: f64,
    pub target_light_on: bool,
    pub pitch: f64,
}

pub fn render_background(layout: &TurntableLayout, w: i32, h: i32) -> Option<cairo::ImageSurface> {
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
    vu_meter::draw_vu_background(&cr, layout.vu_x, layout.vu_y, layout.vu_w, layout.vu_h);

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

pub fn render_rotating_disc(
    r_disc: f64,
    target_dim: i32,
    surface_to_draw: Option<cairo::ImageSurface>,
    scale: i32,
    rpm: u32,
) -> Option<cairo::ImageSurface> {
    let side = (2.0 * r_disc + 4.0).ceil() as i32;
    let surface =
        cairo::ImageSurface::create(cairo::Format::ARgb32, side * scale, side * scale).ok()?;
    surface.set_device_scale(scale as f64, scale as f64);
    let cr = cairo::Context::new(&surface).ok()?;
    cr.translate(side as f64 / 2.0, side as f64 / 2.0);
    cr.arc(0.0, 0.0, r_disc, 0.0, TAU);
    cr.clip();

    if let Some(ref surf) = surface_to_draw {
        let half = target_dim as f64 / 2.0;
        cr.set_source_surface(surf, -half, -half).ok();
        cr.paint().ok();

        let edge_shade = cairo::RadialGradient::new(0.0, 0.0, r_disc * 0.76, 0.0, 0.0, r_disc);
        edge_shade.add_color_stop_rgba(0.0, 0.0, 0.0, 0.0, 0.0);
        edge_shade.add_color_stop_rgba(0.90, 0.0, 0.0, 0.0, 0.28);
        edge_shade.add_color_stop_rgba(1.0, 0.0, 0.0, 0.0, 0.58);
        cr.set_source(&edge_shade).ok();
        cr.arc(0.0, 0.0, r_disc, 0.0, TAU);
        cr.fill().ok();
    } else {
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

pub fn draw_turntable(cr: &gtk::cairo::Context, params: &TurntableRenderParams<'_>) {
    let w = params.w;
    let h = params.h;
    let disc_angle = params.disc_angle;
    let arm_angle = params.arm_angle;
    let is_playing = params.is_playing;
    let cache = params.cache;
    let scale = params.scale;
    let rpm = params.rpm;
    let volume_level = params.volume_level;
    let volume_alpha = params.volume_alpha;
    let vu_needle_l = params.vu_needle_l;
    let vu_needle_r = params.vu_needle_r;
    let target_light_on = params.target_light_on;
    let pitch = params.pitch;

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
    volume_hud::draw_volume_hud(cr, layout.cx, layout.cy, volume_level, volume_alpha);

    // 8. Medidores VU Analógicos Estéreo Dinâmicos (Ponteiros Balísticos Hi-Fi)
    vu_meter::draw_vu_meters(
        cr,
        (layout.vu_x, layout.vu_y, layout.vu_w, layout.vu_h),
        vu_needle_l,
        vu_needle_r,
        is_playing,
    );
}

pub fn draw_strobe_dots(
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

pub fn draw_target_light(
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

pub fn draw_pitch_fader(cr: &gtk::cairo::Context, layout: &TurntableLayout, pitch: f64) {
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

#[cfg(test)]
mod tests {
    use super::*;

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
        let cache = Rc::new(RefCell::new(TurntableCache::default()));

        // Estado 1: Luz Alvo ativa, Pitch em +4.5% (LED Quartz desligado)
        let p1 = TurntableRenderParams {
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
            pitch: 0.045,
        };
        draw_turntable(&cr, &p1);

        // Estado 2: Luz Alvo desligada, Pitch em 0.0% (LED Quartz travado e iluminado)
        let p2 = TurntableRenderParams {
            w: 580,
            h: 360,
            disc_angle: 0.85,
            arm_angle: 2.15,
            is_playing: true,
            cache: &cache,
            scale: 1,
            rpm: 45,
            volume_level: 0.0,
            volume_alpha: 0.0,
            vu_needle_l: 0.62,
            vu_needle_r: 0.85,
            target_light_on: false,
            pitch: 0.0,
        };
        draw_turntable(&cr, &p2);
        drop(cr);
    }
}
