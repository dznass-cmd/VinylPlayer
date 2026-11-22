//! Gerenciamento de playlists no formato padrão M3U / M3U8 com persistência atômica.
use std::cell::RefCell;
use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::rc::Rc;

/// Representa uma playlist de músicas com ordem sequencial de faixas.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Playlist {
    pub name: String,
    pub file_path: PathBuf,
    pub tracks: Vec<PathBuf>,
}

impl Playlist {
    pub fn new(name: impl Into<String>, file_path: PathBuf) -> Self {
        Self {
            name: name.into(),
            file_path,
            tracks: Vec::new(),
        }
    }

    /// Analisa o conteúdo de um arquivo M3U / M3U8, extraindo os caminhos das músicas.
    /// Resolve caminhos relativos usando `base_dir`, suporta URIs file:// e normaliza caminhos Windows.
    pub fn parse_m3u(content: &str, base_dir: Option<&Path>) -> Vec<PathBuf> {
        let mut tracks = Vec::new();
        // Remove Byte Order Mark (BOM) UTF-8 caso presente
        let clean_content = content.strip_prefix('\u{feff}').unwrap_or(content);
        for raw_line in clean_content.lines() {
            let line = raw_line.trim();
            // Ignora linhas vazias ou comentários/diretivas que começam com #
            if line.is_empty() || line.starts_with('#') {
                continue;
            }

            // Suporte completo a URIs do padrão file://
            if line.starts_with("file://") {
                if let Ok((path, _)) = glib::filename_from_uri(line) {
                    tracks.push(path);
                    continue;
                } else if let Some(stripped) = line.strip_prefix("file://localhost") {
                    let decoded = percent_decode_path(stripped);
                    tracks.push(PathBuf::from(decoded));
                    continue;
                } else if let Some(stripped) = line.strip_prefix("file://") {
                    let decoded = percent_decode_path(stripped);
                    tracks.push(PathBuf::from(decoded));
                    continue;
                }
            }

            // Normaliza barras invertidas do Windows (ex: Music\Rock\Song.mp3)
            let normalized = if !line.starts_with('/') && line.contains('\\') {
                line.replace('\\', "/")
            } else {
                line.to_string()
            };

            let mut track_path = PathBuf::from(&normalized);
            if track_path.is_relative() {
                if let Some(base) = base_dir {
                    track_path = base.join(&track_path);
                }
            }
            tracks.push(track_path);
        }
        tracks
    }

    /// Carrega uma playlist a partir de um arquivo no disco (.m3u ou .m3u8).
    pub fn load_from_file(path: impl AsRef<Path>) -> io::Result<Self> {
        let path = path.as_ref();
        let content = match fs::read_to_string(path) {
            Ok(s) => s,
            Err(_) => {
                let bytes = fs::read(path)?;
                bytes.iter().map(|&b| b as char).collect::<String>()
            }
        };

        let base_dir = path.parent();
        let mut name = path
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("Playlist")
            .to_string();

        // Permite nome personalizado definido com #PLAYLIST: Nome (case-insensitive)
        for line in content.lines() {
            let line = line.trim();
            if line.to_ascii_uppercase().starts_with("#PLAYLIST:") {
                let custom = line[10..].trim();
                if !custom.is_empty() {
                    name = custom.to_string();
                    break;
                }
            }
        }

        let tracks = Self::parse_m3u(&content, base_dir);
        Ok(Self {
            name,
            file_path: path.to_path_buf(),
            tracks,
        })
    }

