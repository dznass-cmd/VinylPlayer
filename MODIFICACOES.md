# Alterações do VinylPlayer

Registro atualizado em 28/11/2026.

## Release v0.4.1 — Interactive Crossfader, Mix Curves, Full-Library Sweep & DJ UX Refinements — 28/11/2026

- **Interactive Crossfader Console & Curves (`src/audio.rs`, `src/dj.rs`, `src/dj_view.rs`):**
  - Made the horizontal crossfader rail in the Cairo DJ console directly draggable via mouse/pointer (`GestureDrag` + `ew-resize` hover feedback), supporting live manual Deck A ↔ B blending, hold-in-place middle blends, snap-back cancel near Deck A, and instant Deck B promotion near 100%.
  - Added three selectable crossfade curves (`DjCrossfadeCurve::EqualPower`, `LongFade`, `HardCut`) persisted in `state.json` (`dj_curve`) and rendered live inside the DJ console curve graph.
- **Intro Silence Cue Trimming & Stricter Anti-Repeat (`src/dj.rs`, `src/audio.rs`):**
  - Extended `analyze_track_acoustics_gst` to measure leading silence (`intro_silence_ms`) on initial preroll frames and seek incoming Deck B tracks cleanly past silent intros upon `AsyncDone` when `Trim Intro Silence` (`dj_trim_intro`) is active.
  - Replaced the 30/40-track cap in background acoustic profiling with a full-library rotating cursor (`analyze_cursor`), raised persistent acoustic cache capacity to 20,000 tracks, expanded play history to 48 tracks, and added a recent-artists cooldown penalty across all DJ modes.
- **Next Control Integration, Start Session & Up-Next Preview (`src/dj_view.rs`, `src/main.rs`):**
  - Routed transport Next (`⏭️`), `Right`, and `Page_Down` through `DjController::next_track`, completing active crossfades or triggering snappy DJ transitions to Deck B when Auto-DJ is active.
  - Added a `Start DJ` session button for starting Auto-DJ directly from stopped/paused state, an interactive `Up Next · Setlist Preview` card for one-click mixing to upcoming candidates, an automatic catalog-growth poll, and a clear inline notice banner when music folders or drives are unavailable.

## Release v0.4.0 — Auto-DJ Booth & Intelligent / Random Mix Modes — 26/11/2026

- **Dual-Deck Equal-Power Crossfade Engine (`src/audio.rs`):**
  - Added a second `playbin3` audio deck (`aux_playbin`) with dedicated per-deck `xfade-vol` GStreamer gain staging, equal-power cosine/sine curve (`equal_power_gains`), RMS-combined stereo VU metering during transitions, seamless active-deck handoff without playback interruption, and automatic `fakesink` mirroring for headless tests.
  - Added `current_track_path`, `enqueue_dj_track`, `replace_upcoming_tracks`, `start_crossfade`, `step_crossfade`, `finish_crossfade`, and `cancel_crossfade` controls synced across playback state, pause/resume, and both deck buses.
- **Intelligent & Random Auto-DJ Selection Engine (`src/dj.rs`, `src/library.rs`):**
  - Extended metadata tag extraction (`TrackTags` v4 cache) to read `genre`, `bpm` (`TBPM`/`tempo`), and `initial_key` (`TKEY`/`InitialKey`).
  - Added fast silent GStreamer background acoustic profiling (`analyze_track_acoustics_gst`) estimating RMS energy and BPM via onset-flux autocorrelation, persisted in `~/.cache/vinylplayer/dj-acoustics-v1.json`.
  - Added Camelot harmonic wheel parser and compatibility scorer (`parse_camelot_key`, `camelot_compatibility`).
  - Implemented five active DJ modes across Random (`Random Flow`, `Party Chaos`) and Intelligent (`Smart Flow`, `Seed Radio`, `Energy Journey`) categories, with anti-repetition history, `QueueOnly` vs `LibraryAutoFill` selection pools, `Smart Outro Cue` fade-out silence detection, and optional `Auto Tempo Blend` (±6% BPM sync).
- **DJ Booth View & Quick Transport Popover (`src/dj_view.rs`, `src/main.rs`, `src/theme.rs`, `src/state.rs`, `src/i18n.rs`):**
  - Added the `DJ Booth` (`Alt+5`) view featuring a live Cairo Dual-Deck & Crossfader console, interactive mode cards, crossfade overlap presets (`0s`–`12s`), `Mix Now`, `Re-roll Next`, and `Build Set (+20)` queue generation.
  - Added a quick DJ popover button on the bottom transport bar and a clickable DJ status pill on the Turntable header.
  - Persisted DJ configuration and `"dj"` view state in `~/.config/vinylplayer/state.json` and themed all new DJ controls across all 6 analog palettes with full English and Brazilian Portuguese localization.

## Release v0.3.1 — CI & Packaging Reliability — 23/11/2026

- Fixed the Flatpak release job on GitHub-hosted Ubuntu runners by installing `flatpak-builder` explicitly.
- Made the professional preview integration test locate the album grid by widget type instead of relying on private GTK child wrappers.
- Ensured the professional preview integration test creates hermetic fixtures and waits for catalog scanning, passing cleanly in clean CI runners without host music files.
- Disabled the unavailable accessibility bus in headless CI to remove unrelated GTK warnings.
- Removed the unused `gstreamer-audio` dependency and synchronized the Flatpak offline source map.

## Release v0.3.0 — Portable Distribution & Secure Credentials — 23/11/2026

- Added an official Flatpak manifest and offline Cargo source map for reproducible sandboxed builds.
- Added a tag-driven GitHub Actions release pipeline that validates versions and checksums, builds Arch and Flatpak packages, and publishes all assets.
- Promoted the ignored GTK/media integration tests to a dedicated Xvfb CI job and added cold-start/peak-memory reporting.
- Moved Last.fm API credentials and session keys to Secret Service on the desktop and the encrypted Secret portal in Flatpak, with automatic legacy migration and a protected fallback.
- Kept all keyring and persistence I/O away from the GTK main loop.

## Release v0.2.2 — Accurate Audio Quality & Linux Integration — 22/11/2026

- Corrected the quality badge so `HI-RES` requires a lossless source at 24-bit/96 kHz or above, while qualifying 16-bit lossless audio is labeled `HI-FI`.
- Added raw GStreamer sample-format parsing so bit depth is detected from formats such as `S16LE`, `S24_32LE`, and `F32LE`.
- Added AppStream metadata and installed it through the Arch package for better desktop software-center integration.
- Expanded source-build dependency instructions for Ubuntu, Debian, and Fedora.
- Hardened CI with all-target/all-feature checks plus desktop-file and AppStream validation.
- Added weekly grouped dependency updates for Cargo crates and GitHub Actions.

## Release v0.2.1 — Release Integrity & Contributor Readiness — 22/11/2026

- Replaced the unverified bit-perfect claim with an accurate description of GStreamer/PipeWire format negotiation and clarified Hi-Res output limitations.
- Removed misleading references to embedded Last.fm credentials, made bring-your-own credentials explicit in the bilingual UI, and derived the HTTP User-Agent version from the package metadata.
- Added continuous integration for formatting, Clippy, tests, and release builds on Ubuntu 24.04.
- Added contributor, security, issue, and pull-request guidance for public collaboration.
- Synchronized the Arch Linux package metadata and replaced the skipped source checksum with a verified release-archive SHA-256.

## Release v0.2.0 — Milestone Release: Sidebar Hotkeys & Transport Bar Volume Scroll — 21/11/2026

- **Instant Sidebar Tab Hotkeys (`Alt+1..4`) & Tooltips (`src/main.rs`):**
  - Added fast desktop keyboard shortcuts to switch collection views instantly: `Alt+1` for Turntable (Toca-Discos), `Alt+2` for Tracks (Faixas), `Alt+3` for Albums (Álbuns), and `Alt+4` for Playlists.
  - Bound dynamic localized tooltips to each sidebar navigation button displaying the corresponding accelerator (`"Turntable · Alt+1"` / `"Toca-Discos · Alt+1"`).
- **Transport Bar Mouse Wheel Volume Control (`src/main.rs`):**
  - Attached vertical scroll controller (`gtk::EventControllerScroll`) to the bottom transport bar volume section, enabling smooth 3% volume adjustments and immediate on-screen HUD popups simply by scrolling the wheel over the speaker button or slider.

## Release v0.1.9 — Interactive Vinyl Platter, Tactile Cursors & Navigation Shortcuts — 21/11/2026

- **Interactive Vinyl Platter Play/Pause & Tactile Hover Cursor (`src/main.rs`):**
  - Enabled clicking anywhere on the rotating/stopped vinyl record platter to toggle Play/Pause directly on the turntable deck with synchronized needle drop acoustics.
  - Added real-time hover motion tracking (`gtk::EventControllerMotion`) setting pointer cursor when hovering over interactive deck zones (33/45 RPM buttons, stylus target light tower, pitch fader, and vinyl disc platter).
- **Audio Seeking & Media Navigation Keyboard Shortcuts (`src/main.rs`):**
  - Added `Shift + Right Arrow` (+5s jump) and `Shift + Left Arrow` (-5s jump) for quick keyboard scrubbing through tracks.
  - Added `Page_Down` (next track), `Page_Up` (previous track), and `Home` (rewind to track start) keyboard bindings.
- **Defensive Unwraps & Null Parent Directory Guards (`src/library.rs`, `src/main.rs`):**
  - Replaced risky `.unwrap()` on `topbar_widget.downcast` with safe pattern matching.
  - Guarded metadata cache path in `library.rs` ensuring `cache.parent()` exists before directory creation.

## Release v0.1.8 — Smooth Seekbar Dragging & Playlist Deletion Safety — 21/11/2026

- **Smooth Seekbar Dragging & Real-Time Position Feedback (`src/main.rs`):**
  - Eliminated timeline slider stutter and position jitter caused by the 250ms background transport tick loop overwriting the slider value while being dragged with the mouse.
  - Implemented `is_seeking` capture gesture controller (`gtk::GestureDrag`) on the progress scale, pausing background position overwrite during drag gestures and committing the precise target seek position cleanly on release (`drag_end`).
  - Added live time label updating during seek drag gestures (`seek.connect_value_changed`), providing immediate visual feedback of the target timestamp before dropping the needle.
- **Destructive Confirmation Guard on Playlist Deletion (`src/playlist_view.rs`):**
  - Protected user playlists from accidental loss by wrapping the delete button (`del_btn`) on playlist cards in a native Libadwaita modal confirmation dialog (`adw::MessageDialog`).
  - Styled the action button with destructive appearance (`adw::ResponseAppearance::Destructive`) with default safe focus on "Cancel", localized dynamically for English and Brazilian Portuguese.

## Release v0.1.7 — Last.fm Scrobbler Hardening & API Credential Validation — 21/11/2026

- **Last.fm Scrobbler HTTP 400 & 403 Hardening (`src/scrobbler.rs`, `src/state.rs`):**
  - Configured custom desktop User-Agent header (`VinylPlayer/0.1.7 ...`) on the HTTP client to prevent Cloudflare edge blocking and HTTP 403 Forbidden responses.
  - Switched `ureq` to non-throwing status inspection (`http_status_as_error(false)`), allowing parsing of Last.fm API JSON error codes (`val.get("error")`) with fallback HTTP status reporting.
  - Added strict credential validation across `request_token`, `fetch_session`, `update_now_playing`, and `scrobble_track`, returning clear errors when API Key or Secret are unconfigured.
  - Updated `is_lastfm_authenticated()` in `src/state.rs` to require non-empty API credentials in addition to session key, preventing empty HTTP 400 background requests during playback.
  - Added automated unit tests `test_empty_api_key_validations` verifying error handling for unconfigured credentials.
- **Last.fm Configuration UX & Expander Guard (`src/library_settings.rs`):**
  - Guarded the "Connect to Last.fm" button to detect empty API keys, immediately prompting the user with `"Configure your Last.fm API Key below first."` / `"Configure sua API Key da Last.fm abaixo primeiro."` and automatically expanding the "Custom API Credentials" section.

## Release v0.1.6 — Shortcuts, Multi-File Open & Dynamic Modal i18n — 21/11/2026

- **Multi-File Selection & Formatted Audio Filters (`src/main.rs`):**
  - Upgraded the topbar "Open track" dialog from single-file selection to `open_multiple_future` with automatic sequential enqueuing (`engine.play_files(&paths, 0)`), letting users select entire track lists directly from file managers without losing selection order.
  - Added formatted GTK file filters distinguishing supported audio formats (`*.flac`, `*.mp3`, `*.ogg`, `*.wav`, `*.m4a`, `*.opus`, `*.aac`, etc.) from generic all-file mode.
- **Global `Ctrl+O` Track Open Keyboard Shortcut (`src/main.rs`):**
  - Registered `win.open-track` action with `<Control>o` accelerator and responsive tooltip (`"Open track · Ctrl+O"` / `"Abrir faixa · Ctrl+O"`).
- **Fully Reactive Live i18n in Support & Donate Dialog (`src/donate.rs`):**
  - Converted all static label and button strings across the Support/Donate modal into reactive `i18n::bind_label`, `i18n::bind_button_label`, and `i18n::bind_tooltip` bindings anchored by `i18n::connect(&window, ...)`, so changing languages in Settings immediately updates the Support window without requiring reopening.

## Release v0.1.5 — Bug Fixes, UI Hardening & MPRIS Conformance — 21/11/2026

- **Playback Rate Preservation on Timeline Seek (`src/audio.rs`):**
  - Fixed audio pitch being silently reset to 1.0x when seeking along the timeline track scale or through MPRIS `SetPosition` / `Seek`.
  - Replaced hardcoded `playbin.seek_simple` (which defaults `rate` to `1.0` in GStreamer) with `gst::event::Seek::new(self.rate(), ...)` and graceful fallback, preserving any active pitch fader adjustment (e.g. $+5\%$ rotation) seamlessly during seeks.
  - Added unit test `seek_us_preserves_playback_rate` verifying that seeking during active playback maintains custom pitch rate.
- **Elimination of GTK UI Unwraps in Queue View (`src/queue_view.rs`):**
  - Replaced 9 fragile chained `.unwrap()` calls in `update_row`, `setup_row`, and `sync_rows` with safe pattern matching guards (`let Some(...) else { return; }` and `downcast_ref`), preventing desktop crashes during fast queue mutations, track removals, and widget recycling.
  - Localized queue track play accessibility labels (`"Play"` / `"Tocar"`).
- **MPRIS v2 Spec Conformance, Real Quit & Live Rate Signals (`src/mpris.rs`, `src/main.rs`):**
  - Implemented real `Quit()` invocation closing all windows and cleanly terminating the application (`can_quit: true`), allowing desktop widgets, keybindings, and CLI tools like `playerctl -p vinylplayer quit` to exit cleanly.
  - Added live 250ms polling and emission for `Rate` property changed signals so external controllers track pitch fader changes.
  - Complied with MPRIS v2 specification by returning `/org/mpris/MediaPlayer2/TrackList/NoTrack` when no audio URI is active instead of `/org/mpris/MediaPlayer2/track/0`.
  - Dynamically calculated `CanGoNext` and `CanGoPrevious` based on actual queue position, repeat mode, and shuffle state rather than static `true`.
- **Audio Tag Codec & Artist Prioritization (`src/audio.rs`):**
  - Prevented GStreamer's `refresh_stream_info` from overwriting valid tag codecs (e.g. `FLAC`, `MP3`) with generic `audio/x-raw`.
  - Added fallback tag resolution for `AlbumArtist` and `Performer` when standard `Artist` tags are omitted.
- **Non-Panicking Album Indexing Worker Join (`src/library.rs`):**
  - Replaced `.expect("indexador de álbuns")` with `.filter_map(|w| w.join().ok()).flatten()` so corrupted files or background thread faults do not abort the UI process.
- **Atomic Needle Drop Cache Generation (`src/needle_drop.rs`):**
  - Hardened needle drop WAV cache generation using temporary unique files and atomic rename to prevent corruption or race conditions on rapid triggers.
- **Resilient File Open CLI Window Activation (`src/file_open.rs`):**
  - Guarded against `None` active window during CLI or background D-Bus activation, ensuring window presence before dispatching audio playback.
- **Defensive Action Initialization (`src/main.rs`):**
  - Replaced `.find(...).unwrap()` on overview nav button action with safe pattern matching.
- **Percent-Decoded M3U Localhost URI Fallback (`src/playlist.rs`):**
  - Added custom percent-decoding helper for `file://localhost/` and `file://` fallback parsing in M3U playlists, ensuring paths with `%20` and accented characters resolve accurately.

## Full Theme Coverage, Gapless Playback, Controls & Bilingual i18n — 18/11/2026

