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
            port,
            #[cfg(feature = "tls")]
            tls_config_path: (env::var("TLS_CERT_PATH")?, env::var("TLS_KEY_PATH")?),
        })
    }
}

pub struct GoogleOAuthConfig {
    pub client_id: String,
    pub client_secret: String,
    pub redirect_uri: String,
}

pub struct MicrosoftOAuthConfig {
    pub client_id: String,
    pub client_secret: String,
    pub tenant_id: String,
    pub redirect_uri: String,
}

pub struct AppConfig {
    pub database_url: String,
    pub google: GoogleOAuthConfig,
    pub microsoft: Option<MicrosoftOAuthConfig>,
    /// Writable directory the embedding model files are downloaded into and
    /// loaded from. On NixOS the store is read-only, so `module.nix` points
    /// this at a `StateDirectory`.
    pub embedding_cache_dir: String,
    /// Batch size for embedding inference.
    pub embedding_batch_size: usize,
    /// Writable directory downloaded activity images are stored in (content-
    /// addressed). Like `embedding_cache_dir`, NixOS points this at a StateDirectory.
    pub image_store_dir: String,
    /// Outdooractive image `{variant}` size token (e.g. `300x300`). A small square
    /// bucket is plenty since CLIP center-crops to 224².
    pub image_variant: String,
    /// Max concurrent image downloads (politeness / rate-limit guard).
    pub image_download_concurrency: usize,
}

impl AppConfig {
    pub fn from_env() -> Result<Self> {
        let database_url =
            env::var("DATABASE_URL").map_err(|_| anyhow::anyhow!("DATABASE_URL must be set"))?;

        let google = GoogleOAuthConfig {
            client_id: env::var("GOOGLE_CLIENT_ID")
                .map_err(|_| anyhow::anyhow!("Missing GOOGLE_CLIENT_ID"))?,
            client_secret: env::var("GOOGLE_CLIENT_SECRET")
                .map_err(|_| anyhow::anyhow!("Missing GOOGLE_CLIENT_SECRET"))?,
            redirect_uri: env::var("OAUTH_REDIRECT_URL").unwrap_or_else(|_| {
                "https://linus-x1.bangus-firefighter.ts.net:8080/oauth/callback".to_string()
            }),
        };

        let microsoft = env::var("MICROSOFT_CLIENT_ID").ok().map(|client_id| {
            let client_secret = env::var("MICROSOFT_CLIENT_SECRET")
                .expect("MICROSOFT_CLIENT_SECRET required when MICROSOFT_CLIENT_ID is set");
            let tenant_id = env::var("MICROSOFT_TENANT_ID")
                .expect("MICROSOFT_TENANT_ID required when MICROSOFT_CLIENT_ID is set");
            let redirect_uri = env::var("MICROSOFT_OAUTH_REDIRECT_URL").unwrap_or_else(|_| {
                "https://linus-x1.bangus-firefighter.ts.net:8080/oauth/microsoft/callback"
                    .to_string()
            });
            MicrosoftOAuthConfig {
                client_id,
                client_secret,
                tenant_id,
                redirect_uri,
            }
        });

        // Default to the XDG cache dir (falls back to ~/.cache, then a local
        // dir); NixOS overrides this via EMBEDDING_CACHE_DIR → StateDirectory.
        let embedding_cache_dir = env::var("EMBEDDING_CACHE_DIR").unwrap_or_else(|_| {
            let base = env::var("XDG_CACHE_HOME")
                .or_else(|_| env::var("HOME").map(|h| format!("{h}/.cache")))
                .unwrap_or_else(|_| ".cache".to_string());
            format!("{base}/travelai/models/clip-multilingual")
        });
        let embedding_batch_size = env::var("EMBEDDING_BATCH_SIZE")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(32);

        let image_store_dir = env::var("IMAGE_STORE_DIR").unwrap_or_else(|_| {
            let base = env::var("XDG_CACHE_HOME")
                .or_else(|_| env::var("HOME").map(|h| format!("{h}/.cache")))
                .unwrap_or_else(|_| ".cache".to_string());
            format!("{base}/travelai/images")
        });
        let image_variant = env::var("IMAGE_VARIANT").unwrap_or_else(|_| "300x300".to_string());
        let image_download_concurrency = env::var("IMAGE_DOWNLOAD_CONCURRENCY")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(6);

        Ok(Self {
            database_url,
            google,
            microsoft,
            embedding_cache_dir,
            embedding_batch_size,
            image_store_dir,
            image_variant,
            image_download_concurrency,
        })
    }
}
