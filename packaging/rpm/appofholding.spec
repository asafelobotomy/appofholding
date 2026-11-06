# A DRAFT spec: package names follow Fedora conventions but have not been checked on Fedora.
# Build from a prepared source tree with:  rpmbuild -bb --build-in-place packaging/rpm/appofholding.spec
Name:           appofholding
Version:        0.4.1
Release:        1%{?dist}
Summary:        AppImage manager with updates, launch options and sandboxing
License:        GPL-3.0-or-later
URL:            https://github.com/asafelobotomy/appofholding

BuildRequires:  cargo
BuildRequires:  gcc-c++
BuildRequires:  cmake
BuildRequires:  qt6-qtbase-devel
BuildRequires:  qt6-qtdeclarative-devel
Requires:       bubblewrap
Requires:       xdg-dbus-proxy
Requires:       qt6-qtdeclarative
Requires:       kf6-kirigami
Requires:       kf6-qqc2-desktop-style
Recommends:     zsync
Recommends:     libnotify

%description
appOFholding adds AppImages to your application menu, keeps them up to date, lets you change
how each one is launched, and can run them in a bubblewrap sandbox.

%build
make PROFILE=release

%install
make install PREFIX=%{_prefix} DESTDIR=%{buildroot} PROFILE=release

%files
%license LICENSE
%{_bindir}/appofholding
%{_bindir}/appofholding-gui
%{_datadir}/applications/io.github.appofholding.desktop
%{_datadir}/icons/hicolor/scalable/apps/io.github.appofholding.svg
%{_datadir}/metainfo/io.github.appofholding.metainfo.xml
%{_datadir}/doc/appofholding/
%{_mandir}/man1/appofholding*.1*
%{_datadir}/bash-completion/completions/appofholding
%{_datadir}/zsh/site-functions/_appofholding
%{_datadir}/fish/vendor_completions.d/appofholding.fish
%{_datadir}/licenses/appofholding/

%changelog
* Fri Nov 06 2026 asafelobotomy <213836723+asafelobotomy@users.noreply.github.com> - 0.4.1-1
- The interface no longer needs the exact Qt version it was built with

* Fri Nov 06 2026 asafelobotomy <213836723+asafelobotomy@users.noreply.github.com> - 0.4.0-1
- Library views, release workflow, review fixes

* Thu Nov 05 2026 asafelobotomy <213836723+asafelobotomy@users.noreply.github.com> - 0.3.0-1
- Import existing AppImages, redesigned interface

* Thu Nov 05 2026 asafelobotomy <213836723+asafelobotomy@users.noreply.github.com> - 0.2.0-1
- Self-updater, new icon

* Thu Nov 05 2026 asafelobotomy <213836723+asafelobotomy@users.noreply.github.com> - 0.1.0-1
- First release
