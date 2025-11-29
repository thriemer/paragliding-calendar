# TravelAI

Intelligent paragliding and outdoor adventure travel planning CLI

## Description

TravelAI is a command-line tool that helps outdoor adventure enthusiasts plan their trips by providing intelligent recommendations for paragliding sites, weather analysis, and travel optimization. The primary focus is on paragliding flyability analysis, but the system also supports comprehensive travel planning including accommodation and activity recommendations.

## Installation

### Prerequisites

- Rust 1.75 or later (using 2024 edition)
- Cargo (comes with Rust)

### Building from source

```bash
git clone <repository-url>
cd travelai
cargo build --release
```

The compiled binary will be available at `target/release/travelai`.

### Running

```bash
# Run directly with cargo
cargo run -- --help

# Or use the compiled binary
./target/release/travelai --help
```

## Usage

### Basic Commands

```bash
# Show help
travelai --help

# Get weather forecast for a location (placeholder - not yet implemented)
travelai weather --location "46.8,8.0"
travelai weather --location "Chamonix"

# Enable verbose output
travelai --verbose weather --location "Chamonix"
```

### Configuration

TravelAI uses a configuration file for API keys and default settings. The configuration file will be located at:

- Linux/macOS: `~/.config/travelai/config.toml`
- Windows: `%APPDATA%/travelai/config.toml`

Example configuration structure:

```toml
[weather]
api_key = "your_openweathermap_api_key"

[defaults]
search_radius_km = 50
max_sites = 10
```

## Development

### Running Tests

```bash
cargo test
```

### Code Formatting

```bash
cargo fmt
```

### Linting

```bash
cargo clippy
```

### Project Structure

```
src/
├── main.rs          # CLI entry point and command handling
├── lib.rs           # Core library with public API
└── error.rs         # Error types and handling
```

## Features

### Current Status

- ✅ Basic CLI framework with clap
- ✅ Error handling with user-friendly messages
- ✅ Project structure and configuration
- ⏳ Weather API integration (planned)
- ⏳ Paragliding site analysis (planned)
- ⏳ Travel planning recommendations (planned)

### Planned Features

- Weather forecast integration
- Paragliding flyability analysis
- Site recommendations based on weather conditions
- Travel route optimization
- Accommodation suggestions
- Activity recommendations for different weather conditions

## Contributing

1. Follow the established code style (rustfmt)
2. Ensure all tests pass (`cargo test`)
3. Run clippy and address any warnings (`cargo clippy`)
4. Add tests for new functionality

## License

MIT License - see LICENSE file for details.