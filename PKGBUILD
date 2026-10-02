# Maintainer: Andrea Larboullet Marin <a.larboulletmarin@gmail.com>
pkgname=spot-launcher
_name=spot
pkgver=0.3.1
pkgrel=1
pkgdesc="Application and file launcher for GNOME"
arch=('x86_64' 'aarch64')
url="https://github.com/alarboulletmarin/spot"
license=('MIT')
depends=('gtk4' 'libadwaita' 'glib2')
makedepends=('cargo')
optdepends=('plocate: indexed file search')
source=("$pkgname-$pkgver.tar.gz::$url/archive/refs/tags/v$pkgver.tar.gz")
sha256sums=('6286d02dbab4a1e245d896d1447002256318eee2d4d387641bfd2a4e44781e86')

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
