//! Mostrador analógico de volume sobreposto (Volume HUD Gauge).

use gtk::cairo;
use std::f64::consts::{PI, TAU};
use std::time::Instant;

#[derive(Clone, Debug)]
pub struct VolumeHudState {
    pub level: f64,
    pub alpha: f64,
    pub last_active: Option<Instant>,
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
    pub fn trigger(&mut self, level: f64) {
        self.level = level.clamp(0.0, 1.0);
        self.alpha = 1.0;
        self.last_active = Some(Instant::now());
    }

    pub fn tick_fade(&mut self) -> bool {
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

pub fn draw_volume_hud(cr: &gtk::cairo::Context, cx: f64, cy: f64, level: f64, alpha: f64) {
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

#[cfg(test)]
mod tests {
    use super::*;

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
}
