#!/bin/bash
# Build BookLore Frontend
# Requires: Node.js 20+

set -e

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_DIR="$(dirname "$SCRIPT_DIR")"
UPSTREAM_DIR="$PROJECT_DIR/booklore-upstream"
OUTPUT_DIR="$PROJECT_DIR/src-tauri/resources/frontend"

echo "🎨 Building BookLore Frontend..."
echo "   Source: $UPSTREAM_DIR/booklore-ui"
echo "   Output: $OUTPUT_DIR"

# Check if upstream exists
if [ ! -d "$UPSTREAM_DIR/booklore-ui" ]; then
    echo "❌ Error: booklore-upstream not found!"
    echo "   Run: git submodule update --init"
    exit 1
fi

# Create output directory
mkdir -p "$OUTPUT_DIR"

# Build the frontend
cd "$UPSTREAM_DIR/booklore-ui"

echo "📦 Installing dependencies..."
npm ci

echo "🔨 Building Angular app..."
npm run build -- --configuration=production

# Copy the build output
if [ -d "dist/booklore/browser" ]; then
    cp -r dist/booklore/browser/* "$OUTPUT_DIR/"
elif [ -d "dist/booklore" ]; then
    cp -r dist/booklore/* "$OUTPUT_DIR/"
else
    echo "❌ Error: Build output not found!"
    exit 1
fi

echo "✅ Frontend copied to $OUTPUT_DIR"

echo ""
echo "🎉 Frontend build complete!"
