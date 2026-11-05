#!/usr/bin/env bash
set -euo pipefail

format="${1:?Usage: package-linux.sh deb|rpm}"
: "${VERSION:?}" "${BINARY_DIR:?}" "${GITHUB_REPOSITORY:?}" "${GITHUB_REPOSITORY_OWNER:?}"
case "$format" in
  deb) : "${DEBIAN_VERSION:?}" ;;
  rpm) : "${BASE_VERSION:?}" "${RPM_RELEASE:?}" ;;
  *) echo "Unknown package format: $format" >&2; exit 1 ;;
esac

mkdir -p target/packaging dist
work="$(mktemp -d "$PWD/target/packaging/$format.XXXXXX")"
root="$work/root"
for binary in ansiedad ansiedad-gui; do
  install -Dm755 "$BINARY_DIR/$binary" "$root/usr/bin/$binary"
done
install -Dm644 packaging/ansiedad-gui.desktop "$root/usr/share/applications/ansiedad-gui.desktop"
install -Dm644 assets/ansiedad.svg "$root/usr/share/icons/hicolor/scalable/apps/ansiedad.svg"
install -Dm644 assets/ansiedad.png "$root/usr/share/icons/hicolor/256x256/apps/ansiedad.png"
install -Dm644 README.md "$root/usr/share/doc/ansiedad/README.md"
for example in examples/*.txt; do
  install -Dm644 "$example" "$root/usr/share/doc/ansiedad/$example"
done
desktop-file-validate "$root/usr/share/applications/ansiedad-gui.desktop"

if [[ "$format" == deb ]]; then
  install -Dm644 LICENSE "$root/usr/share/doc/ansiedad/copyright"
  mkdir -p "$root/DEBIAN" "$work/debian"
  cat > "$work/debian/control" <<EOF
Source: ansiedad
Section: graphics
Priority: optional
Maintainer: ANSIedad contributors <${GITHUB_REPOSITORY_OWNER}@users.noreply.github.com>

Package: ansiedad
Architecture: amd64
Description: Live-coded bytebeat visuals in a terminal or graphical canvas
EOF
  dependencies="$(cd "$work" && dpkg-shlibdeps -O -e"$root/usr/bin/ansiedad" -e"$root/usr/bin/ansiedad-gui")"
  cat > "$root/DEBIAN/control" <<EOF
Package: ansiedad
Version: $DEBIAN_VERSION
Section: graphics
Priority: optional
Architecture: amd64
Maintainer: ANSIedad contributors <${GITHUB_REPOSITORY_OWNER}@users.noreply.github.com>
Homepage: https://github.com/$GITHUB_REPOSITORY
Installed-Size: $(du -sk "$root/usr" | cut -f1)
Depends: ${dependencies#shlibs:Depends=}, libx11-6, libxcursor1, libxi6, libxrandr2, libxkbcommon0, libxkbcommon-x11-0, libwayland-client0, libwayland-cursor0, libwayland-egl1, libegl1, libgl1
Description: Live-coded bytebeat visuals in a terminal or graphical canvas
 Includes the ANSIedad terminal editor and the optional graphical application,
 with character and pixel rendering, live expression editing and example patches.
EOF
  dpkg-deb --root-owner-group --build "$root" "dist/ansiedad_${DEBIAN_VERSION}_amd64.deb"
else
  install -Dm644 LICENSE "$root/usr/share/licenses/ansiedad/LICENSE"
  mkdir -p "$work/rpm"
  cat > "$work/ansiedad.spec" <<EOF
Name: ansiedad
Version: $BASE_VERSION
Release: $RPM_RELEASE%{?dist}
Summary: Live-coded bytebeat visuals in a terminal or graphical canvas
License: AGPL-3.0-only
URL: https://github.com/$GITHUB_REPOSITORY
BuildArch: x86_64
Requires: libX11, libXcursor, libXi, libXrandr, libxkbcommon, libxkbcommon-x11
Requires: libwayland-client, libwayland-cursor, libwayland-egl, libglvnd-egl, libglvnd-glx
%global debug_package %{nil}

%description
Includes the ANSIedad terminal editor and the optional graphical application,
with character and pixel rendering, live expression editing and example patches.

%prep

%build

%install
mkdir -p %{buildroot}
cp -a "$root/usr" %{buildroot}/

%files
/usr/bin/ansiedad
/usr/bin/ansiedad-gui
/usr/share/applications/ansiedad-gui.desktop
/usr/share/icons/hicolor/scalable/apps/ansiedad.svg
/usr/share/icons/hicolor/256x256/apps/ansiedad.png
%doc /usr/share/doc/ansiedad
%license /usr/share/licenses/ansiedad
EOF
  rpmbuild -bb --define "_topdir $work/rpm" "$work/ansiedad.spec"
  cp "$work"/rpm/RPMS/x86_64/*.rpm dist/
fi
