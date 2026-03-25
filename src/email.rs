use anyhow::{Context, Result};
use lettre::{
    Message, Transport, transport::smtp::SmtpTransport,
    transport::smtp::authentication::Credentials,
};
use std::env;

pub trait EmailProvider: Send + Sync + Clone {
    async fn send_mail(&self, to: &str, subject: &str, body: &str) -> Result<()>;
}

fn create_mailer() -> Result<SmtpTransport> {
    let gmail_address = env::var("GMAIL_ADDRESS").context("Missing GMAIL_ADDRESS env var")?;
    let gmail_app_password =
        env::var("GMAIL_APP_PASSWORD").context("Missing GMAIL_APP_PASSWORD env var")?;

    let credentials = Credentials::new(gmail_address, gmail_app_password);

    let mailer = SmtpTransport::relay("smtp.gmail.com")?
        .credentials(credentials)
        .build();

    Ok(mailer)
}

pub async fn send_auth_link(url: &str) -> Result<()> {
    let notification_email =
        env::var("NOTIFICATION_EMAIL").context("Missing NOTIFICATION_EMAIL env var")?;
    let gmail_address = env::var("GMAIL_ADDRESS").context("Missing GMAIL_ADDRESS env var")?;

    let email = Message::builder()
        .from(
            format!("TravelAI <{}>", gmail_address)
                .parse()
                .context("Failed to parse from address")?,
        )
        .to(
            notification_email
                .parse()
                .context("Failed to parse to address")?,
        )
        .subject("Google Calendar Authentication Link")
        .body(format!(
            "Click the following link to authenticate with Google Calendar:\n\n{}\n\nAfter clicking, grant permissions and you'll be redirected back.",
            url
        ))?;

    let mailer = create_mailer()?;

    mailer.send(&email).context("Failed to send email")?;

    tracing::info!("Sent authentication link email to {}", notification_email);

    Ok(())
}

#[derive(Clone)]
pub struct GmailEmailProvider {
    gmail_address: String,
    gmail_app_password: String,
    notification_email: String,
}

impl GmailEmailProvider {
    pub fn new() -> Result<Self> {
        Ok(Self {
            gmail_address: env::var("GMAIL_ADDRESS").context("Missing GMAIL_ADDRESS env var")?,
            gmail_app_password: env::var("GMAIL_APP_PASSWORD")
                .context("Missing GMAIL_APP_PASSWORD env var")?,
            notification_email: env::var("NOTIFICATION_EMAIL")
                .context("Missing NOTIFICATION_EMAIL env var")?,
        })
    }
}

impl EmailProvider for GmailEmailProvider {
    async fn send_mail(&self, to: &str, subject: &str, body: &str) -> Result<()> {
        let credentials =
            Credentials::new(self.gmail_address.clone(), self.gmail_app_password.clone());

        let mailer = SmtpTransport::relay("smtp.gmail.com")?
            .credentials(credentials)
            .build();

        let email = Message::builder()
            .from(
                format!("TravelAI <{}>", self.gmail_address)
                    .parse()
                    .context("Failed to parse from address")?,
            )
            .to(to.parse().context("Failed to parse to address")?)
            .subject(subject)
            .body(body.to_string())?;

        mailer.send(&email).context("Failed to send email")?;

        tracing::info!("Sent email to {}", to);

        Ok(())
    }
}
