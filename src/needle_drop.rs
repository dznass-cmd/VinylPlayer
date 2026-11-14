//! Síntese procedimental e reprodução do efeito sonoro de Needle Drop (Pouso da Agulha).
//!
//! Emite um micro-efeito acústico autêntico de agulha de diamante pousando no sulco
//! inicial de um disco de vinil: transiente mecânico de contato (thump a ~72 Hz),
//! atrito quente de micro-ranhuras (lead-in groove rumble com modulação a 33 RPM)
//! e micro-estática sutil antes da faixa iniciar.

use std::path::PathBuf;
use gstreamer as gst;

/// Gera em memória um arquivo WAV (PCM 16-bit, 44.1 kHz, Mono) sintetizando o pouso da agulha.
pub fn generate_needle_drop_wav() -> Vec<u8> {
    let sample_rate = 44_100_u32;
    let duration = 0.65_f64;
    let num_samples = (sample_rate as f64 * duration) as usize;

    let mut samples = Vec::with_capacity(num_samples);

    // Gerador de ruído pseudo-aleatório determinístico (LCG)
    let mut seed = 1234567_u64;
    let mut lcg = move || {
        seed = (1103515245_u64.wrapping_mul(seed).wrapping_add(12345)) & 0x7fff_ffff;
        (seed as f64 / 2147483647.0) * 2.0 - 1.0
    };

    // Filtro IIR passa-baixas de 1ª ordem (~380 Hz)
    let alpha = 0.052_f64;
    let mut lp = 0.0_f64;

    for i in 0..num_samples {
        let t = i as f64 / sample_rate as f64;

        // 1. Thump de contato mecânico da agulha de diamante no disco (72 Hz + clique 1750 Hz)
        let thump = if t >= 0.035 {
            let dt = t - 0.035;
            let low = (-dt * 26.0).exp() * (2.0 * std::f64::consts::PI * 72.0 * dt).sin() * 0.55;
            let click = (-dt * 160.0).exp() * (2.0 * std::f64::consts::PI * 1750.0 * dt).sin() * 0.22;
            low + click
        } else {
            0.0
        };

        // 2. Atrito suave do sulco silencioso de entrada (lead-in run-in groove)
        let noise = lcg();
        lp += alpha * (noise - lp);

        let env = if t < 0.035 {
            0.0
        } else if t < 0.065 {
            (t - 0.035) / 0.030
        } else if t < 0.45 {
            1.0
        } else {
            (1.0 - (t - 0.45) / 0.20).max(0.0)
        };

        let rumble = 1.0 + 0.18 * (2.0 * std::f64::consts::PI * 1.8 * t).sin();
        let friction = lp * 0.28 * env * rumble;

        // 3. Dois micro-pops característicos de estática de vinil
        let pop = if (0.118..=0.120).contains(&t) {
            0.32 * (2.0 * std::f64::consts::PI * 1200.0 * (t - 0.118)).sin()
        } else if (0.340..=0.342).contains(&t) {
            -0.25 * (2.0 * std::f64::consts::PI * 950.0 * (t - 0.340)).sin()
        } else {
            0.0
        };

        let val = (thump + friction + pop).clamp(-0.95, 0.95);
        let sample_16 = (val * 32767.0) as i16;
        samples.push(sample_16);
    }

    // Montagem do cabeçalho RIFF/WAVE canônico
    let data_bytes = samples.len() * 2;
    let mut wav = Vec::with_capacity(44 + data_bytes);

    // RIFF chunk
    wav.extend_from_slice(b"RIFF");
    wav.extend_from_slice(&((36 + data_bytes) as u32).to_le_bytes());
    wav.extend_from_slice(b"WAVE");

    // fmt subchunk (PCM)
    wav.extend_from_slice(b"fmt ");
    wav.extend_from_slice(&16_u32.to_le_bytes()); // subchunk1 size
    wav.extend_from_slice(&1_u16.to_le_bytes());  // AudioFormat 1 = PCM
    wav.extend_from_slice(&1_u16.to_le_bytes());  // NumChannels 1 = Mono
    wav.extend_from_slice(&sample_rate.to_le_bytes());
    wav.extend_from_slice(&(sample_rate * 2).to_le_bytes()); // ByteRate (sample_rate * 2 bytes)
    wav.extend_from_slice(&2_u16.to_le_bytes());  // BlockAlign
    wav.extend_from_slice(&16_u16.to_le_bytes()); // BitsPerSample

    // data subchunk
    wav.extend_from_slice(b"data");
    wav.extend_from_slice(&(data_bytes as u32).to_le_bytes());

    for s in samples {
        wav.extend_from_slice(&s.to_le_bytes());
    }

    wav
}