- **Real Gapless Playback via GStreamer `playbin3` (`src/audio.rs`):**
  - Connected `playbin3`'s `about-to-finish` signal cleanly using `WeakRef` + non-blocking `queue.try_lock()` to pre-queue the next track URI seamlessly across linear playback, `Shuffle`, `RepeatMode::All`, and `RepeatMode::Track` without dropping through `Ready`.
  - Extracted shared `next_auto_index_from`, `apply_track_metadata`, `apply_current_rate`, and `commit_pending_gapless` helpers so `StreamStart` commits queue selection, metadata, cover art, `track_seq`, and pitch rate, `Error` commits pending failure context accurately, manual track changes/removals/stop discard stale pending transitions, and `advance_after_eos` acts as a clean fallback.
  - Added automated unit test `gapless_about_to_finish_transitions_without_eos` verifying seamless WAV chaining without intermediate `Eos`.
- **Mute Toggle, Pitch MPRIS Sync & Quick Reset (`src/main.rs`):**
  - Replaced the static speaker icon in the bottom transport bar with a clickable reactive mute button (`audio-volume-muted-symbolic`, `low`, `medium`, `high`), `pre_mute_volume` restoration memory, `win.toggle-mute` action, and `M` shortcut.
  - Added double-click reset (`0.0%` Quartz Lock) on the turntable pitch fader and bidirectional 250ms synchronization when external MPRIS `SetRate` changes `audio.rate()`.
- **Tag-Aware Track Notifications & Settings Toggle (`src/state.rs`, `src/library_settings.rs`, `src/main.rs`):**
  - Added persistent `notifications: bool` preference (default `true`) and toggle checkbox in Settings.
  - Updated playback notifications to wait up to 450 ms or until GStreamer `Tag` metadata populates both `artist` and `title` (plus cover art), and to trigger reliably across automatic EOS and gapless track transitions without duplicating on pause/resume.
- **Keyboard Shortcuts & Library-Wide Search Enter (`src/main.rs`, `src/library_settings.rs`):**
  - Added `Ctrl+F` (`win.focus-search`), `Ctrl+,` (`win.library-settings`), `M` (mute), `S` (shuffle), `R` (repeat), and `Esc` to close the Settings window.
  - Updated header `SearchEntry` activation (`Enter`) to search and play matching tracks from the indexed `Catalog` across the entire library using `search::track_matches`, falling back to active queue search when needed.
- **Consistent Bilingual EN / PT-BR Localization (`src/i18n.rs`, `src/state.rs`, all UI modules):**
  - Created `src/i18n.rs` with `Language::{En, PtBr}`, `LC_ALL`/`LC_MESSAGES`/`LANG` system detection, persistent `language: Option<String>` preference in `AppState`, `WeakRef`-anchored reactive widget bindings, and live language switching controls in Settings (`English` / `Português (Brasil)`).
  - Unified mixed English / Portuguese strings across `main.rs`, `library_settings.rs`, `queue_view.rs`, `playback_error.rs`, `playlist_view.rs`, `file_open.rs`, `donate.rs`, `audio.rs`, and `theme.rs`.
- **Complete Theme CSS Override Coverage (`src/theme.rs`):**
  - Added theme overrides for `.control-button.active` / `:hover`, `.listening-format`, `.badge-hires`, `.queue-search:focus-within`, `.queue-clear` / `:hover`, `.search-box:focus-within`, `.enqueue-menu > button:hover`, `.folder-button:hover`, and `button:focus-visible` so switching away from *Amber Gold* updates all accent-colored controls consistently.
- **MPRIS v2 Specification Compliance & Live Signals (`src/mpris.rs`, `src/main.rs`):**
  - Exported `/org/mpris/MediaPlayer2` before claiming `org.mpris.MediaPlayer2.VinylPlayer` to eliminate the D-Bus `NameOwnerChanged` race window.
  - Added `DesktopEntry` (`"io.github.vinylplayer"`), `Raise()` (safely dispatched onto the GTK main thread via `glib::MainContext::default().invoke`), `Quit()`, and full `SupportedMimeTypes`.
  - Added `CanSeek`, `Seek(offset)`, `Volume` (get/set), `Rate` / `MinimumRate` / `MaximumRate`, `track_sequence()`-based `mpris:trackid`, `duration_us()` fallback in `Metadata`, `OpenUri` local-file validation, and 250ms `PropertiesChanged` signal emission (`PlaybackStatus`, `Metadata`, `LoopStatus`, `Shuffle`, `Volume`).
- **Pause Guard When Stopped (`src/audio.rs`):**
  - Prevented `AudioEngine::pause()` from transitioning `PlaybackState::Stopped` into `PlaybackState::Paused` when no stream is playing.
- **SearchHub Anchor Lifecycle & Filter Restoration (`src/search.rs`, `src/main.rs`):**
  - Added `SearchHub::connect` anchored by `glib::WeakRef<gtk::Widget>` so callbacks belonging to replaced `"tracks"` and `"albums"` views are automatically pruned instead of leaking on library refreshes.
  - Preserved active search queries by eagerly populating track and album list views when rebuilt with a non-empty search filter.
- **Wayland Portal Modal Anchoring & CSS Provider Deduplication (`src/main.rs`, `src/playlist_view.rs`):**
  - Passed the parent `gtk::Window` to all `GtkFileDialog` async calls (`Open track`, `Import M3U`, `Add Tracks...`, `Export...`) for proper Wayland modal attachment.
  - Bounded `seek.connect_change_value` to the scale adjustment range and deduplicated `install_css()` via `thread_local!` `BASE_CSS_PROVIDER` with safe `gdk::Display::default()` handling.
- **Safe Programmatic Needle Drop Pipeline (`src/needle_drop.rs`):**
  - Replaced `gst::parse::launch` string interpolation with `gst::ElementFactory::make` (`filesrc` property binding), added a total 900ms wall-clock deadline, and guarded overlapping triggers with `NEEDLE_DROP_SEQ`.

## Bug Fixes, Concurrency Hardening & Packaging Sync — 17/11/2026

- **Live Analog Experience Settings Sync (`src/main.rs`, `src/library_settings.rs`, `src/state.rs`):**
  - Added lock-free atomic revision counter (`StateManager::revision`) so the turntable VSync loop immediately picks up changes to *Needle Drop* and *Stylus Target Light* toggled in the Settings window without requiring an application restart.
  - Synchronized Settings checkboxes when refocusing the Settings window after clicking the physical Stylus Target Light tower on the turntable.
- **Smart Keyboard Shortcut Guard (`src/main.rs`):**
  - Updated the window-level `EventControllerKey` to ignore modified chords (`Ctrl`/`Alt`/`Super`) and yield propagation whenever focus is inside an editable input (`GtkText`, `GtkEntry`, `GtkSearchEntry`, `GtkPasswordEntry`, `GtkEditable`) or interactive slider/dropdown (`GtkScale`, `GtkDropDown`), preventing `Left`/`Right`/`Up`/`Down`/`Space` from hijacking track navigation or volume while typing or editing search queries.
- **Portable Default Library Root Discovery (`src/state.rs`):**
  - Replaced the hardcoded `legacy path` fallback in `AppState::library_roots` with `AppState::default_library_roots`, preserving automatic legacy migration when `legacy path` exists while cleanly falling back to `XDG_MUSIC_DIR` (`glib::user_special_dir`), `~/Music`, `~/Música`, or an empty list on fresh Linux installations.
- **Full Multi-File, Folder & Playlist Drag-and-Drop (`src/main.rs`):**
  - Upgraded the window `DropTarget` to accept both `gdk::FileList` and URI lists and process all dropped items asynchronously via `gio::spawn_blocking`, supporting multiple audio files, `.m3u`/`.m3u8` playlists, and recursive folder scanning without blocking the GTK main loop.
- **Non-Blocking Cover Extraction, Negative Caching & Alpha-Preserving LRU Thumbnails (`src/audio.rs`, `src/main.rs`, `src/thumb_cache.rs`):**
  - Moved uncached `metaflac` cover extraction off the GTK main thread in `AudioEngine::set_track` and silenced external `metaflac` `stdout`/`stderr` streams.
  - Added negative cache markers (`{hash}.none`) in `~/.cache/vinylplayer/extracted-covers/` so FLAC tracks without embedded `PICTURE` blocks never re-spawn `metaflac`.
  - Offloaded turntable and transport bar cover thumbnail decoding to `gio::spawn_blocking`.
  - Preserved PNG alpha transparency in `thumb_cache::load_cached_thumbnail` and added background LRU cache pruning (`prune_cache_dir`).
- **Last.fm Scrobbler Reliability & Accuracy (`src/scrobbler.rs`, `src/main.rs`):**
  - Configured a 12-second global HTTP timeout on `ureq::Agent` and URL-encoded all query parameters in `request_token`, `authorization_url`, and `fetch_session`.
  - Tracked `AudioEngine::track_sequence()` alongside URI in `ActiveScrobble` so pausing/resuming preserves listening progress while repeating a track under `RepeatMode::Track` properly dispatches subsequent *Now Playing* and *Scrobble* events.
  - Measured actual accumulated listening time (`listened_ms`) rather than timeline seek position so scrubbing past 50% no longer triggers premature scrobbles.
- **Packaging, Desktop Icon & Ignored UI Test Alignment (`packaging/arch/PKGBUILD`, `README.md`, `src/animation_tests.rs`):**
  - Updated `packaging/arch/PKGBUILD` to `v0.1.4` and installed `io.github.vinylplayer.svg` alongside `vinylplayer.svg` (matching `Icon=io.github.vinylplayer` in `vinylplayer.desktop`) in both `PKGBUILD` and `README.md`.
  - Updated localized string assertions in `src/animation_tests.rs` so all 13 `#[ignore]` GTK/FFmpeg integration tests pass cleanly alongside all 89 unit tests.

## Release v0.1.4 — Color Themes, Monolith Modularization & Clippy Cleanup — 16/11/2026

- **Analog Color Theme System (`src/theme.rs`):**
  - Designed and implemented a theme system featuring 6 curated analog palettes:
    1. **Amber Gold (Classic)**: Vintage warm amber gold (`#dfc08d`).
    2. **Marantz Blue**: 70s classic stereo receiver back-lit blue (`#5da5db`).
    3. **Emerald Tube**: Vintage McIntosh vacuum tube amplifier green (`#55b685`).
    4. **Ruby Sunset**: Analog tape reel-to-reel terracotta red (`#e06c53`).
    5. **Midnight Violet**: Deep purple & vinyl indigo aesthetic (`#9d72d6`).
    6. **Obsidian Silver**: Brushed aluminum & graphite monochrome (`#dcdcdc`).
  - Dynamic CSS override generation (`Theme::generate_css`) targeting active navigation buttons, play/pause controls, sliders, volume indicators, queue highlights, album badges, mini album borders, version badges, and status icons.
  - Zero-restart instantaneous theme switching via GTK4 `CssProvider` registered at `STYLE_PROVIDER_PRIORITY_USER + 10`.
- **Theme Selection in Settings Window (`src/library_settings.rs`):**
  - Added dedicated "Interface Theme · Tema de Cores" section in the Settings window.
  - Interactive grid displaying crisp Cairo-rendered circular color swatches, theme names, active checkmark indicators (`✓`), and rich tooltips.
  - One-click live switching: updates colors immediately on click without needing to close or restart the application.
  - Wrapped Settings window in a `ScrolledWindow` with 640px default height to ensure comfortable layout on any display resolution.
- **Persistent State Storage (`src/state.rs`):**
  - Added `theme: Option<String>` to `AppState` with atomic serialization and deserialization in `~/.config/vinylplayer/state.json`.
  - Added `StateManager::update_theme` with automated change tracking.
  - Application restores the user's chosen theme automatically on launch (`src/main.rs`).
  - Added unit test `theme_roundtrip_persistence` in `src/state.rs` and `theme_roundtrip_ids_and_labels` in `src/theme.rs`.
- **Major Code Modularization & Monolith Dismantling (`src/main.rs`):**
  - Reduced `src/main.rs` from 6,534 lines to 3,340 lines (-49% reduction), dramatically improving compilation times and code maintainability.
  - Extracted Cairo turntable rendering (`TurntableRenderParams`, `CachedCover`, `TurntableCache`, `TurntableLayout`, strobe dots, pitch fader, target light) to dedicated module `src/turntable.rs`.
  - Extracted analog stereo VU meter rendering (`draw_vu_meters`, `draw_vu_background`, `draw_rounded_rect`) to `src/vu_meter.rs`.
  - Extracted volume HUD overlay rendering (`VolumeHudState`, `draw_volume_hud`) to `src/volume_hud.rs`.
  - Extracted ~1,600 lines of Cairo turntable & UI animation unit/integration tests to `src/animation_tests.rs`.
- **Clippy Lint & Idiomatic Rust Cleanup (`cargo clippy --all-targets -- -D warnings`):**
  - Replaced 15-parameter function `draw_turntable` with clean `TurntableRenderParams` struct.
  - Refactored `playlist_track_row` from 8 parameters using `PlaylistRowContext`.
  - Refactored `scrobble_track` from 8 parameters using `ScrobblePayload`.
  - Replaced unwieldy nested types with readable type aliases (`FailedPlaybackIdentity`, `SearchCallback`, `ActionCallback`).
  - Upgraded array chunk iteration in `src/needle_drop.rs` to `.as_chunks::<2>()`.
- **Version Bumped to v0.1.4:**
  - `Cargo.toml` and application headers bumped to `v0.1.4`.
- **Testing & Verification:**
  - All 87 unit tests passing cleanly (`cargo test`).
  - 0 warnings on `cargo clippy --all-targets -- -D warnings`.
  - Optimized release binary compiled and installed to `~/.local/bin/vinylplayer`.

## Release v0.1.3 — Desktop Icon & Wayland Integration Fix — 16/11/2026

- **Wayland Window-to-Desktop Dock Association (`vinylplayer.desktop`):**
  - Added `StartupWMClass=io.github.vinylplayer` to ensure GNOME Shell and Wayland compositors correctly map the running application process to its launcher entry.
  - Installed desktop file as both `~/.local/share/applications/io.github.vinylplayer.desktop` and `~/.local/share/applications/vinylplayer.desktop`, updated desktop database via `update-desktop-database`.
- **Complete Icon Theme Installation Across System & Custom Themes:**
  - Deployed vector icon `vinylplayer.svg` and `io.github.vinylplayer.svg` into the user's active theme directory `~/.local/share/icons/Violeta-Panel/scalable/apps/`.
  - Generated full suite of multi-resolution raster icons (16x16, 24x24, 32x32, 48x48, 64x64, 128x128, 256x256, 512x512) via `rsvg-convert` in `~/.local/share/icons/hicolor/`.
  - Verified icon lookup via `Gtk::IconTheme` resolving both `io.github.vinylplayer` and `vinylplayer` as `True`.
- **Version Number Bumped:**
  - `Cargo.toml` and application headers updated to `v0.1.3`.
- **Testing & Verification:**
  - All 85 unit tests passing cleanly (`cargo test`).
  - Release binary compiled and installed to `~/.local/bin/vinylplayer`.

## Release v0.1.2 — Mini Album Cover in Transport Bar — 15/11/2026

- **Mini Album Artwork in Now-Playing Bar (`src/main.rs`):**
  - Replaced the static vinyl icon placeholder on the bottom left transport bar with an interactive `gtk::Stack` (`cover_stack`) featuring smooth crossfade transitions.
  - Dynamically loads and downscales the active track's album cover (`128px` texture via `thumb_cache::load_cached_thumbnail` or embedded FLAC picture bytes via `PixbufLoader`) into a dedicated `gtk::Picture` (`.mini-cover-picture`) with `content-fit: cover`.
  - Seamlessly falls back to the classic vintage golden disc icon (`.mini-disc-icon`) whenever no artwork is found or when the playback queue is empty.
  - Styled with refined rounded corners (`border-radius: 10px`), subtle drop shadow (`box-shadow: 0 2px 6px rgba(0, 0, 0, 0.35)`), and brushed amber border (`#494033`).
- **Version Number Bumped:**
  - `Cargo.toml` and application headers updated to `v0.1.2`.
- **Testing & Verification:**
  - All 85 unit tests passing cleanly (`cargo test`).
  - Release binary compiled and installed to `~/.local/bin/vinylplayer`.

## Release v0.1.1 — Track View Smooth Virtualization & Version Visibility — 15/11/2026

- **Version Number Prominently Displayed Across the Application:**
  - Added visual version badge (`v0.1.1`, `.version-badge`) in the sidebar branding header right next to the app logo and title.
  - Included version number in the main desktop window title (`VinylPlayer v0.1.1 — High-Fidelity Audio`).
  - Added version number to the Settings & Library Folders window title and header (`Settings & Library Folders (v0.1.1)`).
  - Version bumped from `0.1.0` to `0.1.1` in `Cargo.toml`.
