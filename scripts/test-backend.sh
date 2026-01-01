#!/bin/bash
# Integration test for Backend startup flow
# Replicates exactly what the Tauri app does

set -e

echo "=== BookLore Backend Integration Test ==="
echo ""

# Configuration
APP_DATA_DIR="$HOME/Library/Application Support/BookLore"
SOCKET_PATH="$APP_DATA_DIR/mysql.sock"
JAR_PATH="$PWD/src-tauri/resources/booklore-api.jar"
CONFIG_DIR="$APP_DATA_DIR/config"
BOOKS_DIR="$APP_DATA_DIR/books"
BOOKDROP_DIR="$APP_DATA_DIR/bookdrop"
BACKEND_PORT=8080
LOG_FILE="$APP_DATA_DIR/backend.log"

echo "Configuration:"
echo "  APP_DATA_DIR: $APP_DATA_DIR"
echo "  SOCKET_PATH: $SOCKET_PATH"
echo "  JAR_PATH: $JAR_PATH"
echo "  BACKEND_PORT: $BACKEND_PORT"
echo ""

# Step 1: Find Java
echo "=== Step 1: Finding Java ==="

JAVA_PATH=""

# Try java_home
if [ -x "/usr/libexec/java_home" ]; then
    JAVA_HOME_PATH=$(/usr/libexec/java_home -v 21 2>/dev/null || echo "")
    if [ -n "$JAVA_HOME_PATH" ] && [ -f "$JAVA_HOME_PATH/bin/java" ]; then
        JAVA_PATH="$JAVA_HOME_PATH/bin/java"
        echo "  Found via java_home: $JAVA_PATH"
    fi
fi

# Fallback to PATH
if [ -z "$JAVA_PATH" ]; then
    JAVA_PATH=$(which java 2>/dev/null || echo "")
    if [ -n "$JAVA_PATH" ]; then
        echo "  Found in PATH: $JAVA_PATH"
    fi
fi

if [ -z "$JAVA_PATH" ]; then
    echo "  ERROR: Java not found!"
    exit 1
fi

# Verify version
echo "  Java version:"
"$JAVA_PATH" -version 2>&1 | head -3 | sed 's/^/    /'
echo ""

# Step 2: Check JAR
echo "=== Step 2: Checking JAR ==="
if [ -f "$JAR_PATH" ]; then
    echo "  JAR found: $JAR_PATH"
    echo "  Size: $(du -h "$JAR_PATH" | cut -f1)"
else
    echo "  ERROR: JAR not found at $JAR_PATH"
    exit 1
fi
echo ""

# Step 3: Check MariaDB socket
echo "=== Step 3: Checking MariaDB Socket ==="
if [ -S "$SOCKET_PATH" ]; then
    echo "  Socket exists: $SOCKET_PATH"
else
    echo "  WARNING: Socket not found! MariaDB may not be running."
    echo "  Run ./scripts/test-mariadb.sh first or start the app"
fi
echo ""

# Step 4: Create directories
echo "=== Step 4: Creating Directories ==="
mkdir -p "$CONFIG_DIR" "$BOOKS_DIR" "$BOOKDROP_DIR"
echo "  Done"
echo ""

# Step 5: Build database URL
echo "=== Step 5: Building Database URL ==="
DATABASE_URL="jdbc:mariadb://127.0.0.1:13306/booklore?createDatabaseIfNotExist=true"
echo "  URL: $DATABASE_URL"
echo ""

# Step 6: Start Backend
echo "=== Step 6: Starting Backend ==="

JAVA_HOME_DIR=$(dirname $(dirname "$JAVA_PATH"))

echo "  Command: $JAVA_PATH"
echo "    -Xmx512m -Xms128m"
echo "    -Dapp.path-config=$CONFIG_DIR"
echo "    -Dapp.bookdrop-folder=$BOOKDROP_DIR"
echo "    -Dserver.port=$BACKEND_PORT"
echo "    -jar $JAR_PATH"
echo ""

export JAVA_HOME="$JAVA_HOME_DIR"
export DATABASE_URL="$DATABASE_URL"
export DATABASE_USERNAME="root"
export DATABASE_PASSWORD=""
export BOOKLORE_PORT="$BACKEND_PORT"

"$JAVA_PATH" \
    -Xmx512m \
    -Xms128m \
    -Dapp.path-config="$CONFIG_DIR" \
    -Dapp.bookdrop-folder="$BOOKDROP_DIR" \
    -Dserver.port="$BACKEND_PORT" \
    -jar "$JAR_PATH" \
    > "$LOG_FILE" 2>&1 &

BACKEND_PID=$!
echo "  Started with PID: $BACKEND_PID"
echo ""

# Step 7: Wait for health check
echo "=== Step 7: Waiting for Health Check ==="

HEALTH_URL="http://localhost:$BACKEND_PORT/api/v1/healthcheck"
MAX_ATTEMPTS=60

for i in $(seq 1 $MAX_ATTEMPTS); do
    RESPONSE=$(curl -s -o /dev/null -w "%{http_code}" "$HEALTH_URL" 2>/dev/null || echo "000")
    
    if [ "$RESPONSE" = "200" ]; then
        echo "  Attempt $i: SUCCESS (HTTP 200)"
        echo ""
        
        # Cleanup
        echo "=== Cleanup ==="
        echo "  Stopping backend (PID: $BACKEND_PID)..."
        kill $BACKEND_PID 2>/dev/null || true
        wait $BACKEND_PID 2>/dev/null || true
        echo "  Done"
        echo ""
        
        echo "=== TEST PASSED ==="
        echo ""
        echo "Last 50 lines of backend log:"
        echo "---"
        tail -50 "$LOG_FILE"
        echo "---"
        exit 0
    else
        if [ $((i % 5)) -eq 0 ]; then
            echo "  Attempt $i: HTTP $RESPONSE (waiting...)"
        fi
    fi
    
    # Check if process died
    if ! ps -p $BACKEND_PID > /dev/null 2>&1; then
        echo "  Attempt $i: Process DIED!"
        break
    fi
    
    sleep 1
done

echo ""
echo "=== TEST FAILED: Timeout waiting for backend ==="
echo ""
echo "Process status:"
if ps -p $BACKEND_PID > /dev/null 2>&1; then
    echo "  Backend process is still running (PID: $BACKEND_PID)"
    kill $BACKEND_PID 2>/dev/null || true
else
    echo "  Backend process has DIED"
fi
echo ""
echo "Backend log file contents (last 100 lines):"
echo "---"
tail -100 "$LOG_FILE" 2>&1 || echo "  (log file not found)"
echo "---"

exit 1
