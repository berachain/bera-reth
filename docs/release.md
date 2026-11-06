# Release Process

## Prerequisites for Releases

To create signed releases, you need to set up GPG signing keys in your GitHub repository:

1. **Generate a GPG key pair** (if you don't have one):
   ```bash
   gpg --full-generate-key
   ```

2. **Export the signing key, pinned by fingerprint** (the workflow refuses a secret holding more than one key):
   ```bash
   gpg --armor --export-secret-keys <FINGERPRINT> | base64 -w 0 > private_key.b64
   ```

3. **Add GitHub Secrets** (Settings → Secrets and variables → Actions):
   - `GPG_SIGNING_KEY`: the contents of `private_key.b64`
   - `GPG_PASSPHRASE`: the passphrase for the key

4. **Publish the public key**. The release workflow fails if `.github/workflows/bera-reth_release.asc` is missing or does not match the signing key, and attaches that file to every release so validators can verify the signatures.
   ```bash
   gpg --armor --export <FINGERPRINT> > .github/workflows/bera-reth_release.asc
   gpg --keyserver keyserver.ubuntu.com --send-keys <FINGERPRINT>
   ```
   Commit the file and update the secrets together. Do not push a release tag while they disagree, the workflow fails on the mismatch.

## Rotating or Retiring the Key

Rotate when the key is about to expire or when its holder leaves. Treat a key whose holder left as leaked. Do the steps in this ord## Rotating or Retiring the Key

Rotate when the key is about to expire or when its holder leaves. Treat a key whose holder left as leaked.

1. **Generate the new key** and store the private key, its passphrase and the revocation certificate gpg wrote to `~/.gnupg/openpgp-revocs.d/<FINGERPRINT>.rev` in the team vault, with a named owner and the expiry date on a calendar.
2. **Re-sign the current release** with the new key if its signatures must stay verifiable. The archives do not change, only the `.asc` files:
   ```bash
   gpg --armor --detach-sign --local-user <FINGERPRINT> bera-reth-<tag>-<target>.tar.gz
   gh release upload <tag> bera-reth-<tag>-*.tar.gz.asc bera-reth_release.asc --clobber
   ```
   Then update the fingerprint in the release notes and say the signatures were re-issued.
3. **Commit the new public key and replace the secrets** (steps 2 to 4 under Prerequisites).
4. **Revoke the old key** if you still hold it, with `gpg --gen-revoke <OLD FINGERPRINT>`, then import the certificate and send the key to the keyserver. Overwriting the secret discards the old private key, so do this first if it matters.

in v0.1.0
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
