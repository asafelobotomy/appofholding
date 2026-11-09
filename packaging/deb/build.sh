#!/bin/sh
# Builds a .deb from an installed tree (a DRAFT: the dependency names below follow Debian/Ubuntu
# conventions but have not been checked against a real Debian or Ubuntu system).
#   packaging/deb/build.sh [output-dir]
set -eu
here=$(cd "$(dirname "$0")" && pwd)
root=$(cd "$here/../.." && pwd)
out=${1:-$root/target/packages}
version=$(sed -n 's/^version = "\(.*\)"/\1/p' "$root/Cargo.toml" | head -1)
arch=$(dpkg --print-architecture)
stage=$(mktemp -d)
trap 'rm -rf "$stage"' EXIT

make -C "$root" install PREFIX=/usr DESTDIR="$stage" PROFILE="${PROFILE:-release}" CARGOFLAGS="${CARGOFLAGS---locked}"
# Debian expects compressed man pages and no stray license directory name clashes.
gzip -9n "$stage"/usr/share/man/man1/*.1
mkdir -p "$stage/DEBIAN" "$out"
size=$(du -sk "$stage" | cut -f1)
cat > "$stage/DEBIAN/control" <<CONTROL
Package: appofholding
Version: $version
Architecture: $arch
Maintainer: asafelobotomy <213836723+asafelobotomy@users.noreply.github.com>
Installed-Size: $size
Depends: libc6, libstdc++6, bubblewrap, xdg-dbus-proxy, qml6-module-org-kde-kirigami, qml6-module-qtquick-controls, qml6-module-qtquick-layouts, qml6-module-qtquick-dialogs, qml6-module-qtquick-window, qml6-module-qt-labs-platform, qml6-module-org-kde-desktop, libqt6widgets6
Recommends: zsync, libnotify-bin
Section: utils
Priority: optional
Description: AppImage manager with updates, launch options and sandboxing
 appOFholding adds AppImages to your application menu, keeps them up to date,
 lets you change how each one is launched, and can run them in a sandbox.
CONTROL
dpkg-deb --root-owner-group --build "$stage" "$out/appofholding_${version}_${arch}.deb"
echo "Wrote $out/appofholding_${version}_${arch}.deb"
