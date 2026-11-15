use crate::audio::RepeatMode;
use crate::playback_queue::QueueSnapshot;
use gtk::gio::prelude::FileExt;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

/// O vetor é compartilhado entre snapshots; mudar a seleção não copia a fila.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SavedQueue {
    pub tracks: Arc<Vec<PathBuf>>,
    pub current: Option<usize>,
}

impl SavedQueue {
    fn to_json(&self) -> serde_json::Value {
        let files: Vec<_> = self
            .tracks
            .iter()
            .map(|path| gtk::gio::File::for_path(path).uri().to_string())
            .collect();
        serde_json::json!({"version": 1, "files": files, "current": self.current})
    }

    fn from_json(value: &serde_json::Value) -> Option<Self> {
        if value.get("version")?.as_u64()? != 1 {
            return None;
        }
        let files = value.get("files")?.as_array()?;
        let requested = value
            .get("current")
            .and_then(|v| v.as_u64())
            .and_then(|n| usize::try_from(n).ok())
            .filter(|n| *n < files.len())
            .unwrap_or(0);
        let entries: Vec<_> = files
            .iter()
            .enumerate()
            .filter_map(|(index, file)| {
                let uri = file.as_str()?;
                if !uri.starts_with("file://") {
                    return None;
                }
                let (path, host) = glib::filename_from_uri(uri).ok()?;
                if host
                    .as_deref()
                    .is_some_and(|host| !host.is_empty() && host != "localhost")
                {
                    return None;
                }
                path.is_absolute().then_some((index, path))
            })
            .collect();
        // Mantém arquivos ausentes (por exemplo, disco desmontado) e entradas repetidas.
        // Se a URI selecionada for inválida, usa a próxima válida ou a anterior.
        let current = entries
            .iter()
            .position(|(index, _)| *index >= requested)
            .or_else(|| entries.len().checked_sub(1));
        Some(Self {
            tracks: Arc::new(entries.into_iter().map(|(_, path)| path).collect()),
            current,
        })
    }
}

/// Estado persistido da sessão do VinylPlayer.
#[derive(Debug, Clone, PartialEq)]
pub struct AppState {
    pub volume: f64,
    pub active_view: String,
    pub track_sort_mode: usize,
    pub album_sort_mode: usize,
    pub last_track_path: Option<PathBuf>,
    /// None significa preferências antigas; Some com vetor vazio é uma fila vazia explícita.
    pub queue: Option<SavedQueue>,
    /// None migra a pasta da versão anterior; Some vazio é uma biblioteca sem pastas.
    pub library_folders: Option<Vec<PathBuf>>,
    /// Velocidade de rotação do toca-discos (33 ou 45 RPM). Padrão: 33.
    pub rpm: u32,
    /// Ordem aleatória (Shuffle). Padrão: false.
    pub shuffle: bool,
    /// Modo de repetição (off, all, track). Padrão: off.
    pub repeat_mode: RepeatMode,
    /// Efeito sonoro autêntico de pouso da agulha (Needle Drop). Padrão: true.
    pub needle_drop: bool,
    /// Luz alvo da agulha (Stylus Target Light). Padrão: true.
    pub target_light: bool,
    /// Controle fino de pitch analógico (±8%). Padrão: 0.0.
    pub pitch: f64,
    /// Integração nativa com Last.fm Scrobbler ativa. Padrão: true.
    pub lastfm_enabled: bool,
    /// Chave de sessão permanente da Last.fm (sk).
    pub lastfm_session_key: Option<String>,
    /// Nome de usuário da Last.fm autenticado.
    pub lastfm_username: Option<String>,
    /// Chave de API personalizada da Last.fm (None usa a padrão embutida).
    pub lastfm_api_key: Option<String>,
    /// Shared secret personalizado da Last.fm (None usa o padrão embutido).
    pub lastfm_api_secret: Option<String>,
}

impl Default for AppState {
    fn default() -> Self {
        Self {
            volume: 1.0,
            active_view: "overview".to_string(),
            track_sort_mode: 0,
            album_sort_mode: 0,
            last_track_path: None,
            queue: None,
            library_folders: None,
            rpm: 33,
            shuffle: false,
            repeat_mode: RepeatMode::Off,
            needle_drop: true,
            target_light: true,
            pitch: 0.0,
            lastfm_enabled: true,
            lastfm_session_key: None,
            lastfm_username: None,
            lastfm_api_key: None,
            lastfm_api_secret: None,
        }
    }
}

