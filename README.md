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

TravelAI uses a configuration file for API keys and default settings. To set up configuration:

1. **Copy the sample configuration:**
   ```bash
   # Create config directory
   mkdir -p ~/.config/travelai
   
   # Copy sample configuration
   cp config/default.toml ~/.config/travelai/config.toml
   ```

2. **Edit the configuration file:**
   ```bash
   # Open in your preferred editor
   nano ~/.config/travelai/config.toml
   ```

3. **Set your OpenWeatherMap API key:**
   - Get a free API key at: https://openweathermap.org/api
   - Replace `your_openweathermap_api_key_here` with your actual API key

#### Configuration Locations

- Linux/macOS: `~/.config/travelai/config.toml`
- Windows: `%APPDATA%/travelai/config.toml`

#### Environment Variable Override

You can override any configuration setting using environment variables with the `TRAVELAI_` prefix:

```bash
# Set API key via environment variable
export TRAVELAI_WEATHER__API_KEY="your_api_key_here"

# Set cache TTL via environment variable  
export TRAVELAI_CACHE__TTL_HOURS=12

# Set log level
export TRAVELAI_LOGGING__LEVEL=debug
```

#### Configuration Structure

See `config/default.toml` for the complete configuration with detailed comments. Key sections:

- `[weather]` - OpenWeatherMap API settings
- `[cache]` - Data caching configuration  
- `[logging]` - Log level and format settings
- `[defaults]` - Application default values

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