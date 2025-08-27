#!/bin/bash

# Build script for WASM web showcase

set -e

echo "🔨 Building WASM showcase..."

# Install dependencies if needed
if [ ! -d "node_modules" ]; then
    echo "📦 Installing dependencies..."
    npm install
fi

# Install wasm-pack if not present
if ! command -v wasm-pack &> /dev/null; then
    echo "📦 Installing wasm-pack..."
    curl https://rustwasm.github.io/wasm-pack/installer/init.sh -sSf | sh
fi

# Build CSS with Tailwind
echo "🎨 Building CSS..."
npx tailwindcss -i ./styles.css -o ./dist/styles.css --minify

# Build the WASM module
echo "🦀 Building Rust to WASM..."
wasm-pack build --target web --out-dir pkg

echo "✅ Build complete!"
echo ""
echo "To run the showcase:"
echo "  1. Start a local server: python3 -m http.server 8000"
echo "  2. Open http://localhost:8000 in your browser"