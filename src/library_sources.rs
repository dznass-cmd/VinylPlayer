//! Descoberta de arquivos, executada pelo worker da biblioteca.
use std::collections::HashSet;
use std::path::PathBuf;

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ScanReport {
    pub tracks: Vec<PathBuf>,
    pub warnings: Vec<String>,
}

pub struct CachedLibrary;

impl CachedLibrary {
    pub fn load_from(cache_path: &std::path::Path, roots: &[PathBuf]) -> Option<Vec<PathBuf>> {
        if roots.is_empty() {
            return None;
        }
        let content = std::fs::read_to_string(cache_path).ok()?;
        let value = serde_json::from_str::<serde_json::Value>(&content).ok()?;
        let version = value.get("version").and_then(|v| v.as_u64())?;
        if version != 1 {
            return None;
        }
        let cached_roots: Vec<PathBuf> = value
            .get("roots")
            .and_then(|v| v.as_array())?
            .iter()
            .filter_map(|v| v.as_str().map(PathBuf::from))
            .collect();
        if cached_roots != roots {
            return None;
        }
        let tracks: Vec<PathBuf> = value
            .get("tracks")
            .and_then(|v| v.as_array())?
            .iter()
            .filter_map(|v| v.as_str().map(PathBuf::from))
            .filter(|p| p.is_file())
            .collect();
        if tracks.is_empty() {
            None
        } else {
            Some(tracks)
        }
    }

    pub fn save_to(cache_path: &std::path::Path, roots: &[PathBuf], tracks: &[PathBuf]) {
        if let Some(parent) = cache_path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        let roots_json: Vec<String> = roots
            .iter()
            .map(|p| p.to_string_lossy().to_string())
            .collect();
        let tracks_json: Vec<String> = tracks
            .iter()
            .map(|p| p.to_string_lossy().to_string())
            .collect();
        let json_value = serde_json::json!({
            "version": 1,
            "roots": roots_json,
            "tracks": tracks_json,
        });
        let Ok(content) = serde_json::to_string(&json_value) else {
            return;
        };
        let tmp = cache_path.with_extension(format!("tmp.{}", std::process::id()));
        if std::fs::write(&tmp, content).is_ok() {
            let _ = std::fs::rename(tmp, cache_path);
        }
    }
}

pub fn scan(roots: &[PathBuf]) -> ScanReport {
    let mut report = ScanReport::default();
    let mut visited = HashSet::new();
    let mut files = HashSet::new();
    let mut pending = Vec::new();
    for root in roots.iter().rev() {
        if root.is_dir() {
            pending.push(root.clone());
        } else {
            report.warnings.push(format!(
                "{}: pasta indisponível ou não é uma pasta",
                root.display()
            ));
        }
    }
    while let Some(path) = pending.pop() {
        let canonical = match std::fs::canonicalize(&path) {
            Ok(path) => path,
            Err(error) => {
                report.warnings.push(format!("{}: {error}", path.display()));
                continue;
            }
        };
        if canonical.is_dir() {
            if !visited.insert(canonical) {
                continue;
            }
            match std::fs::read_dir(&path) {
                Ok(entries) => {
                    let mut children = Vec::new();
                    for entry in entries {
                        match entry {
                            Ok(entry) => children.push(entry.path()),
                            Err(error) => {
                                report.warnings.push(format!("{}: {error}", path.display()))
                            }
                        }
                    }
                    children.sort();
                    pending.extend(children.into_iter().rev());
                }
                Err(error) => report.warnings.push(format!("{}: {error}", path.display())),
            }
        } else if canonical.is_file()
            && matches!(
                path.extension()
                    .and_then(|s| s.to_str())
                    .map(str::to_ascii_lowercase)
                    .as_deref(),
                Some(
                    "flac"
                        | "mp3"
                        | "ogg"
                        | "oga"
                        | "wav"
                        | "m4a"
                        | "aac"
                        | "opus"
                        | "alac"
                        | "aif"
                        | "aiff"
                        | "wma"
                )
            )
            && files.insert(canonical)
        {
            report.tracks.push(path);
        }
    }
    report.tracks.sort();
    report
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scans_multiple_roots_deduplicates_overlap_and_reports_missing_folders() {
        let config = crate::state::TestConfig::new();
        let root = config.path().parent().unwrap().to_path_buf();
        let nested = root.join("Álbum");
        std::fs::create_dir(&nested).unwrap();
        let song = nested.join("01 música.FLAC");
        std::fs::write(&song, b"fixture").unwrap();
        std::fs::write(root.join("notes.txt"), b"text").unwrap();
        #[cfg(unix)]
        std::os::unix::fs::symlink(&root, nested.join("loop")).unwrap();
        let result = scan(&[root.clone(), nested, root.join("missing")]);
        assert_eq!(result.tracks, vec![song]);
        assert_eq!(result.warnings.len(), 1);
        assert!(scan(&[]).tracks.is_empty());
    }

    #[test]
    fn cached_library_roundtrip_and_missing_file_handling() {
        let config = crate::state::TestConfig::new();
        let root = config.path().parent().unwrap().to_path_buf();
        let cache_file = root.join("test_cache.json");
        let roots = vec![root.clone()];
        assert!(CachedLibrary::load_from(&cache_file, &roots).is_none());

        let song = root.join("song1.flac");
        std::fs::write(&song, b"audio").unwrap();
        let tracks = vec![song.clone()];
        CachedLibrary::save_to(&cache_file, &roots, &tracks);
        assert!(cache_file.is_file());

        let loaded = CachedLibrary::load_from(&cache_file, &roots).expect("Cache should load");
        assert_eq!(loaded, tracks);

        // Mismatched roots should invalidate cache
        let other_roots = vec![PathBuf::from("/other/folder")];
        assert!(CachedLibrary::load_from(&cache_file, &other_roots).is_none());
    }
}
