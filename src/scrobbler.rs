//! Integração nativa com Last.fm Audioscrobbler 2.0 API.
//! Suporta autenticação web, updateNowPlaying e scrobble com assinatura MD5.

use md5::{Digest, Md5};
use std::time::Duration;

pub const DEFAULT_API_KEY: &str = "4b4f73bda181868353f9b438604adf52";
pub const DEFAULT_API_SECRET: &str = "716cc0a784bb62835de5bd674e65eb57";
pub const API_URL: &str = "https://ws.audioscrobbler.com/2.0/";
pub const AUTH_URL: &str = "https://www.last.fm/api/auth/";
const HTTP_TIMEOUT: Duration = Duration::from_secs(12);

fn http_agent() -> ureq::Agent {
    ureq::Agent::config_builder()
        .timeout_global(Some(HTTP_TIMEOUT))
        .build()
        .into()
}

/// Calcula a assinatura MD5 (`api_sig`) de acordo com a especificação Audioscrobbler 2.0:
/// 1. Ordena os parâmetros alfabeticamente pela chave (excluindo format, callback e api_sig).
/// 2. Concatena chave e valor sem separadores: `key1value1key2value2...`
/// 3. Concatena o `api_secret` no final.
/// 4. Retorna o hash MD5 em formato hexadecimal minúsculo (32 caracteres).
pub fn sign_params(params: &[(&str, &str)], api_secret: &str) -> String {
    let mut sorted: Vec<(&str, &str)> = params
        .iter()
        .filter(|(k, _)| *k != "format" && *k != "callback" && *k != "api_sig")
        .copied()
        .collect();
    sorted.sort_by_key(|(k, _)| *k);

    let mut buf = String::new();
    for (k, v) in sorted {
        buf.push_str(k);
        buf.push_str(v);
    }
    buf.push_str(api_secret);

    let mut hasher = Md5::new();
    hasher.update(buf.as_bytes());
    let result = hasher.finalize();
    hex::encode(result)
}

/// Helper para codificar parâmetros como URL query string / form-urlencoded.
pub fn encode_form(params: &[(&str, &str)]) -> String {
    let mut encoded = String::new();
    for (i, (k, v)) in params.iter().enumerate() {
        if i > 0 {
            encoded.push('&');
        }
        encoded.push_str(&urlencoding::encode(k));
        encoded.push('=');
        encoded.push_str(&urlencoding::encode(v));
    }
    encoded
}

/// Helper para urlencoding simples se não tiver a crate urlencoding
pub mod urlencoding {
    pub fn encode(s: &str) -> String {
        let mut result = String::with_capacity(s.len());
        for b in s.bytes() {
            match b {
                b'a'..=b'z' | b'A'..=b'Z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                    result.push(b as char);
                }
                b' ' => result.push('+'),
                _ => {
                    result.push_str(&format!("%{:02X}", b));
                }
            }
        }
        result
    }
}

/// Obtém um token de autorização temporário via `auth.getToken`.
pub fn request_token(api_key: &str) -> Result<String, String> {
    let query = encode_form(&[
        ("method", "auth.getToken"),
        ("api_key", api_key),
        ("format", "json"),
    ]);
    let url = format!("{API_URL}?{query}");
    let mut res = http_agent()
        .get(&url)
        .call()
        .map_err(|e| format!("Falha de conexão com Last.fm: {e}"))?;
    let body_str = res
        .body_mut()
        .read_to_string()
        .map_err(|e| format!("Falha ao ler resposta da Last.fm: {e}"))?;
    let val: serde_json::Value = serde_json::from_str(&body_str)
        .map_err(|e| format!("Resposta JSON inválida da Last.fm: {e}"))?;

    if let Some(token) = val.get("token").and_then(|t| t.as_str()) {
        Ok(token.to_string())
    } else {
        let msg = val.get("message").and_then(|m| m.as_str()).unwrap_or("Token não retornado");
        Err(msg.to_string())
    }
}

/// Constrói a URL para o usuário abrir no navegador e autorizar o app.
pub fn authorization_url(api_key: &str, token: &str) -> String {
    let query = encode_form(&[("api_key", api_key), ("token", token)]);
    format!("{AUTH_URL}?{query}")
}

/// Obtém a chave de sessão permanente (`sk`) e nome de usuário a partir do token autorizado.
pub fn fetch_session(api_key: &str, api_secret: &str, token: &str) -> Result<(String, String), String> {
    let params = [
        ("api_key", api_key),
        ("method", "auth.getSession"),
        ("token", token),
    ];
    let api_sig = sign_params(&params, api_secret);
    let query = encode_form(&[
        ("method", "auth.getSession"),
        ("api_key", api_key),
        ("token", token),
        ("api_sig", &api_sig),
        ("format", "json"),
    ]);
    let url = format!("{API_URL}?{query}");

    let mut res = http_agent()
        .get(&url)
        .call()
        .map_err(|e| format!("Falha ao obter sessão do Last.fm: {e}"))?;
    let body_str = res
        .body_mut()
        .read_to_string()
        .map_err(|e| format!("Falha ao ler resposta da sessão: {e}"))?;
    let val: serde_json::Value = serde_json::from_str(&body_str)
        .map_err(|e| format!("Resposta JSON inválida: {e}"))?;

    if let Some(session) = val.get("session") {
        let sk = session.get("key").and_then(|k| k.as_str()).ok_or("Chave de sessão ausente")?;
        let name = session.get("name").and_then(|n| n.as_str()).ok_or("Nome de usuário ausente")?;
        Ok((sk.to_string(), name.to_string()))
    } else {
        let err_code = val.get("error").and_then(|e| e.as_i64()).unwrap_or(0);
        let msg = val.get("message").and_then(|m| m.as_str()).unwrap_or("Autorização pendente");
        Err(format!("Erro {err_code}: {msg}"))
    }
}

