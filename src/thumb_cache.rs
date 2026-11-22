//! Thumbnail disk caching, alpha-preserving downscaling, and LRU eviction for album covers.
use gtk::gdk_pixbuf;
use gtk::glib;
use std::hash::{Hash, Hasher};
use std::path::{Path, PathBuf};
use std::sync::Once;
use std::time::SystemTime;

const MAX_CACHED_THUMBS: usize = 600;
static PRUNE_ONCE: Once = Once::new();

fn hash_key(bytes: &[u8]) -> u64 {
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    bytes.hash(&mut hasher);
    hasher.finish()
}

pub fn get_thumb_cache_dir() -> PathBuf {
    let dir = glib::user_cache_dir().join("vinylplayer").join("thumbs");
    let _ = std::fs::create_dir_all(&dir);
    dir
}

fn schedule_cache_prune(cache_dir: PathBuf) {
    PRUNE_ONCE.call_once(move || {
        let _ = std::thread::Builder::new()
            .name("vinyl-thumb-prune".into())
            .spawn(move || {
                prune_cache_dir(&cache_dir, MAX_CACHED_THUMBS);
            });
    });
}

pub fn prune_cache_dir(cache_dir: &Path, max_entries: usize) {
    let Ok(entries) = std::fs::read_dir(cache_dir) else {
        return;
    };
    let mut files: Vec<(PathBuf, SystemTime)> = entries
        .flatten()
        .filter_map(|e| {
            let path = e.path();
            if !path.is_file() {
                return None;
            }
            let mtime = e
                .metadata()
                .and_then(|m| m.modified())
                .unwrap_or(SystemTime::UNIX_EPOCH);
            Some((path, mtime))
        })
        .collect();
    if files.len() <= max_entries {
        return;
    }
    files.sort_by_key(|(_, mtime)| *mtime);
    let to_remove = files.len() - max_entries;
    for (path, _) in files.into_iter().take(to_remove) {
        let _ = std::fs::remove_file(path);
    }
}

/// Loads an album cover as a downscaled thumbnail (e.g. 256x256),
/// caching the result on disk in ~/.cache/vinylplayer/thumbs/ for instant loading and minimal RAM usage.
/// Preserves PNG transparency when the source image has an alpha channel.
pub fn load_cached_thumbnail(source_path: &Path, target_size: i32) -> Option<gdk_pixbuf::Pixbuf> {
    let cache_dir = get_thumb_cache_dir();
    schedule_cache_prune(cache_dir.clone());

    let mtime = std::fs::metadata(source_path)
        .and_then(|m| m.modified())
        .map(|t| {
            t.duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs()
        })
        .unwrap_or(0);

    let stem = format!(
        "{:016x}_{}_{}",
        hash_key(source_path.to_string_lossy().as_bytes()),
        mtime,
        target_size
    );
    let jpg_path = cache_dir.join(format!("{stem}.jpg"));
    let png_path = cache_dir.join(format!("{stem}.png"));

    // Fast path: load pre-scaled thumbnail from disk cache
    for cached in [&jpg_path, &png_path] {
        if cached.exists() {
            if let Ok(pix) = gdk_pixbuf::Pixbuf::from_file(cached) {
                return Some(pix);
            }
        }
    }

    // Slow path: downscale from original file and persist thumbnail
    if let Ok(image) =
        gdk_pixbuf::Pixbuf::from_file_at_scale(source_path, target_size, target_size, true)
    {
        if image.has_alpha() {
            let _ = image.savev(&png_path, "png", &[]);
        } else {
            let _ = image.savev(&jpg_path, "jpeg", &[("quality", "85")]);
        }
        return Some(image);
    }

    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_thumb_cache_dir_creation() {
        let dir = get_thumb_cache_dir();
        assert!(dir.exists(), "Thumbnail cache directory must exist");
    }

    #[test]
    fn test_prune_cache_dir_evicts_oldest_entries() {
        let temp = std::env::temp_dir().join(format!(
            "vinyl-thumb-prune-test-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&temp).unwrap();
        for i in 0..5 {
            std::fs::write(temp.join(format!("thumb_{i}.jpg")), b"img").unwrap();
        }
        prune_cache_dir(&temp, 3);
        let remaining = std::fs::read_dir(&temp).unwrap().count();
        assert_eq!(remaining, 3);
        let _ = std::fs::remove_dir_all(&temp);
    }
}