    /// Grava a playlist em formato M3U8 UTF-8 de forma atômica.
    pub fn save_to_file(&self) -> io::Result<()> {
        if let Some(parent) = self.file_path.parent() {
            fs::create_dir_all(parent)?;
        }
        let file_stem = self
            .file_path
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("playlist");
        let tmp_name = format!(
            ".{}.tmp.{}.{}",
            file_stem,
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or(0)
        );
        let tmp_path = self.file_path.with_file_name(tmp_name);
        {
            let mut file = fs::File::create(&tmp_path)?;
            writeln!(file, "#EXTM3U")?;
            writeln!(file, "#PLAYLIST: {}", self.name)?;
            for track in &self.tracks {
                writeln!(file, "{}", track.display())?;
            }
            file.flush()?;
            file.sync_all()?;
        }
        fs::rename(&tmp_path, &self.file_path)?;
        Ok(())
    }

    /// Adiciona uma faixa ao final da playlist.
    pub fn add_track(&mut self, track: PathBuf) {
        self.tracks.push(track);
    }

    /// Adiciona múltiplas faixas ao final da playlist.
    pub fn add_tracks(&mut self, new_tracks: impl IntoIterator<Item = PathBuf>) {
        self.tracks.extend(new_tracks);
    }

    /// Remove uma faixa pelo seu índice na playlist.
    pub fn remove_track(&mut self, index: usize) -> Option<PathBuf> {
        if index < self.tracks.len() {
            Some(self.tracks.remove(index))
        } else {
            None
        }
    }

    /// Move uma faixa de uma posição para outra na playlist.
    pub fn move_track(&mut self, from: usize, to: usize) -> bool {
        if from >= self.tracks.len() || to >= self.tracks.len() || from == to {
            return false;
        }
        let track = self.tracks.remove(from);
        self.tracks.insert(to, track);
        true
    }

    /// Renomeia a playlist e atualiza o arquivo físico correspondente.
    #[allow(dead_code)]
    pub fn rename(&mut self, new_name: &str) -> io::Result<()> {
        let trimmed = new_name.trim();
        if trimmed.is_empty() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "Nome da playlist não pode ser vazio",
            ));
        }
        self.name = trimmed.to_string();
        self.save_to_file()
    }
}

/// Gerenciador de playlists locais armazenadas no diretório padrão do usuário.
#[derive(Clone, Debug)]
pub struct PlaylistManager {
    dir: PathBuf,
    playlists: Rc<RefCell<Vec<Playlist>>>,
}

impl PlaylistManager {
    pub fn new(custom_dir: Option<PathBuf>) -> Self {
        let dir = custom_dir.unwrap_or_else(Self::default_dir);
        let _ = fs::create_dir_all(&dir);
        let manager = Self {
            dir,
            playlists: Rc::new(RefCell::new(Vec::new())),
        };
        manager.reload();
        manager
    }

    /// Diretório padrão de playlists: ~/.local/share/vinylplayer/playlists/
    pub fn default_dir() -> PathBuf {
        let base = std::env::var_os("XDG_DATA_HOME")
            .map(PathBuf::from)
            .unwrap_or_else(|| {
                std::env::var_os("HOME")
                    .map(|h| PathBuf::from(h).join(".local").join("share"))
                    .unwrap_or_else(|| PathBuf::from("."))
            });
        base.join("vinylplayer").join("playlists")
    }

    /// Caminho do diretório gerenciado.
    #[allow(dead_code)]
    pub fn dir(&self) -> &Path {
        &self.dir
    }

    /// Recarrega todas as playlists do diretório.
    pub fn reload(&self) -> Vec<Playlist> {
        let mut list = Vec::new();
        if let Ok(entries) = fs::read_dir(&self.dir) {
            for entry in entries.flatten() {
                let path = entry.path();
                if path.is_file() {
                    let ext = path
                        .extension()
                        .and_then(|s| s.to_str())
                        .unwrap_or("")
                        .to_lowercase();
                    if ext == "m3u" || ext == "m3u8" {
                        if let Ok(playlist) = Playlist::load_from_file(&path) {
                            list.push(playlist);
                        }
                    }
                }
            }
        }
        list.sort_by_key(|a| a.name.to_lowercase());
        *self.playlists.borrow_mut() = list.clone();
        list
    }

