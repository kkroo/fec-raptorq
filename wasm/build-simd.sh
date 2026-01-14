#!/bin/bash
# Build RaptorQ WASM with SIMD128 support
#
# This script builds the WASM bindings with WASM SIMD128 acceleration enabled.
# SIMD128 provides 16-byte vector operations similar to SSE/NEON on native platforms.

set -e

echo "Building RaptorQ WASM with SIMD128 support..."
echo ""

# Check for required tools
if ! command -v wasm-pack &> /dev/null; then
    echo "❌ Error: wasm-pack not found"
    echo "Install with: curl https://rustwasm.github.io/wasm-pack/installer/init.sh -sSf | sh"
    exit 1
fi

# Check Rust version
RUST_VERSION=$(rustc --version | cut -d' ' -f2)
echo "✓ Rust version: $RUST_VERSION"

# Set RUSTFLAGS to enable SIMD128
export RUSTFLAGS="-C target-feature=+simd128"

echo "✓ SIMD128 target feature enabled"
echo ""

# Build with wasm-pack
echo "Building WASM package..."
wasm-pack build --target web --release

# Check if build was successful
if [ $? -eq 0 ]; then
    echo ""
    echo "✓ Build successful!"
    echo ""
    echo "Output in pkg/"
    echo ""
    
    # Show file sizes
    if [ -f "pkg/raptorq_wasm_bg.wasm" ]; then
        WASM_SIZE=$(du -h pkg/raptorq_wasm_bg.wasm | cut -f1)
        echo "WASM binary size: $WASM_SIZE"
        
        # Check if SIMD is actually enabled
        if command -v wasm-objdump &> /dev/null; then
            echo ""
            echo "Checking for SIMD instructions..."
            if wasm-objdump -d pkg/raptorq_wasm_bg.wasm | grep -q "v128"; then
                echo "✓ SIMD128 instructions found in binary"
            else
                echo "⚠ Warning: No SIMD128 instructions found"
            fi
        fi
    fi
    
    echo ""
    echo "Usage:"
    echo "  Copy pkg/ contents to your web application"
    echo "  Import with: import init, { RaptorQDecoder } from './pkg/raptorq_wasm.js';"
else
    echo ""
    echo "❌ Build failed"
    exit 1
fi