impl AppState {
    pub fn library_roots(&self) -> Vec<PathBuf> {
        self.library_folders
            .clone()
            .unwrap_or_else(|| vec![PathBuf::from("/mnt/armazenamento/music")])
    }

    pub fn lastfm_api_key(&self) -> &str {
        self.lastfm_api_key
            .as_deref()
            .unwrap_or(crate::scrobbler::DEFAULT_API_KEY)
    }

    pub fn lastfm_api_secret(&self) -> &str {
        self.lastfm_api_secret
            .as_deref()
            .unwrap_or(crate::scrobbler::DEFAULT_API_SECRET)
    }

    pub fn is_lastfm_authenticated(&self) -> bool {
        self.lastfm_session_key
            .as_ref()
            .is_some_and(|k| !k.is_empty())
    }

    /// Serializa o estado para formato JSON formatado.
    pub fn to_json(&self) -> String {
        let mut value = serde_json::json!({
            "volume": self.volume.clamp(0.0, 1.0),
            "active_view": self.active_view,
            "track_sort_mode": self.track_sort_mode,
            "album_sort_mode": self.album_sort_mode,
            "last_track_path": self.last_track_path.as_ref().map(|p| p.to_string_lossy()),
            "rpm": if self.rpm == 45 { 45 } else { 33 },
            "shuffle": self.shuffle,
            "repeat_mode": self.repeat_mode.to_str(),
            "needle_drop": self.needle_drop,
            "target_light": self.target_light,
            "pitch": self.pitch.clamp(-0.08, 0.08),
            "lastfm_enabled": self.lastfm_enabled,
            "lastfm_session_key": self.lastfm_session_key,
            "lastfm_username": self.lastfm_username,
            "lastfm_api_key": self.lastfm_api_key,
            "lastfm_api_secret": self.lastfm_api_secret,
        });
        if let Some(queue) = &self.queue {
            value["queue"] = queue.to_json();
        }
        if let Some(folders) = &self.library_folders {
            value["library_folders"] = serde_json::json!(folders
                .iter()
                .map(|path| gtk::gio::File::for_path(path).uri().to_string())
                .collect::<Vec<_>>());
        }
        serde_json::to_string_pretty(&value).expect("objeto JSON de preferências") + "\n"
    }

    /// Desserializa de forma tolerante a partir de JSON (suporta multilinhas e linha única),
    /// usando valores padrão para chaves ausentes ou corrompidas.
    pub fn from_json(json: &str) -> Self {
        let mut state = Self::default();

        let Ok(value) = serde_json::from_str::<serde_json::Value>(json) else {
            return state;
        };
        if let Some(volume) = value
            .get("volume")
            .and_then(|v| v.as_f64())
            .filter(|n| n.is_finite())
        {
            state.volume = volume.clamp(0.0, 1.0);
        }
        if let Some(view) = value.get("active_view").and_then(|v| v.as_str()) {
            if matches!(view, "overview" | "tracks" | "albums" | "playlists") {
                state.active_view = view.into();
            }
        }
        if let Some(mode) = value.get("track_sort_mode").and_then(|v| v.as_u64()) {
            state.track_sort_mode = mode.min(2) as usize;
        }
        if let Some(mode) = value.get("album_sort_mode").and_then(|v| v.as_u64()) {
            state.album_sort_mode = mode.min(4) as usize;
        }
        if let Some(rpm) = value.get("rpm").and_then(|v| v.as_u64()) {
            state.rpm = if rpm == 45 { 45 } else { 33 };
        }
        if let Some(shuffle) = value.get("shuffle").and_then(|v| v.as_bool()) {
            state.shuffle = shuffle;
        }
        if let Some(repeat) = value.get("repeat_mode").and_then(|v| v.as_str()) {
            state.repeat_mode = RepeatMode::from_str(repeat);
        }
        if let Some(nd) = value.get("needle_drop").and_then(|v| v.as_bool()) {
            state.needle_drop = nd;
        }
        if let Some(tl) = value.get("target_light").and_then(|v| v.as_bool()) {
            state.target_light = tl;
        }
        if let Some(pitch) = value
            .get("pitch")
            .and_then(|v| v.as_f64())
            .filter(|n| n.is_finite())
        {
            state.pitch = pitch.clamp(-0.08, 0.08);
        }
        if let Some(lfm) = value.get("lastfm_enabled").and_then(|v| v.as_bool()) {
            state.lastfm_enabled = lfm;
        }
        state.lastfm_session_key = value
            .get("lastfm_session_key")
            .and_then(|v| v.as_str())
            .map(String::from);
        state.lastfm_username = value
            .get("lastfm_username")
            .and_then(|v| v.as_str())
            .map(String::from);
        state.lastfm_api_key = value
            .get("lastfm_api_key")
            .and_then(|v| v.as_str())
            .map(String::from);
        state.lastfm_api_secret = value
            .get("lastfm_api_secret")
            .and_then(|v| v.as_str())
            .map(String::from);
        state.last_track_path = value
            .get("last_track_path")
            .and_then(|v| v.as_str())
            .map(PathBuf::from)
            .filter(|p| p.is_file());
        state.queue = value.get("queue").and_then(SavedQueue::from_json);
        if let Some(folders) = value.get("library_folders").and_then(|v| v.as_array()) {
            let mut roots = Vec::new();
            for uri in folders
                .iter()
                .filter_map(|v| v.as_str())
                .filter(|uri| uri.starts_with("file://"))
            {
                if let Ok((path, host)) = glib::filename_from_uri(uri) {
                    if path.is_absolute()
                        && host
                            .as_deref()
                            .is_none_or(|h| h.is_empty() || h == "localhost")
                        && !roots.contains(&path)
                    {
                        roots.push(path);
                    }
                }
            }
            state.library_folders = Some(roots);
        }
        if let Some(queue) = &state.queue {
            state.last_track_path = queue.current.and_then(|i| queue.tracks.get(i)).cloned();
        }
        state
    }

