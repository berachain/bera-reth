#!/bin/bash

# Test script to verify all build targets work correctly
# This helps ensure the CI/CD workflow will work properly

set -e

echo "🔨 Testing Bera-Reth Build Targets"
echo "=================================="
echo ""

# Colors for output
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
NC='\033[0m' # No Color

# Function to test a build target
test_build_target() {
    local target=$1
    local description=$2
    
    echo -n "Testing $description... "
    
    if make "build-$target" PROFILE=release > /dev/null 2>&1; then
        echo -e "${GREEN}✅ PASS${NC}"
        return 0
    else
        echo -e "${RED}❌ FAIL${NC}"
        return 1
    fi
}

# Check if we're on the right platform for native builds
if [[ "$OSTYPE" == "linux-gnu"* ]]; then
    PLATFORM="linux"
elif [[ "$OSTYPE" == "darwin"* ]]; then
    PLATFORM="macos"
else
    PLATFORM="other"
fi

echo "Detected platform: $PLATFORM"
echo ""

# Test native build first
echo "Testing native build..."
if cargo build --release > /dev/null 2>&1; then
    echo -e "${GREEN}✅ Native build PASS${NC}"
else
    echo -e "${RED}❌ Native build FAIL${NC}"
    exit 1
fi

echo ""

# Test cross-compilation targets
echo "Testing cross-compilation targets..."

# Linux targets
test_build_target "x86_64-unknown-linux-gnu" "Linux x86_64"
test_build_target "aarch64-unknown-linux-gnu" "Linux aarch64"

echo ""
echo "🎉 Build target testing complete!"
echo ""
echo "Note: Some targets may be skipped if not installed locally."
echo "The CI/CD workflow will install all necessary targets automatically." 