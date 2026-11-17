//! Mostradores analógicos de nível de áudio estéreo (VU Meters vintage).

use gtk::cairo;
use std::f64::consts::{PI, TAU};

/// Desenha um retângulo com cantos arredondados no contexto Cairo.
pub fn draw_rounded_rect(cr: &gtk::cairo::Context, x: f64, y: f64, w: f64, h: f64, r: f64) {
    cr.new_sub_path();
    cr.arc(x + w - r, y + r, r, -PI / 2.0, 0.0);
    cr.arc(x + w - r, y + h - r, r, 0.0, PI / 2.0);
    cr.arc(x + r, y + h - r, r, PI / 2.0, PI);
    cr.arc(x + r, y + r, r, PI, 3.0 * PI / 2.0);
    cr.close_path();
}

/// Desenha o bezel metálico, mostradores de pergaminho, trilhas em arco e marcações de escala dos VU Meters.
pub fn draw_vu_background(cr: &gtk::cairo::Context, x: f64, y: f64, w: f64, h: f64) {
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

/// Desenha os ponteiros balísticos móveis e a iluminação incandescente dos medidores VU estéreo.
pub fn draw_vu_meters(
    cr: &gtk::cairo::Context,
    rect: (f64, f64, f64, f64),
    needle_l: f64,
    needle_r: f64,
    is_playing: bool,
) {
    let (x, y, w, h) = rect;
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

#[cfg(test)]
mod tests {
    use super::*;

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
        let rise = update_needle(0.0, 0.80, dt);
        assert!(
            rise > 0.50,
            "ataque deve subir > 50% em 50ms (obtido {rise:.2})"
        );

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

        draw_vu_background(&cr, 10.0, 10.0, 96.0, 35.0);

        // Repouso (silêncio)
        draw_vu_meters(&cr, (10.0, 10.0, 96.0, 35.0), 0.0, 0.0, false);
        // Nominal (0 VU) tocando
        draw_vu_meters(&cr, (10.0, 10.0, 96.0, 35.0), 0.708, 0.708, true);
        // Pico (+3 dB VU) e Sobrecarga (> +3 dB)
        draw_vu_meters(&cr, (10.0, 10.0, 96.0, 35.0), 1.0, 1.15, true);
        drop(cr);
    }
}
