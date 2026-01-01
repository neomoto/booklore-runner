#!/bin/bash
# Download and bundle MariaDB for macOS ARM64 using Homebrew
# This creates a portable bundle by fixing dylib paths

set -e

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_DIR="$(dirname "$SCRIPT_DIR")"
OUTPUT_DIR="$PROJECT_DIR/src-tauri/resources/mariadb"

echo "🍺 Installing MariaDB via Homebrew..."
# Ensure existing installs don't conflict (runner might have it)
brew_prefix=$(brew --prefix)
export PATH="$brew_prefix/bin:$PATH"

if ! brew list mariadb &>/dev/null; then
    brew install mariadb
fi

# We also need dylibbundler to make it portable
if ! brew list dylibbundler &>/dev/null; then
    brew install dylibbundler
fi

MARIADB_PREFIX=$(brew --prefix mariadb)
echo "✅ MariaDB found at: $MARIADB_PREFIX"

# Create output directory
rm -rf "$OUTPUT_DIR"
mkdir -p "$OUTPUT_DIR/bin"
mkdir -p "$OUTPUT_DIR/lib"
mkdir -p "$OUTPUT_DIR/scripts"
mkdir -p "$OUTPUT_DIR/share"

echo "📁 Copying binaries..."
# Copy main binaries
cp "$MARIADB_PREFIX/bin/mariadbd" "$OUTPUT_DIR/bin/"
cp "$MARIADB_PREFIX/bin/mariadb" "$OUTPUT_DIR/bin/"
cp "$MARIADB_PREFIX/bin/mariadb-install-db" "$OUTPUT_DIR/scripts/" 2>/dev/null || cp "$MARIADB_PREFIX/scripts/mariadb-install-db" "$OUTPUT_DIR/scripts/"

# Make executable
chmod +x "$OUTPUT_DIR/bin/"*
chmod +x "$OUTPUT_DIR/scripts/"*

echo "🔧 Fixing dynamic libraries for portability..."
# Use dylibbundler to copy dependencies and rewrite paths to be relative
dylibbundler -od -b -x "$OUTPUT_DIR/bin/mariadbd" -d "$OUTPUT_DIR/lib/" -p "@executable_path/../lib"
dylibbundler -od -b -x "$OUTPUT_DIR/bin/mariadb" -d "$OUTPUT_DIR/lib/" -p "@executable_path/../lib"

echo "📁 Copying support files..."
cp -r "$MARIADB_PREFIX/share/mariadb/"* "$OUTPUT_DIR/share/" 2>/dev/null || cp -r "$MARIADB_PREFIX/share/mysql/"* "$OUTPUT_DIR/share/"

# Create a minimal my.cnf for the embedded instance?
# Not strictly necessary as we pass flags via CLI, but improved portability.

# Calculate size
SIZE=$(du -sh "$OUTPUT_DIR" | cut -f1)
echo "✅ MariaDB bundled to $OUTPUT_DIR ($SIZE)"

echo ""
echo "🎉 MariaDB bundling complete!"
