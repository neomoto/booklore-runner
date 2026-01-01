#!/bin/bash
# Build BookLore Backend JAR
# Requires: Java 21, Gradle

set -e

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_DIR="$(dirname "$SCRIPT_DIR")"
UPSTREAM_DIR="$PROJECT_DIR/booklore-upstream"
OUTPUT_DIR="$PROJECT_DIR/src-tauri/resources"

echo "📦 Building BookLore Backend..."
echo "   Source: $UPSTREAM_DIR/booklore-api"
echo "   Output: $OUTPUT_DIR"

# Check if upstream exists
if [ ! -d "$UPSTREAM_DIR/booklore-api" ]; then
    echo "❌ Error: booklore-upstream not found!"
    echo "   Run: git submodule update --init"
    exit 1
fi

# Create output directory
mkdir -p "$OUTPUT_DIR"

# Build the JAR
cd "$UPSTREAM_DIR/booklore-api"

echo "🔨 Running Gradle build..."
./gradlew clean build -x test --no-daemon

# Copy the JAR
JAR_FILE=$(find build/libs -name "*.jar" -type f | head -1)
if [ -z "$JAR_FILE" ]; then
    echo "❌ Error: JAR file not found!"
    exit 1
fi

cp "$JAR_FILE" "$OUTPUT_DIR/booklore-api.jar"
echo "✅ Backend JAR copied to $OUTPUT_DIR/booklore-api.jar"

echo ""
echo "🎉 Backend build complete!"
