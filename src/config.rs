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
    #[allow(dead_code)]
    pub brouter_base_url: String,
    #[allow(dead_code)]
    pub valhalla_base_url: String,
    pub google: GoogleOAuthConfig,
    pub microsoft: Option<MicrosoftOAuthConfig>,
}

impl AppConfig {
    pub fn from_env() -> Result<Self> {
        let database_url = env::var("DATABASE_URL")
            .map_err(|_| anyhow::anyhow!("DATABASE_URL must be set"))?;

        let brouter_base_url =
            env::var("BROUTER_BASE_URL").map_err(|_| anyhow::anyhow!("Missing BROUTER_BASE_URL"))?;

        let valhalla_base_url = env::var("VALHALLA_BASE_URL")
            .map_err(|_| anyhow::anyhow!("Missing VALHALLA_BASE_URL"))?;

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

        Ok(Self {
            database_url,
            brouter_base_url,
            valhalla_base_url,
            google,
            microsoft,
        })
    }
}