    /// Caminho do arquivo de configuração em ~/.config/vinylplayer/state.json.
    pub fn file_path() -> PathBuf {
        glib::user_config_dir()
            .join("vinylplayer")
            .join("state.json")
    }

    /// Lê somente o arquivo indicado; testes injetam um caminho temporário.
    fn load_from(path: &Path) -> Self {
        std::fs::read_to_string(path)
            .map(|content| Self::from_json(&content))
            .unwrap_or_default()
    }

    /// Publica o arquivo por rename, depois de escrever e sincronizar um temporário exclusivo.
    fn save_to(&self, path: &Path) -> std::io::Result<()> {
        use std::io::Write;
        static NEXT_TEMP: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        let parent = path
            .parent()
            .filter(|p| !p.as_os_str().is_empty())
            .unwrap_or(Path::new("."));
        std::fs::create_dir_all(parent)?;
        let name = path.file_name().unwrap_or_default().to_string_lossy();
        let temporary = parent.join(format!(
            ".{name}-{}-{}.tmp",
            std::process::id(),
            NEXT_TEMP.fetch_add(1, Ordering::Relaxed)
        ));
        let mut options = std::fs::OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        let mut file = options.open(&temporary)?;
        let result = (|| {
            file.write_all(self.to_json().as_bytes())?;
            file.sync_all()?;
            drop(file);
            std::fs::rename(&temporary, path)
        })();
        if result.is_err() {
            let _ = std::fs::remove_file(&temporary);
        }
        result
    }
}

struct VersionedState {
    value: AppState,
    revision: u64,
    saved_revision: u64,
}

struct Store {
    path: PathBuf,
    data: Mutex<VersionedState>,
    writer: Mutex<()>,
    saving: AtomicBool,
}

/// Preferências compartilhadas, com escrita serializada e revisões confirmadas após sucesso.
#[derive(Clone)]
pub struct StateManager {
    inner: Arc<Store>,
}

impl StateManager {
    pub fn new() -> Self {
        Self::with_path(AppState::file_path())
    }

    pub(crate) fn with_path(path: PathBuf) -> Self {
        let value = AppState::load_from(&path);
        Self {
            inner: Arc::new(Store {
                path,
                data: Mutex::new(VersionedState {
                    value,
                    revision: 0,
                    saved_revision: 0,
                }),
                writer: Mutex::new(()),
                saving: AtomicBool::new(false),
            }),
        }
    }

    pub fn snapshot(&self) -> AppState {
        self.inner.data.lock().unwrap().value.clone()
    }

    fn update(&self, edit: impl FnOnce(&mut AppState)) {
        let mut data = self.inner.data.lock().unwrap();
        let previous = data.value.clone();
        edit(&mut data.value);
        if data.value != previous {
            data.revision += 1;
        }
    }