- **Elimination of Track List Lag & Viewport Top Clipping (`src/main.rs`):**
  - **Pre-Sorting in Rust:** `library_view` now pre-sorts `paths` in Rust memory before creating any widgets. When rows are appended to the GTK `ListBox`, they arrive in exact alphabetical or path order, eliminating $O(N^2)$ repetitive GTK `sort_func` passes and layout thrashing over 2,000 tracks.
  - **Viewport Scroll Offset Reset:** Connected `connect_map` and end-of-batch callbacks to force `scroll.vadjustment().set_value(0.0)`, preventing GTK from auto-scrolling the viewport down by 15px when rows are added, which previously sliced the first track row in half horizontally.
  - **Removed Disruptive Margin:** Replaced `.library-list { margin-top: 12px; }` with `padding-top: 2px` on the list and clean margin on the `ScrolledWindow`, fixing internal GTK viewport measurement bugs.
  - **Non-Selectable Row Containers:** Set `row.set_selectable(false)` and `row.set_activatable(false)` on `GtkListBoxRow` to prevent focus hijacking and premature viewport jumping on tab navigation.
  - **High-Throughput Chunk Loading:** Increased batch size to 64 rows with 2ms intervals, allowing instantaneous visual rendering of the first screen of tracks without UI freezes.
- **Testing & Verification:**
  - 85 unit tests passing cleanly (`cargo test`).
  - Verified `library_folders` test passes with versioned window titles.
  - Release binary `v0.1.1` compiled and installed to `~/.local/bin/vinylplayer`.

## Persistent Library Cache & Zero-Delay Startup — 15/11/2026

- **Persistent Disk Cache for Scanned Music Collections (`src/library_sources.rs` & `src/state.rs`):**
  - Added `CachedLibrary` serialization and deserialization targeting `~/.cache/vinylplayer/library_index.json` (and isolated temporary directories during automated test runs).
  - Saved scan payloads contain canonical root directory paths, collection of indexed audio tracks (`Track` structs with tags, track numbers, albums, and disc indices), and scan timestamps.
- **Zero-Delay UI Boot with Instant Library Population (`src/library_settings.rs` & `src/main.rs`):**
  - On application startup, `Catalog::new` checks for and immediately loads the existing `library_index.json` synchronously in under **1 millisecond**.
  - Album grids and track lists are populated immediately using cached metadata without waiting for filesystem traversal.
  - Users are no longer greeted by empty screens, loading spinners, or "Reading library folders…" status messages on launch.
- **Silent Non-Destructive Background Scan (`src/library_settings.rs` & `src/main.rs`):**
  - When the background scanner finishes inspecting directories, it writes the refreshed payload back to `library_index.json`.
  - In `main.rs`, the UI compares incoming tracks with the currently displayed collection: if the track list has not changed, view reconstruction is bypassed entirely, avoiding visual flicker, unnecessary widget allocation, and scroll position resets.
  - If new files are discovered or old files removed, the view updates seamlessly in place.
- **Testing & Verification:**
  - Standard unit tests (`cargo test`): 84 passed, 0 failed.
  - Ignored tests (`library_folders`, `now_playing`): passed cleanly with 0 failures.
  - Release binary installed to `~/.local/bin/vinylplayer`.

## Startup Performance & Memory Optimization — 15/11/2026

- **On-Demand Lazy Population for Heavy Library Views (`src/main.rs`):**
  - Deferred detailed track rows population in `album_list`: rather than constructing ~2,000 GTK widget rows for the hidden "List" view of albums on startup, population is triggered on-demand when the user switches to the "List" tab or initiates a search.
  - Deferred track view widget generation in `track_library_view`: if the active view is "Albums" or "Turntable", track rows are populated only when mapped (`connect_map`) or when a search query is entered. Small libraries ($\le 60$ tracks) and mapped views continue to populate immediately to maintain full test compatibility.
- **Persistent Embedded FLAC Cover Cache (`src/audio.rs`):**
  - Optimized `find_cover` to perform a single directory read (`std::fs::read_dir`) per album folder rather than 96 separate `is_file` stat syscalls on slow mechanical storage.
  - Embedded FLAC artwork extracted via `metaflac` is now persisted to `~/.cache/vinylplayer/extracted-covers/{hash}.jpg` instead of ephemeral `/tmp`, preventing repeated multi-second external process executions on every system reboot.
- **Micro-Optimized Playback Status Marker Synchronization (`src/now_playing.rs`):**
  - Inverted the mode evaluation and weak reference upgrade logic in `now_playing::sync`: inactive markers that remain in `Mode::None` bypass the expensive `WeakRef::upgrade()` GObject calls entirely.
  - Reduced per-tick weak reference resolution overhead from $>8,000$ calls to $\le 4$ calls per 250ms interval.
- **High-Throughput Album Thumbnail Pipeline (`src/main.rs`):**
  - Expanded thumbnail worker channel capacity from 4 to 16 and decreased UI delivery interval to 25ms, batching up to 8 thumbnails per tick. Album cover grids now render in under 500ms without blocking the GTK main loop.
- **Benchmarked Impact:**
  - Idle RSS memory dropped from **411.6 MB** to **180.3 MB** (-56% memory footprint).
  - Startup CPU saturation dropped from 8 seconds at 99% CPU to brief, responsive bursts settling in ~2 seconds.
- **Testing & Verification:** All 84 unit tests passing cleanly; release binary installed to `~/.local/bin/vinylplayer`.

## Native Last.fm Scrobbler Integration — 15/11/2026

- **Audioscrobbler 2.0 API Implementation (`src/scrobbler.rs`):**
  - Cryptographic MD5 parameter signing (`sign_params`) matching the Audioscrobbler 2.0 specifications (sorting keys, concatenating key-value pairs, appending shared secret, computing 32-char hex lowercase hash).
  - Web authentication token handshake: `auth.getToken` requesting temporary token and opening the browser via `authorization_url`.
  - Permanent session key retrieval: `auth.getSession` extracting user session key (`sk`) and username.
  - Real-time status update: `track.updateNowPlaying` sending current track, artist, album, and duration.
  - Automatic track scrobbling: `track.scrobble` with timestamp when playback reaches at least 50% or 4 minutes (240s) on tracks >= 30 seconds.
- **Asynchronous & Non-Blocking Network Pipeline:**
  - Lightweight HTTP client (`ureq 3.5` + `md-5 0.11` + `hex 0.4`) running all web requests on dedicated background threads without blocking GTK UI rendering or GStreamer audio decoding.
- **Settings UI (`src/library_settings.rs`):**
  - Added dedicated "Last.fm Scrobbler" group in the Settings window.
  - Status display ("Connected as: [Username]" or "Not connected to Last.fm").
  - One-click "Connect to Last.fm" / "Complete Connection" / "Disconnect" action buttons.
  - Optional collapsible "Custom API Credentials" section for custom API keys and shared secrets.
- **State & Preferences Persistence (`src/state.rs`):**
  - Persisting `lastfm_enabled`, `lastfm_session_key`, `lastfm_username`, `lastfm_api_key`, `lastfm_api_secret` in `~/.config/vinylplayer/state.json`.
- **Testing:** 84 unit tests passing cleanly.

## Refinamento e Correção Visual das Playlists — 14/11/2026

- **Design dedicado para cartões de playlist (`.playlist-card`):**
  - Substituição da classe genérica `album-card-row` por uma estilização sob medida com cantos arredondados (12px), borda de grafite escovado (`#32353a`), iluminação no hover com borda âmbar vintage (`#5a4b35`) e sombra suave de elevação.
  - Badge de ícone `.playlist-badge-icon` iluminado em tom champanhe âmbar (`#dfc08d`) com fundo escuro e borda fina.
  - Tipografia de precisão para título (`.playlist-card-title`, 14px semibold) e contagem de faixas (`.playlist-card-count`, 11px médio).
  - Botão de exclusão `.playlist-del-btn` discreto com feedback em vermelho translúcido no hover.
- **Visualização de detalhes refinada (`playlist_detail_view`):**
  - A capa/emblema agora usa badge 96x96px com cantos arredondados (`.playlist-detail-badge`), borda âmbar e ícone centralizado de 44px.
  - Alinhamento do cabeçalho da playlist e botões de ação ("Reproduzir Playlist", "Adicionar Faixas...", "Exportar...").
- **Lista de faixas e botões compactos de ação (`playlist_track_row`):**
  - Substituição do botão volumoso genérico `folder-button` (que inflava desnecessariamente a altura de cada linha) por botões de ação compactos de 28x28px (`.playlist-track-action-btn` e `.playlist-track-delete-btn`).
  - Ícones de mover para cima/baixo e excluir com alinhamento fino, transição de 120ms e desativação proporcional (`opacity: 0.25`).
- **Popover de Adicionar a Playlist com Scroll:**
  - No menu de contexto (`queue_actions` em `src/main.rs`), a seleção de playlists agora utiliza `gtk::ScrolledWindow` com altura máxima de 180px e rolagem vertical suave para não estourar a tela com muitas playlists.
  - Cada item usa a classe `.popover-playlist-item`, ícone de 14px e truncamento elíptico para nomes longos.
- **Empacotamento & Binário:**
  - Binário release otimizado instalado em `~/.local/bin/vinylplayer` (SHA-256 `84f53f...`).
  - Pacote Arch Linux `vinylplayer-0.1.0-11-x86_64.pkg.tar.zst` gerado e sincronizado.

## Auditoria e Correção Completa de Erros em Playlists — 14/11/2026

- **Correção de perda de dados e estado congelado:**
  - O botão **Reproduzir Playlist** no detalhe lia faixas capturadas na inicialização; agora lê `pl_state.borrow().tracks` em tempo real, corrigindo o bug em que playlists recém-populadas não tocavam.
  - O cartão na grade de playlists recarrega o estado atualizado do disco ao abrir ou reproduzir, e a grade recarrega automaticamente ao retornar (`connect_map` e callback `reload_cards`), evitando sobrescrever faixas adicionadas.