/// Envia o status de reprodução atual ("Now Playing") para a Last.fm.
pub fn update_now_playing(
    api_key: &str,
    api_secret: &str,
    session_key: &str,
    artist: &str,
    track: &str,
    album: Option<&str>,
    duration_sec: Option<u64>,
) -> Result<(), String> {
    let dur_str;
    let mut params: Vec<(&str, &str)> = vec![
        ("method", "track.updateNowPlaying"),
        ("api_key", api_key),
        ("sk", session_key),
        ("artist", artist),
        ("track", track),
    ];
    if let Some(alb) = album.filter(|a| !a.is_empty()) {
        params.push(("album", alb));
    }
    if let Some(dur) = duration_sec.filter(|d| *d > 0) {
        dur_str = dur.to_string();
        params.push(("duration", &dur_str));
    }

    let api_sig = sign_params(&params, api_secret);
    let mut post_params = params.clone();
    post_params.push(("api_sig", &api_sig));
    post_params.push(("format", "json"));

    let body = encode_form(&post_params);
    let mut res = http_agent()
        .post(API_URL)
        .header("Content-Type", "application/x-www-form-urlencoded")
        .send(body.as_bytes())
        .map_err(|e| format!("Erro ao enviar Now Playing: {e}"))?;

    let body_str = res
        .body_mut()
        .read_to_string()
        .map_err(|e| format!("Erro ao ler resposta Now Playing: {e}"))?;
    let val: serde_json::Value = serde_json::from_str(&body_str)
        .map_err(|e| format!("Resposta inválida Now Playing: {e}"))?;

    if let Some(err) = val.get("error") {
        let msg = val.get("message").and_then(|m| m.as_str()).unwrap_or("Falha no Now Playing");
        return Err(format!("Erro {err}: {msg}"));
    }

    Ok(())
}

#[derive(Debug, Clone)]
pub struct ScrobblePayload<'a> {
    pub artist: &'a str,
    pub track: &'a str,
    pub album: Option<&'a str>,
    pub timestamp: u64,
    pub duration_sec: Option<u64>,
}

/// Envia o Scrobble de uma faixa tocada para a Last.fm.
pub fn scrobble_track(
    api_key: &str,
    api_secret: &str,
    session_key: &str,
    payload: &ScrobblePayload<'_>,
) -> Result<(), String> {
    let ts_str = payload.timestamp.to_string();
    let dur_str;
    let mut params: Vec<(&str, &str)> = vec![
        ("method", "track.scrobble"),
        ("api_key", api_key),
        ("sk", session_key),
        ("artist", payload.artist),
        ("track", payload.track),
        ("timestamp", &ts_str),
    ];
    if let Some(alb) = payload.album.filter(|a| !a.is_empty()) {
        params.push(("album", alb));
    }
    if let Some(dur) = payload.duration_sec.filter(|d| *d > 0) {
        dur_str = dur.to_string();
        params.push(("duration", &dur_str));
    }

    let api_sig = sign_params(&params, api_secret);
    let mut post_params = params.clone();
    post_params.push(("api_sig", &api_sig));
    post_params.push(("format", "json"));

    let body = encode_form(&post_params);
    let mut res = http_agent()
        .post(API_URL)
        .header("Content-Type", "application/x-www-form-urlencoded")
        .send(body.as_bytes())
        .map_err(|e| format!("Erro ao enviar Scrobble: {e}"))?;

    let body_str = res
        .body_mut()
        .read_to_string()
        .map_err(|e| format!("Erro ao ler resposta Scrobble: {e}"))?;
    let val: serde_json::Value = serde_json::from_str(&body_str)
        .map_err(|e| format!("Resposta inválida Scrobble: {e}"))?;

    if let Some(err) = val.get("error") {
        let msg = val.get("message").and_then(|m| m.as_str()).unwrap_or("Falha no Scrobble");
        return Err(format!("Erro {err}: {msg}"));
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_sign_params() {
        let params = [
            ("method", "auth.getSession"),
            ("api_key", "my_api_key"),
            ("token", "my_token"),
        ];
        let secret = "my_secret";
        let sig = sign_params(&params, secret);
        let mut hasher = Md5::new();
        hasher.update(b"api_keymy_api_keymethodauth.getSessiontokenmy_tokenmy_secret");
        let expected = hex::encode(hasher.finalize());
        assert_eq!(sig, expected);
    }

    #[test]
    fn test_encode_form() {
        let params = [
            ("artist", "Pink Floyd"),
            ("track", "Time & Space"),
        ];
        let encoded = encode_form(&params);
        assert_eq!(encoded, "artist=Pink+Floyd&track=Time+%26+Space");
    }

    #[test]
    fn test_authorization_url_encodes_params() {
        let url = authorization_url("key with space", "tok&123");
        assert_eq!(
            url,
            "https://www.last.fm/api/auth/?api_key=key+with+space&token=tok%26123"
        );
    }
}
