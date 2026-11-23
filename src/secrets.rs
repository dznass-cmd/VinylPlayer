//! Last.fm credentials stored outside the regular preferences file.
//!
//! OO7 selects Secret Service on a normal Linux desktop and the Secret portal
//! backed encrypted file inside Flatpak. All calls run outside GTK's thread.

use serde_json::json;

const APPLICATION: &str = "io.github.vinylplayer";
const PURPOSE: &str = "lastfm";

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct LastFmSecrets {
    pub api_key: Option<String>,
    pub api_secret: Option<String>,
    pub session_key: Option<String>,
}

impl LastFmSecrets {
    pub fn is_empty(&self) -> bool {
        self.api_key.as_deref().is_none_or(str::is_empty)
            && self.api_secret.as_deref().is_none_or(str::is_empty)
            && self.session_key.as_deref().is_none_or(str::is_empty)
    }

    fn to_payload(&self) -> Result<String, String> {
        serde_json::to_string(&json!({
            "api_key": self.api_key,
            "api_secret": self.api_secret,
            "session_key": self.session_key,
        }))
        .map_err(|error| error.to_string())
    }

    fn from_payload(payload: &str) -> Result<Self, String> {
        let value: serde_json::Value =
            serde_json::from_str(payload).map_err(|error| error.to_string())?;
        Ok(Self {
            api_key: value
                .get("api_key")
                .and_then(|value| value.as_str())
                .map(str::to_owned),
            api_secret: value
                .get("api_secret")
                .and_then(|value| value.as_str())
                .map(str::to_owned),
            session_key: value
                .get("session_key")
                .and_then(|value| value.as_str())
                .map(str::to_owned),
        })
    }
}

fn attributes() -> [(&'static str, &'static str); 2] {
    [("application", APPLICATION), ("purpose", PURPOSE)]
}

fn run<T: Send + 'static>(
    task: impl std::future::Future<Output = Result<T, String>> + Send + 'static,
) -> Result<T, String> {
    std::thread::Builder::new()
        .name("vinyl-secret-service".into())
        .spawn(move || {
            let runtime = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .map_err(|error| error.to_string())?;
            runtime.block_on(task)
        })
        .map_err(|error| error.to_string())?
        .join()
        .map_err(|_| "Secret Service worker panicked".to_string())?
}

pub fn load() -> Result<LastFmSecrets, String> {
    run(async {
        let keyring = oo7::Keyring::new()
            .await
            .map_err(|error| error.to_string())?;
        let items = keyring
            .search_items(&attributes())
            .await
            .map_err(|error| error.to_string())?;
        let Some(item) = items.first() else {
            return Ok(LastFmSecrets::default());
        };
        let secret = item.secret().await.map_err(|error| error.to_string())?;
        let payload = secret
            .as_str()
            .ok_or_else(|| "Secret Service returned non-text Last.fm credentials".to_string())?;
        LastFmSecrets::from_payload(payload)
    })
}

pub fn store(secrets: LastFmSecrets) -> Result<(), String> {
    run(async move {
        let keyring = oo7::Keyring::new()
            .await
            .map_err(|error| error.to_string())?;
        if secrets.is_empty() {
            keyring
                .delete(&attributes())
                .await
                .map_err(|error| error.to_string())?;
            return Ok(());
        }
        let payload = secrets.to_payload()?;
        keyring
            .create_item("VinylPlayer Last.fm", &attributes(), payload, true)
            .await
            .map_err(|error| error.to_string())?;
        Ok(())
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn payload_roundtrip_preserves_unicode_and_missing_values() {
        let expected = LastFmSecrets {
            api_key: Some("chave ç".into()),
            api_secret: Some("secret".into()),
            session_key: None,
        };
        assert_eq!(
            LastFmSecrets::from_payload(&expected.to_payload().unwrap()).unwrap(),
            expected
        );
    }
}
