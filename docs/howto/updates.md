# Signed application updates

TokenKeeper verifies downloaded updates against `update-public-key.bin`, the
32-byte Ed25519 public key embedded in each build. The release workflow signs
both archives with [zipsign 0.2.2](https://docs.rs/crate/zipsign/0.2.2), verifies
them with that public key, attaches them to a draft release, and publishes only
after both uploads finish. Build workflow artifacts are unsigned; install them
manually for testing.

## Configure the release secret

The initial keypair is stored locally in the gitignored `.local/update-signing/`
directory. Keep an encrypted backup of `private.key` outside the repository.
Only `update-public-key.bin` belongs in Git. The private file is a raw 64-byte
Ed25519 keypair; the Actions secret contains its base64 encoding.

From the repository root, set the repository Actions secret with GitHub CLI:

```sh
base64 -w 0 .local/update-signing/private.key |
  gh secret set TOKENKEEPER_UPDATE_SIGNING_KEY --repo ricnewton/tokenkeeper
```

This command sends the key directly to GitHub without printing it. The release
workflow supplies it only to the signing step and removes its temporary decoded
file when the step exits. A missing, malformed, or mismatched key fails before
creating a release. Restrict repository write access and review changes to the
release workflow before running it.

## Generate keys for a new installation

Do this only before distributing the first build with a trusted key, or as part
of a planned key rotation. Replacing the public key immediately prevents older
installed builds from accepting releases signed only by the new key.

```sh
umask 077
mkdir -p .local/update-signing
cargo install zipsign --version 0.2.2 --locked --root .local/update-signing/tools
.local/update-signing/tools/bin/zipsign gen-key \
  .local/update-signing/private.key .local/update-signing/public.key
cp .local/update-signing/public.key update-public-key.bin
```

`gen-key` refuses to overwrite existing key files. Commit the public key and set
the secret above before running the release workflow. Never commit the private
key or its base64 representation.

## Verify a downloaded release

Use the public key from a trusted checkout and preserve the published archive
filename; zipsign binds the signature to that basename, as does `self_update`.

```sh
zipsign verify zip tokenkeeper-win-VERSION.zip update-public-key.bin
zipsign verify tar tokenkeeper-linux-VERSION.tar.gz update-public-key.bin
```

Run the Release workflow after bumping `Cargo.toml`. If attachment or publishing
fails, the release stays a draft and the updater ignores it. Inspect that draft
before deleting it and rerunning the workflow; the version check deliberately
rejects a release tag that already exists.

## Recovery backup

The signing key must have a recovery copy outside the checkout. GitHub Actions
secrets cannot be read back, so the configured secret is not an exportable backup.
The owner's Bitwarden secure note should hold the base64-encoded 64-byte private
key, public-key fingerprint, encrypted-backup password and restore instructions.
A separately stored encrypted archive should contain the raw private/public keys
and those instructions. Keep the archive password out of the backup machine.

After preparing a backup, decrypt it in memory or a private temporary directory
and compare the recovered key with the original. Verify the uploaded archive's
checksum. Delete plaintext import files after confirming the Bitwarden import.
Never commit private keys, recovery passwords or Bitwarden import files.

Trust starts with the user's first installation from the official repository.
That app embeds the public key; later archives must verify against it, including
the platform/version filename. This is independent of Windows Authenticode.