    pub fn update_volume(&self, vol: f64) {
        self.update(|state| state.volume = vol.clamp(0.0, 1.0));
    }
    pub fn update_active_view(&self, view: &str) {
        self.update(|state| state.active_view = view.to_owned());
    }
    pub fn update_library_folders(&self, folders: Vec<PathBuf>) {
        self.update(|state| state.library_folders = Some(folders));
    }
    pub fn update_track_sort_mode(&self, mode: usize) {
        self.update(|state| state.track_sort_mode = mode);
    }
    pub fn update_album_sort_mode(&self, mode: usize) {
        self.update(|state| state.album_sort_mode = mode);
    }
    pub fn update_rpm(&self, rpm: u32) {
        let rpm = if rpm == 45 { 45 } else { 33 };
        self.update(|state| state.rpm = rpm);
    }
    pub fn update_shuffle(&self, shuffle: bool) {
        self.update(|state| state.shuffle = shuffle);
    }
    pub fn update_repeat_mode(&self, mode: RepeatMode) {
        self.update(|state| state.repeat_mode = mode);
    }
    pub fn update_needle_drop(&self, enabled: bool) {
        self.update(|state| state.needle_drop = enabled);
    }
    pub fn update_target_light(&self, enabled: bool) {
        self.update(|state| state.target_light = enabled);
    }
    pub fn update_pitch(&self, pitch: f64) {
        let p = pitch.clamp(-0.08, 0.08);
        self.update(|state| state.pitch = p);
    }
    pub fn update_lastfm_credentials(&self, session_key: Option<String>, username: Option<String>) {
        self.update(|state| {
            state.lastfm_session_key = session_key;
            state.lastfm_username = username;
        });
    }
    pub fn update_lastfm_enabled(&self, enabled: bool) {
        self.update(|state| state.lastfm_enabled = enabled);
    }
    pub fn update_lastfm_custom_api(&self, api_key: Option<String>, api_secret: Option<String>) {
        self.update(|state| {
            state.lastfm_api_key = api_key;
            state.lastfm_api_secret = api_secret;
        });
    }
    pub fn update_queue(&self, snapshot: QueueSnapshot) {
        let queue = SavedQueue {
            tracks: Arc::new(snapshot.tracks),
            current: snapshot.current,
        };
        self.update(|state| {
            state.last_track_path = queue.current.and_then(|i| queue.tracks.get(i)).cloned();
            state.queue = Some(queue);
        });
    }

    pub fn update_queue_selection(&self, current: Option<usize>) {
        self.update(|state| {
            if let Some(queue) = state.queue.as_mut() {
                queue.current = current.filter(|i| *i < queue.tracks.len());
                state.last_track_path = queue.current.and_then(|i| queue.tracks.get(i)).cloned();
            }
        });
    }

    fn is_dirty(&self) -> bool {
        let data = self.inner.data.lock().unwrap();
        data.revision != data.saved_revision
    }

    fn flush_with(
        &self,
        force: bool,
        save: impl FnOnce(&AppState, &Path) -> std::io::Result<()>,
    ) -> std::io::Result<()> {
        // A revisão é capturada DEPOIS de obter o lock de escrita: um worker atrasado
        // nunca sobrescreve um flush mais novo realizado ao fechar a janela.
        let _writer = self.inner.writer.lock().unwrap();
        let (snapshot, revision) = {
            let data = self.inner.data.lock().unwrap();
            if !force && data.revision == data.saved_revision {
                return Ok(());
            }
            (data.value.clone(), data.revision)
        };
        save(&snapshot, &self.inner.path)?;
        let mut data = self.inner.data.lock().unwrap();
        data.saved_revision = data.saved_revision.max(revision);
        Ok(())
    }

    /// Agenda somente um worker por gerenciador, sem I/O de disco na chamada GTK.
    pub fn flush_if_dirty(&self) {
        if !self.is_dirty()
            || self
                .inner
                .saving
                .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
                .is_err()
        {
            return;
        }
        let manager = self.clone();
        let spawn = std::thread::Builder::new()
            .name("vinyl-state-writer".into())
            .spawn(move || {
                if let Err(error) = manager.flush_with(false, AppState::save_to) {
                    eprintln!(
                        "Não foi possível salvar as preferências; nova tentativa pendente: {error}"
                    );
                }
                manager.inner.saving.store(false, Ordering::Release);
            });
        if let Err(error) = spawn {
            self.inner.saving.store(false, Ordering::Release);
            eprintln!("Não foi possível iniciar a gravação das preferências: {error}");
        }
    }

