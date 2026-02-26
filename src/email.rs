use anyhow::{Context, Result};
use lettre::{
    Message, Transport, transport::smtp::SmtpTransport,
    transport::smtp::authentication::Credentials,
};
use std::env;

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

pub async fn send_device_auth(verification_url: &str, user_code: &str) -> Result<()> {
    let notification_email =
        env::var("NOTIFICATION_EMAIL").context("Missing NOTIFICATION_EMAIL env var")?;
    let gmail_address = env::var("GMAIL_ADDRESS").context("Missing GMAIL_ADDRESS env var")?;

    let email = Message::builder()
        .from(
            format!("TravelAI <{}>", gmail_address)
                .parse()
                .context("Failed to parse from address")?,
        )
        .to(notification_email
            .parse()
            .context("Failed to parse to address")?)
        .subject("Google Calendar Device Authentication")
        .body(format!(
            "To authenticate with Google Calendar:\n\n\
1. Visit this URL: {}\n\
2. Enter this code: {}\n\
3. Grant permissions\n\n\
The code will expire in a few minutes.",
            verification_url, user_code
        ))?;

    let mailer = create_mailer()?;

    mailer
        .send(&email)
        .context("Failed to send device auth email")?;

    tracing::info!(
        "Sent device authentication email to {} with code {}",
        notification_email,
        user_code
    );

    Ok(())
}
