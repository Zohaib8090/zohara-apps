# Maintainer: Zohara OS Team <https://github.com/Zohaib8090/zohara>
# Split PKGBUILD for the zohara-apps workspace.
#
# Packages the four binaries CI already builds with `cargo build --release`
# (CARGO_TARGET_DIR=/home/builder/target, see .github/workflows/build.yml)
# into four real pacman packages, one per crate. Before this, build.yml
# copied the raw ELF binaries straight into a release as release assets --
# not .pkg.tar.zst files -- so zohara-packages/publish.yml's
# `find -name '*.pkg.tar.zst'` never found anything to add to zohara.db, and
# `pacman -S zohara-welcome` had nothing installable to find even if it had.
#
# usermgr and migrate are still stub binaries (`eprintln!(...); exit(1)`) --
# packaging them ships that stub, not a missing feature; see their src/main.rs.
#
# Local build/test: cargo build --release --workspace, then makepkg --nodeps
# (CARGO_TARGET_DIR must be unset or point at ./target so $srcdir/../target
# resolves the way CI's symlink trick sets it up).

pkgbase=zohara-apps
pkgname=(zohara-welcome zohara-update zohara-usermgr zohara-migrate)
pkgver=0.1.0
pkgrel=1
arch=('x86_64')
url="https://github.com/Zohaib8090/zohara-apps"
license=('MIT' 'GPL-3.0-or-later')
makedepends=('cargo' 'pkgconf')
source=()
sha256sums=()

pkgver() {
  if [ -n "${_PKGVER:-}" ]; then
    echo "$_PKGVER"
  else
    grep '^version' "$startdir/Cargo.toml" | head -1 | cut -d'"' -f2
  fi
}

_bin() {
  # $1 = binary name. CI builds into $CARGO_TARGET_DIR (a symlink at
  # $srcdir/../target, matching every other crate's PKGBUILD in this
  # ecosystem -- see zohara-settings/PKGBUILD's build.yml comment).
  echo "$startdir/target/release/$1"
}

build() {
  for bin in zohara-welcome zohara-update zohara-usermgr zohara-migrate; do
    if [ ! -f "$(_bin "$bin")" ]; then
      echo "ERROR: prebuilt binary not found at $(_bin "$bin")" >&2
      echo "CI is supposed to cargo build --release --workspace before makepkg." >&2
      return 1
    fi
  done
}

package_zohara-welcome() {
  pkgdesc="Zohara OS first-boot welcome window (GTK4 + libadwaita)"
  depends=('gtk4' 'libadwaita' 'glib2')
  provides=('zohara-welcome')
  conflicts=('zohara-welcome-git')
  install -Dm755 "$(_bin zohara-welcome)" "$pkgdir/usr/bin/zohara-welcome"
  install -Dm644 "$startdir/data/zohara-welcome.desktop" \
    "$pkgdir/usr/share/applications/zohara-welcome.desktop"
}

package_zohara-update() {
  pkgdesc="Zohara OS updates center (pacman / kernel / driver / Zohara-OTA panels)"
  depends=('gtk4' 'libadwaita' 'glib2' 'pacman')
  optdepends=('polkit: installing updates without a terminal')
  provides=('zohara-update')
  conflicts=('zohara-update-git')
  install -Dm755 "$(_bin zohara-update)" "$pkgdir/usr/bin/zohara-update"
}

package_zohara-usermgr() {
  pkgdesc="Zohara OS user account manager (currently a stub -- see update/usermgr/src/main.rs)"
  depends=('glibc')
  provides=('zohara-usermgr')
  conflicts=('zohara-usermgr-git')
  install -Dm755 "$(_bin zohara-usermgr)" "$pkgdir/usr/bin/zohara-usermgr"
}

package_zohara-migrate() {
  pkgdesc="Zohara OS cross-distro migration tool (currently a stub -- see migrate/src/main.rs)"
  depends=('glibc')
  provides=('zohara-migrate')
  conflicts=('zohara-migrate-git')
  install -Dm755 "$(_bin zohara-migrate)" "$pkgdir/usr/bin/zohara-migrate"
}
