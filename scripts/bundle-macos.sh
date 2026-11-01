#!/bin/sh
set -eu
cd "$(dirname "$0")/.."
profile="${1:-debug}"
case "$profile" in
  debug) cargo build -p type-app ;;
  release) cargo build -p type-app --release ;;
  *) echo 'Usage: scripts/bundle-macos.sh [debug|release]' >&2; exit 1 ;;
esac
bundle="target/$profile/Type.app"
mkdir -p "$bundle/Contents/MacOS"
# Atomic executable replacement.
cp "target/$profile/type" "$bundle/Contents/MacOS/Type.next"
mv -f "$bundle/Contents/MacOS/Type.next" "$bundle/Contents/MacOS/Type"
mkdir -p "$bundle/Contents/Resources"
cp LICENSE "$bundle/Contents/Resources/LICENSE"
cat > "$bundle/Contents/Info.plist" <<'PLIST'
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0"><dict>
<key>CFBundleExecutable</key><string>Type</string>
<key>CFBundleIdentifier</key><string>com.peterdpong.type</string>
<key>CFBundleName</key><string>Type</string>
<key>CFBundlePackageType</key><string>APPL</string>
<key>CFBundleShortVersionString</key><string>0.1.0</string>
<key>CFBundleVersion</key><string>1</string>
<key>NSHighResolutionCapable</key><true/>
<key>LSMinimumSystemVersion</key><string>12.0</string>
</dict></plist>
PLIST
printf 'Built %s\n' "$bundle"
