//! Ordem e seleção da fila, independentes do pipeline de áudio.
use std::path::{Path, PathBuf};

#[derive(Clone, Debug, Default)]
pub struct QueueSnapshot {
    pub revision: u64,
    pub tracks: Vec<PathBuf>,
    pub current: Option<usize>,
}

#[derive(Default)]
pub struct PlaybackQueue {
    pub(crate) tracks: Vec<PathBuf>,
    pub(crate) current: Option<usize>,
    revision: u64,
}

impl PlaybackQueue {
    pub fn new(tracks: Vec<PathBuf>) -> Self {
        Self {
            tracks,
            current: None,
            revision: 1,
        }
    }

    pub fn snapshot(&self) -> QueueSnapshot {
        QueueSnapshot {
            revision: self.revision,
            tracks: self.tracks.clone(),
            current: self.current,
        }
    }

    pub fn status(&self) -> (u64, Option<usize>, usize) {
        (self.revision, self.current, self.tracks.len())
    }

    pub fn accepts(&self, revision: u64, index: usize) -> bool {
        self.revision == revision && index < self.tracks.len()
    }

    pub fn select_path(&mut self, path: &Path) {
        self.current = Some(match self.tracks.iter().position(|p| p == path) {
            Some(index) => index,
            None => {
                self.tracks.push(path.to_owned());
                self.revision += 1;
                self.tracks.len() - 1
            }
        });
    }

    pub fn replace(&mut self, tracks: &[PathBuf], start: usize) {
        self.tracks = tracks.to_vec();
        self.current = Some(start);
        self.revision += 1;
    }

    pub fn remove(&mut self, index: usize) {
        self.tracks.remove(index);
        self.current = self.current.and_then(|current| {
            if self.tracks.is_empty() {
                None
            } else if current > index {
                Some(current - 1)
            } else {
                Some(current.min(self.tracks.len() - 1))
            }
        });
        self.revision += 1;
    }

    /// Insere o bloco inteiro sem alterar a seleção nem a ordem interna do bloco.
    pub fn enqueue(&mut self, tracks: &[PathBuf], next: bool) {
        if tracks.is_empty() {
            return;
        }
        let empty = self.tracks.is_empty();
        let position = if next {
            self.current.map_or(0, |i| i + 1)
        } else {
            self.tracks.len()
        };
        self.tracks
            .splice(position..position, tracks.iter().cloned());
        if empty {
            self.current = Some(0);
        } else if let Some(current) = self.current.as_mut() {
            if position <= *current {
                *current += tracks.len();
            }
        }
        self.revision += 1;
    }

    /// `to` é a posição final da faixa, depois da remoção da posição original.
    pub fn move_item(&mut self, from: usize, to: usize) {
        if from == to {
            return;
        }
        let path = self.tracks.remove(from);
        self.tracks.insert(to, path);
        self.current = self.current.map(|current| {
            if current == from {
                to
            } else if from < current && current <= to {
                current - 1
            } else if to <= current && current < from {
                current + 1
            } else {
                current
            }
        });
        self.revision += 1;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn enqueue_preserves_current_block_order_duplicates_and_empty_queue() {
        let mut queue = PlaybackQueue::new(vec!["a".into(), "b".into()]);
        queue.current = Some(0);
        let revision = queue.status().0;
        queue.enqueue(&["c".into(), "c".into(), "d".into()], true);
        assert_eq!(queue.tracks, ["a", "c", "c", "d", "b"].map(PathBuf::from));
        assert_eq!(queue.current, Some(0));
        assert!(!queue.accepts(revision, 0));
        queue.enqueue(&["e".into(), "f".into()], false);
        assert_eq!(
            queue.tracks,
            ["a", "c", "c", "d", "b", "e", "f"].map(PathBuf::from)
        );
        let revision = queue.status().0;
        queue.enqueue(&[], true);
        assert_eq!(queue.status().0, revision);
        let mut empty = PlaybackQueue::default();
        empty.enqueue(&["x".into(), "y".into()], false);
        assert_eq!(empty.current, Some(0));
        assert_eq!(empty.tracks, ["x", "y"].map(PathBuf::from));
    }

    #[test]
    fn every_move_keeps_the_same_current_entry_including_duplicate_paths() {
        for current in 0..4 {
            for from in 0..4 {
                for to in 0..4 {
                    let tracks: Vec<_> = ["a.wav", "same.wav", "same.wav", "d.wav"]
                        .map(PathBuf::from)
                        .into();
                    let mut ids = vec![0, 1, 2, 3];
                    let moved = ids.remove(from);
                    ids.insert(to, moved);
                    let mut queue = PlaybackQueue::new(tracks.clone());
                    queue.current = Some(current);
                    queue.move_item(from, to);
                    assert_eq!(queue.current, ids.iter().position(|id| *id == current));
                    assert_eq!(
                        queue.tracks,
                        ids.iter().map(|id| tracks[*id].clone()).collect::<Vec<_>>()
                    );
                }
            }
        }
    }

    #[test]
    fn removals_preserve_current_or_select_successor_then_previous() {
        for current in 0..4 {
            for removed in 0..4 {
                let mut queue = PlaybackQueue::new(["a", "b", "c", "d"].map(PathBuf::from).into());
                queue.current = Some(current);
                queue.remove(removed);
                let expected = if current > removed {
                    current - 1
                } else {
                    current.min(2)
                };
                assert_eq!(queue.current, Some(expected));
            }
        }
        let mut queue = PlaybackQueue::new(vec!["only.wav".into()]);
        queue.current = Some(0);
        queue.remove(0);
        assert!(queue.tracks.is_empty());
        assert_eq!(queue.current, None);
    }

    #[test]
    fn stale_edits_are_rejected_and_new_paths_do_not_sort_an_edited_queue() {
        let mut queue = PlaybackQueue::new(vec!["c.wav".into(), "b.wav".into()]);
        let old = queue.snapshot().revision;
        queue.move_item(0, 1);
        assert!(!queue.accepts(old, 0));
        let rev = queue.snapshot().revision;
        queue.select_path(Path::new("a.wav"));
        assert_eq!(
            queue.tracks,
            [PathBuf::from("b.wav"), "c.wav".into(), "a.wav".into()]
        );
        assert!(!queue.accepts(rev, 0));
        assert!(!queue.accepts(queue.snapshot().revision, 10));
        queue.replace(&["other.wav".into()], 0);
        assert_eq!(queue.current, Some(0));
    }
}
