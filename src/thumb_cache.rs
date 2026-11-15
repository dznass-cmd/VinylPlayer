//! Thumbnail disk caching and fast downscaling for album covers.
use gtk::gdk_pixbuf;
use gtk::glib;
use std::hash::{Hash, Hasher};
use std::path::{Path, PathBuf};

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

/// Loads an album cover as a downscaled thumbnail (e.g. 256x256),
/// caching the result on disk in ~/.cache/vinylplayer/thumbs/ for instant loading and minimal RAM usage.
pub fn load_cached_thumbnail(source_path: &Path, target_size: i32) -> Option<gdk_pixbuf::Pixbuf> {
    let cache_dir = get_thumb_cache_dir();

    let mtime = std::fs::metadata(source_path)
        .and_then(|m| m.modified())
        .map(|t| t.duration_since(std::time::UNIX_EPOCH).unwrap_or_default().as_secs())
        .unwrap_or(0);

    let key = format!(
        "{:016x}_{}_{}.jpg",
        hash_key(source_path.to_string_lossy().as_bytes()),
        mtime,
        target_size
    );
    let thumb_path = cache_dir.join(&key);

    // Fast path: load pre-scaled thumbnail from disk cache
    if thumb_path.exists() {
        if let Ok(pix) = gdk_pixbuf::Pixbuf::from_file(&thumb_path) {
            return Some(pix);
        }
    }

    // Slow path: downscale from original file and persist thumbnail
    if let Ok(image) = gdk_pixbuf::Pixbuf::from_file_at_scale(source_path, target_size, target_size, true) {
        let _ = image.savev(&thumb_path, "jpeg", &[("quality", "85")]);
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
}
