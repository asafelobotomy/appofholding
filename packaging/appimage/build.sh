#!/bin/sh
# Builds an AppImage of appOFholding itself, so it can also be installed and updated like any
# other app. A DRAFT: it has not been run, because it needs tools that are not part of this
# repository and not installed everywhere:
#   - linuxdeploy                (https://github.com/linuxdeploy/linuxdeploy)
#   - linuxdeploy-plugin-qt      (bundles Qt libraries, plugins and QML modules, incl. Kirigami)
#   - linuxdeploy-plugin-appimage or appimagetool
# Put them on PATH, then:
#   UPDATE_INFO='gh-releases-zsync|OWNER|REPO|latest|appofholding-*x86_64.AppImage.zsync' packaging/appimage/build.sh
# (UPDATE_INFO is optional; with it the AppImage updates itself through appOFholding.)
#
# Things to check on the first real run:
#   - Kirigami and qqc2-desktop-style QML modules are bundled (QML_SOURCES_PATHS below helps the
#     Qt plugin find them);
#   - the sandbox and update features need `bwrap` and `xdg-dbus-proxy` on the host, which an
#     AppImage cannot sensibly bundle.
set -eu
here=$(cd "$(dirname "$0")" && pwd)
root=$(cd "$here/../.." && pwd)
appdir=$root/target/AppDir
out=${1:-$root/target/packages}

rm -rf "$appdir"
make -C "$root" install PREFIX=/usr DESTDIR="$appdir" PROFILE=release CARGOFLAGS="${CARGOFLAGS---locked}"
mkdir -p "$out"

export QML_SOURCES_PATHS="$root/crates/gui/qml"
export EXTRA_QT_MODULES="Quick;QuickControls2;QuickDialogs2;QuickLayouts;Svg"
export EXTRA_PLATFORM_PLUGINS="libqwayland-generic.so;libqwayland-egl.so"
[ -n "${UPDATE_INFO:-}" ] && export LDAI_UPDATE_INFORMATION="$UPDATE_INFO"
export LDAI_OUTPUT="$out/appofholding-$(uname -m).AppImage"

cd "$out"
linuxdeploy \
  --appdir "$appdir" \
  --executable "$appdir/usr/bin/appofholding-gui" \
  --executable "$appdir/usr/bin/appofholding" \
  --desktop-file "$appdir/usr/share/applications/io.github.appofholding.desktop" \
  --icon-file "$appdir/usr/share/icons/hicolor/scalable/apps/io.github.appofholding.svg" \
  --plugin qt \
  --output appimage
