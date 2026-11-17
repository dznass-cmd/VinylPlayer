use std::cell::RefCell;
use std::path::Path;
use std::rc::Rc;
use crate::library::AlbumMetadata;

pub type SearchCallback = Rc<dyn Fn()>;

/// Hub reativo para gerenciar a consulta de busca em tempo real entre diferentes
/// visões da interface gráfica.
#[derive(Clone, Default)]
pub struct SearchHub {
    query: Rc<RefCell<String>>,
    callbacks: Rc<RefCell<Vec<SearchCallback>>>,
}

impl SearchHub {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn query(&self) -> String {
        self.query.borrow().clone()
    }

    pub fn set_query(&self, q: &str) {
        *self.query.borrow_mut() = q.to_string();
        for cb in self.callbacks.borrow().iter() {
            cb();
        }
    }

    pub fn on_query_changed<F: Fn() + 'static>(&self, f: F) {
        self.callbacks.borrow_mut().push(Rc::new(f));
    }
}

/// Normaliza um texto para busca: converte para minúsculas e remove acentos
/// comuns (á->a, é->e, ç->c, etc.) para busca tolerante e fluida.
pub fn normalize_for_search(s: &str) -> String {
    s.chars()
        .map(|c| match c {
            'á' | 'à' | 'â' | 'ã' | 'ä' | 'Á' | 'À' | 'Â' | 'Ã' | 'Ä' => 'a',
            'é' | 'è' | 'ê' | 'ë' | 'É' | 'È' | 'Ê' | 'Ë' => 'e',
            'í' | 'ì' | 'î' | 'ï' | 'Í' | 'Ì' | 'Î' | 'Ï' => 'i',
            'ó' | 'ò' | 'ô' | 'õ' | 'ö' | 'Ó' | 'Ò' | 'Ô' | 'Õ' | 'Ö' => 'o',
            'ú' | 'ù' | 'û' | 'ü' | 'Ú' | 'Ù' | 'Û' | 'Ü' => 'u',
            'ç' | 'Ç' => 'c',
            'ñ' | 'Ñ' => 'n',
            other => other.to_ascii_lowercase(),
        })
        .collect()
}

/// Verifica se o texto pesquisado (`query`) coincide com o conteúdo (`haystack`).
///
/// - Se `query` for vazio ou composto só por espaços, retorna `true`.
/// - Divide `query` em palavras; todas as palavras devem estar presentes no `haystack`
///   (busca multi-termo independente da ordem).
pub fn matches_query(haystack: &str, query: &str) -> bool {
    let q = query.trim();
    if q.is_empty() {
        return true;
    }
    let norm_haystack = normalize_for_search(haystack);
    for term in q.split_whitespace() {
        let norm_term = normalize_for_search(term);
        if !norm_haystack.contains(&norm_term) {
            return false;
        }
    }
    true
}

/// Verifica se uma faixa musical corresponde aos termos de busca.
/// Avalia nome do arquivo, extensão e caminho do diretório pai.
pub fn track_matches(path: &Path, query: &str) -> bool {
    if query.trim().is_empty() {
        return true;
    }
    let stem = path.file_stem().and_then(|s| s.to_str()).unwrap_or("");
    let ext = path.extension().and_then(|s| s.to_str()).unwrap_or("");
    let parent = path
        .parent()
        .and_then(|p| p.file_name())
        .and_then(|s| s.to_str())
        .unwrap_or("");
    let full = path.to_string_lossy();

    let combined = format!("{stem} {ext} {parent} {full}");
    matches_query(&combined, query)
}

/// Verifica se um álbum corresponde aos termos de busca.
/// Avalia título do álbum, artista, ano, pasta e nomes das faixas contidas.
pub fn album_matches(album: &AlbumMetadata, query: &str) -> bool {
    if query.trim().is_empty() {
        return true;
    }
    let mut combined = String::with_capacity(256);
    combined.push_str(&album.title);
    combined.push(' ');
    combined.push_str(&album.artist);
    combined.push(' ');
    if let Some(year) = album.year {
        combined.push_str(&year.to_string());
        combined.push(' ');
    }
    if let Some(folder_name) = album.folder.file_name().and_then(|s| s.to_str()) {
        combined.push_str(folder_name);
        combined.push(' ');
    }
    for track in &album.tracks {
        if let Some(stem) = track.file_stem().and_then(|s| s.to_str()) {
            combined.push_str(stem);
            combined.push(' ');
        }
    }
    matches_query(&combined, query)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn search_hub_notifies_listeners_on_query_change() {
        let hub = SearchHub::new();
        let count = Rc::new(std::cell::Cell::new(0));
        let count_ref = count.clone();
        hub.on_query_changed(move || {
            count_ref.set(count_ref.get() + 1);
        });

        assert_eq!(hub.query(), "");
        assert_eq!(count.get(), 0);

        hub.set_query("savage");
        assert_eq!(hub.query(), "savage");
        assert_eq!(count.get(), 1);

        hub.set_query("");
        assert_eq!(hub.query(), "");
        assert_eq!(count.get(), 2);
    }

    #[test]
    fn normalize_removes_accents_and_lowercases() {
        assert_eq!(normalize_for_search("Música"), "musica");
        assert_eq!(normalize_for_search("CANÇÃO"), "cancao");
        assert_eq!(normalize_for_search("Épico"), "epico");
        assert_eq!(normalize_for_search("Beyoncé"), "beyonce");
    }

    #[test]
    fn empty_query_matches_everything() {
        assert!(matches_query("Qualquer coisa", ""));
        assert!(matches_query("Qualquer coisa", "   "));
    }

    #[test]
    fn multi_word_order_independent_search() {
        let hay = "21 Savage & Metro Boomin - Savage Mode II";
        assert!(matches_query(hay, "21 savage"));
        assert!(matches_query(hay, "metro savage"));
        assert!(matches_query(hay, "boomin 21 mode"));
        assert!(!matches_query(hay, "drake savage"));
    }

    #[test]
    fn accent_tolerant_search() {
        let hay = "Legião Urbana - As Quatro Estações";
        assert!(matches_query(hay, "legiao urbana"));
        assert!(matches_query(hay, "estacoes"));
        assert!(matches_query(hay, "LEGIÃO"));
    }

    #[test]
    fn track_matches_filename_and_folder() {
        let p = PathBuf::from("/home/user/Música/Rap/01 - No Heart.flac");
        assert!(track_matches(&p, "no heart"));
        assert!(track_matches(&p, "flac"));
        assert!(track_matches(&p, "rap"));
        assert!(!track_matches(&p, "rock"));
    }

    #[test]
    fn album_matches_artist_title_year_and_subtracks() {
        let album = AlbumMetadata {
            title: "Savage Mode".to_string(),
            artist: "21 Savage".to_string(),
            year: Some(2016),
            folder: PathBuf::from("/music/21 Savage/Savage Mode"),
            tracks: vec![
                PathBuf::from("/music/21 Savage/Savage Mode/01 - Bad Guy.flac"),
                PathBuf::from("/music/21 Savage/Savage Mode/02 - No Heart.flac"),
            ],
        };
        assert!(album_matches(&album, "savage"));
        assert!(album_matches(&album, "2016"));
        assert!(album_matches(&album, "no heart"));
        assert!(!album_matches(&album, "metallica"));
    }
}
