# Project Instructions — VinylPlayer

**VinylPlayer** is an analog-inspired, lightweight, and native music player for the Linux desktop (Wayland & X11), developed with **Rust 2021**, **GTK4 / Libadwaita**, and **GStreamer / PipeWire**.

---

## 🛠️ Tech Stack & Dependencies

- **Language:** Rust (2021 Edition).
- **UI Toolkit:** GTK4 (`gtk4 0.9`, feature `v4_12`) and Libadwaita (`libadwaita 0.7`, feature `v1_5`).
- **Audio Pipeline:** GStreamer (`gstreamer 0.23` and `gstreamer-audio 0.23`) streaming to PipeWire via `playbin3`.
- **System Integration:** D-Bus MPRIS v2 (`zbus 4` & `zvariant 4`), native desktop notifications (`notify-send`).
- **Networking & Scrobbling:** `ureq 3.5` (JSON & TLS), `md-5 0.11`, `hex 0.4`.
- **Serialization:** `serde_json 1`.

---

## 📂 Source Code Architecture (`src/`)

- [`src/main.rs`](src/main.rs): Application shell, main window layout, sidebar, real-time Cairo turntable rendering loop (VSync-synced via `FrameClock`), playback transport bar with 250ms refresh tick.
- [`src/audio.rs`](src/audio.rs): AudioEngine encapsulating GStreamer pipeline, format decoding (FLAC, WAV, MP3, OGG, M4A, OPUS, AAC), gapless playback, stereo VU meter level emission, volume control, and seeking.
- [`src/scrobbler.rs`](src/scrobbler.rs): Native Last.fm Audioscrobbler 2.0 API implementation:
  - Cryptographic MD5 parameter hashing (`api_sig`) with strict ASCII parameter sort.
  - Web authentication handshake (`auth.getToken`, browser redirect, and `auth.getSession`).
  - Real-time `track.updateNowPlaying` call on track start.
  - Automatic `track.scrobble` dispatch when playback reaches $\ge \min(\text{duration} / 2, 240\text{s})$ on tracks $\ge 30\text{s}$.
- [`src/library_settings.rs`](src/library_settings.rs): Settings window:
  - Local music folder selection & background indexing.
  - Analog experience toggles (Needle Drop effect, Stylus Target Light).
  - Last.fm Scrobbler configuration, browser OAuth handshake, connection status, and optional custom API credentials.
- [`src/state.rs`](src/state.rs): Atomic state persistence to `~/.config/vinylplayer/state.json` via exclusive temporary file and atomic rename.
- [`src/donate.rs`](src/donate.rs): In-app donation dialog featuring Binance BEP-20 wallet (`0xbc1ce0b7d0e5bc3332502bb82cf34125e3062b86`), vector QR code, and one-click clipboard copy.
- [`src/thumb_cache.rs`](src/thumb_cache.rs): On-disk LRU thumbnail cache downscaling album covers to 256px JPEG, reducing idle RAM consumption from 572.9 MB to 212.5 MB (-63%).
- [`src/needle_drop.rs`](src/needle_drop.rs): Procedural / cached authentic acoustic needle drop sound effect triggered on playback start.
- [`src/playlist.rs`](src/playlist.rs) & [`src/playlist_view.rs`](src/playlist_view.rs): M3U / M3U8 playlist management, parsing, track reordering, and detail views.
- [`src/mpris.rs`](src/mpris.rs): Linux desktop MPRIS v2 integration for hardware media keys and notification bar controls.
- [`src/playback_queue.rs`](src/playback_queue.rs) & [`src/queue_view.rs`](src/queue_view.rs): Low-latency playback queue with instant multi-keyword search.

---

## ⚙️ Development Guidelines & Rules

1. **Language & Documentation:**
   - Write all changelog entries (`MODIFICACOES.md`), release notes, and commit messages in **English**.
   - Maintain user-facing application localization in both **English** and **Brazilian Portuguese**.
2. **GTK & Audio Concurrency:**
   - **NEVER** run blocking I/O, heavy file parsing, or network HTTP requests on the GTK main thread.
   - Always dispatch background tasks using `std::thread::Builder::new().spawn(...)` or `gtk::gio::spawn_blocking(...)`.
3. **Automated Verification:**
   - Ensure all automated unit tests (`cargo test`) pass with 0 failures prior to pushing any changes.

---

## 🚀 Key Commands

```bash
# Run in debug mode
cargo run

# Run automated test suite
cargo test

# Build optimized release binary (with thin LTO and symbol stripping)
cargo build --release

# Install binary to local path
cp target/release/vinylplayer ~/.local/bin/vinylplayer
```
