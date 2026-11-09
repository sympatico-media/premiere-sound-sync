# Agent notes

The owner does not run terminal commands. Run builds, signing, notarization, and releases yourself; do not hand the user commands to run.

## macOS release

Signing and notarization are already set up on this Mac:

- Identity `Developer ID Application: Everett Glovier (V8RN3L8WUW)` is in the login keychain.
- Notarization credentials are stored as keychain profile `notary` (Apple ID everettglovier@icloud.com, team V8RN3L8WUW).

Build a release from the tagged commit, not the working tree, so uncommitted work never ships:

```sh
git worktree add --detach /tmp/pss-release vX.Y.Z
cp scripts/bundle-macos.sh /tmp/pss-release/scripts/   # only if the script has uncommitted changes
cd /tmp/pss-release
SIGN_IDENTITY="Developer ID Application: Everett Glovier (V8RN3L8WUW)" NOTARY_PROFILE=notary scripts/bundle-macos.sh
```

The script signs, notarizes, staples, and writes `target/release/Premiere Sync-<version>.zip`. Rename it to `Premiere-Sync-<version>-macos-arm64.zip` before uploading.

Before publishing, verify it the way a downloader sees it:

```sh
ditto -x -k <zip> "$TMP" && xattr -w com.apple.quarantine "0081;$(printf %x $(date +%s));Safari;" "$TMP/Premiere Sync.app"
spctl -a -vvv -t exec "$TMP/Premiere Sync.app"   # must say: source=Notarized Developer ID
```

Publish with `gh release create` / `gh release upload --clobber` on `sympatico-media/premiere-sound-sync`. Put the zip's SHA-256 in the release notes. Remove the worktree afterwards.

If notarization fails with an auth error, the app-specific password was revoked; ask the user for a new one from account.apple.com and run `xcrun notarytool store-credentials notary --apple-id everettglovier@icloud.com --team-id V8RN3L8WUW --password <new>`.

If the identity is missing on a new Mac, restore it from `~/SYMPATICO Dropbox/Everett Glovier/Apple Developer ID/Developer ID Application - Everett Glovier (V8RN3L8WUW).p12` with `security import <p12> -k ~/Library/Keychains/login.keychain-db -P <password> -T /usr/bin/codesign`. The password is not stored anywhere on disk; ask the user for it. The certificate expires Oct 22, 2031.

If the certificate is expired or the backup is lost, Xcode cannot create a Developer ID certificate for this account. Generate a CSR with `openssl req`, import the key into the login keychain with `security import ... -T /usr/bin/codesign`, and have the user upload the CSR at developer.apple.com/account/resources/certificates/add (Developer ID Application, G2 Sub-CA). Import the downloaded `.cer` and, if `security find-identity -v -p codesigning` shows nothing, import Apple's `DeveloperIDG2CA.cer` intermediate.
