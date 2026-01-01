#!/bin/bash
# Integration test for MariaDB startup flow
# Replicates exactly what the Tauri app does

set -e

echo "=== BookLore MariaDB Integration Test ==="
echo ""

# Configuration - same as app
APP_DATA_DIR="$HOME/Library/Application Support/BookLore"
DATA_DIR="$APP_DATA_DIR/data"
SOCKET_PATH="$APP_DATA_DIR/mysql.sock"
LOG_FILE="$APP_DATA_DIR/mariadb.log"

echo "Configuration:"
echo "  APP_DATA_DIR: $APP_DATA_DIR"
echo "  DATA_DIR: $DATA_DIR"
echo "  SOCKET_PATH: $SOCKET_PATH"
echo "  LOG_FILE: $LOG_FILE"
echo ""

# Step 1: Find system MariaDB (same logic as Rust code)
echo "=== Step 1: Finding System MariaDB ==="

MARIADBD=""
BASEDIR=""
MARIADB_CLIENT=""
INSTALL_DB=""

# Try brew --prefix first
if command -v brew &> /dev/null; then
    BREW_PREFIX=$(brew --prefix mariadb 2>/dev/null || echo "")
    if [ -n "$BREW_PREFIX" ] && [ -f "$BREW_PREFIX/bin/mariadbd" ]; then
        MARIADBD="$BREW_PREFIX/bin/mariadbd"
        BASEDIR="$BREW_PREFIX"
        MARIADB_CLIENT="$BREW_PREFIX/bin/mariadb"
        INSTALL_DB="$BREW_PREFIX/bin/mariadb-install-db"
        echo "  Found via brew --prefix: $MARIADBD"
    fi
fi

# Fallback to common paths
if [ -z "$MARIADBD" ]; then
    for path in "/opt/homebrew/opt/mariadb" "/usr/local/opt/mariadb"; do
        if [ -f "$path/bin/mariadbd" ]; then
            MARIADBD="$path/bin/mariadbd"
            BASEDIR="$path"
            MARIADB_CLIENT="$path/bin/mariadb"
            INSTALL_DB="$path/bin/mariadb-install-db"
            echo "  Found at fallback path: $MARIADBD"
            break
        fi
    done
fi

if [ -z "$MARIADBD" ]; then
    echo "  ERROR: MariaDB not found!"
    echo "  Please install via: brew install mariadb"
    exit 1
fi

echo "  MARIADBD: $MARIADBD"
echo "  BASEDIR: $BASEDIR"
echo "  MARIADB_CLIENT: $MARIADB_CLIENT"
echo "  INSTALL_DB: $INSTALL_DB"
echo ""

# Step 2: Check if database is initialized
echo "=== Step 2: Checking Database Initialization ==="

mkdir -p "$DATA_DIR"
mkdir -p "$APP_DATA_DIR"

if [ -d "$DATA_DIR/mysql" ]; then
    echo "  Database already initialized (mysql folder exists)"
else
    echo "  Database NOT initialized, running mariadb-install-db..."
    
    if [ ! -f "$INSTALL_DB" ]; then
        echo "  ERROR: mariadb-install-db not found at $INSTALL_DB"
        exit 1
    fi
    
    echo "  Running: $INSTALL_DB --basedir=$BASEDIR --datadir=$DATA_DIR --auth-root-authentication-method=normal"
    
    "$INSTALL_DB" \
        --basedir="$BASEDIR" \
        --datadir="$DATA_DIR" \
        --auth-root-authentication-method=normal
    
    if [ $? -eq 0 ]; then
        echo "  Database initialized successfully"
    else
        echo "  ERROR: Database initialization failed!"
        exit 1
    fi
fi
echo ""

# Step 3: Clean up old socket
echo "=== Step 3: Cleaning Up Old Socket ==="
if [ -S "$SOCKET_PATH" ] || [ -e "$SOCKET_PATH" ]; then
    echo "  Removing old socket file..."
    rm -f "$SOCKET_PATH"
fi
echo "  Done"
echo ""

# Step 4: Start MariaDB
echo "=== Step 4: Starting MariaDB ==="

echo "  Command: $MARIADBD"
echo "    --basedir=$BASEDIR"
echo "    --datadir=$DATA_DIR"
echo "    --socket=$SOCKET_PATH"
echo "    --skip-networking"
echo "    --skip-grant-tables"
echo "    --port=0"
echo ""

# Start in background, redirect output to log
"$MARIADBD" \
    --basedir="$BASEDIR" \
    --datadir="$DATA_DIR" \
    --socket="$SOCKET_PATH" \
    --skip-networking \
    --skip-grant-tables \
    --port=0 \
    > "$LOG_FILE" 2>&1 &

MARIADB_PID=$!
echo "  Started with PID: $MARIADB_PID"
echo ""

# Step 5: Wait for socket (same logic as Rust code)
echo "=== Step 5: Waiting for Socket ==="

MAX_ATTEMPTS=60
for i in $(seq 1 $MAX_ATTEMPTS); do
    if [ -S "$SOCKET_PATH" ]; then
        echo "  Attempt $i: Socket file exists, testing connection..."
        
        # Try to connect
        RESULT=$("$MARIADB_CLIENT" --socket="$SOCKET_PATH" -e "SELECT 1" 2>&1) && {
            echo "  SUCCESS: Connection established!"
            echo ""
            
            # Step 6: Create database
            echo "=== Step 6: Creating booklore Database ==="
            "$MARIADB_CLIENT" --socket="$SOCKET_PATH" -e \
                "CREATE DATABASE IF NOT EXISTS booklore CHARACTER SET utf8mb4 COLLATE utf8mb4_unicode_ci" 2>&1 || true
            echo "  Database ready"
            echo ""
            
            # Cleanup
            echo "=== Cleanup ==="
            echo "  Stopping MariaDB (PID: $MARIADB_PID)..."
            kill $MARIADB_PID 2>/dev/null || true
            wait $MARIADB_PID 2>/dev/null || true
            rm -f "$SOCKET_PATH"
            echo "  Done"
            echo ""
            
            echo "=== TEST PASSED ==="
            echo ""
            echo "MariaDB log file contents:"
            echo "---"
            cat "$LOG_FILE"
            echo "---"
            exit 0
        }
        
        echo "  Attempt $i: Socket exists but connection failed: $RESULT"
    else
        if [ $((i % 5)) -eq 0 ]; then
            echo "  Attempt $i: Socket file not found yet"
        fi
    fi
    
    sleep 1
done

echo ""
echo "=== TEST FAILED: Timeout waiting for MariaDB ==="
echo ""
echo "Process status:"
if ps -p $MARIADB_PID > /dev/null 2>&1; then
    echo "  MariaDB process is still running (PID: $MARIADB_PID)"
else
    echo "  MariaDB process has DIED"
fi
echo ""
echo "Socket file:"
ls -la "$SOCKET_PATH" 2>&1 || echo "  Socket file does not exist"
echo ""
echo "MariaDB log file contents:"
echo "---"
cat "$LOG_FILE" 2>&1 || echo "  (log file not found)"
echo "---"
echo ""

# Cleanup
kill $MARIADB_PID 2>/dev/null || true
rm -f "$SOCKET_PATH"

exit 1
