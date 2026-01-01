#!/bin/bash
# Replicates the app's startup command logic to verify failure

# 1. Find system MariaDB
if [ -f "/opt/homebrew/opt/mariadb/bin/mariadbd" ]; then
    MARIADBD="/opt/homebrew/opt/mariadb/bin/mariadbd"
    SYSTEM_BASEDIR="/opt/homebrew/opt/mariadb"
elif [ -f "/usr/local/opt/mariadb/bin/mariadbd" ]; then
    MARIADBD="/usr/local/opt/mariadb/bin/mariadbd"
    SYSTEM_BASEDIR="/usr/local/opt/mariadb"
else
    echo "System MariaDB not found"
    exit 1
fi

echo "Found MariaDB at: $MARIADBD"
echo "System BaseDir: $SYSTEM_BASEDIR"

# 2. Simulate App Data Dir
APP_DATA_DIR="$HOME/Library/Application Support/org.booklore.runner_TEST"
MARIADB_DIR="$APP_DATA_DIR/mariadb"
DATA_DIR="$APP_DATA_DIR/data"
SOCKET_PATH="$APP_DATA_DIR/mysql.sock"

mkdir -p "$MARIADB_DIR" "$DATA_DIR"
rm -f "$SOCKET_PATH"

echo "---------------------------------------------------"
echo "TEST 1: Running with App-Local Basedir (Current Bug)"
echo "BaseDir: $MARIADB_DIR"
echo "DataDir: $DATA_DIR"
echo "---------------------------------------------------"

# Attempt start with WRONG basedir (simulating the bug)
"$MARIADBD" \
    --basedir="$MARIADB_DIR" \
    --datadir="$DATA_DIR" \
    --socket="$SOCKET_PATH" \
    --skip-networking \
    --skip-grant-tables \
    --port=0 \
    --console &

PID=$!
sleep 2

if ps -p $PID > /dev/null; then
    echo "SUCCESS: Process is running"
    kill $PID
else
    echo "FAILURE: Process died immediately"
fi

echo ""
echo "---------------------------------------------------"
echo "TEST 2: Running with Correct System Basedir (Fix)"
echo "BaseDir: $SYSTEM_BASEDIR"
echo "---------------------------------------------------"

# Attempt start with CORRECT system basedir
"$MARIADBD" \
    --basedir="$SYSTEM_BASEDIR" \
    --datadir="$DATA_DIR" \
    --socket="$SOCKET_PATH" \
    --skip-networking \
    --skip-grant-tables \
    --port=0 \
    --console &

PID=$!
sleep 2

if ps -p $PID > /dev/null; then
    echo "SUCCESS: Process is running"
    kill $PID
else
    echo "FAILURE: Process died immediately"
fi

# Cleanup
rm -rf "$APP_DATA_DIR"