/// Garante que o arquivo WAV sintetizado esteja presente em cache.
pub fn ensure_needle_drop_wav() -> std::io::Result<PathBuf> {
    let cache_dir = gtk::glib::user_cache_dir().join("vinylplayer");
    std::fs::create_dir_all(&cache_dir)?;
    let wav_path = cache_dir.join("needle_drop_v1.wav");

    if !wav_path.exists() || std::fs::metadata(&wav_path)?.len() < 1000 {
        let bytes = generate_needle_drop_wav();
        std::fs::write(&wav_path, bytes)?;
    }

    Ok(wav_path)
}

/// Dispara a reprodução do needle drop em uma thread de segundo plano desacoplada,
/// sem bloquear a thread da interface gráfica.
pub fn play_needle_drop() {
    std::thread::spawn(|| {
        let Ok(path) = ensure_needle_drop_wav() else {
            return;
        };
        let path_str = path.to_string_lossy();
        let pipeline_str = format!(
            "filesrc location=\"{}\" ! wavparse ! audioconvert ! audioresample ! volume volume=0.72 ! autoaudiosink",
            path_str
        );
        if let Ok(pipeline) = gst::parse::launch(&pipeline_str) {
            use gst::prelude::*;
            let _ = pipeline.set_state(gst::State::Playing);
            if let Some(bus) = pipeline.bus() {
                for msg in bus.iter_timed(gst::ClockTime::from_mseconds(850)) {
                    use gst::MessageView;
                    match msg.view() {
                        MessageView::Eos(..) | MessageView::Error(..) => break,
                        _ => {}
                    }
                }
            }
            let _ = pipeline.set_state(gst::State::Null);
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn needle_drop_wav_has_valid_riff_header_and_expected_length() {
        let bytes = generate_needle_drop_wav();
        assert!(bytes.len() > 44);
        assert_eq!(&bytes[0..4], b"RIFF");
        assert_eq!(&bytes[8..12], b"WAVE");
        assert_eq!(&bytes[12..16], b"fmt ");
        assert_eq!(&bytes[36..40], b"data");

        let sample_rate = u32::from_le_bytes(bytes[24..28].try_into().unwrap());
        assert_eq!(sample_rate, 44100);

        let channels = u16::from_le_bytes(bytes[22..24].try_into().unwrap());
        assert_eq!(channels, 1);

        let bits = u16::from_le_bytes(bytes[34..36].try_into().unwrap());
        assert_eq!(bits, 16);

        // Verifica energia não-nula nos dados PCM
        let mut max_val = 0_i16;
        for chunk in bytes[44..].chunks_exact(2) {
            let val = i16::from_le_bytes(chunk.try_into().unwrap());
            max_val = max_val.max(val.abs());
        }
        assert!(max_val > 10000, "o áudio do needle drop deve possuir amplitude audível");
    }

    #[test]
    fn ensure_needle_drop_creates_cache_file() {
        let path = ensure_needle_drop_wav().expect("criação do arquivo de cache");
        assert!(path.exists());
        let meta = std::fs::metadata(&path).expect("metadados do arquivo");
        assert!(meta.len() > 10000);
    }
}