    /// Retorna as playlists atualmente em memória.
    pub fn playlists(&self) -> Vec<Playlist> {
        self.playlists.borrow().clone()
    }

    /// Sanitiza o nome fornecido para um nome seguro de arquivo.
    pub fn sanitize_filename(name: &str) -> String {
        let clean: String = name
            .chars()
            .map(|c| match c {
                '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|' => '_',
                c if c.is_control() => '_',
                other => other,
            })
            .collect();
        let trimmed = clean.trim().trim_matches('.');
        if trimmed.is_empty() || trimmed == ".." {
            "playlist".to_string()
        } else {
            trimmed.to_string()
        }
    }

    /// Cria uma nova playlist vazia com o nome informado.
    pub fn create_playlist(&self, name: &str) -> io::Result<Playlist> {
        let trimmed = name.trim();
        if trimmed.is_empty() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "Nome da playlist não pode ser vazio",
            ));
        }
        let safe_name = Self::sanitize_filename(trimmed);
        let mut file_path = self.dir.join(format!("{safe_name}.m3u8"));
        let mut counter = 1;
        while file_path.exists() {
            file_path = self.dir.join(format!("{safe_name}_{counter}.m3u8"));
            counter += 1;
        }

        let playlist = Playlist::new(trimmed, file_path);
        playlist.save_to_file()?;
        self.reload();
        Ok(playlist)
    }

    /// Exclui uma playlist do disco e da memória.
    pub fn delete_playlist(&self, file_path: &Path) -> io::Result<()> {
        if fs::symlink_metadata(file_path).is_ok() {
            fs::remove_file(file_path)?;
        }
        self.playlists.borrow_mut().retain(|p| p.file_path != file_path);
        Ok(())
    }

    /// Adiciona faixas a uma playlist existente no disco e salva atomicamente.
    pub fn add_tracks_to_playlist(
        &self,
        file_path: &Path,
        new_tracks: &[PathBuf],
    ) -> io::Result<()> {
        let mut playlist = Playlist::load_from_file(file_path)?;
        playlist.add_tracks(new_tracks.iter().cloned());
        playlist.save_to_file()?;
        self.reload();
        Ok(())
    }

    /// Importa um arquivo .m3u ou .m3u8 externo para a pasta de playlists locais.
    pub fn import_playlist(&self, source_path: &Path) -> io::Result<Playlist> {
        let loaded = Playlist::load_from_file(source_path)?;
        let safe_name = Self::sanitize_filename(&loaded.name);
        let mut target = self.dir.join(format!("{safe_name}.m3u8"));
        let mut counter = 1;
        while target.exists() {
            target = self.dir.join(format!("{safe_name}_{counter}.m3u8"));
            counter += 1;
        }
        let imported = Playlist {
            name: loaded.name,
            file_path: target,
            tracks: loaded.tracks,
        };
        imported.save_to_file()?;
        self.reload();
        Ok(imported)
    }
}