    /// O fechamento aguarda a escrita em curso e confirma o snapshot mais recente.
    pub fn flush_now(&self) -> std::io::Result<()> {
        self.flush_with(false, AppState::save_to)
    }
}

#[cfg(test)]
pub(crate) struct TestConfig {
    dir: PathBuf,
}

#[cfg(test)]
impl TestConfig {
    pub(crate) fn new() -> Self {
        static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        let dir = std::env::temp_dir().join(format!(
            "vinyl-state-test-{}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir(&dir).unwrap();
        Self { dir }
    }
    pub(crate) fn path(&self) -> PathBuf {
        self.dir.join("state.json")
    }
}

#[cfg(test)]
impl Drop for TestConfig {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn library_folders_persist_empty_missing_and_special_paths_with_legacy_fallback() {
        let config = TestConfig::new();
        let manager = StateManager::with_path(config.path());
        assert_eq!(
            manager.snapshot().library_roots(),
            vec![PathBuf::from("/mnt/armazenamento/music")]
        );
        let roots = vec![
            config.path().with_file_name("Coleção \"A\"\n#"),
            config.path().with_file_name("disco ausente"),
        ];
        manager.update_library_folders(roots.clone());
        manager.flush_now().unwrap();
        assert_eq!(
            StateManager::with_path(config.path())
                .snapshot()
                .library_roots(),
            roots
        );
        manager.update_library_folders(Vec::new());
        manager.flush_now().unwrap();
        assert_eq!(
            StateManager::with_path(config.path())
                .snapshot()
                .library_folders,
            Some(Vec::new())
        );
        let parsed = AppState::from_json(
            r#"{"volume":0.4,"library_folders":["https://example.invalid/music","file:///tmp/music","file:///tmp/music","file://remote-host/music"]}"#,
        );
        assert_eq!(parsed.library_roots(), vec![PathBuf::from("/tmp/music")]);
        assert_eq!(parsed.volume, 0.4);
    }

    #[test]
    fn default_state_has_expected_defaults() {
        let st = AppState::default();
        assert!((st.volume - 1.0).abs() < f64::EPSILON);
        assert_eq!(st.active_view, "overview");
        assert_eq!(st.track_sort_mode, 0);
        assert_eq!(st.album_sort_mode, 0);
        assert_eq!(st.last_track_path, None);
        assert_eq!(st.rpm, 33);
        assert!(!st.shuffle);
        assert_eq!(st.repeat_mode, RepeatMode::Off);
        assert!(st.needle_drop);
        assert!(st.target_light);
        assert!((st.pitch - 0.0).abs() < f64::EPSILON);
        assert!(st.lastfm_enabled);
        assert_eq!(st.lastfm_session_key, None);
        assert_eq!(st.lastfm_username, None);
        assert_eq!(st.lastfm_api_key, None);
        assert_eq!(st.lastfm_api_secret, None);
    }

    #[test]
    fn roundtrip_serialization_and_deserialization() {
        let st = AppState {
            volume: 0.725,
            active_view: "albums".to_string(),
            track_sort_mode: 1,
            album_sort_mode: 3,
            last_track_path: None,
            queue: None,
            library_folders: None,
            rpm: 45,
            shuffle: true,
            repeat_mode: RepeatMode::Track,
            needle_drop: false,
            target_light: true,
            pitch: 0.035,
            lastfm_enabled: true,
            lastfm_session_key: Some("test_session_key_123".into()),
            lastfm_username: Some("vinyl_fan".into()),
            lastfm_api_key: None,
            lastfm_api_secret: None,
        };
        let json = st.to_json();
        let restored = AppState::from_json(&json);
        assert!((restored.volume - 0.725).abs() < 0.001);
        assert_eq!(restored.active_view, "albums");
        assert_eq!(restored.track_sort_mode, 1);
        assert_eq!(restored.album_sort_mode, 3);
        assert_eq!(restored.last_track_path, None);
        assert_eq!(restored.rpm, 45);
        assert!(restored.shuffle);
        assert_eq!(restored.repeat_mode, RepeatMode::Track);
        assert!(!restored.needle_drop);
        assert!(restored.target_light);
        assert!((restored.pitch - 0.035).abs() < 0.0001);
        assert!(restored.lastfm_enabled);
        assert_eq!(restored.lastfm_session_key.as_deref(), Some("test_session_key_123"));
        assert_eq!(restored.lastfm_username.as_deref(), Some("vinyl_fan"));
    }

    #[test]
    fn shuffle_and_repeat_mode_updates_and_persists() {
        let dir = std::env::temp_dir().join(format!(
            "vinylplayer-shuffle-repeat-test-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let path = dir.join("state.json");
        let mgr = StateManager::with_path(path.clone());

        assert!(!mgr.snapshot().shuffle);
        assert_eq!(mgr.snapshot().repeat_mode, RepeatMode::Off);

        mgr.update_shuffle(true);
        mgr.update_repeat_mode(RepeatMode::All);
        assert!(mgr.snapshot().shuffle);
        assert_eq!(mgr.snapshot().repeat_mode, RepeatMode::All);

        mgr.flush_now().unwrap();
        let restored = StateManager::with_path(path.clone());
        assert!(restored.snapshot().shuffle);
        assert_eq!(restored.snapshot().repeat_mode, RepeatMode::All);

        restored.update_repeat_mode(RepeatMode::Track);
        restored.flush_now().unwrap();
        let re_restored = StateManager::with_path(path);
        assert_eq!(re_restored.snapshot().repeat_mode, RepeatMode::Track);

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn single_line_compact_json_parses_properly() {
        let json = r#"{"volume":0.85,"active_view":"tracks","track_sort_mode":2,"album_sort_mode":4,"last_track_path":null,"rpm":45}"#;
        let st = AppState::from_json(json);
        assert!((st.volume - 0.85).abs() < 0.001);
        assert_eq!(st.active_view, "tracks");
        assert_eq!(st.track_sort_mode, 2);
        assert_eq!(st.album_sort_mode, 4);
        assert_eq!(st.last_track_path, None);
        assert_eq!(st.rpm, 45);
    }

    #[test]
    fn rpm_updates_and_persists() {
        let config = TestConfig::new();
        let mgr = StateManager::with_path(config.path());
        assert_eq!(mgr.snapshot().rpm, 33);
        mgr.update_rpm(45);
        assert_eq!(mgr.snapshot().rpm, 45);
        mgr.flush_now().unwrap();
        let reloaded = StateManager::with_path(config.path());
        assert_eq!(reloaded.snapshot().rpm, 45);
        mgr.update_rpm(78);
        assert_eq!(mgr.snapshot().rpm, 33);
    }

    #[test]
    fn volume_is_clamped_properly() {
        let json = r#"{ "volume": 1.5, "track_sort_mode": 99 }"#;
        let st = AppState::from_json(json);
        assert_eq!(st.volume, 1.0);
        assert_eq!(st.track_sort_mode, 2);

        let json_neg = r#"{ "volume": -0.5 }"#;
        let st_neg = AppState::from_json(json_neg);
        assert_eq!(st_neg.volume, 0.0);
    }

    #[test]
    fn corrupt_json_falls_back_safely() {
        let corrupt = "random string not even json";
        let st = AppState::from_json(corrupt);
        assert_eq!(st, AppState::default());
    }

    #[test]
    fn queue_roundtrip_preserves_order_duplicates_and_escaped_paths() {
        let config = TestConfig::new();
        let path = config
            .path()
            .with_file_name("Música #1, \"ao vivo\" \\ bis\n.wav");
        let other = config.path().with_file_name("02 - Outra.flac");
        let manager = StateManager::with_path(config.path());
        manager.update_volume(0.37);
        manager.update_queue(QueueSnapshot {
            revision: 1,
            tracks: vec![path.clone(), other.clone(), path.clone()],
            current: Some(2),
        });
        manager.flush_now().unwrap();
        let json = std::fs::read_to_string(config.path()).unwrap();
        assert!(serde_json::from_str::<serde_json::Value>(&json).is_ok());
        let restored = StateManager::with_path(config.path()).snapshot();
        let saved = restored.queue.unwrap();
        assert_eq!(saved.tracks.as_ref(), &[path.clone(), other, path.clone()]);
        assert_eq!(saved.current, Some(2));
        assert_eq!(restored.last_track_path, Some(path));
        assert_eq!(restored.volume, 0.37);
    }

    #[test]
    fn empty_queue_is_distinct_from_legacy_and_clears_last_track() {
        let config = TestConfig::new();
        let track = config.path().with_file_name("legacy.wav");
        std::fs::write(&track, b"fixture").unwrap();
        let legacy =
            serde_json::json!({"volume": 0.42, "active_view": "albums", "last_track_path": track});
        let old = AppState::from_json(&legacy.to_string());
        assert!(old.queue.is_none());
        assert_eq!(old.last_track_path, Some(track.clone()));
        assert_eq!(old.active_view, "albums");
        let manager = StateManager::with_path(config.path());
        manager.update_queue(QueueSnapshot {
            revision: 1,
            tracks: vec![track],
            current: Some(0),
        });
        manager.update_queue(QueueSnapshot::default());
        manager.flush_now().unwrap();
        let restored = AppState::load_from(&config.path());
        assert!(restored.queue.unwrap().tracks.is_empty());
        assert_eq!(restored.last_track_path, None);
    }

    #[test]
    fn malformed_queue_does_not_discard_valid_preferences_and_invalid_entries_are_remapped() {
        let first = "file:///tmp/vinyl-first.wav";
        let last = "file:///tmp/vinyl-last.wav";
        let mut value = serde_json::json!({"volume": 0.4, "active_view": "albums", "queue": {
            "version": 1, "files": [first, "https://example.invalid/a.wav", 42, last, "file://remote.invalid/tmp/b.wav"], "current": 1
        }});
        let saved = AppState::from_json(&value.to_string()).queue.unwrap();
        assert_eq!(
            saved.tracks.as_ref(),
            &[
                PathBuf::from("/tmp/vinyl-first.wav"),
                PathBuf::from("/tmp/vinyl-last.wav")
            ]
        );
        assert_eq!(saved.current, Some(1));
        value["queue"]["current"] = serde_json::json!(99999);
        assert_eq!(
            AppState::from_json(&value.to_string())
                .queue
                .unwrap()
                .current,
            Some(0)
        );
        value["queue"]["version"] = serde_json::json!(99);
        let restored = AppState::from_json(&value.to_string());
        assert!(restored.queue.is_none());
        assert_eq!(restored.volume, 0.4);
        assert_eq!(restored.active_view, "albums");
        value["queue"] = serde_json::json!({"version": 1, "files": "broken"});
        assert!(AppState::from_json(&value.to_string()).queue.is_none());
    }

    #[test]
    fn selection_updates_share_the_track_vector_and_are_persisted() {
        let config = TestConfig::new();
        let manager = StateManager::with_path(config.path());
        let tracks = vec![
            config.path().with_file_name("a.wav"),
            config.path().with_file_name("b.wav"),
        ];
        manager.update_queue(QueueSnapshot {
            revision: 1,
            tracks: tracks.clone(),
            current: Some(0),
        });
        manager.flush_now().unwrap();
        let before = manager.snapshot().queue.unwrap();
        manager.update_queue_selection(Some(1));
        let after = manager.snapshot().queue.unwrap();
        assert!(Arc::ptr_eq(&before.tracks, &after.tracks));
        assert!(manager.is_dirty());
        manager.flush_now().unwrap();
        let restored = AppState::load_from(&config.path());
        assert_eq!(restored.queue.unwrap().current, Some(1));
        assert_eq!(restored.last_track_path, Some(tracks[1].clone()));
        manager.update_queue_selection(Some(1));
        assert!(!manager.is_dirty());
    }

    #[test]
    fn queue_changed_during_save_is_not_lost_when_closing() {
        let config = TestConfig::new();
        let manager = StateManager::with_path(config.path());
        let tracks = vec![
            config.path().with_file_name("a.wav"),
            config.path().with_file_name("b.wav"),
        ];
        manager.update_queue(QueueSnapshot {
            revision: 1,
            tracks: tracks.clone(),
            current: Some(0),
        });
        manager
            .flush_with(false, |snapshot, path| {
                manager.update_queue(QueueSnapshot {
                    revision: 2,
                    tracks: vec![tracks[1].clone()],
                    current: Some(0),
                });
                snapshot.save_to(path)
            })
            .unwrap();
        assert!(manager.is_dirty());
        assert_eq!(
            AppState::load_from(&config.path())
                .queue
                .unwrap()
                .tracks
                .len(),
            2
        );
        manager.flush_now().unwrap();
        assert_eq!(
            AppState::load_from(&config.path())
                .queue
                .unwrap()
                .tracks
                .as_ref(),
            &[tracks[1].clone()]
        );
    }

    #[cfg(unix)]
    #[test]
    fn non_utf8_paths_roundtrip_through_native_file_uris() {
        use std::os::unix::ffi::OsStringExt;
        let config = TestConfig::new();
        let path = config
            .path()
            .with_file_name(std::ffi::OsString::from_vec(b"track-\xff.wav".to_vec()));
        let manager = StateManager::with_path(config.path());
        manager.update_queue(QueueSnapshot {
            revision: 1,
            tracks: vec![path.clone()],
            current: Some(0),
        });
        manager.flush_now().unwrap();
        assert_eq!(
            AppState::load_from(&config.path())
                .queue
                .unwrap()
                .tracks
                .as_ref(),
            &[path]
        );
    }

    #[test]
    fn state_manager_tracks_dirty_and_flushes() {
        let config = TestConfig::new();
        let mgr = StateManager::with_path(config.path());
        assert!(!mgr.is_dirty());

        mgr.update_volume(0.65);
        assert!(mgr.is_dirty());
        assert_eq!(mgr.snapshot().volume, 0.65);

        mgr.flush_now().unwrap();
        assert!(!mgr.is_dirty());
        assert_eq!(AppState::load_from(&config.path()).volume, 0.65);
    }

    #[test]
    fn flushing_unchanged_preferences_does_not_write_a_file() {
        let config = TestConfig::new();
        let manager = StateManager::with_path(config.path());
        manager.flush_now().unwrap();
        manager.flush_if_dirty();
        assert!(!config.path().exists());
        assert!(!manager.inner.saving.load(Ordering::Acquire));
    }

    #[test]
    fn managers_use_independent_paths_and_restore_saved_preferences() {
        let a = TestConfig::new();
        let b = TestConfig::new();
        let first = StateManager::with_path(a.path());
        let second = StateManager::with_path(b.path());
        first.update_volume(0.2);
        second.update_volume(0.8);
        first.update_active_view("albums");
        first.flush_now().unwrap();
        second.flush_now().unwrap();
        let restored = StateManager::with_path(a.path()).snapshot();
        assert_eq!(restored.volume, 0.2);
        assert_eq!(restored.active_view, "albums");
        assert_eq!(AppState::load_from(&b.path()).volume, 0.8);
    }

    #[test]
    fn write_errors_keep_changes_pending_and_retry_succeeds() {
        let config = TestConfig::new();
        let blocker = config.dir.join("blocked");
        std::fs::write(&blocker, b"not a directory").unwrap();
        let path = blocker.join("state.json");
        let manager = StateManager::with_path(path.clone());
        manager.update_volume(0.3);
        assert!(manager.flush_now().is_err());
        assert!(manager.is_dirty());
        std::fs::remove_file(&blocker).unwrap();
        manager.flush_now().unwrap();
        assert!(!manager.is_dirty());
        assert_eq!(AppState::load_from(&path).volume, 0.3);
    }

    #[test]
    fn edits_during_save_remain_pending_until_the_next_save() {
        let config = TestConfig::new();
        let manager = StateManager::with_path(config.path());
        manager.update_volume(0.2);
        manager
            .flush_with(false, |snapshot, path| {
                manager.update_volume(0.9);
                snapshot.save_to(path)
            })
            .unwrap();
        assert!(manager.is_dirty());
        assert_eq!(AppState::load_from(&config.path()).volume, 0.2);
        manager.flush_now().unwrap();
        assert!(!manager.is_dirty());
        assert_eq!(AppState::load_from(&config.path()).volume, 0.9);
    }

    #[test]
    fn background_flush_returns_while_writer_is_busy_and_close_saves_latest() {
        let config = TestConfig::new();
        let manager = StateManager::with_path(config.path());
        let writer = manager.inner.writer.lock().unwrap();
        manager.update_volume(0.2);
        manager.flush_if_dirty();
        assert!(manager.inner.saving.load(Ordering::Acquire));
        assert!(!config.path().exists());
        manager.update_volume(0.8);
        manager.flush_if_dirty(); // Não agenda outro worker.
        drop(writer);
        manager.flush_now().unwrap();
        assert!(!manager.is_dirty());
        assert_eq!(AppState::load_from(&config.path()).volume, 0.8);
    }

    #[test]
    fn failed_save_preserves_previous_file_and_pending_revision() {
        let config = TestConfig::new();
        let manager = StateManager::with_path(config.path());
        manager.update_volume(0.4);
        manager.flush_now().unwrap();
        let original = std::fs::read(config.path()).unwrap();
        manager.update_volume(0.7);
        let error = manager.flush_with(false, |_, _| {
            Err(std::io::Error::other("simulated I/O error"))
        });
        assert!(error.is_err());
        assert!(manager.is_dirty());
        assert_eq!(std::fs::read(config.path()).unwrap(), original);
        manager.flush_now().unwrap();
        assert_eq!(AppState::load_from(&config.path()).volume, 0.7);
    }
}