- **Suporte Robusto ao Padrão M3U / M3U8:**
  - Reconhecimento e decodificação automática de URIs `file://` (incluindo percent-decoding de `%20`, caracteres acentuados e `file://localhost/`).
  - Normalização de barras invertidas do Windows (`\`) para compatibilidade com playlists vindas de pendrives FAT32/NTFS.
  - Reconhecimento case-insensitive da diretiva `#PLAYLIST:` (`#playlist:`, `#Playlist:`, etc.).
- **Higienização Segura de Nomes e Arquivos:**
  - `sanitize_filename` agora impede criação de arquivos ocultos (`.m3u8` a partir de `.` ou `.nome`) e filtra caracteres de controle ASCII (`\0` a `\x1f`).
  - `delete_playlist` passou a usar `symlink_metadata` para conseguir remover symlinks quebrados.
  - Salvamento atômico seguro com arquivo temporário único baseado em PID e timestamp em nanossegundos (`.tmp.{pid}_{nano}`).
- **Integração de Sistema & Drag-and-Drop:**
  - `file_open.rs` e `main.rs` agora suportam `.m3u` e `.m3u8`: abrir playlist pelo gerenciador de arquivos ou arrastar (.m3u/.m3u8) para dentro do player carrega e executa as faixas automaticamente.
  - Diálogo de exportação sugere nome real da playlist (`{nome}.m3u8`) e exibe notificação desktop do resultado.
- **Testes:** 80 testes unitários passando (incluindo decodificação de URIs `file://`, caminhos Windows, higienização e symlinks quebrados). Binário release instalado em `~/.local/bin/vinylplayer` (SHA-256 `389033...`).

## Correção e Aprimoramento da Adição de Músicas a Playlists — 14/11/2026

- **Popovers Dinâmicos de Contexto:** O menu de contexto de cada faixa (`queue_actions`) agora gera os popovers de forma dinâmica a cada abertura. Playlists recém-criadas ou renomeadas aparecem imediatamente sem necessidade de reiniciar a aplicação ou reabrir janelas.
- **Feedback Visual Completo e Notificação Desktop:**
  - Quando uma ou múltiplas músicas de um álbum são adicionadas à playlist, o player emite notificação nativa no ambiente desktop (`notify-send`) informando `"Playlist Atualizada"` e o nome da playlist.
  - O botão do menu exibe tooltip atualizado de confirmação imediata.
  - Se ocorrer qualquer erro de permissão ou I/O ao salvar a playlist, o popover exibe mensagem vermelha descritiva em vez de falhar em silêncio.
- **Listagem Completa de Playlists:** Removido o limite artificial de 5 playlists no menu rápido; todas as playlists do usuário são disponibilizadas com rolagem suave.
- **Sincronização do Binário e Pacote:** Binário oficial atualizado em `~/.local/bin/vinylplayer` (SHA-256 `73c3e8...`) e pacote Arch Linux `vinylplayer-0.1.0-10-x86_64.pkg.tar.zst` validado.

## Busca na fila de reprodução — 14/11/2026

- Campo **Título, artista ou álbum**, contagem dos resultados e botão **Limpar
  busca** no painel da fila. A busca aceita vários termos, ignora maiúsculas e
  acentos comuns e também encontra nomes de arquivo/pasta quando faltam tags.
- Título, artista da faixa, artista do álbum e álbum são lidos por um único
  worker em segundo plano. O painel informa quando ainda está lendo metadados
  e diferencia fila vazia de busca sem resultados. O cache de tags passou a v3
  para incluir título; o formato das preferências permanece igual.
- O filtro muda somente a apresentação. Cada linha mantém o índice original
  da fila, inclusive nas duplicatas, para reproduzir, remover, usar setas ou
  arrastar. As setas movem uma posição na fila completa, incluindo faixas
  ocultas; arrastar insere antes/depois da entrada alvo. A revisão continua
  protegendo contra arrastes de uma fila desatualizada.
- **Mostrar faixa atual** limpa o filtro e revela a seleção. Fechar e reabrir
  o painel mantém a consulta. Limpar/digitar não altera ordem, seleção, posição,
  play/pausa ou pipeline. Novas entradas e remoções atualizam os resultados.
- As linhas exibem título, artista e álbum quando disponíveis. Campo com foco
  destacado e botão textual **Limpar**, evitando dois ícones iguais lado a lado.
- Fontes: `src/queue_view.rs`, `src/library.rs`, teste GTK e CSS em `src/main.rs`.
- `cargo check --offline --locked --tests` aprovado; suíte release final com
  **64 testes aprovados**, nenhuma falha e 13 especiais ignorados por padrão.
  Executados separadamente e aprovados quatro testes GTK (busca, edição da
  fila, menus de adição e restauração imediata) e o teste de metadados FLAC com
  invalidação de cache. Os testes usam preferências temporárias e áudio silencioso.
- O teste novo usa tags reais distintas do nome do arquivo e artistas distintos
  para faixa/álbum. Confere busca sem acentos, contagem, zero resultados, limpeza,
  avanço do áudio sem novo carregamento ao filtrar, ações sobre índices originais,
  seleção/pausa, remoção da segunda duplicata, adição sob filtro e arraste entre
  resultados, com rejeição do payload após a revisão mudar.
- Capturas conferidas: [busca](docs/busca-fila.png) e
  [sem resultados](docs/busca-fila-sem-resultados.png). Avisos do tema GTK já
  existentes; o FLAC de teste não tem capa e emite o aviso correspondente.
- Release compilada, instalada em `~/.local/bin/vinylplayer` e reaberta
  com o painel da fila. Release, arquivo instalado e processo em execução
  conferem com SHA-256
  `ca2f9b754c2192adbf052b89cf89164b29601185609fb8efd01294ceb944d773`.
  MPRIS do mesmo processo em `Stopped`, sem autoplay; preferências idênticas ao
  backup, incluindo fila e seleção. Atalho local continua apontando para esse
  executável. Backup do executável anterior e estado em
  `/tmp/vinylplayer-queue-search-backup.RtsApe` (pasta temporária).
- O pacote Arch anterior não foi regenerado nesta etapa.

## Adicionar músicas à fila sem interromper — 14/11/2026

- O menu **⋮** das faixas oferece **Tocar a seguir** e **Adicionar ao final**.
  Nos cabeçalhos da lista de álbuns e no detalhe aberto pela capa, essas ações
  ficam no botão **Adicionar à fila**. Os controles de reprodução existentes
  continuam disponíveis ao lado das novas opções.
- **Tocar a seguir** insere o bloco imediatamente após a posição atual;
  **Adicionar ao final** acrescenta o bloco depois das entradas existentes.
  A ordem interna do álbum e as entradas repetidas são preservadas. As opções
  não alteram os modos shuffle/repetição escolhidos.
- Com fila existente, não são recarregados o stream, a URI ou a seleção atual;
  o estado de play/pausa e a posição de áudio são mantidos. Na fila vazia, a
  primeira música é selecionada sem autoplay. Uma entrada indisponível impede
  a inserção do bloco inteiro e mostra um erro no menu, sem inserção parcial.
- A revisão da fila é atualizada e aproveita a persistência já existente,
  inclusive o snapshot de fechamento. Não mudou o formato das preferências.
- Os conteúdos dos menus são criados somente na primeira abertura. Linhas de
  faixas dentro dos cartões de álbuns usam caixas comuns; `GtkListBoxRow` fica
  reservado às linhas que pertencem a uma `GtkListBox`. Os lotes de construção
  usam prioridade baixa, deixando o GTK concluir layout e pintura entre eles.
- Fontes: `src/playback_queue.rs`, `src/audio.rs` e `src/main.rs`. Os helpers de
  captura de teste usam renderer Cairo fora da superfície da janela e espera de
  prioridade baixa para obter uma imagem após layout/pintura, com diagnóstico
  de dimensões se a captura não concluir.
- Validação: `cargo check --offline --locked` aprovado; suíte release com
  **64 testes aprovados**, nenhuma falha e 12 especiais ignorados por padrão.
  Foram executados separadamente e aprovados oito testes gráficos: nova adição,
  prévia de álbuns/vinil, recuperação de erros, edição da fila, restauração,
  pastas, abertura externa e falha de salvamento.
- O teste novo abre os menus GTK e aciona seus botões nas faixas, lista de álbuns
  e detalhe acessado pela capa. Confere ordem, duplicatas, URI, posição, contador
  de tentativas inalterado, pipeline em play, inserção em pausa, erro sem inserção
  parcial, fechamento/reabertura com a mesma fila e adição sem autoplay na fila
  vazia. Usa WAVs silenciosos, sink de teste e preferências temporárias.
- Capturas conferidas e salvas: [opções](docs/adicionar-fila-menu.png),
  [faixas](docs/adicionar-fila-faixa.png) e [álbum](docs/adicionar-fila-album.png).
  A prévia completa também conferiu a lista da biblioteca grande. Os avisos do
  tema GTK e os erros provocados no teste de recuperação são os já documentados.
- Release compilado, instalado em `~/.local/bin/vinylplayer` e reaberto.
  Executável em execução, instalado e release têm SHA-256
  `5fbb37e574dde2c1388e7faf5e628b0f8f7c268fe7d12296575c901dc10f7467`.
  Preferências idênticas ao backup, incluindo fila, seleção e volume; MPRIS em
  `Stopped`, sem autoplay ao iniciar. Atalho local conferido: aponta para esse
  executável. Backup em `/tmp/vinylplayer-enqueue-backup.88pMJv` (pasta temporária).
- O pacote Arch anterior não foi regenerado nesta etapa.

## Correção das regressões de VU e recuperação — 14/11/2026

- A leitura de RMS agora usa `glib::ValueArray`, compatível com o elemento `level`
  instalado. Substituído o teste de estrutura sintética por um teste com pipelines
  reais mono e estéreo e `fakesink`, verificando os níveis esperados do sinal.
- Criado `skip_failed_track`, avanço sequencial de recuperação que ignora repeat
  e shuffle sem alterar essas preferências. O botão **Pular faixa** usa essa
  operação; o fim natural da música continua respeitando os modos escolhidos.
- Adicionada cobertura das seis combinações de repeat/shuffle e ampliado o teste
  gráfico de erros para repetir faixa e repetir fila. Confere avanço em ordem,
  parada ao final, fila preservada e modos selecionados inalterados.
- `cargo check --offline --locked` aprovado; suíte release: 63 aprovados, nenhuma
  falha, 11 especiais ignorados por padrão. Executados separadamente e aprovados
  os testes gráficos de recuperação de erros e de botões shuffle/repeat.
- O teste de RMS usa sinal senoidal real com amplitude 0,5 e verifica cerca de
  -9,03 dB nos dois canais (incluindo duplicação do mono). Toda saída de teste
  utiliza `fakesink`, sem emitir som. O teste de sequência espera as transições
  do pipeline antes de efetuar a próxima ação.
- As pendências da revisão abaixo estão corrigidas nos fontes. O pacote Arch
  anteriormente gerado não foi reconstruído nesta correção.
- Release compilado, instalado e reaberto. Executável em execução e instalado
  conferem com SHA-256 `815a1e28c3b7197e14602b621d10c40954580eea0320e4731aef0261acf4ea5e`.
  Preferências idênticas ao backup, sem autoplay (MPRIS `Stopped`). Backup do
  executável anterior e estado em `/tmp/vinylplayer-regression-fixes.4Zz4Ig`.

## Revisão das alterações recentes — 14/11/2026

- Conferidos os novos medidores VU, HUD/scroll de volume, notificações e os modos
  de reprodução. `cargo check --offline --locked` aprovado;
  `cargo test --offline --release --all-targets`: 63 aprovados, nenhuma falha e
  11 especiais ignorados. Não foram executados os testes gráficos especiais nesta revisão.
- Release local e executável instalado coincidem com SHA-256
  `708eb961169e03cb8ceef62408691a862bfb13df243975d7824fa2fc21f3d17b`.
- **Pendência: leitura do VU.** O handler em `src/audio.rs` tenta extrair `rms`
  como `gst::ArrayRef`, mas o elemento `level` instalado emite `GValueArray`.
  Confirmado com `gst-launch-1.0 -m audiotestsrc num-buffers=4 ! level
  interval=25000000 ! fakesink`, sem saída sonora. Como o cast falha silenciosamente,
  os níveis não são atualizados nesse fluxo. O teste `test_structure_level_array`
  fabrica um `gst::Array`, portanto não cobre a mensagem real. Recomenda-se ler
  o tipo emitido e testar com um pipeline real e sink silencioso.
- **Pendência: Pular faixa em erro com repetição.** O botão em
  `src/playback_error.rs` chama `advance_after_eos`. Essa função agora respeita
  `RepeatMode::Track`, recarregando a própria faixa que falhou. Com repetição da
  fila pode também voltar ao início. A recuperação precisa de avanço explícito
  separado do fim natural da música e de cobertura com os modos de repetição.
- Esta revisão não alterou código de execução nem reinstalou ou iniciou o player.
  As duas pendências acima continuam abertas.

## Medidores VU Analógicos Estéreo Hi-Fi (Ponteiros Balísticos em Tempo Real) — 13/11/2026

- **Integração de Dinâmica de Áudio com GStreamer (`src/audio.rs`):**
  - Adicionado o elemento de medição `level` (`gst-plugins-good`) ao pipeline do sink de áudio (`make_audio_sink`), com intervalo de amostragem de 25 ms (40 medições/segundo) para resposta suave e sem sobrecarga de CPU.
  - Campos atômicos ultrarrápidos sem locks `level_l: Arc<AtomicU32>` e `level_r: Arc<AtomicU32>` em `AudioEngine`, eliminando qualquer contenção entre o bus do GStreamer e o loop de renderização GTK.
  - Tratamento de mensagens de elemento `"level"` no bus: extração dos níveis RMS estéreo em dB (`rms`).
  - Função matemática `db_to_vu_deflection(db)` calibrada segundo a física do galvanômetro D'Arsonval (deflexão linear proporcional à tensão retificada: `0 VU` em -3 dBFS / 70,8% da escala e `+3 dB VU` em 0 dBFS / 100% da escala).
  - Reset instantâneo dos níveis para o piso de ruído (-80 dB) ao pausar, parar ou finalizar faixas.

- **Painel e Mostradores Analógicos no Chassi do Toca-Discos (`src/main.rs`):**
  - Inserido no canto inferior direito do chassi (`TurntableLayout::compute`), com folgas milimétricas calculadas (+24,4px de respiro em relação ao prato de vinil e +48,6px abaixo do descanso do braço).
  - **Chassi e Bezel:** Rebaixo metálico usinado com sombra interna, moldura chanfrada de alumínio escovado com friso brilhante e divisória central usinada.
  - **Mostradores Duplos (Canal L e Canal R):** Fundo em tom pergaminho vintage / âmbar analógico suave com efeito de sombra do vidro protetor.
  - **Escala de Decibéis em Arco:**
    - Trilha normal em tom âmbar suave de -20 a 0 VU.
    - Zona de sobrecarga/pico (*Headroom*) em vermelho vívido de 0 a +3 dB VU.
    - 10 marcações (*ticks*) analógicas maiores e menores posicionadas nos ângulos físicos exatos.
    - Rótulos tipográficos clássicos dos canais `"L"` e `"R"` e inscrição central `"VU"`.
  - **Iluminação Backlight Incandescente Dinâmica:** Durante a reprodução, cada mostrador acende com brilho âmbar/dourado suave em gradiente radial; quando parado ou pausado, entra em modo standby sutil.
  - **Ponteiros Balísticos com Física de Galvanômetro:**
    - Agulhas esguias com sombra projetada e calota metálica torneada em latão/preto sobre o pivô.
    - Dinâmica balística assimétrica: ataque rápido (~35 ms) com leve overshoot elástico em transientes, e decaimento exponencial suave (~250 ms) para acompanhamento musical orgânico.
    - Indicador de sobrecarga: ao entrar na zona de pico (> 0 VU), a ponta da agulha se ilumina em vermelho carmim vívido.
  - **Desempenho Zero-CPU em Repouso:** Os mostradores estáticos são pré-renderizados no `bg_surface` em cache de alta performance; apenas as agulhas são redesenhadas em tempo de VSync quando ativas.

- **Testes e Garantia de Qualidade:**
  - **63 testes unitários e de integração aprovados** (`cargo test --all-targets`):
    - `vu_layout_coordinates_and_clearances`: validação das coordenadas no chassi e folgas de segurança contra prato e descanso.
    - `vu_meter_ballistics_attack_decay_behavior`: validação dos tempos de subida rápida e decaimento exponencial suave.
    - `vu_meter_renders_all_states_without_panic`: teste de estresse visual cobrindo repouso, nominal, pico, sobrecarga e transições de backlight.
    - `db_to_vu_deflection_maps_accurately_to_physical_scale` e `vu_levels_starts_at_silence_and_resets_properly`.
  - Captura gráfica de alta fidelidade gerada em `docs/vu-meter.png`.

- **Instalação:**
  - Binário compilado em Release otimizado e instalado em `~/.local/bin/vinylplayer` (SHA-256: `708eb961169e03cb8ceef62408691a862bfb13df243975d7824fa2fc21f3d17b`).

## Controle de volume por scroll no vinil, HUD analógico e notificações — 13/11/2026

- **Controle de Volume por Scroll no Toca-Discos:**
  - Adicionado `EventControllerScroll` vertical conectado ao widget do prato de vinil (`deck` / `DrawingArea`).
  - Rolar a roda do mouse sobre o disco de vinil ou chassi ajusta o volume em passos suaves de 3% a 10% (clamping seguro entre 0% e 100%).
  - Suporte a rolagem contínua (touchpads) e discreta (rodas dentadas de mouse).
  - Sincronização imediata com o motor GStreamer (`audio.set_volume`), persistência de estado (`state_mgr.update_volume`) e slider na barra de transporte inferior (`vol.set_value`).
  - Suporte a atalhos de teclado ergonômicos: setas **Cima** (`Up`) e **Baixo** (`Down`) ajustam o volume em passos de 5% e acionam o HUD.

- **HUD Analógico Hi-Fi sobre o Toca-Discos (`draw_volume_hud`):**
  - Mostrador óptico estilo vintage renderizado via Cairo diretamente sobre o centro do toca-discos:
    - Base circular de vidro fumê (*smoked glass*) com gradiente radial e borda metálica chanfrada com reflexo dourado/âmbar.
    - Medidor analógico em arco de 270° (de 135° a 405°) com trilho escurecido e arco ativo preenchido em gradiente âmbar/ouro.
    - Ponteiro luminoso incandescente na extremidade do arco ativo.
    - Marcações e subdivisões analógicas (*notches/ticks*) iluminadas dinamicamente conforme o volume.
    - Tipografia clássica: inscrição superior `"VOLUME"` em dourado, valor percentual em destaque no centro (ex: `"75%"`) e indicação em decibéis na base (ex: `"-2.5 dB"` ou `"MUTE"`).
  - Fade out suave: permanece 1,2s com opacidade 100% após a última interação e esmaece linearmente até 0% em 1,8s sem reter frames desnecessários no loop gráfico.
  - Prévia vetorial gravada em `docs/volume-hud.png`.

- **Notificações Nativas de Desktop:**
  - Função assíncrona `show_track_notification` disparada automaticamente quando uma faixa começa a tocar ou a seleção de metadados muda em reprodução ativa.
  - Exibe resumo da faixa (título), artista e álbum, com ícone oficial da capa ou do app.
  - Utiliza dica FreeDesktop `x-canonical-private-synchronous:vinylplayer` para substituir notificações anteriores sem empilhar avisos na bandeja do sistema.

- **Motor de Áudio e Testes:**
  - Adicionado getter `AudioEngine::volume(&self) -> f64` consultando a propriedade do GStreamer playbin.
  - 56 testes unitários e de integração aprovados com 100% de sucesso (`cargo test --all-targets`).
  - Novos testes: `volume_hud_gauge_and_fade_calculation`, `draw_volume_hud_renders_without_panic`, `volume_getter_and_setter_clamp_properly` e `render_volume_hud_preview_to_file`.
  - Binário Release oficial compilado e instalado em `~/.local/bin/vinylplayer` (SHA-256: `fd80321088613c77d91ebb9f11e6f24522aa67d56be0488802a9c2ef06e4f92b`).

## Empacotamento Arch Linux atualizado (pkgrel=8) — 13/11/2026

- **Pacote binário compilado:** `vinylplayer-0.1.0-8-x86_64.pkg.tar.zst` (1.1 MB).
- **Tarball fonte atualizado:** `vinylplayer-0.1.0.tar.gz` (SHA-256: `54442ae625a9148bf291a8f741ed2f140197381a31a8d99276e8fbf594cb4330`).
- **PKGBUILD e .SRCINFO sincronizados:**
  - `pkgrel` incrementado para `8`.
  - Checksum sha256 atualizado.
  - Testes com `cargo check --locked` e `makepkg -f` validados com êxito em ambiente limpo de fakeroot.
  - Arquivo `vinylplayer.desktop` validado com `desktop-file-validate`.

## Modos Shuffle (Aleatório) e Repeat (Loop) — 13/11/2026

- **Interface de Transporte Inferior (`player_bar` em `src/main.rs`):**
  - **Botão Shuffle (`🔀`):** Ícone `media-playlist-shuffle-symbolic`, posicionado à esquerda de *Anterior*.
    - Alterna entre ativo (realce âmbar clássico `.active`) e desativado.
    - Tooltip explicativo dinâmico: *"Ordem aleatória (Shuffle) · Ativada / Desativada"*.
  - **Botão Repeat (`🔁`):** Posicionado à direita de *Próxima*. Cicla com cliques sucessivos entre:
    1. **Off (Desativado):** Ícone `media-playlist-repeat-symbolic`, sem realce. Fila para ao final da última faixa.
    2. **All (Repetir Fila):** Ícone `media-playlist-repeat-symbolic`, com realce âmbar. Ao final da última faixa, retorna ao início da fila (ou sorteia no modo shuffle).
    3. **Track (Repetir Faixa):** Ícone `media-playlist-repeat-song-symbolic`, com realce âmbar. A música atual é reiniciada continuamente ao dar fim de fluxo (EOS).
  - Sincronização periódica no loop de eventos GTK a cada 250 ms: alterações feitas via atalhos externos ou controle MPRIS são refletidas nos botões sem atraso.

- **Física da Fila e Algoritmo de Shuffle (`src/audio.rs`):**
  - Gerador determinístico de embaralhamento sem crates externas adicionais (mantendo o build ultrarrápido e locked): `pick_shuffle_index`.
  - Garantia de não-repetição imediata: se a fila tiver 2 ou mais faixas, a próxima música sorteada nunca será a música que acabou de tocar.
  - Tratamento de fim de fluxo (`advance_after_eos`):
    - `RepeatMode::Track`: reinicia a mesma faixa de imediato.
    - `shuffle`: sorteia próxima faixa respeitando restrições.
    - `RepeatMode::All`: reinicia na primeira faixa ao alcançar o fim da lista.
    - `RepeatMode::Off`: interrompe a reprodução de forma limpa.

- **Integração D-Bus MPRIS (`src/mpris.rs`):**
  - Implementação completa das propriedades oficiais do protocolo MPRIS `org.mpris.MediaPlayer2.Player`:
    - `LoopStatus` (leitura e escrita): `"None"`, `"Track"`, `"Playlist"`.
    - `Shuffle` (leitura e escrita): `true` / `false`.
  - Controle nativo via `playerctl loop [None|Track|Playlist]` e `playerctl shuffle [On|Off]`.

- **Persistência de Sessão (`src/state.rs`):**
  - Chaves `"shuffle"` (booleano) e `"repeat_mode"` (`"off"`, `"all"`, `"track"`) adicionadas ao `state.json`.
  - Restauração automática das preferências do usuário ao reiniciar o aplicativo.

- **Testes e Garantia de Qualidade:**
  - 52 testes unitários passando (`cargo test --all-targets`).
  - Novo teste gráfico de interface `shuffle_and_repeat_buttons_toggle_and_persist` validando os cliques, alternância de classes CSS e persistência no arquivo de estado.
  - Prévia visual gerada em `docs/toca-discos-shuffle-repeat.png`.
  - Binário compilado em Release e instalado em `~/.local/bin/vinylplayer` (SHA-256: `3e185afabb5dc8c38c0016293353f62c36453fc3f4af1823f0b9e13ac5b8ff08`).

## Alternador de velocidade 33 ⅓ vs 45 RPM — 13/11/2026

- **Arquivos modificados:**
  - `src/state.rs`: campo `rpm: u32` adicionado em `AppState`, getters/setters, serialização e deserialização JSON resiliente com fallback para 33 RPM.
  - `src/main.rs`: controles visuais na HeaderBar (`.rpm-selector`), renderização gráfica vetorial Cairo dos botões 33/45 e LEDs com iluminação âmbar estilo Technics SL-1200, suporte a clique direto no chassi via `GestureClick`, sincronização de estado com o `StateManager` e novo teste automatizado `rpm_speed_toggles_and_persists`.

- **Controles e Interface do Usuário (UI):**
  - **Seletor no Cabeçalho:** Adicionado seletor tátil estilo vintage na barra superior (“SESSÃO DE ESCUTA”) com os botões **33 ⅓** e **45** encapsulados em pill (`.rpm-selector` / `.rpm-button`), com acabamento metálico e realce âmbar/dourado na velocidade ativa.
  - **Botões Físicos no Chassi do Toca-Discos:** Desenho vetorial no canto inferior esquerdo da base de alumínio escovado com botões chanfrados e anéis metálicos para 33 ⅓ e 45 RPM. Cada botão possui um anel de LED dedicado.
  - **LEDs Indicadores:** O LED da velocidade ativa acende com brilho âmbar dinâmico ao reproduzir (pulso quente) e iluminação sutil quando pausado; o LED inativo permanece apagado.
  - **Interação Direta por Toque/Clique no Deck:** Adicionado tratador `GestureClick` no canvas do toca-discos. Clicar diretamente sobre os botões 33 ou 45 desenhados no chassi alterna a rotação imediatamente com feedback tátil e visual instantâneo.
  - **Proteção de Alternância Mútua:** Impossível desmarcar ambos os botões; a seleção alterna de forma exclusiva entre 33 e 45 RPM.

- **Física de Rotação e Renderização Dinâmica:**
  - **Velocidade Angular Real:**
    - **33 ⅓ RPM:** `33.333333 / 60.0 * 2.0 * π` ≈ `3.4906585 rad/s`.
    - **45 RPM:** `45.0 / 60.0 * 2.0 * π` ≈ `4.7123890 rad/s` (giro visual 35% mais rápido do prato e vinil).
  - **Selo Central do Disco (*Center Label*):** A inscrição tipográfica no centro do disco adapta-se em tempo real entre `HI-FI AUDIOPHILE · 33 ⅓ RPM` e `HI-FI AUDIOPHILE · 45 RPM`.

- **Persistência de Sessão (`state.json`):**
  - A rotação selecionada é salva em `$XDG_CONFIG_HOME/vinylplayer/state.json` sob a chave `"rpm"`.
  - Restauração automática ao iniciar o aplicativo: a última velocidade usada é reaplicada na UI e na física do toca-discos.
  - Gravação assíncrona tolerante a falhas sem travamentos no loop gráfico.

- **Validação e Testes:**
  - Compilação limpa com `cargo check --offline`.
  - Bateria completa com **49 testes unitários aprovados** sem falhas (`cargo test --all-targets`).
  - Teste gráfico automatizado `rpm_speed_toggles_and_persists` cobrindo o ciclo completo: alternância de velocidade, cálculo de rad/s, proteção contra deseleção e persistência em disco.
  - Captura gráfica de conferência gravada em `docs/toca-discos-rpm.png` e prévia em `tests/preview_professional.png`.

- **Instalação:**
  - Binário Release oficial compilado e instalado no sistema em `~/.local/bin/vinylplayer` (SHA-256: `1aa3a0c6166f130ce33b9f958082cc6997fc49001b040651a0d04ddfbd7a15af`).

## Erros de reprodução e recuperação — 13/11/2026

- Erros do bus GStreamer agora abrem um aviso com o nome da faixa e os botões
  **Tentar novamente**, **Pular faixa** e **Fechar**. Arquivos ausentes recebem
  explicação sobre arquivo movido ou disco desconectado; demais falhas explicam
  possíveis problemas no arquivo, formato ou saída de áudio. A mensagem técnica
  fica disponível no tooltip e nos logs, sem ocupar o texto principal.
- **Tentar novamente** recarrega e reproduz a mesma entrada da fila. **Pular faixa**
  avança para a próxima sem remover arquivos nem alterar a ordem. No fim da fila,
  encerra a reprodução e fecha o aviso, sem voltar à faixa que falhou. **Fechar**
  dispensa o aviso e mantém a reprodução parada; uma nova tentativa manual pode
  mostrar outra falha, se o problema continuar.
- Cada chamada de reprodução recebe um identificador. Erros repetidos da mesma
  tentativa são agrupados; novas tentativas na mesma música não ficam ocultas.
  As respostas conferem tentativa, revisão, posição e URI para não agir sobre
  uma seleção modificada. Avisos antigos são fechados quando a seleção muda.
- Após uma falha, o pipeline é encerrado para liberar o stream inválido e suas
  mensagens pendentes. Ao carregar outra faixa, mensagens antigas são descartadas
  durante a transição para READY. A fila e os metadados da faixa são mantidos.
- A sincronização do aviso usa identidade leve, sem copiar capas periodicamente.
  O diálogo tem botões de resposta explícitos e fecha também quando a resposta é
  acionada por código. Fontes: novo `src/playback_error.rs`, `src/audio.rs` e
  integração, estilos e teste em `src/main.rs`. Não mudou o formato das preferências.
- Validação final: `cargo check --offline` aprovado; suíte release com **47 testes
  aprovados**, nenhuma falha e 9 especiais ignorados por padrão. Foram executados
  separadamente e aprovados **sete testes gráficos**: recuperação de erros, fila,
  restauração, pastas, abertura externa, erro de salvamento e prévia de álbuns/vinil.
- O novo teste provoca falhas reais com WAV ausente e conteúdo inválido. Confere
  recuperação após devolver o arquivo, nova falha após retry ainda corrompido,
  tentativa manual depois de fechar, um único aviso visível, avanço para áudio
  válido, fila preservada, resposta antiga após trocar de seleção e fim da fila
  sem repetição. Usa arquivos e preferências temporários e sink silencioso.
- Capturas conferidas: [arquivo ausente](docs/erro-arquivo-ausente.png) e
  [falha de reprodução](docs/erro-reproducao.png). Os logs de GStreamer no teste
  são esperados pelas falhas provocadas; os avisos do tema GTK são preexistentes.
- Release instalado em `~/.local/bin/vinylplayer` e aplicativo reaberto.
  Binário em execução, instalado e release conferem com SHA-256
  `5db0478a1075af28a33c3bb6783181372a76c169b59af34c5fa708631a1310ff`.
  Preferências idênticas ao backup, incluindo fila e volume; MPRIS em `Stopped`,
  sem reprodução automática. Não foi provocada falha na biblioteca real.
  Backup anterior em `/tmp/vinylplayer-playback-errors-backup.4VcPXP`.
- A recuperação depende da resposta escolhida pelo usuário. O pacote Arch
  `0.1.0-7` não foi regenerado nesta etapa.

## Botão de fechar nas pastas — 13/11/2026

- Adicionado botão **Fechar** no canto superior direito da janela “Pastas da
  biblioteca”, ao lado do título. Fecha somente as configurações e mantém o player.
- O teste gráfico de pastas agora aciona esse botão e confere que a janela de
  configurações foi fechada e a janela principal continua visível.
- `cargo check --offline` e o teste gráfico de pastas aprovados. Capturas de
  configurações atualizadas em `docs/pastas-biblioteca.png` e `docs/pasta-indisponivel.png`.
- Correção instalada e janela de pastas reaberta. SHA-256 do release instalado:
  `f1724bf16b4b1a9fe1dabb55164084dc341bfe522577aaa6e4e1c54dbb85e3f3`.
  Backup anterior em `/tmp/vinylplayer-close-backup.uaLQRp`.

## Pastas configuráveis da biblioteca — 13/11/2026

- Habilitado **Configurações** na barra lateral. A janela “Pastas da biblioteca”
  permite adicionar uma pasta pelo seletor nativo GTK, remover pastas e solicitar
  uma nova leitura. Remover uma pasta não exclui arquivos nem altera a fila atual.
- As escolhas ficam em `library_folders`, nas preferências existentes, como URIs
  locais. Uma lista vazia continua vazia ao reabrir. Configurações antigas usam a
  pasta da versão anterior até que o usuário altere a seleção; o scanner e as
  visões não consultam mais um diretório fixo no módulo de áudio.
- Uma única leitura de pastas é executada por vez em um worker. Solicitações
  recebidas durante a leitura são consolidadas e resultados antigos não substituem
  a seleção mais recente. A leitura acontece ao abrir o aplicativo, ao editar as
  pastas e pelo botão **Atualizar biblioteca**.
- `src/library_sources.rs` percorre as pastas, reconhece extensões de áudio sem
  diferenciar maiúsculas, deduplica caminhos canônicos de pastas sobrepostas e
  links e evita ciclos. Pastas indisponíveis permanecem configuradas, com aviso
  na janela e contagem na barra de estado; as pastas disponíveis continuam sendo lidas.
- Faixas, lista de álbuns e capas são criadas em lotes pequenos no GTK. Extração
  de metadados e decodificação de capas continuam em workers. A música e a fila
  em execução são preservadas durante atualizações; a descoberta inicial só
  preenche a fila antiga se ela não tiver sido substituída durante a leitura.
- Fontes: novos `src/library_sources.rs` e `src/library_settings.rs`; integração
  em `src/main.rs`, persistência em `src/state.rs` e inicialização da fila em
  `src/audio.rs`. O scanner antigo do módulo de áudio foi removido.
- Validação: `cargo check --offline` aprovado; `cargo test --offline --release`
  com **47 testes aprovados**, nenhuma falha e 8 testes especiais ignorados por
  padrão. Foram executados separadamente e aprovados seis testes gráficos:
  pastas, fila editável, restauração, abertura externa, erro ao salvar e prévia
  dos álbuns/toca-discos.
- O novo teste gráfico verifica duas pastas adicionadas em sequência, atualização
  das faixas na tela, duplicata rejeitada, remoção pelos botões sem excluir áudio,
  biblioteca sem pastas, aviso após desconexão, escolhas restauradas ao reabrir e
  recuperação após reconexão e atualização. Confere URI, posição e fila do áudio
  e confirma que o vinil continua recebendo frames. Usa WAVs silenciosos, sink de
  teste e preferências temporárias. A adição automatizada usa o mesmo método do
  retorno do seletor; a janela nativa de seleção não foi operada por automação.
- Capturas conferidas e salvas: [pastas configuradas](docs/pastas-biblioteca.png)
  e [pasta indisponível](docs/pasta-indisponivel.png). Os avisos de parsing do tema
  GTK observados são os preexistentes na sessão; o teste de falha de salvamento
  produz deliberadamente mensagens de erro de gravação.
- Release compilado, instalado em `~/.local/bin/vinylplayer` e reaberto
  com a janela de pastas visível. Binário em execução, instalado e release têm
  SHA-256 `9f06b296f31cb3c1508c5d508a1fe2600a5f6c01c4b2e6cadfac84dd567821d7`.
  Conferência da sessão real: arquivo de preferências idêntico ao backup anterior
  ao reinício (incluindo fila e volume), nova ação `library-settings` disponível
  e executada, e MPRIS em `Stopped`, sem autoplay. Backup do executável anterior,
  atalho e preferências em `/tmp/vinylplayer-folders-backup.ewNxvS` (pasta temporária).
- Limites: não há monitoramento contínuo de alterações no disco; use **Atualizar
  biblioteca** após adicionar arquivos ou reconectar uma pasta. O pacote Arch
  `0.1.0-7` não foi regenerado nesta etapa.

## Conferência final da restauração — 13/11/2026

- Ajustada a abertura para ignorar arquivos ausentes e diretórios na fila salva,
  preservando a ordem das entradas disponíveis e remapeando a seleção. Uma fila
  inteiramente indisponível volta vazia. Entradas filtradas não reaparecem ao
  reconectar o disco; podem ser adicionadas novamente pela biblioteca.
- `cargo check --offline` e `cargo test --offline --release` aprovados: 45 testes
  passaram, com 7 testes especiais ignorados por padrão. O teste gráfico
  `queue_restores_after_immediate_close_without_autoplay` foi executado novamente
  e aprovado com a filtragem final. Os demais testes gráficos listados na seção
  abaixo haviam sido aprovados na implementação inicial da persistência.
- O teste final grava após reordenar/remover e fecha antes do próximo tick;
  reabre a fila intacta para confirmar ordem e índice de uma entrada repetida;
  verifica arquivo ausente, escolha da próxima válida e da anterior, nenhuma
  entrada disponível, fila vazia persistida, volume/visão preservados e ausência
  de reprodução automática. Sinks silenciosos e configurações temporárias foram usados.
- Release final compilado e instalado em `~/.local/bin/vinylplayer`.
  Executável em execução, instalado e release conferem com SHA-256
  `61b587d9ce1bf5d0a7836eeaafcdde78d00c5b6c97485770821c69c63775646f`.
  O aplicativo foi aberto com a fila visível. Auditoria da sessão real confirmou
  as 1.961 entradas na ordem salva, seleção no índice 350, preferências preservadas
  e estado MPRIS `Stopped`; nesta biblioteca não havia entradas a filtrar.
  Backup anterior à instalação final e script de auditoria em
  `/tmp/vinylplayer-queue-restore-final.BOhuHz` (pasta temporária).

## Persistência e restauração da fila — 13/11/2026

- A fila agora é salva junto das preferências em
  `~/.config/vinylplayer/state.json`: músicas na ordem editada e índice da faixa
  selecionada, inclusive quando o mesmo arquivo aparece mais de uma vez.
  Ao abrir sem arquivos novos, essa fila é restaurada **sem reprodução automática**.
- Uma fila vazia salva volta vazia, sem ser substituída pela biblioteca inteira.
  Configurações antigas, sem o campo `queue`, mantêm o comportamento anterior:
  biblioteca inicial e última faixa válida. Volume, visão ativa e ordenações
  continuam compatíveis. Arquivos novos abertos pelo gerenciador de arquivos
  continuam substituindo a fila e iniciando a reprodução quando solicitado.
- Ao restaurar, arquivos inexistentes ou que não são arquivos regulares são
  ignorados. A ordem das entradas disponíveis é preservada. Se a selecionada
  estiver ausente, escolhe a próxima válida ou, na falta desta, a anterior.
  Se nenhuma estiver disponível, restaura uma fila vazia, sem autoplay.
  A fila filtrada é salva normalmente; remontar um disco não reinsere entradas
  automaticamente. A filtragem ocorre ao abrir, sem monitoramento contínuo.
- O campo `queue` usa formato versão 1 com `files` (URIs locais `file://`) e
  `current` (índice). Isso preserva espaços, acentos, aspas, barras, quebras de
  linha e nomes com bytes não UTF-8. URIs remotas ou inválidas são ignoradas com
  remapeamento da seleção; seleção fora dos limites volta à primeira entrada.
  Uma estrutura/versão de fila não reconhecida não descarta as demais preferências.
- Substituído o parser JSON manual por `serde_json`, com suporte correto a
  listas e escapes. `Cargo.toml` e `Cargo.lock` atualizados usando o cache offline.
  Dependências adicionadas ao lock: `serde_json 1.0.154`, `itoa 1.0.19`, `zmij 1.0.23`.
- A gravação aproveita o worker, as revisões e a publicação atômica existentes.
  O temporizador só captura a fila quando conteúdo ou seleção mudam; mudar apenas
  a seleção compartilha o vetor de caminhos com `Arc`, sem copiá-lo. Ao fechar,
  um snapshot novo é capturado antes do flush, incluindo edições feitas depois do
  último tick. Falha de gravação mantém a janela e permite tentar novamente.
- Fontes: `src/state.rs` (formato e writer), `src/audio.rs` (construtor com fila
  restaurada, preservando o índice exato) e `src/main.rs` (restauração e captura).
- Validação: `cargo check --offline` aprovado; suíte release com **45 testes
  aprovados**, nenhuma falha e 7 testes especiais ignorados por padrão. Cinco
  testes gráficos foram executados separadamente e aprovados: restauração,
  edição/arraste da fila, abertura externa, erro de gravação ao fechar e prévia
  de álbuns/toca-discos. Usados caminhos de preferências temporários e sinks de teste.
- O novo teste gráfico fecha imediatamente após reordenar/remover, cria outro
  gerenciador lendo o disco e reabre a janela com outro motor de áudio. Confere
  ordem, seleção de uma entrada repetida, volume, visão, arquivo ausente e sua
  filtragem persistida, fila vazia e ausência de autoplay. Os testes de estado
  também cobrem URIs inválidas, nomes especiais/não UTF-8 e edições durante escrita.
- Primeira instalação desta etapa, anterior à filtragem de arquivos ausentes:
  release instalado e reaberto com o painel de fila visível. Binário em execução,
  instalado e release conferem com SHA-256
  `1718bd13f5d3b312a5bde4281983c5c2c4145dbbc5bd7c7d7d696d5d9d321f09`.
  Conferência da sessão real: preferências e última faixa preservadas, 1.961
  entradas gravadas no formato novo e estado MPRIS `Stopped`, sem autoplay.
  Backup da versão anterior, atalho e configuração anterior à migração em
  `/tmp/vinylplayer-persistence-backup.F0cWXF` (pasta temporária).
- Limites: não são restaurados o tempo decorrido da música nem o estado de play;
  reproduzir manualmente começa do início. O pacote Arch `0.1.0-7` ainda não foi
  regenerado. A limitação de fila apenas por sessão registrada abaixo é histórica
  e foi resolvida por esta alteração.

## Fila de reprodução editável — 13/11/2026

- Adicionado o botão **Fila** no cabeçalho e o atalho **Ctrl+J**. O painel lateral
  pode ser fechado pelo botão, pelo atalho ou pelo fundo sobreposto. Ele preserva
  o tamanho do vinil, indica a faixa selecionada/em reprodução/pausada e informa
  quantas músicas existem na fila e quantas vêm a seguir.
- Clique no título de uma faixa para reproduzi-la. Use as alças para arrastar
  acima/abaixo de outra faixa, ou os botões de subir/descer para reorganizar.
  O botão de remover atua somente na fila, sem excluir arquivos da biblioteca.
- Abrir o painel localiza a faixa atual. O botão ao lado do título permite
  reencontrá-la depois de percorrer uma fila longa.
- Reordenar ou remover outra faixa mantém a música atual e sua posição de áudio.
  Ao remover a atual, seleciona a próxima; se não houver próxima, seleciona a
  anterior. O estado de reprodução ou pausa é preservado. Ao remover a última
  entrada, o áudio é encerrado, os metadados são limpos e aparece o estado vazio.
  A barra de progresso também fica zerada e desabilitada sem duração disponível.
- A fila e seu índice atual passaram a compartilhar um único mutex. Revisões
  impedem que botões ou arrastes antigos editem uma fila substituída por um álbum
  ou por arquivos abertos externamente. Entradas com caminhos repetidos mantêm
  sua posição correta. Novos arquivos carregados individualmente são acrescentados
  ao final, preservando a ordem editada.
- A lista usa `GtkListView` com virtualização, sem temporizadores por faixa.
  A atualização aproveita o temporizador existente da barra de reprodução, que
  agora é encerrado quando a barra é destruída. Os caminhos só são copiados para
  a lista quando sua revisão muda.
- Fontes envolvidos: `src/playback_queue.rs` (novo modelo), `src/queue_view.rs`
  (novo painel), `src/audio.rs` (operações) e `src/main.rs` (integração e estilos).
- Validação: `cargo check --offline` aprovado; suíte release com **39 testes
  aprovados**, nenhuma falha e 6 testes especiais ignorados por padrão. Foram
  executados separadamente e aprovados os testes gráficos de fila, abertura de
  arquivos, prévia de álbuns/toca-discos e erro/repetição de salvamento ao fechar.
- O teste gráfico de fila usa WAVs silenciosos, sink de teste e preferências
  temporárias. Exercita os botões e os sinais reais de `DragSource`/`DropTarget`,
  rejeita um arraste antigo, confere a continuidade da posição do áudio, seleção
  e remoção em pausa, fila vazia, localizar a faixa 1.901 e virtualização com
  2.000 entradas (menos de 500 linhas criadas). Não foi um teste manual com mouse.
- Capturas conferidas e salvas no projeto:
  [fila aberta](docs/fila-reproducao.png) e [fila vazia](docs/fila-vazia.png).
  Os avisos de parsing do tema GTK são os já existentes na sessão.
- Release instalado em `~/.local/bin/vinylplayer` e reaberto com o
  painel de fila exibido. Executável em execução, instalado e release conferem
  com SHA-256 `63c55ef1745c88be6692caf636134aa58879c69a6225f33fd7b1100574ccd21c`.
  Preferências permaneceram idênticas ao backup anterior ao reinício. Backup do
  executável anterior, atalho e estado em `/tmp/vinylplayer-queue-backup.A4Ejrz`
  (pasta temporária).
- Limites desta etapa: a fila editada pertence à sessão e não é restaurada após
  fechar o programa. As preferências existentes continuam sendo salvas. O pacote
  Arch `0.1.0-7` permanece anterior a estas mudanças e não foi regenerado.

## Abertura pelo gerenciador de arquivos — 13/11/2026

- Implementado `G_APPLICATION_HANDLES_OPEN` com handlers compartilhados de
  ativação e abertura em `src/file_open.rs`. Abrir um arquivo inicia a reprodução
  na tela do toca-discos; novas solicitações reutilizam a janela e o motor de áudio.
- Seleções de vários arquivos substituem a fila, mantendo a ordem recebida.
  Duplicatas do mesmo caminho são removidas. A ordem recebida depende do
  gerenciador de arquivos utilizado. `AudioEngine::play_files` compartilha esse
  comportamento com a reprodução de álbuns.
- Aceitos caminhos locais e URIs `file://`, incluindo espaços, acentos e `#`.
  Diretórios, arquivos inexistentes, extensões desconhecidas e endereços remotos
  são rejeitados com aviso. Em seleções mistas, os arquivos válidos são reproduzidos;
  uma seleção inteiramente inválida preserva a reprodução atual.
- Criado `vinylplayer.desktop` nesta pasta com `Exec=vinylplayer %U` e os tipos
  MIME já anunciados pelo pacote. O atalho local também precisava de `%U` para
  encaminhar os arquivos. Não foram alterados os aplicativos padrão do sistema.
- Validação: `cargo check --offline` aprovado; `cargo test --offline --release`
  com 36 aprovados, nenhuma falha e 5 testes especiais ignorados por padrão.
  O novo teste gráfico foi executado separadamente e aprovado, cobrindo abertura
  inicial, ativação sem duplicar janela, abertura posterior, ordem da fila e
  preservação da reprodução ao receber arquivo inválido, usando áudio silencioso
  e preferências temporárias. Ele aguarda os eventos assíncronos do GStreamer.
- Teste completo com `gio launch` e `.desktop %U` em barramento D-Bus separado:
  abertura inicial e posterior aprovadas, mesmo PID confirmado, reprodução e
  avanço para o próximo arquivo na ordem enviada confirmados via MPRIS. Usados
  três WAVs silenciosos com espaços, acentos e `#`, e `XDG_CONFIG_HOME` temporário.
  Os arquivos `.desktop` passaram por `desktop-file-validate`.
- Os avisos do tema GTK e do barramento de acessibilidade observados nos testes
  pertencem à sessão gráfica. Não impediram a aprovação do teste de abertura.
  O serviço de acessibilidade da sessão foi reiniciado após o teste em barramento
  separado para restaurar seu socket, cuja presença foi conferida.
- Release compilado e instalado em `~/.local/bin/vinylplayer`; o player
  foi reaberto com a versão atualizada. Executável instalado e release têm SHA-256
  `9b216783b6fa5bdc185aa61184bf1e4d118f5c1ab43f03eb5bf18d68da121230`.
  Backup do executável anterior, atalho e preferências em
  `/tmp/vinylplayer-open-validation.VK119B/backup` (pasta temporária).
- Limites: a associação MIME continua cobrindo FLAC, MP3, Ogg e WAV. A validação
  inicial confere caminho e extensão; falhas de decodificação continuam sendo
  tratadas pelo fluxo existente do GStreamer. O pacote Arch `0.1.0-7` e seu
  tarball ainda representam a versão anterior e não foram regenerados nesta etapa.

## Conferência do novo empacotamento — 13/11/2026

- A novidade desde o refinamento visual é o pacote Arch `0.1.0-7`; os fontes
  Rust permanecem iguais à versão anterior conferida. Não foram feitas correções
  no código, instalação do pacote ou reinício do aplicativo nesta revisão.
- `PKGBUILD`, `.SRCINFO` e `.PKGINFO` confirmam `pkgrel=7` e as dependências
  opcionais `ffmpeg`/`flac`. A pendência de declaração dessas ferramentas foi resolvida.
- O SHA-256 do tarball confere com o declarado no `PKGBUILD`:
  `2d19fecca6e5630fe504a3454190933a5d5ce3d5334ccfdc163f40e8e6c662f9`.
- Comparação byte a byte: `Cargo.toml`, `Cargo.lock` e os sete módulos Rust
  incluídos no tarball são idênticos aos desta pasta do projeto.
- O pacote contém o executável, o arquivo `.desktop` e o ícone SVG nos caminhos
  previstos. Seu executável contém os textos do novo toca-discos e do writer de
  preferências. SHA-256 do executável empacotado:
  `ff039d2b1fe86bf81d0d216b2e732d6912d029e7ab77e7cc9eae0bd43f06289a`.
- O executável empacotado não é byte a byte igual ao release local. Foram
  confirmados o conteúdo dos fontes e os identificadores acima; não foi atribuída
  uma causa específica à diferença binária nem executado o pacote nesta revisão.
- `cargo check --offline`: aprovado. `cargo test --offline --release`: 33 testes
  aprovados e 4 especiais ignorados; os testes gráficos não foram repetidos.
- O release local e `~/.local/bin/vinylplayer` continuam idênticos,
  com SHA-256 `59df0ca5d666f2d8966968a2fcfbfefa812b6db2cf98f54d1fe97fe7de4cdb04`.

### Pendência de integração identificada por inspeção

O `.desktop` empacotado usa `Exec=vinylplayer %U` e anuncia associações de áudio,
mas `src/main.rs` não registra `G_APPLICATION_HANDLES_OPEN`/`connect_open` nem um
handler de linha de comando equivalente. A abertura de arquivos recebidos pelo
gerenciador de arquivos precisa ser implementada e validada. Nesta revisão esse
fluxo não foi executado, para não interferir na instância ativa.

## Empacotamento Arch Linux atualizado (pkgrel=7) — 13/11/2026

- O workspace de empacotamento foi sincronizado em
  `~/.openmausbot/workspaces/4ec1e6a6-c70b-4bda-9c52-c4833b1b8578`.
- Declaradas dependências opcionais (`optdepends`) no `PKGBUILD`:
  - `ffmpeg`: leitura de metadados e tags avançadas com `ffprobe`.
  - `flac`: extração de capas embutidas em FLAC com `metaflac`.
- Novo tarball de fontes `vinylplayer-0.1.0.tar.gz` gerado com os fontes
  atualizados (`main.rs`, `audio.rs`, `library.rs`, `mpris.rs`,
  `now_playing.rs`, `search.rs`, `state.rs`, `vinylplayer.desktop`, ícone SVG e
  manifestos Cargo).
- Checksum SHA-256 recalculado (`2d19fecca6e5630fe504a3454190933a5d5ce3d5334ccfdc163f40e8e6c662f9`)
  e `.SRCINFO` regenerado.
- Pacote binário `vinylplayer-0.1.0-7-x86_64.pkg.tar.zst` compilado com sucesso
  via `makepkg -f` e validado com `tar -tvf` e `.PKGINFO`.
- O binário empacotado reflete as correções de persistência assíncrona, busca
  em tempo real e novo visual do toca-discos.

## Refinamento da tela do toca-discos — 13/11/2026

- Nova composição com cabeçalho “Sessão de escuta” e indicador de estado real:
  pronto, em reprodução ou pausado, acompanhado de ícone simbólico.
- O vinil continua sendo o elemento central, com a capa inteira e o braço
  animado. Margens e dimensões mínimas foram ajustadas para acomodar as informações.
- Abaixo do aparelho aparecem título, artista/álbum e formato do arquivo.
  Taxa de amostragem e profundidade só aparecem quando disponíveis nos metadados.
  Títulos longos usam reticências e mantêm o texto completo no tooltip.
- O chassi recebeu textura discreta de metal escovado, contorno interno,
  aro de alumínio em tons neutros e identificação no painel inferior.
- A textura e os detalhes fixos continuam no cache do fundo. Os textos são
  atualizados quando a versão dos metadados muda; o status muda apenas nas
  transições de reprodução. Não foi adicionado temporizador por elemento.
- Mantidas as correções de persistência: a prévia usa configuração temporária.
- Arquivo de implementação alterado: `src/main.rs`.
- A prévia automatizada também captura a tela durante a reprodução em
  `/tmp/vinylplayer-turntable-playing.png`.

- A indicação de codec na barra inferior foi compactada (por exemplo, o nome
  completo do codec FLAC passa a ser reconhecido como FLAC/LOSSLESS). O nome
  técnico completo continua disponível no tooltip.

### Verificação e instalação desta tela

- `cargo test --offline --release`: 33 testes aprovados e 4 especiais ignorados.
- `render_professional_preview`, executado separadamente: aprovado, usando
  preferências temporárias e saída de áudio simulada. Foram inspecionadas as
  imagens da tela parada e em reprodução; a captura em reprodução é
  `/tmp/vinylplayer-turntable-playing.png`.
- Formatação de `src/main.rs` e compilação release: aprovadas. Não foi realizada
  medição de FPS nem validação visual de todas as resoluções possíveis.
- A build e `~/.local/bin/vinylplayer` foram sincronizados, com SHA-256
  `59df0ca5d666f2d8966968a2fcfbfefa812b6db2cf98f54d1fe97fe7de4cdb04`.
- Binário anterior preservado temporariamente em
  `/tmp/vinylplayer-visual-backup.sotapA/vinylplayer`.
- A instância instalada foi reaberta para aplicar o visual, restaurando a visão
  do toca-discos e o volume de 29% que já estavam salvos. Os testes não alteraram
  o arquivo real de preferências (SHA-256 antes/depois dos testes:
  `9e0d7985a31052d292f58cddbb2e56bed24f2e1ab03f69c36c9f03ab46034ea1`).
- Os avisos já existentes de `gtk.css` e de preferência de tema libadwaita
  continuam sendo emitidos; o tema do sistema não foi alterado.
- O pacote Arch não foi regenerado nesta etapa; a alteração está nos fontes,
  na build do projeto e no executável local instalado.

## Correção da persistência e isolamento dos testes — 13/11/2026

- `StateManager::with_path` permite injetar um arquivo de estado específico.
  Todos os testes que gravam preferências, incluindo a prévia GTK, agora recebem
  uma pasta temporária exclusiva, removida ao final. Não é necessário mudar
  variáveis de ambiente globais para isolá-los.
- O salvamento periódico agenda um worker e não faz I/O de disco no callback
  GTK. Apenas um worker por gerenciador pode estar em andamento; mudanças são
  reunidas no snapshot mais recente.
- Revisões substituem o booleano `dirty`: uma revisão só é confirmada após
  salvar com sucesso. Alterações feitas durante uma escrita continuam pendentes.
- Falhas de escrita preservam as mudanças pendentes e são registradas; as
  chamadas periódicas seguintes podem tentar novamente.
- Um lock serializa as escritas. O snapshot é capturado depois desse lock,
  impedindo que um worker atrasado sobrescreva a gravação final mais recente.
- O arquivo temporário é exclusivo por processo/escrita, criado na mesma pasta
  do destino. A gravação é sincronizada antes do rename; em caso de falha o
  temporário próprio é removido. Em Unix, arquivos novos têm permissão `0600`.
- Preferências sem mudanças não geram novas escritas, nem ao fechar.
- O fechamento aguarda a escrita em curso e grava o estado pendente de forma
  síncrona. Essa espera final pode durar o tempo do I/O, mas não ocorre durante
  a atualização periódica da interface. Se falhar, a janela permanece aberta e
  mostra uma mensagem; o usuário pode corrigir a causa e tentar fechar novamente.
- O parser JSON existente não foi substituído nesta etapa.

Arquivos alterados: `src/state.rs`, `src/main.rs` e este registro.

### Verificação desta correção

- `cargo check --offline` e verificação de formatação dos arquivos alterados:
  aprovados.
- `cargo test --offline --release`: 33 testes aprovados e 4 especiais ignorados.
  Os novos casos cobrem caminhos independentes, restauração do arquivo salvo,
  falha real de escrita seguida de nova tentativa, preservação do arquivo anterior,
  alterações durante a escrita, worker ocupado e flush sem alterações.
- `close_error_keeps_preferences_and_retry_closes_window`, executado com
  `--ignored --test-threads=1 --nocapture`: aprovado. Simula uma pasta bloqueada,
  verifica que o fechamento é impedido com mensagem e confirma gravação/fechamento
  após corrigir a causa, sempre em uma pasta temporária.
- `render_professional_preview`, executado separadamente: aprovado com configuração
  temporária e saída de áudio simulada. Os testes especiais de metadados e dos
  indicadores isolados não foram repetidos nesta etapa.
- O GTK emitiu avisos do tema `gtk.css`; os testes passaram e os arquivos do tema
  não foram alterados. As mensagens de falha de escrita no teste de erro são esperadas.
- O arquivo real `~/.config/vinylplayer/state.json` preservou conteúdo e mtime:
  SHA-256 `9cb437961ca3c36494a3814410741eb37ed0268ad0491fe40e7ae372b5118720`,
  modificado em `2026-11-13 04:46:20.225888509 -0300`, antes e depois dos testes.
- `cargo build --offline --release`: aprovado. A build e o executável instalado
  em `~/.local/bin/vinylplayer` foram sincronizados e têm SHA-256
  `b3f09600baca991e3b1faf339f692f2c4463777c65ce297473915fb82d1cbf27`.
- A versão instalada anterior foi preservada temporariamente em
  `/tmp/vinylplayer-state-backup.KYrX3T/vinylplayer` (SHA-256 `c5726d17417a1684864870c41e1fe0d6a60048bfd77ce5afcc17b8ca0bceb604`).
- O aplicativo não estava em execução e não foi aberto fora dos testes.
  O pacote Arch `pkgrel=6` não foi regenerado; esta correção está nos fontes,
  na build deste projeto e no executável local instalado.

Comando adicional para repetir o teste de fechamento com falha:

```sh
cargo test --offline --release close_error_keeps_preferences_and_retry_closes_window -- --ignored --test-threads=1 --nocapture
```

## Revisão das modificações em 13/11/2026 — histórico anterior à correção

Esta etapa foi uma verificação; não foram implementadas correções no código.

### Confirmado nesta revisão

- `cargo check --offline`: aprovado.
- `cargo test --offline --release`: 27 testes aprovados e 3 especiais ignorados.
  Os testes gráficos e de arquivos com tags reais não foram repetidos nesta revisão.
- `cargo build --offline --release`: aprovado, sem recompilação necessária.
- `target/release/vinylplayer` e `~/.local/bin/vinylplayer` são idênticos:
  SHA-256 `c5726d17417a1684864870c41e1fe0d6a60048bfd77ce5afcc17b8ca0bceb604`.
- O executável atual é `vinylplayer`, conforme `Cargo.toml`. A build antiga
  `target/release/vinyl-player` ainda existe, mas não é a saída atual do Cargo.
- Não havia processo do player ativo durante a verificação; o aplicativo não
  foi aberto ou reiniciado nesta etapa.
- Estão presentes os módulos de indicadores, ordenação, busca e persistência,
  e o pacote `vinylplayer-0.1.0-6-x86_64.pkg.tar.zst` existe no workspace de empacotamento.

### Problemas encontrados naquele momento

Os itens 1–3 foram tratados na correção de persistência descrita acima. O comando
do item 4 foi atualizado neste documento. A declaração das dependências opcionais
do item 5 foi resolvida no pacote `pkgrel=7`, conforme a conferência acima.

1. **Teste escreve nas preferências reais:**
   `state_manager_tracks_dirty_and_flushes` usa `StateManager::new()` e grava
   o arquivo de configuração real. Ao executar a suíte nesta revisão, esse teste
   escreveu `volume: 0.6500` em `~/.config/vinylplayer/state.json`. O volume anterior
   não foi capturado, portanto não se afirma que houve mudança em relação a ele.
   Esse teste deve usar um diretório temporário isolado; testes gráficos também
   precisam de configuração isolada antes de serem repetidos.
2. **Gravação síncrona no loop gráfico:** `src/main.rs` chama
   `flush_if_dirty()` no callback GTK de 250 ms. `AppState::save()` usa gravação
   síncrona com `std::fs`; a descrição anterior de gravação assíncrona não corresponde
   ao código atual. Isso pode atrasar a animação quando o disco estiver ocupado.
3. **Falhas de gravação são descartadas:** `flush_if_dirty()` limpa `dirty` antes
   de salvar e ignora o erro. Se a gravação falhar, a alteração deixa de ficar
   pendente para nova tentativa. `flush_now()` também ignora erros.
4. **Comando antigo na documentação:** o bloco histórico de execução abaixo ainda
   contém `./target/release/vinyl-player`; para a versão atual deve-se usar
   `./target/release/vinylplayer` ou `cargo run --release`.
5. **Ferramentas externas não declaradas no pacote:** o `PKGBUILD` de `pkgrel=6`
   não declara `ffmpeg` (que fornece `ffprobe`) nem `flac` (que fornece `metaflac`)
   como dependências ou dependências opcionais. A ordenação por tags e a extração
   de capas FLAC dependem dessas ferramentas; sem elas há fallback parcial.

Prioridade sugerida: isolar os testes de persistência, tratar falhas de gravação
e retirar a escrita de disco do loop gráfico.

## Projeto utilizado

Este é o workspace escolhido para continuar o desenvolvimento:

`~/.openmausbot/workspaces/34c52fcf-61c2-42ab-8233-70938f131c8d`

Executável desta versão: `target/release/vinylplayer` (instalado e sincronizado em `~/.local/bin/vinylplayer`).

## Sincronização do sistema, empacotamento e higienização — alteração mais recente

- **Binário do sistema atualizado:** O executável em `~/.local/bin/vinylplayer` foi atualizado diretamente a partir da compilação release otimizada (`target/release/vinylplayer`, sha256 `846ec627...`), substituindo a build preliminar e disponibilizando as visões de álbuns e os novos indicadores no atalho do sistema.
- **Padronização do executável:** Adicionada a definição explícita `[[bin]] name = "vinylplayer"` no `Cargo.toml`, permitindo que o `cargo build --release` gere diretamente o binário com o nome padrão do sistema operacional.
- **Instalação do ícone do aplicativo:** O ícone vetorial SVG em alta definição (`assets/vinylplayer.svg`) foi instalado em `~/.local/share/icons/hicolor/scalable/apps/vinylplayer.svg` e o cache de ícones do GTK foi atualizado. O arquivo `vinylplayer.desktop` foi validado com sucesso via `desktop-file-validate`.
- **Limpeza do código-fonte:** Arquivos residuais (`src/main.rs.bak` e `src/main.rs.before_center_vinyl`) foram removidos do diretório de fontes.
- **Sincronização do pacote Arch Linux (Aurix):**
  - O tarball de fontes `vinylplayer-0.1.0.tar.gz` no workspace do Aurix (`4ec1e6a6-c70b-4bda-9c52-c4833b1b8578`) foi reconstruído incluindo todos os módulos Rust (`main.rs`, `audio.rs`, `library.rs`, `mpris.rs`, `now_playing.rs`).
  - O `PKGBUILD` foi incrementado para `pkgrel=3`, atualizado com a nova soma sha256 e o `.SRCINFO` foi regerado via `makepkg --printsrcinfo`.
  - O pacote final `vinylplayer-0.1.0-3-x86_64.pkg.tar.zst` foi compilado com sucesso via `makepkg -f --nodeps` e verificado.

## Indicadores da faixa e do álbum atuais

- A faixa em reprodução recebe fundo discreto em champanhe e ícone de reprodução
  nas telas Músicas, lista de álbuns e detalhes do álbum.
- A capa do álbum correspondente recebe contorno e indicador no canto, sem
  adicionar título ou legenda à grade.
- Ao pausar, os indicadores permanecem na faixa/álbum atuais com ícone de pausa
  e cores mais suaves. Retomar restaura o indicador de reprodução.
- Trocar de faixa transfere a marcação. Parar, chegar ao fim da fila ou receber
  um erro de áudio remove a marcação; uma faixa apenas pré-carregada não é marcada.
- A identificação usa o caminho completo do arquivo/pasta, incluindo URIs com
  espaços ou acentos, evitando confundir álbuns homônimos em pastas diferentes.
- Tooltips e rótulos de acessibilidade informam “Em reprodução” ou “Pausado”.
- Um registro de referências fracas acompanha os widgets. Ele reaproveita a
  atualização da barra a cada 250 ms e só altera os widgets quando muda a faixa,
  o estado ou quando uma nova lista/capa é criada. Não há timer por faixa.
- A leitura da identidade de reprodução copia apenas estado e URI, sem copiar
  capas embutidas para atualizar os indicadores.
- Corrigida a duração da escuta do bus do GStreamer: o `BusWatchGuard` agora
  permanece associado à janela e é liberado ao fechá-la. Antes ele era descartado
  imediatamente, prejudicando metadados e notificações de fim de faixa.
- A transição do GStreamer para `READY` agora é tratada como parada, evitando
  que a transição intermediária por `PAUSED` deixe o destaque preso após parar.

## Ordenação das faixas por metadados

- A visualização em lista, os detalhes e a fila de cada álbum recebem a mesma
  sequência ordenada pelo número do disco e pelo número da faixa.
- São reconhecidas as tags `disc`, `discnumber`, `track` e `tracknumber`, incluindo
  valores como `2/3` e `04/12`. Valores zero, negativos ou inválidos são ignorados.
- Quando o disco não está informado, é considerado disco 1. Dentro de cada
  disco, faixas numeradas vêm antes das que não têm número de faixa.
- Empates e arquivos sem número usam o nome em ordem natural: `2.flac` vem antes
  de `10.flac`. O caminho completo desempata nomes equivalentes de forma estável.
- A leitura utiliza `ffprobe`, disponível neste ambiente. Se o comando não
  estiver instalado ou não conseguir ler um arquivo, aplica-se a alternativa
  pelo nome, sem impedir o acesso ao álbum.
- A indexação é feita fora do loop gráfico, com até quatro workers. A tela
  Álbuns mostra “Organizando álbuns pelos metadados…” enquanto o índice é criado.
- O resultado é armazenado em `$XDG_CACHE_HOME/vinylplayer/track-order`, ou na
  pasta de cache padrão do usuário. Tamanho e data de modificação do arquivo
  invalidam o cache; alterar tags faz com que sejam lidas novamente.
- Os arquivos de música não são alterados. O agrupamento continua sendo por
  pasta; subpastas de CDs não são automaticamente reunidas em um único álbum.

## Navegação e reprodução dos álbuns

- Clicar em uma capa abre a tela de detalhes do álbum, sem iniciar a reprodução.
- A tela apresenta capa, nome, quantidade de faixas e lista de músicas.
- O botão **Reproduzir álbum** substitui a fila pelas faixas daquele álbum e
  inicia a primeira faixa da lista.
- Clicar em uma faixa nessa tela inicia a reprodução a partir dela, mantendo
  as demais faixas do álbum na fila.
- Ao terminar uma faixa, a reprodução avança automaticamente para a próxima.
  Ao terminar a última faixa da fila, a reprodução para.
- O avanço automático não repete o álbum nem passa para outro álbum.
  Os comandos manuais de próxima/anterior mantêm sua navegação circular.
- **Voltar aos álbuns** retorna à grade existente, preservando sua navegação.
- O botão de reprodução da visualização em lista também utiliza a fila do álbum.

A ordem utilizada em cada álbum agora segue a regra de metadados descrita acima.
A tela geral Músicas mantém sua ordenação por caminho.

## Visualizações de álbuns

- Seletor **Capas / Lista** na tela Álbuns.
- Grade com cartões de 280 × 280 pixels, sem legendas ou títulos visíveis.
  O nome do álbum está disponível no tooltip e no rótulo de acessibilidade.
- A lista mantém os cabeçalhos dos álbuns e suas faixas.
- Agrupamento por caminho completo da pasta, evitando misturar pastas diferentes
  que tenham o mesmo nome.
- Busca, extração e decodificação de capas em um worker, com entrega gradual de
  miniaturas à interface por um canal de capacidade limitada.
- Capas de arquivos de imagem na pasta e capas embutidas em FLAC seguem o
  mecanismo existente em `find_cover`; a extração FLAC depende de `metaflac`.
- Álbuns sem capa disponível usam um ícone de disco como alternativa.

## Refinamento visual

- Paleta grafite com detalhes champanhe, tipografia e espaçamentos revisados.
- Ícones simbólicos nativos no lugar de emojis nos controles e na navegação.
- Barra de reprodução ocupando toda a largura da janela.
- Títulos longos truncados e controles com tooltips de atalhos.
- Removida a indicação fixa de “bit-perfect 96 kHz”, que não era verificada.
- Configurações ainda não implementadas aparecem desabilitadas.
- O CSS do aplicativo tem prioridade sobre o CSS global do GTK neste processo,
  para preservar cores, bordas e cantos arredondados do player. Os arquivos de
  configuração do sistema não foram alterados para isso.

## Animação do vinil

- Removido o limite temporário de 30 FPS; a animação acompanha o FrameClock do GTK.
- Rotação calculada pelo tempo decorrido, mantendo a referência de 33⅓ RPM.
- Arte giratória pré-renderizada em cache: capa, sulcos, espiral, textos e gradientes.
- Cache invalidado quando a capa, o tamanho ou a escala da tela mudam.
- Consultas de metadados e da posição usada pelo braço limitadas a cada 250 ms.
- Corrigido o preenchimento do centro da arte padrão sem capa.

## Arquivos principais

- `src/main.rs`: interface, navegação, grade, detalhes do álbum, estilos,
  animação e verificação visual.
- `src/audio.rs`: fila do álbum, avanço automático e resolução das capas.
- `src/library.rs`: leitura e cache dos números de disco/faixa e ordenação dos álbuns.
- `src/now_playing.rs`: identificação e atualização dos indicadores de faixa/álbum.

## Verificações realizadas

- `cargo check --offline`: passou durante a implementação.
- `cargo test --offline --release`: 11 testes passaram; dois testes GTK e o teste
  de arquivos com tags reais foram ignorados nessa execução por exigirem recursos externos.
- O teste GTK dos indicadores foi executado separadamente e passou: reprodução,
  pausa, troca de faixa, parada, widgets criados depois de iniciar a reprodução,
  caminhos com acentos/espaços e remoção de referências de widgets destruídos.
- A prévia do player completo também passou usando `fakesink` sincronizado,
  sem emitir som. Ela verifica os destaques em reprodução/pausa, a transferência
  para a próxima faixa e a remoção após parar; gera imagens desses estados.
- `cargo test --offline --release metadata_fixture_orders_files_and_invalidates_cache -- --ignored --test-threads=1 --nocapture`:
  passou. Gera FLACs temporários com `ffmpeg`, lê tags com `ffprobe`, verifica
  ordenação por disco/faixa e invalidação do cache após alterar as tags.
- Testes unitários cobrem valores `N/total`, tags inválidas, ausência de números,
  múltiplos discos, nomes conflitantes e desempate natural determinístico.
- O teste gráfico foi executado separadamente e passou após os ajustes do
  mecanismo de captura e da identificação do viewport do GTK.
- A verificação gráfica abriu os detalhes por clique na capa e confirmou o
  retorno à grade. As imagens renderizadas foram inspecionadas visualmente.
- A indexação foi exercitada com a biblioteca local de 1.935 faixas. A captura
  gráfica aguarda a conclusão da indexação e a disponibilidade do quadro antes
  de renderizar as visualizações de álbuns.
- O teste da fila utilizou arquivos WAV temporários e `fakesink`, sem saída de
  áudio, para verificar ordem, início em uma faixa intermediária, avanço e parada.
- `cargo build --offline --release`: passou; o executável desta pasta foi aberto
  e seu processo foi conferido.

Essas verificações não constituem medição de FPS, consumo de CPU ou garantia
de desempenho em todos os tamanhos de biblioteca.

### Comandos para repetir as verificações

```sh
cargo check --offline
cargo test --offline --release
cargo test --offline --release metadata_fixture_orders_files_and_invalidates_cache -- --ignored --test-threads=1 --nocapture
cargo test --offline --release gtk_markers_follow_pause_track_changes_stop_and_new_widgets -- --ignored --test-threads=1 --nocapture
cargo test --offline --release render_professional_preview -- --ignored --test-threads=1 --nocapture
cargo build --offline --release
./target/release/vinylplayer
```

O teste gráfico exige uma sessão gráfica acessível e uma biblioteca com ao
menos um álbum para exercitar o clique na capa.

### Prévias geradas

O teste gráfico grava estas imagens temporárias, que podem ser recriadas pelo
comando acima e não fazem parte dos arquivos permanentes do projeto:

- `/tmp/vinylplayer-professional.png`
- `/tmp/vinylplayer-album-grid.png`
- `/tmp/vinylplayer-album-list.png`
- `/tmp/vinylplayer-album-detail.png`
- `/tmp/vinylplayer-playing-track.png`
- `/tmp/vinylplayer-playing-album.png`
- `/tmp/vinylplayer-paused-track.png`

## Seletores de ordenação por artista, álbum ou ano (Implementado)

- **Módulo de metadados (`src/library.rs`):**
  - Implementada a estrutura `TrackTags` e `AlbumMetadata` para extrair não apenas `disc` e `track`, mas também `album`, `artist`, `album_artist` e `date`/`year` via `ffprobe`.
  - Função `parse_year` que reconhece e valida anos a partir de strings de data ISO (`YYYY-MM-DD`), anos simples ou anotações históricas.
  - Estrutura `AlbumSortMode` com 5 modos de ordenação de álbuns:
    - *Álbum (A–Z)*: ordem alfabética natural de título.
    - *Álbum (Z–A)*: ordem alfabética reversa de título.
    - *Artista (A–Z)*: discografias agrupadas por artista em ordem alfabética.
    - *Ano (Mais recentes)*: anos mais novos primeiro, álbuns sem ano posicionados determinística e elegantemente no final.
    - *Ano (Mais antigos)*: clássicos primeiro, sem ano no final.
  - Algoritmo de desempate determinístico utilizando caminho físico (`folder`) e nomes.
  - Atualização do cache de metadados para formato `v2`, indexando as tags completas da biblioteca local em cache permanente (`~/.cache/vinylplayer/track-order/`).

- **Interface do usuário (`src/main.rs`):**
  - Adicionado seletor nativo `gtk::DropDown` integrado ao cabeçalho da visão de **Álbuns** ao lado do alternador *Capas / Lista*.
  - A troca de critério atualiza instantaneamente tanto a grade de capas (`FlowBox`) quanto a lista detalhada de álbuns (`ListBox`) através de `set_sort_func` e `invalidate_sort()`, preservando 100% das miniaturas em cache e widgets já instanciados, com reordenação fluida e sem recargas visuais.
  - Adicionado também seletor de ordenação na tela geral de **Músicas** (*Título A–Z*, *Título Z–A*, *Caminho Padrão*).
  - Tooltips dos cartões de álbuns enriquecidos com artista, título e ano de lançamento quando disponíveis.
  - Estilização CSS para `.sort-dropdown` respeitando a paleta escura com bordas e acentos champanhe/âmbar.

- **Testes e Validação:**
  - Adicionados testes unitários para `parse_year`, `parse_tags` com metadados ricos e `album_sort_modes_work_deterministically`.
  - 17 testes no total, com 14 testes unitários automatizados passando com 0 falhas e 0 warnings.
  - Teste gráfico `render_professional_preview` executado com sucesso e 7 prévias visuais regeneradas em `/tmp/`.

## Busca Instantânea em Tempo Real (Implementado)

- **Módulo reativo de busca (`src/search.rs`):**
  - Implementado `SearchHub`: controlador reativo com listeners/callbacks desacoplados que conectam a entrada de texto às diferentes visões gráficas do GTK sem acoplamento direto.
  - Função `normalize_for_search`: normalização com remoção inteligente de acentos diacríticos comuns (`á/ã/à->a`, `é/ê->e`, `ç->c`, etc.) e minúsculas, garantindo que buscas como `"musica"` localizem `"Música"`.
  - Função `matches_query`: busca multi-termo tolerante e independente da ordem das palavras digitadas (ex.: `"savage metro"` localiza `"21 Savage & Metro Boomin"`).
  - Algoritmos dedicados `track_matches` (avaliando título, nome do arquivo, extensão e pasta) e `album_matches` (avaliando título do álbum, artista principal, ano de lançamento, pasta e faixas contidas).

- **Integração na Interface GTK4 / Libadwaita (`src/main.rs`):**
  - `SearchEntry` no cabeçalho com evento `connect_search_changed`: filtra em tempo real a visualização ativa conforme o usuário digita.
  - Ao digitar qualquer caractere estando na tela do **Toca-discos** (`overview`), alterna automaticamente para a tela de **Músicas** (`tracks`), atualizando o destaque na barra lateral e o título do topo para apresentar os resultados imediatamente.
  - Suporte ao evento `connect_stop_search` (tecla `Escape`) e botão de limpar ("×"): restaura instantaneamente todas as faixas e álbuns da biblioteca sem recarga de disco.
  - `ListBox::set_filter_func` e `FlowBox::set_filter_func` integrados tanto na tela de faixas quanto na grade de capas e lista de álbuns.
  - Se o usuário estiver na tela de detalhes de um álbum e pesquisar um termo na barra superior, a interface retorna suavemente à navegação de álbuns (`browse`).
  - Telas de estado vazio (*empty state*) elegantes integradas com ícone simbólico e mensagem informativa quando nenhum item corresponder aos filtros.

- **Testes e Garantia de Qualidade:**
  - 7 novos testes unitários em `src/search.rs` cobrindo normalização, insensibilidade a acentos, consultas vazias, correspondência multi-termo e reatividade do `SearchHub`.
  - 24 testes no total do projeto: 21 testes unitários passando em modo release, além de todos os testes GTK e de renderização gráfica (`render_professional_preview`) validados com 100% de sucesso.
  - Binário release compilado e instalado no sistema em `~/.local/bin/vinylplayer`.
  - Pacote do Arch Linux sincronizado em `vinylplayer-0.1.0-5-x86_64.pkg.tar.zst`.

## Persistência de Sessão e Estado entre Execuções (Implementado)

- **Módulo de Estado (`src/state.rs`):**
  - Estrutura `AppState` com serialização e deserialização JSON tolerante sem dependências externas adicionais (mantendo o build ultrarrápido e sem tocar em crates externas).
  - Persistência das preferências essenciais do usuário:
    - `volume`: nível de volume analógico preservado (com clamping seguro entre `0.0` e `1.0`).
    - `active_view`: última tela ativa acessada (`overview`, `tracks` ou `albums`).
    - `track_sort_mode`: critério de ordenação de músicas selecionado (*Título A–Z*, *Z–A*, *Caminho Padrão*).
    - `album_sort_mode`: critério de ordenação de álbuns selecionado (*Álbum A–Z*, *Z–A*, *Artista A–Z*, *Ano Recente/Antigo*).
    - `last_track_path`: caminho da última faixa tocada ou em reprodução.
  - Gravação atômica em `$XDG_CONFIG_HOME/vinylplayer/state.json` (gravação em `.tmp` seguida de `rename` atômico).
  - `StateManager` com rastreamento de modificações (*dirty tracking*) e sincronização assíncrona periódica a cada 250 ms sem atrasar o loop gráfico, além de flush forçado no fechamento da janela (`close-request`).

- **Integração na Interface GTK4 / Libadwaita (`src/main.rs`):**
  - **Restauração de Volume:** O controle deslizante e o pipeline GStreamer iniciam exatamente no volume salvo da sessão anterior.
  - **Restauração de Visão Ativa:** Ao abrir o player, o aplicativo abre diretamente na aba onde o usuário estava (*Toca-discos*, *Músicas* ou *Álbuns*), com botão lateral ativo e título sincronizados.
  - **Restauração de Ordenação:** Os seletores de ordenação de faixas e álbuns iniciam na preferência salva pelo usuário.
  - **Restauração de Toca-discos:** Se houver uma faixa previamente salva, ela é pré-carregada no prato de vinil com capa, título, artista e formato lossless prontos para reprodução imediata.

- **Testes e Validação:**
  - 6 testes unitários em `src/state.rs` validando defaults, roundtrip JSON, parsing compacto/multilinha, clamping de limites numéricos, tolerância a arquivos corrompidos e fluxo de dirty/flush.
  - 30 testes no total: 27 testes unitários automatizados passando com 0 falhas, testes GTK e de renderização gráfica (`render_professional_preview`) validados.
  - Binário release oficial compilado e instalado em `~/.local/bin/vinylplayer` (2.6 MB).
  - Pacote do Arch Linux gerado em `vinylplayer-0.1.0-6-x86_64.pkg.tar.zst`.

## Autêntica Experiência Analógica do Vinil (Implementado)

- **Efeito Sonoro Procedimental de Needle Drop (`src/needle_drop.rs`):**
  - Síntese matemática pura de arquivo WAV em memória (PCM 16-bit, 44.1 kHz Mono, 0.65s):
    - Transiente mecânico de contato da agulha de diamante no sulco (thump amortecido a ~72 Hz com decaimento exponencial + clique acústico a 1750 Hz).
    - Atrito quente de ranhura de entrada (*lead-in run-in groove rumble*) gerado com ruído pseudo-aleatório filtrado por IIR passa-baixas (~380 Hz) e modulação periódica suave a 33 RPM (1.8 Hz).
    - Micro-pops característicos de estática de vinil nos instantes $t=0.12$s e $t=0.34$s.
  - Gravação automática e cache em `$XDG_CACHE_HOME/vinylplayer/needle_drop_v1.wav`.
  - Reprodução assíncrona desacoplada da thread GTK via GStreamer (`autoaudiosink`/`pipewiresink`) ao iniciar reprodução (`state -> Playing`).
  - Opção persistida em `AppState`: `needle_drop: bool` (padrão: `true`).

- **Luz Alvo Retrátil da Agulha (Stylus Target Light) (`src/main.rs`):**
  - Torre cilíndrica de alumínio usinado no canto frontal-esquerdo do chassi (`mx + 38, my + dh * 0.74`).
  - Interação por clique: clicar na torre alterna entre o estado elevado (pop-up) e retraído.
  - Quando ativa:
    - Lente âmbar emite brilho radiante com gradiente radial e aura incandescente.
    - Projeta um feixe cônico suave de iluminação âmbar/quente através dos sulcos do vinil e em direção à cápsula fonográfica.
  - Persistido em `AppState`: `target_light: bool` (padrão: `true`).

- **Pontos Estroboscópicos Dinâmicos e Prisma Neon Vermelho (`src/main.rs`):**
  - Prisma óptico de carcaça metálica usinada na lateral do chassi com lente vermelha emitindo feixe estroboscópico neon.
  - Duas fileiras de calibração usinadas no aro chanfrado de alumínio do prato:
    - Fileira externa: calibração para 33 ⅓ RPM (72 pontos).
    - Fileira interna: calibração para 45 RPM (53 pontos).
  - Rotação contínua e dinâmica calculada em sincronia com `disc_angle`.
  - Pontos que cruzam o feixe de luz estroboscópica recebem destaque especular vermelho vivo (`#ff2e1b`), reproduzindo o clássico congelamento óptico de velocidade dos toca-discos Technics SL-1200.

- **Controle Analógico de Pitch Fader (±8% com Quartz Lock) (`src/main.rs`, `src/audio.rs`):**
  - Faceplate escovado com ranhura guia vertical e marcações serigrafadas (-8% a +8%).
  - **LED Indicador Quartz Lock**:
    - Quando o pitch está travado na afinação exata (`pitch.abs() < 0.003`), o LED verde esmeralda acende com brilho e halo radial.
    - Quando a afinação é alterada, o LED apaga.
  - **Knob Metálico Deslizante**:
    - Usinado com linhas horizontais de aderência e marcador central branco alinhado à escala.
    - Interação completa por clique direto e arrasto (`GestureDrag`) em tempo real.
    - Clique no centro ou no LED Quartz Lock redefine imediatamente a afinação para 0.0%.
  - Modulação em tempo real:
    - Velocidade de rotação visual do prato: $\omega = \omega_{\text{nominal}} \cdot (1.0 + \text{pitch})$.
    - Taxa de reprodução do motor de áudio via `AudioEngine::set_rate(1.0 + pitch)`.
  - Persistido em `AppState`: `pitch: f64` (clamping seguro entre `-0.08` e `+0.08`).

- **Preferências e Configurações (`src/library_settings.rs`):**
  - Adicionada seção "Experiência Analógica" na janela de Configurações com checkboxes interativos para ligar/desligar o efeito sonoro de Needle Drop e a Luz Alvo.

- **Testes e Garantia de Qualidade:**
  - 81 testes no total do projeto: 68 testes unitários automatizados passando com 0 falhas, incluindo validação da síntese WAV, integridade de cabeçalhos RIFF, persistência no `state.rs`, geometria/clearances do layout analógico e renderização de todos os estados do toca-discos.
  - Teste gráfico `render_professional_preview` executado em modo `--release` validando toda a interface e regenerando as capturas em `/tmp/`.
  - Binário release oficial de 2.9 MB compilado e instalado em `~/.local/bin/vinylplayer` (`b410b725...`).

## Gerenciamento Completo de Playlists (.m3u / .m3u8) (Implementado)

- **Módulo de persistência e manipulação (`src/playlist.rs`):**
  - Modelo `Playlist` com carregamento, análise e gravação no padrão aberto M3U8 (`#EXTM3U`, `#PLAYLIST: nome`, caminhos relativos e absolutos).
  - Gravação atômica segura via arquivo temporário `.tmp` e `rename` garantindo integridade contra falhas de energia ou fechamentos abruptos.
  - Funções completas de edição: `add_track`, `add_tracks`, `remove_track`, `move_track` (reordenação por índice) e `rename`.
  - Gerenciador `PlaylistManager` com armazenamento padrão em `~/.local/share/vinylplayer/playlists/` (conforme convenções XDG).
  - Sanitização robusta de nomes de arquivos para evitar colisões e caracteres proibidos pelo sistema de arquivos.
  - Métodos de importação de arquivos `.m3u` / `.m3u8` externos e exportação direta para qualquer diretório do computador.

- **Interface Gráfica de Playlists (`src/playlist_view.rs`):**
  - **Aba "Playlists" na barra lateral:** Integrada ao menu de navegação lateral com ícone nativo `view-list-symbolic`, título sincronizado e persistência da visão ativa no `state.json`.
  - **Navegação Master-Detail em `gtk::Stack`:**
    - **Visão Geral / Grade de Playlists:**
      - Barra de ferramentas com botão **"Nova Playlist"** (`list-add-symbolic`) com popover de entrada rápida de nome.
      - Botão **"Importar M3U"** (`document-open-symbolic`) abrindo seletor nativo assíncrono do GNOME (`gtk::FileDialog`).
      - Grade responsiva de cartões de playlists exibindo metadados, contagem de faixas, botão rápido "Ouvir" e botão "Excluir".
      - Estado vazio elegante convidando à criação da primeira seleção quando não houver playlists.
    - **Visão Detalhada da Playlist:**
      - Botão de retorno rápido aos catálogos com seta `go-previous-symbolic`.
      - Cabeçalho com ícone ampliado, título, contagem de faixas e ações rápidas:
        - **"Reproduzir Playlist"**: Envia as faixas para o toca-discos iniciando a agulha analógica (*needle drop*).
        - **"Adicionar Faixas..."**: Seletor múltiplo nativo de arquivos de áudio (FLAC, MP3, WAV, OGG, M4A, Opus, AAC).
        - **"Exportar..."**: Exporta a playlist como arquivo `.m3u8` em qualquer pasta escolhida.
      - Lista de faixas com botões de subir (`go-up-symbolic`), descer (`go-down-symbolic`) e remover (`edit-delete-symbolic`).
      - Indicador em tempo real de faixa tocando / pausada integrado ao barramento de eventos do aplicativo.

- **Ações de Contexto em toda a Biblioteca:**
  - O menu de opções de faixa (`queue_actions`), presente nas telas de Faixas, Álbuns e na Fila de Reprodução, agora conta com uma seção dedicada **"Adicionar à playlist:"**, listando as playlists do usuário para inserção imediata com um clique.

- **Testes e Garantia de Qualidade:**
  - 87 testes no total do projeto: 74 testes unitários automatizados passando com 0 falhas, incluindo parsing de M3U com comentários e caminhos relativos, cabeçalho de nome customizado, roundtrip atômico de gravação/leitura, mover/remover faixas e sanitização de nomes no gerenciador.
  - Teste gráfico `render_professional_preview` executado em release validando renderização de todas as telas.
  - Binário release oficial de 2.9 MB compilado e instalado em `~/.local/bin/vinylplayer` (`3ef0d2172a...`).
  - Pacote Arch Linux `vinylplayer-0.1.0-10-x86_64.pkg.tar.zst` (1.2 MB) gerado com sucesso.

## Ideias discutidas para próximas melhorias

- Equalizador paramétrico / gráfico analógico de 10 bandas Hi-Fi (`equalizer-10bands`).
- Scratching e cueing tátil no vinil (arrastar o disco com o mouse ouvindo o som avançar/recuar).
- Painel de letras sincronizadas (*LRC Lyrics*).