fn percent_decode_path(input: &str) -> String {
    let bytes = input.as_bytes();
    let mut decoded = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            if let Ok(byte) = u8::from_str_radix(&input[i + 1..i + 3], 16) {
                decoded.push(byte);
                i += 3;
                continue;
            }
        }
        decoded.push(bytes[i]);
        i += 1;
    }
    String::from_utf8(decoded).unwrap_or_else(|e| String::from_utf8_lossy(e.as_bytes()).into_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_m3u_ignoring_comments_and_resolving_paths() {
        let content = r#"#EXTM3U
#EXTINF:240,Artista - Música 1
musica1.flac
# Comentário no meio
/caminho/absoluto/musica2.wav

subpasta/musica3.mp3
"#;
        let base = Path::new("/home/user/Música");
        let tracks = Playlist::parse_m3u(content, Some(base));
        assert_eq!(tracks.len(), 3);
        assert_eq!(tracks[0], PathBuf::from("/home/user/Música/musica1.flac"));
        assert_eq!(tracks[1], PathBuf::from("/caminho/absoluto/musica2.wav"));
        assert_eq!(tracks[2], PathBuf::from("/home/user/Música/subpasta/musica3.mp3"));
    }

    #[test]
    fn parses_custom_playlist_name_header() {
        let content = "#EXTM3U\n#PLAYLIST: Minhas Favoritas dos Anos 80\nfaixa.mp3\n";
        let temp_dir = std::env::temp_dir().join(format!("vp_test_pl_{}", std::process::id()));
        let _ = fs::create_dir_all(&temp_dir);
        let pl_path = temp_dir.join("test.m3u8");
        fs::write(&pl_path, content).unwrap();

        let pl = Playlist::load_from_file(&pl_path).unwrap();
        assert_eq!(pl.name, "Minhas Favoritas dos Anos 80");
        assert_eq!(pl.tracks.len(), 1);

        let _ = fs::remove_dir_all(&temp_dir);
    }

    #[test]
    fn save_and_reload_roundtrip_preserves_tracks_and_name() {
        let temp_dir = std::env::temp_dir().join(format!("vp_test_round_{}", std::process::id()));
        let _ = fs::create_dir_all(&temp_dir);
        let pl_path = temp_dir.join("vinil_classicos.m3u8");

        let mut pl = Playlist::new("Clássicos do Vinil", pl_path.clone());
        pl.add_track(PathBuf::from("/music/21 Savage - No Heart.flac"));
        pl.add_track(PathBuf::from("/music/Pink Floyd - Time.flac"));
        pl.save_to_file().unwrap();

        let loaded = Playlist::load_from_file(&pl_path).unwrap();
        assert_eq!(loaded.name, "Clássicos do Vinil");
        assert_eq!(loaded.tracks.len(), 2);
        assert_eq!(loaded.tracks[0], PathBuf::from("/music/21 Savage - No Heart.flac"));
        assert_eq!(loaded.tracks[1], PathBuf::from("/music/Pink Floyd - Time.flac"));

        let _ = fs::remove_dir_all(&temp_dir);
    }

    #[test]
    fn add_remove_and_move_tracks_work_correctly() {
        let mut pl = Playlist::new("Teste", PathBuf::from("/tmp/dummy.m3u8"));
        let t1 = PathBuf::from("1.flac");
        let t2 = PathBuf::from("2.flac");
        let t3 = PathBuf::from("3.flac");

        pl.add_track(t1.clone());
        pl.add_track(t2.clone());
        pl.add_track(t3.clone());
        assert_eq!(pl.tracks, vec![t1.clone(), t2.clone(), t3.clone()]);

        // Move 2 de idx 1 para idx 0
        assert!(pl.move_track(1, 0));
        assert_eq!(pl.tracks, vec![t2.clone(), t1.clone(), t3.clone()]);

        // Remove idx 1 (t1)
        let removed = pl.remove_track(1);
        assert_eq!(removed, Some(t1));
        assert_eq!(pl.tracks, vec![t2, t3]);
    }

    #[test]
    fn manager_creates_lists_and_deletes_playlists() {
        let temp_dir = std::env::temp_dir().join(format!("vp_mgr_test_{}", std::process::id()));
        let _ = fs::create_dir_all(&temp_dir);

        let manager = PlaylistManager::new(Some(temp_dir.clone()));
        assert!(manager.playlists().is_empty());

        let pl1 = manager.create_playlist("Sessão Noturna").unwrap();
        assert_eq!(pl1.name, "Sessão Noturna");
        assert!(pl1.file_path.exists());

        let pl2 = manager.create_playlist("Sessão Noturna").unwrap();
        assert!(pl2.file_path.exists());
        assert_ne!(pl1.file_path, pl2.file_path);

        let list = manager.playlists();
        assert_eq!(list.len(), 2);

        manager.delete_playlist(&pl1.file_path).unwrap();
        assert_eq!(manager.playlists().len(), 1);

        let _ = fs::remove_dir_all(&temp_dir);
    }

    #[test]
    fn sanitize_filename_removes_invalid_characters() {
        assert_eq!(
            PlaylistManager::sanitize_filename("Músicas / Rock * Anos 80?"),
            "Músicas _ Rock _ Anos 80_"
        );
        assert_eq!(PlaylistManager::sanitize_filename("   "), "playlist");
        assert_eq!(PlaylistManager::sanitize_filename("."), "playlist");
        assert_eq!(PlaylistManager::sanitize_filename(".."), "playlist");
        assert_eq!(PlaylistManager::sanitize_filename(".oculto"), "oculto");
        assert_eq!(PlaylistManager::sanitize_filename("nome\0com\nnulo"), "nome_com_nulo");
    }

    #[test]
    fn parses_m3u_with_utf8_bom() {
        let content = "\u{feff}#EXTM3U\n#EXTINF:120,Teste\nmusica.flac\n";
        let tracks = Playlist::parse_m3u(content, None);
        assert_eq!(tracks.len(), 1);
        assert_eq!(tracks[0], PathBuf::from("musica.flac"));
    }

    #[test]
    fn parses_custom_playlist_name_case_insensitive() {
        let content = "#EXTM3U\n#playlist: Nome Minúsculo\nfaixa.mp3\n";
        let temp_dir = std::env::temp_dir().join(format!("vp_test_case_{}", std::process::id()));
        let _ = fs::create_dir_all(&temp_dir);
        let pl_path = temp_dir.join("case.m3u8");
        fs::write(&pl_path, content).unwrap();

        let pl = Playlist::load_from_file(&pl_path).unwrap();
        assert_eq!(pl.name, "Nome Minúsculo");

        let _ = fs::remove_dir_all(&temp_dir);
    }

    #[test]
    fn delete_playlist_removes_broken_symlinks() {
        let temp_dir = std::env::temp_dir().join(format!("vp_sym_test_{}", std::process::id()));
        let _ = fs::create_dir_all(&temp_dir);

        let manager = PlaylistManager::new(Some(temp_dir.clone()));
        let broken_target = temp_dir.join("non_existent.m3u8");
        let link_path = temp_dir.join("broken_link.m3u8");
        #[cfg(unix)]
        std::os::unix::fs::symlink(&broken_target, &link_path).unwrap();

        assert!(fs::symlink_metadata(&link_path).is_ok());
        assert!(!link_path.exists()); // link quebrado retorna false para exists()

        manager.delete_playlist(&link_path).unwrap();
        assert!(fs::symlink_metadata(&link_path).is_err());

        let _ = fs::remove_dir_all(&temp_dir);
    }

    #[test]
    fn parses_file_uri_and_windows_paths() {
        let content = "file:///home/user/Music/Pink%20Floyd/Time.mp3\nfile://localhost/home/user/Music/Led%20Zeppelin/Kashmir.flac\nMusic\\Rock\\Queen.mp3\n";
        let tracks = Playlist::parse_m3u(content, Some(Path::new("/home/user")));
        assert_eq!(tracks[0], PathBuf::from("/home/user/Music/Pink Floyd/Time.mp3"));
        assert_eq!(tracks[1], PathBuf::from("/home/user/Music/Led Zeppelin/Kashmir.flac"));
        assert_eq!(tracks[2], PathBuf::from("/home/user/Music/Rock/Queen.mp3"));
    }

    #[test]
    fn handles_empty_or_whitespace_tracks_gracefully() {
        let content = "\n  \n#EXTM3U\n\n   \n#EXTINF:-1,Invalido\n\nfaixa.mp3\n   \n";
        let tracks = Playlist::parse_m3u(content, None);
        assert_eq!(tracks.len(), 1);
        assert_eq!(tracks[0], PathBuf::from("faixa.mp3"));
    }
}
