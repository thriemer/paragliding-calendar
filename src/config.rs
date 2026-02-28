use std::env;

use anyhow::Result;

pub struct WebConfig {
    pub port: u16,
    #[cfg(feature = "tls")]
    pub tls_config_path: (String, String),
}

impl WebConfig {
    pub fn load() -> Result<Self> {
        let port = env::var("PORT")
            .ok()
            .and_then(|p| p.parse().ok())
            .unwrap_or(8080);

        Ok(WebConfig {
            port: port,
            #[cfg(feature = "tls")]
            tls_config_path: (env::var("TLS_CERT_PATH")?, env::var("TLS_KEY_PATH")?),
        })
    }
}
