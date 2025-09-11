#!/bin/bash

# FastShare Installation Script
# This script builds and installs FastShare for easy access

set -e

echo "🚀 Building FastShare..."
cargo build --release

echo "📦 Creating installation directory..."
INSTALL_DIR="$HOME/.local/bin"
mkdir -p "$INSTALL_DIR"

echo "📋 Installing FastShare..."
cp target/release/fastshare "$INSTALL_DIR/"

# Add to PATH if not already there
if [[ ":$PATH:" != *":$INSTALL_DIR:"* ]]; then
    echo "🔧 Adding $INSTALL_DIR to PATH..."
    echo 'export PATH="$HOME/.local/bin:$PATH"' >> ~/.bashrc
    echo "Please run: source ~/.bashrc"
fi

echo "✅ FastShare installed successfully!"
echo "🎉 You can now use 'fastshare' from anywhere in your terminal"
echo ""
echo "Usage examples:"
echo "  fastshare send myfile.txt"
echo "  fastshare receive 192.168.1.100"
