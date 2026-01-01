# BookLore Runner

Native macOS application wrapper for [BookLore](https://github.com/booklore-app/booklore) - a self-hosted digital library with smart shelves, auto metadata, Kobo & KOReader sync, and more.

## Features

- 📦 **One-click install** - No Docker required, just drag to Applications
- 🔄 **Auto-update** - Stays up to date with latest BookLore releases
- 🍎 **Native macOS experience** - Menubar icon, notifications, launch at login
- 💾 **Embedded database** - MariaDB bundled for full compatibility
- ☕ **Java auto-download** - Downloads JRE on first launch if needed

## System Requirements

- macOS 14 (Sonoma) or later
- Apple Silicon (M1/M2/M3)
- ~500MB disk space

## Installation

1. Download the latest `.dmg` from [Releases](https://github.com/booklore-app/booklore-runner/releases)
2. Open the DMG and drag BookLore to Applications
3. Launch BookLore from Applications
4. On first launch:
   - MariaDB will be initialized (~30 seconds)
   - Java runtime will be downloaded if needed (~150MB)
   - BookLore backend will start automatically

## Data Location

All data is stored in `~/Library/Application Support/BookLore/`:
- `data/` - MariaDB database files
- `books/` - Your book library
- `bookdrop/` - Auto-import folder
- `jre/` - Java runtime
- `config/` - Application settings

## Development

### Prerequisites

- Node.js 22+
- Rust 1.75+
- Java 21 (for building backend)

### Setup

```bash
# Clone with submodules
git clone --recursive https://github.com/booklore-app/booklore-runner.git
cd booklore-runner

# Install dependencies
npm install

# Build BookLore backend
./scripts/build-backend.sh

# Build BookLore frontend
./scripts/build-frontend.sh

# Download MariaDB binaries
./scripts/download-mariadb.sh

# Run in development mode
npm run dev

# Build for release
npm run build
```

### Project Structure

```
booklore-runner/
├── src/                    # Launcher UI (loading screen)
├── src-tauri/              # Rust/Tauri backend
│   ├── src/
│   │   ├── main.rs         # Entry point
│   │   ├── jre.rs          # JRE download/management
│   │   ├── mariadb.rs      # Embedded MariaDB
│   │   ├── backend.rs      # Spring Boot launcher
│   │   └── tray.rs         # System tray
│   └── resources/          # Bundled resources
├── scripts/                # Build scripts
├── booklore-upstream/      # BookLore source (submodule)
└── .github/workflows/      # CI/CD
```

## License

GPL-3.0 - Same as BookLore

## Acknowledgments

- [BookLore](https://github.com/booklore-app/booklore) - The amazing library app this wraps
- [Tauri](https://tauri.app) - Native app framework
- [Eclipse Adoptium](https://adoptium.net) - Java runtime
