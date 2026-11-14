//! Ordenação dos álbuns e extração de metadados sem carregar nem reproduzir faixas.
use std::cmp::Ordering;
use std::collections::BTreeMap;
use std::hash::{Hash, Hasher};
use std::path::{Path, PathBuf};

#[allow(dead_code)]
pub type AlbumGroups = BTreeMap<PathBuf, Vec<PathBuf>>;

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct TrackTags {
    pub title: Option<String>,
    pub performer: Option<String>,
    pub disc: Option<u32>,
    pub track: Option<u32>,
    pub album: Option<String>,
    pub artist: Option<String>,
    pub year: Option<u32>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AlbumMetadata {
    pub folder: PathBuf,
    pub title: String,
    pub artist: String,
    pub year: Option<u32>,
    pub tracks: Vec<PathBuf>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum AlbumSortMode {
    #[default]
    TitleAsc,
    TitleDesc,
    ArtistAsc,
    YearDesc,
    YearAsc,
}

pub fn parse_year(value: &str) -> Option<u32> {
    let bytes = value.trim().as_bytes();
    for i in 0..bytes.len().saturating_sub(3) {
        if bytes[i..i + 4].iter().all(|b| b.is_ascii_digit()) {
            if let Ok(y) = std::str::from_utf8(&bytes[i..i + 4])
                .unwrap()
                .parse::<u32>()
            {
                if (1000..=3000).contains(&y) {
                    return Some(y);
                }
            }
        }
    }
    None
}

fn number(value: &str) -> Option<u32> {
    value
        .split('/')
        .next()?
        .trim()
        .parse::<u32>()
        .ok()
        .filter(|n| *n > 0)
}

pub fn parse_tags(text: &str) -> TrackTags {
    let mut tags = TrackTags::default();
    for line in text.lines() {
        let Some((key, value)) = line.split_once('=') else {
            continue;
        };
        let key = key.trim().trim_start_matches("TAG:").to_ascii_lowercase();
        let value = value.trim();
        match key.as_str() {
            "title" if !value.is_empty() && tags.title.is_none() => {
                tags.title = Some(value.to_string());
            }
            "disc" | "discnumber" => tags.disc = tags.disc.or_else(|| number(value)),
            "track" | "tracknumber" => tags.track = tags.track.or_else(|| number(value)),
            "album" => {
                if !value.is_empty() && tags.album.is_none() {
                    tags.album = Some(value.to_string());
                }
            }
            "album_artist" | "albumartist" => {
                if !value.is_empty() {
                    tags.artist = Some(value.to_string());
                }
            }
            "artist" => {
                if !value.is_empty() && tags.performer.is_none() {
                    tags.performer = Some(value.to_string());
                }
                if !value.is_empty() && tags.artist.is_none() {
                    tags.artist = Some(value.to_string());
                }
            }
            "date" | "year" if tags.year.is_none() => {
                tags.year = parse_year(value);
            }
            _ => {}
        }
    }
    tags
}

pub fn read_order(path: &Path) -> TrackTags {
    let stamp = std::fs::metadata(path).ok().and_then(|m| {
        let time = m
            .modified()
            .ok()?
            .duration_since(std::time::UNIX_EPOCH)
            .ok()?;
        Some(format!(
            "v3 {} {} {}",
            m.len(),
            time.as_secs(),
            time.subsec_nanos()
        ))
    });
    let mut hash = std::collections::hash_map::DefaultHasher::new();
    path.hash(&mut hash);
    let cache = glib::user_cache_dir()
        .join("vinylplayer/track-order")
        .join(format!("{:x}.txt", hash.finish()));
    if let (Some(stamp), Ok(saved)) = (&stamp, std::fs::read_to_string(&cache)) {
        if let Some((saved_stamp, tags)) = saved.split_once('\n') {
            if saved_stamp == stamp {
                return parse_tags(tags);
            }
        }
    }
    let output = std::process::Command::new("ffprobe")
        .args([
            "-v",
            "error",
            "-threads",
            "1",
            "-show_entries",
            "format_tags=title,track,tracknumber,disc,discnumber,album,artist,album_artist,albumartist,date,year:stream_tags=title,track,tracknumber,disc,discnumber,album,artist,album_artist,albumartist,date,year",
            "-of",
            "default=noprint_wrappers=1",
            "-i",
        ])
        .arg(path)
        .output();
    let Ok(output) = output else {
        return TrackTags::default();
    };
    if !output.status.success() {
        return TrackTags::default();
    }
    let tags = String::from_utf8_lossy(&output.stdout);
    let parsed = parse_tags(&tags);
    if let Some(stamp) = stamp {
        if std::fs::create_dir_all(cache.parent().unwrap()).is_ok() {
            let temporary = cache.with_extension(format!("{}.tmp", std::process::id()));
            if std::fs::write(&temporary, format!("{stamp}\n{tags}")).is_ok() {
                let _ = std::fs::rename(temporary, cache);
            }
        }
    }
    parsed
}

pub fn natural_cmp(a: &str, b: &str) -> Ordering {
    let (a, b) = (a.as_bytes(), b.as_bytes());
    let (mut i, mut j) = (0, 0);
    while i < a.len() && j < b.len() {
        if a[i].is_ascii_digit() && b[j].is_ascii_digit() {
            let (start_i, start_j) = (i, j);
            while i < a.len() && a[i].is_ascii_digit() {
                i += 1;
            }
            while j < b.len() && b[j].is_ascii_digit() {
                j += 1;
            }
            let mut x = &a[start_i..i];
            let mut y = &b[start_j..j];
            while x.first() == Some(&b'0') {
                x = &x[1..];
            }
            while y.first() == Some(&b'0') {
                y = &y[1..];
            }
            let result = x.len().cmp(&y.len()).then_with(|| x.cmp(y));
            if result != Ordering::Equal {
                return result;
            }
        } else {
            let result = a[i].cmp(&b[j]);
            if result != Ordering::Equal {
                return result;
            }
            i += 1;
            j += 1;
        }
    }
    (a.len() - i).cmp(&(b.len() - j))
}

pub fn order_tracks(
    tracks: Vec<PathBuf>,
    read: impl Fn(&Path) -> TrackTags,
) -> (Vec<PathBuf>, Option<TrackTags>) {
    let mut album_tags: Option<TrackTags> = None;
    let mut entries: Vec<_> = tracks
        .into_iter()
        .map(|path| {
            let tags = read(&path);
            if album_tags.is_none()
                && (tags.album.is_some() || tags.artist.is_some() || tags.year.is_some())
            {
                album_tags = Some(tags.clone());
            }
            let name = path
                .file_name()
                .unwrap_or_default()
                .to_string_lossy()
                .to_lowercase();
            (path, tags, name)
        })
        .collect();
    if album_tags.is_none() {
        if let Some((_, first_tags, _)) = entries.first() {
            album_tags = Some(first_tags.clone());
        }
    }
    entries.sort_by(|a, b| {
        a.1.disc
            .unwrap_or(1)
            .cmp(&b.1.disc.unwrap_or(1))
            .then_with(|| a.1.track.is_none().cmp(&b.1.track.is_none()))
            .then_with(|| a.1.track.cmp(&b.1.track))
            .then_with(|| natural_cmp(&a.2, &b.2))
            .then_with(|| a.0.cmp(&b.0))
    });
    (
        entries.into_iter().map(|(path, _, _)| path).collect(),
        album_tags,
    )
}

pub fn compare_albums(a: &AlbumMetadata, b: &AlbumMetadata, mode: AlbumSortMode) -> Ordering {
    match mode {
        AlbumSortMode::TitleAsc => natural_cmp(&a.title, &b.title)
            .then_with(|| natural_cmp(&a.artist, &b.artist))
            .then_with(|| a.folder.cmp(&b.folder)),
        AlbumSortMode::TitleDesc => natural_cmp(&b.title, &a.title)
            .then_with(|| natural_cmp(&b.artist, &a.artist))
            .then_with(|| b.folder.cmp(&a.folder)),
        AlbumSortMode::ArtistAsc => natural_cmp(&a.artist, &b.artist)
            .then_with(|| natural_cmp(&a.title, &b.title))
            .then_with(|| b.year.cmp(&a.year))
            .then_with(|| a.folder.cmp(&b.folder)),
        AlbumSortMode::YearDesc => match (a.year, b.year) {
            (Some(ya), Some(yb)) => yb
                .cmp(&ya)
                .then_with(|| natural_cmp(&a.title, &b.title))
                .then_with(|| a.folder.cmp(&b.folder)),
            (Some(_), None) => Ordering::Less,
            (None, Some(_)) => Ordering::Greater,
            (None, None) => natural_cmp(&a.title, &b.title).then_with(|| a.folder.cmp(&b.folder)),
        },
        AlbumSortMode::YearAsc => match (a.year, b.year) {
            (Some(ya), Some(yb)) => ya
                .cmp(&yb)
                .then_with(|| natural_cmp(&a.title, &b.title))
                .then_with(|| a.folder.cmp(&b.folder)),
            (Some(_), None) => Ordering::Less,
            (None, Some(_)) => Ordering::Greater,
            (None, None) => natural_cmp(&a.title, &b.title).then_with(|| a.folder.cmp(&b.folder)),
        },
    }
}

pub fn sort_albums(albums: &mut [AlbumMetadata], mode: AlbumSortMode) {
    albums.sort_by(|a, b| compare_albums(a, b, mode));
}

pub fn load_album_metadata(paths: Vec<PathBuf>) -> Vec<AlbumMetadata> {
    let mut groups: BTreeMap<PathBuf, Vec<PathBuf>> = BTreeMap::new();
    for path in paths {
        if let Some(folder) = path.parent() {
            groups.entry(folder.to_path_buf()).or_default().push(path);
        }
    }
    let mut batches: Vec<Vec<_>> = (0..4).map(|_| Vec::new()).collect();
    for (index, group) in groups.into_iter().enumerate() {
        batches[index % 4].push(group);
    }
    let mut albums: Vec<AlbumMetadata> = std::thread::scope(|scope| {
        let workers: Vec<_> = batches
            .into_iter()
            .map(|batch| {
                scope.spawn(move || {
                    batch
                        .into_iter()
                        .map(|(folder, tracks)| {
                            let (sorted_tracks, tags) = order_tracks(tracks, read_order);
                            let title =
                                tags.as_ref()
                                    .and_then(|t| t.album.clone())
                                    .unwrap_or_else(|| {
                                        folder
                                            .file_name()
                                            .and_then(|n| n.to_str())
                                            .unwrap_or("Unknown Album")
                                            .to_string()
                                    });
                            let artist = tags
                                .as_ref()
                                .and_then(|t| t.artist.clone())
                                .unwrap_or_else(|| {
                                    folder
                                        .parent()
                                        .and_then(|p| p.file_name())
                                        .and_then(|n| n.to_str())
                                        .filter(|n| {
                                            *n != "music" && *n != "Albums" && *n != "Músicas" && *n != "Tracks"
                                        })
                                        .unwrap_or("Unknown Artist")
                                        .to_string()
                                });
                            let year = tags.as_ref().and_then(|t| t.year);
                            AlbumMetadata {
                                folder,
                                title,
                                artist,
                                year,
                                tracks: sorted_tracks,
                            }
                        })
                        .collect::<Vec<_>>()
                })
            })
            .collect();
        workers
            .into_iter()
            .flat_map(|worker| worker.join().expect("indexador de álbuns"))
            .collect()
    });
    sort_albums(&mut albums, AlbumSortMode::TitleAsc);
    albums
}

#[allow(dead_code)]
pub fn ordered_albums(paths: Vec<PathBuf>) -> AlbumGroups {
    let albums = load_album_metadata(paths);
    albums
        .into_iter()
        .map(|meta| (meta.folder, meta.tracks))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[ignore = "Requer ffmpeg e ffprobe; verifica arquivos FLAC com tags reais"]
    fn metadata_fixture_orders_files_and_invalidates_cache() {
        let dir = std::env::temp_dir().join(format!(
            "vinyl-order-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir(&dir).unwrap();
        let make = |name: &str, disc: &str, track: &str| {
            let path = dir.join(name);
            let result = std::process::Command::new("ffmpeg")
                .args([
                    "-v",
                    "error",
                    "-y",
                    "-f",
                    "lavfi",
                    "-i",
                    "anullsrc=r=8000:cl=mono",
                    "-t",
                    "0.02",
                    "-c:a",
                    "flac",
                    "-metadata",
                ])
                .arg(format!("disc={disc}"))
                .arg("-metadata")
                .arg(format!("track={track}"))
                .arg(&path)
                .status()
                .unwrap();
            assert!(result.success());
            path
        };
        let disc2 = make("01-wrong.flac", "2/2", "1/3");
        let first = make("20-first.flac", "1/2", "1/3");
        let second = make("02-second.flac", "1/2", "2/3");
        let groups = ordered_albums(vec![disc2.clone(), second.clone(), first.clone()]);
        assert_eq!(groups[&dir], [first.clone(), second, disc2]);
        assert_eq!(
            read_order(&first),
            TrackTags {
                disc: Some(1),
                track: Some(1),
                album: None,
                artist: None,
                year: None,
                ..Default::default()
            }
        );
        make("20-first.flac", "2/2", "3/3");
        assert_eq!(
            read_order(&first),
            TrackTags {
                disc: Some(2),
                track: Some(3),
                album: None,
                artist: None,
                year: None,
                ..Default::default()
            }
        );
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn parses_totals_aliases_and_ignores_invalid_numbers() {
        assert_eq!(
            parse_tags("TAG:DISCNUMBER=2/3\nTAG:TRACKNUMBER=10/12"),
            TrackTags {
                disc: Some(2),
                track: Some(10),
                album: None,
                artist: None,
                year: None,
                ..Default::default()
            }
        );
        assert_eq!(
            parse_tags("TAG:disc=0\nTAG:track=invalid\nTAG:TRACKNUMBER=03/09"),
            TrackTags {
                disc: None,
                track: Some(3),
                album: None,
                artist: None,
                year: None,
                ..Default::default()
            }
        );
        assert_eq!(number("-1"), None);
        assert_eq!(number("1x"), None);
    }

    #[test]
    fn parses_rich_tags_album_artist_year() {
        let text = "TAG:ALBUM=Wish You Were Here\nTAG:ARTIST=Pink Floyd\nTAG:DATE=1975-09-12\nTAG:track=2/5";
        let parsed = parse_tags(text);
        assert_eq!(
            parsed,
            TrackTags {
                disc: None,
                track: Some(2),
                album: Some("Wish You Were Here".to_string()),
                artist: Some("Pink Floyd".to_string()),
                performer: Some("Pink Floyd".to_string()),
                year: Some(1975),
                ..Default::default()
            }
        );
    }

    #[test]
    fn parses_various_year_formats() {
        assert_eq!(parse_year("1975-09-12"), Some(1975));
        assert_eq!(parse_year("2016"), Some(2016));
        assert_eq!(parse_year("2001/01/01"), Some(2001));
        assert_eq!(parse_year("Ano 1994"), Some(1994));
        assert_eq!(parse_year(""), None);
        assert_eq!(parse_year("abc"), None);
        assert_eq!(parse_year("999"), None);
        assert_eq!(parse_year("3001"), None);
    }

    #[test]
    fn album_sort_modes_work_deterministically() {
        let make_album =
            |name: &str, artist: &str, year: Option<u32>, folder: &str| AlbumMetadata {
                folder: PathBuf::from(folder),
                title: name.to_string(),
                artist: artist.to_string(),
                year,
                tracks: Vec::new(),
            };

        let a = make_album(
            "Wish You Were Here",
            "Pink Floyd",
            Some(1975),
            "/music/pink-floyd/wish",
        );
        let b = make_album(
            "Savage Mode",
            "21 Savage",
            Some(2016),
            "/music/21-savage/savage",
        );
        let c = make_album("Abba Gold", "ABBA", Some(1992), "/music/abba/gold");
        let d = make_album("Unknown Album", "Z-Artist", None, "/music/z/unknown");

        let mut list = vec![a.clone(), b.clone(), c.clone(), d.clone()];

        // TitleAsc
        sort_albums(&mut list, AlbumSortMode::TitleAsc);
        assert_eq!(list[0].title, "Abba Gold");
        assert_eq!(list[1].title, "Savage Mode");
        assert_eq!(list[2].title, "Unknown Album");
        assert_eq!(list[3].title, "Wish You Were Here");

        // TitleDesc
        sort_albums(&mut list, AlbumSortMode::TitleDesc);
        assert_eq!(list[0].title, "Wish You Were Here");
        assert_eq!(list[3].title, "Abba Gold");

        // ArtistAsc
        sort_albums(&mut list, AlbumSortMode::ArtistAsc);
        assert_eq!(list[0].artist, "21 Savage");
        assert_eq!(list[1].artist, "ABBA");
        assert_eq!(list[2].artist, "Pink Floyd");
        assert_eq!(list[3].artist, "Z-Artist");

        // YearDesc (mais recentes primeiro, None no fim)
        sort_albums(&mut list, AlbumSortMode::YearDesc);
        assert_eq!(list[0].year, Some(2016));
        assert_eq!(list[1].year, Some(1992));
        assert_eq!(list[2].year, Some(1975));
        assert_eq!(list[3].year, None);

        // YearAsc (mais antigos primeiro, None no fim)
        sort_albums(&mut list, AlbumSortMode::YearAsc);
        assert_eq!(list[0].year, Some(1975));
        assert_eq!(list[1].year, Some(1992));
        assert_eq!(list[2].year, Some(2016));
        assert_eq!(list[3].year, None);
    }

    #[test]
    fn metadata_overrides_names_and_handles_multiple_discs() {
        let paths: Vec<_> = [
            "01-wrong.flac",
            "20-first.flac",
            "02-second.flac",
            "10-no-tags.flac",
            "2-no-tags.flac",
        ]
        .iter()
        .map(PathBuf::from)
        .collect();
        let (sorted, _) = order_tracks(paths, |p| match p.to_str().unwrap() {
            "01-wrong.flac" => TrackTags {
                disc: Some(2),
                track: Some(1),
                ..Default::default()
            },
            "20-first.flac" => TrackTags {
                disc: Some(1),
                track: Some(1),
                ..Default::default()
            },
            "02-second.flac" => TrackTags {
                disc: None,
                track: Some(2),
                ..Default::default()
            },
            _ => TrackTags::default(),
        });
        assert_eq!(
            sorted,
            [
                "20-first.flac",
                "02-second.flac",
                "2-no-tags.flac",
                "10-no-tags.flac",
                "01-wrong.flac"
            ]
            .map(PathBuf::from)
        );
    }

    #[test]
    fn natural_order_is_numeric_and_deterministic_on_ties() {
        assert_eq!(natural_cmp("2.flac", "10.flac"), Ordering::Less);
        assert_eq!(natural_cmp("0002.flac", "2.flac"), Ordering::Equal);
        assert_eq!(
            natural_cmp("9999999999999999999999", "10000000000000000000000"),
            Ordering::Less
        );
        let a: Vec<_> = ["02.flac", "2.flac"].iter().map(PathBuf::from).collect();
        let mut b = a.clone();
        b.reverse();
        let (sorted_a, _) = order_tracks(a, |_| TrackTags::default());
        let (sorted_b, _) = order_tracks(b, |_| TrackTags::default());
        assert_eq!(sorted_a, sorted_b);
    }
}
