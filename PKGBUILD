# Maintainer: Andrea Larboullet Marin <a.larboulletmarin@gmail.com>
pkgname=spot-launcher
_name=spot
pkgver=0.5.0
pkgrel=1
pkgdesc="Application and file launcher for GNOME"
arch=('x86_64' 'aarch64')
url="https://github.com/alarboulletmarin/spot"
license=('MIT')
depends=('gtk4' 'libadwaita' 'glib2')
makedepends=('cargo')
optdepends=('plocate: indexed file search')
source=("$pkgname-$pkgver.tar.gz::$url/archive/refs/tags/v$pkgver.tar.gz")
sha256sums=('3800e175b28bde39caf38cddcca15cdd62756725390ec6c8f1f47c8c4379d902')

prepare() {
    cd "$_name-$pkgver"
    export RUSTUP_TOOLCHAIN=stable
    cargo fetch --locked --target "$(rustc -vV | sed -n 's/host: //p')"
}

build() {
    cd "$_name-$pkgver"
    export RUSTUP_TOOLCHAIN=stable
    export CARGO_TARGET_DIR=target
    cargo build --frozen --release
}

package() {
    cd "$_name-$pkgver"
    make DESTDIR="$pkgdir" PREFIX=/usr install
}
