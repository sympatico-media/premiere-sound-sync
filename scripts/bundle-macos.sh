#!/bin/sh
# Builds "Premiere Sync.app" in target/<profile>/. Usage: scripts/bundle-macos.sh [debug|release]
set -eu

PROFILE="${1:-release}"
NAME="Premiere Sync"
BIN="premiere-sound-sync"
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
VERSION="$(sed -n 's/^version = "\(.*\)"/\1/p' "$ROOT/Cargo.toml" | head -1)"

cd "$ROOT"
if [ "$PROFILE" = "release" ]; then
    cargo build -p "$BIN" --release
else
    cargo build -p "$BIN"
fi

APP="$ROOT/target/$PROFILE/$NAME.app"
rm -rf "$APP"
mkdir -p "$APP/Contents/MacOS" "$APP/Contents/Resources"
cp "$ROOT/target/$PROFILE/$BIN" "$APP/Contents/MacOS/$BIN"

# Static arm64 ffprobe (GPLv3) from https://ffmpeg.martin-riedl.de. Set FFPROBE=/path/to/ffprobe to use another build.
FFPROBE_URL="https://ffmpeg.martin-riedl.de/download/macos/arm64/1789931890_9.0.2/ffprobe.zip"
FFPROBE_SHA256="fcbe839537485eaee7a7a8bc5cbc0f90d53617e80943e8a5b2e31cb851197ea6"
if [ -z "${FFPROBE:-}" ]; then
    CACHE="$ROOT/target/ffprobe-9.0.2-arm64"
    if [ ! -x "$CACHE/ffprobe" ]; then
        mkdir -p "$CACHE"
        curl -fsSL "$FFPROBE_URL" -o "$CACHE/ffprobe.zip"
        echo "$FFPROBE_SHA256  $CACHE/ffprobe.zip" | shasum -a 256 -c - >/dev/null
        unzip -oq "$CACHE/ffprobe.zip" -d "$CACHE"
        rm "$CACHE/ffprobe.zip"
    fi
    FFPROBE="$CACHE/ffprobe"
fi
cp "$FFPROBE" "$APP/Contents/Resources/ffprobe"
chmod +x "$APP/Contents/Resources/ffprobe"

ICONSET="$(mktemp -d)/AppIcon.iconset"
mkdir -p "$ICONSET"
SRC="$ROOT/apps/$BIN/assets/AppIcon.png"
for size in 16 32 128 256 512; do
    sips -z $size $size "$SRC" --out "$ICONSET/icon_${size}x${size}.png" >/dev/null
    double=$((size * 2))
    sips -z $double $double "$SRC" --out "$ICONSET/icon_${size}x${size}@2x.png" >/dev/null
done
iconutil -c icns "$ICONSET" -o "$APP/Contents/Resources/AppIcon.icns"

cat > "$APP/Contents/Info.plist" <<EOF
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>CFBundleName</key><string>$NAME</string>
    <key>CFBundleDisplayName</key><string>$NAME</string>
    <key>CFBundleIdentifier</key><string>com.everettglovier.premiere-sync</string>
    <key>CFBundleExecutable</key><string>$BIN</string>
    <key>CFBundleIconFile</key><string>AppIcon</string>
    <key>CFBundlePackageType</key><string>APPL</string>
    <key>CFBundleShortVersionString</key><string>$VERSION</string>
    <key>CFBundleVersion</key><string>$VERSION</string>
    <key>LSMinimumSystemVersion</key><string>11.0</string>
    <key>NSHighResolutionCapable</key><true/>
</dict>
</plist>
EOF

# Gatekeeper reports unsigned downloaded bundles as "damaged". Without SIGN_IDENTITY the bundle is ad-hoc signed,
# which only runs on other Macs via System Settings > Privacy & Security > Open Anyway.
# For warning-free distribution set SIGN_IDENTITY="Developer ID Application: ..." and NOTARY_PROFILE to a profile
# created with `xcrun notarytool store-credentials`.
IDENTITY="${SIGN_IDENTITY:--}"
TIMESTAMP=""
[ "$IDENTITY" != "-" ] && TIMESTAMP="--timestamp"
codesign --force --options runtime $TIMESTAMP --sign "$IDENTITY" "$APP/Contents/Resources/ffprobe"
codesign --force --options runtime $TIMESTAMP --sign "$IDENTITY" "$APP"
codesign --verify --strict "$APP"

ZIP="$ROOT/target/$PROFILE/$NAME-$VERSION.zip"
rm -f "$ZIP"
if [ -n "${NOTARY_PROFILE:-}" ]; then
    [ "$IDENTITY" = "-" ] && { echo "NOTARY_PROFILE requires SIGN_IDENTITY" >&2; exit 1; }
    ditto -c -k --keepParent "$APP" "$ZIP"
    xcrun notarytool submit "$ZIP" --keychain-profile "$NOTARY_PROFILE" --wait
    xcrun stapler staple "$APP"
    rm "$ZIP"
fi
ditto -c -k --keepParent "$APP" "$ZIP"

echo "$APP"
echo "$ZIP"
