//! Descoberta de arquivos, executada pelo worker da biblioteca.
use std::collections::HashSet;
use std::path::PathBuf;

#[derive(Default, Debug)]
pub struct ScanReport {
    pub tracks: Vec<PathBuf>,
    pub warnings: Vec<String>,
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
}
