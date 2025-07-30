# Release Process

## Prerequisites for Releases

To create signed releases, you need to set up GPG signing keys in your GitHub repository:

1. **Generate a GPG key pair** (if you don't have one):
   ```bash
   gpg --full-generate-key
   ```

2. **Export your GPG signing key**:
   ```bash
   gpg --export-secret-keys --armor YOUR_EMAIL > private_key.asc
   ```

3. **Add GitHub Secrets**:
   - Go to your repository Settings → Secrets and variables → Actions
   - Add `GPG_SIGNING_KEY`: The base64-encoded private key
     ```bash
     cat private_key.asc | base64 -w 0
     ```
   - Add `GPG_PASSPHRASE`: The passphrase for your GPG key

## Creating a Release

1. **Update version in Cargo.toml** to match your release tag
2. **Create and push a git tag**:
   ```bash
   git tag v0.1.0
   git push origin v0.1.0
   ```
3. **The GitHub Actions workflow will automatically**:
   - Build binaries for multiple platforms
   - Sign them with your GPG key
   - Create a draft release with all artifacts
   - Generate a changelog from git commits

## Dry Run Testing

You can test the release process without creating an actual release:
1. Go to Actions → Release workflow
2. Click "Run workflow"
3. Check "Enable dry run mode"
4. Enter a test version (e.g., `v0.1.0-test`)
5. Click "Run workflow"

This will build all binaries and sign them, but won't upload to GitHub or create a release.
