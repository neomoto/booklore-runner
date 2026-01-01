#!/bin/bash
# Download MariaDB for macOS ARM64
# This downloads and extracts MariaDB server binaries for bundling

set -e

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_DIR="$(dirname "$SCRIPT_DIR")"
OUTPUT_DIR="$PROJECT_DIR/src-tauri/resources/mariadb"

MARIADB_VERSION="11.4.5"
DOWNLOAD_URL="https://archive.mariadb.org/mariadb-${MARIADB_VERSION}/bintar-darwin-arm64/mariadb-${MARIADB_VERSION}-darwin-arm64.tar.gz"

echo "🗄️  Downloading MariaDB ${MARIADB_VERSION}..."
echo "   URL: $DOWNLOAD_URL"
echo "   Output: $OUTPUT_DIR"

# Create temp directory
TEMP_DIR=$(mktemp -d)
trap "rm -rf $TEMP_DIR" EXIT

# Download
echo "⬇️  Downloading..."
curl -L -o "$TEMP_DIR/mariadb.tar.gz" "$DOWNLOAD_URL"

# Extract
echo "📦 Extracting..."
cd "$TEMP_DIR"
tar -xzf mariadb.tar.gz

# Find extracted directory
MARIADB_DIR=$(find . -maxdepth 1 -type d -name "mariadb-*" | head -1)
if [ -z "$MARIADB_DIR" ]; then
    echo "❌ Error: MariaDB directory not found!"
    exit 1
fi

# Create output directory
rm -rf "$OUTPUT_DIR"
mkdir -p "$OUTPUT_DIR"

# Copy only necessary files (skip debug symbols and extra stuff)
echo "📁 Copying essential files..."

# Copy bin directory (executables)
mkdir -p "$OUTPUT_DIR/bin"
cp "$MARIADB_DIR/bin/mariadbd" "$OUTPUT_DIR/bin/" 2>/dev/null || cp "$MARIADB_DIR/bin/mysqld" "$OUTPUT_DIR/bin/mariadbd"
cp "$MARIADB_DIR/bin/mariadb" "$OUTPUT_DIR/bin/" 2>/dev/null || cp "$MARIADB_DIR/bin/mysql" "$OUTPUT_DIR/bin/mariadb"
cp "$MARIADB_DIR/bin/mariadb-install-db" "$OUTPUT_DIR/bin/" 2>/dev/null || true
cp "$MARIADB_DIR/bin/mysql_install_db" "$OUTPUT_DIR/bin/" 2>/dev/null || true
cp "$MARIADB_DIR/bin/resolveip" "$OUTPUT_DIR/bin/" 2>/dev/null || true

# Copy scripts directory if exists
if [ -d "$MARIADB_DIR/scripts" ]; then
    cp -r "$MARIADB_DIR/scripts" "$OUTPUT_DIR/"
fi

# Copy share directory (system tables, charsets, etc.)
if [ -d "$MARIADB_DIR/share" ]; then
    cp -r "$MARIADB_DIR/share" "$OUTPUT_DIR/"
fi

# Copy support-files if exists
if [ -d "$MARIADB_DIR/support-files" ]; then
    mkdir -p "$OUTPUT_DIR/support-files"
    cp -r "$MARIADB_DIR/support-files"/* "$OUTPUT_DIR/support-files/" 2>/dev/null || true
fi

# Make binaries executable
chmod +x "$OUTPUT_DIR/bin/"* 2>/dev/null || true
chmod +x "$OUTPUT_DIR/scripts/"* 2>/dev/null || true

# Calculate size
SIZE=$(du -sh "$OUTPUT_DIR" | cut -f1)
echo "✅ MariaDB extracted to $OUTPUT_DIR ($SIZE)"

echo ""
echo "🎉 MariaDB download complete!"
