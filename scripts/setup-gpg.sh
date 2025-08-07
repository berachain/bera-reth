#!/bin/bash

# Setup GPG keys for bera-reth releases
# This script helps generate and export GPG keys for signing releases

set -e

echo "🔐 Bera-Reth GPG Key Setup"
echo "=========================="
echo ""

# Check if GPG is installed
if ! command -v gpg &> /dev/null; then
    echo "❌ GPG is not installed. Please install GPG first."
    echo "   On macOS: brew install gnupg"
    echo "   On Ubuntu: sudo apt-get install gnupg"
    exit 1
fi

echo "This script will help you set up GPG keys for signing bera-reth releases."
echo ""

# Check if user already has GPG keys
if gpg --list-secret-keys --keyid-format LONG | grep -q "sec"; then
    echo "✅ Found existing GPG keys:"
    gpg --list-secret-keys --keyid-format LONG
    echo ""
    read -p "Do you want to use an existing key or create a new one? (existing/new): " choice
    
    if [[ "$choice" == "new" ]]; then
        echo "Creating new GPG key..."
        gpg --full-generate-key
    else
        echo "Using existing key."
    fi
else
    echo "No existing GPG keys found. Creating new key..."
    gpg --full-generate-key
fi

# Get the key ID
KEY_ID=$(gpg --list-secret-keys --keyid-format LONG | grep "sec" | head -1 | awk '{print $2}' | cut -d'/' -f2)
echo ""
echo "Using GPG key ID: $KEY_ID"

# Export the private key
echo ""
echo "📤 Exporting private key..."
gpg --export-secret-keys --armor "$KEY_ID" > "bera-reth-gpg-key-$KEY_ID.asc"

# Create base64 encoded version for GitHub secrets
echo ""
echo "📋 Creating base64 encoded version for GitHub secrets..."
cat "bera-reth-gpg-key-$KEY_ID.asc" | base64 -w 0 > "bera-reth-gpg-key-$KEY_ID.base64"

echo ""
echo "✅ GPG key setup complete!"
echo ""
echo "📁 Generated files:"
echo "   - bera-reth-gpg-key-$KEY_ID.asc (private key)"
echo "   - bera-reth-gpg-key-$KEY_ID.base64 (base64 encoded for GitHub)"
echo ""
echo "🔧 Next steps:"
echo "1. Add the base64 content to GitHub Secrets as 'GPG_SIGNING_KEY':"
echo "   cat bera-reth-gpg-key-$KEY_ID.base64"
echo ""
echo "2. Add your GPG passphrase to GitHub Secrets as 'GPG_PASSPHRASE'"
echo ""
echo "3. Keep the .asc file secure and don't commit it to the repository"
echo ""
echo "4. Test the release process with a dry run in GitHub Actions"