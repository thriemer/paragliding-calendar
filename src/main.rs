use anyhow::Result;
use clap::{Parser, Subcommand};

mod error;
pub use error::TravelAiError;

#[derive(Parser)]
#[command(name = "travelai")]
#[command(about = "Intelligent paragliding and outdoor adventure travel planning CLI")]
#[command(version = "0.1.0")]
pub struct Cli {
    #[command(subcommand)]
    pub command: Option<Commands>,

    /// Enable verbose output
    #[arg(short, long)]
    pub verbose: bool,
}

#[derive(Subcommand)]
pub enum Commands {
    /// Weather forecast for a location (placeholder for future implementation)
    Weather {
        /// Location (coordinates, city name, or postal code)
        #[arg(short, long)]
        location: String,
    },
}

#[allow(clippy::unnecessary_wraps)]
fn main() -> Result<()> {
    let cli = Cli::parse();

    if let Err(e) = run_cli(&cli) {
        // Display user-friendly error message
        if let Some(travel_err) = e.downcast_ref::<TravelAiError>() {
            eprintln!("Error: {}", travel_err.user_message());
        } else {
            eprintln!("Error: {e}");
        }
        std::process::exit(1);
    }

    Ok(())
}

fn run_cli(cli: &Cli) -> Result<()> {
    match &cli.command {
        Some(Commands::Weather { location }) => {
            if location.is_empty() {
                return Err(TravelAiError::validation("Location cannot be empty").into());
            }

            if cli.verbose {
                println!("Fetching weather for: {location}");
            }
            println!("Weather command not yet implemented for location: {location}");
            Ok(())
        }
        None => {
            println!("TravelAI - Intelligent paragliding and outdoor adventure travel planning");
            println!("Use --help for available commands");
            Ok(())
        }
    }
}
