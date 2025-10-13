#!/bin/bash

# Kill any existing saramcp processes
echo "🛑 Stopping any existing server instances..."
pkill -f "target/debug/saramcp" 2>/dev/null || true

# Give it a moment to shut down cleanly
sleep 1

# Ensure data directory exists
mkdir -p data

# Build the project
echo "🔨 Building project..."
cargo build

# Create test user if database exists
# Commented out for rapid iteration during development
# if [ -f "data/saramcp.db" ]; then
#     echo "👤 Ensuring test user exists..."
#     cargo run --bin cli -- user create --email jhiver@gmail.com --password azertyuiop --verified 2>/dev/null || true
# fi

# Run the development server
echo "🚀 Starting development server..."
echo "📌 Server will run on http://localhost:8080"
echo "📔 Test credentials: jhiver@gmail.com / azertyuiop"
echo ""
cargo run --bin saramcp