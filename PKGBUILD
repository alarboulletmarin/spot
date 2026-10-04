# Maintainer: Andrea Larboullet Marin <a.larboulletmarin@gmail.com>
pkgname=spot-launcher
_name=spot
pkgver=0.5.1
pkgrel=1
pkgdesc="Application and file launcher for GNOME"
arch=('x86_64' 'aarch64')
url="https://github.com/alarboulletmarin/spot"
license=('MIT')
depends=('gtk4' 'libadwaita' 'glib2')
makedepends=('cargo')
optdepends=('plocate: indexed file search')
source=("$pkgname-$pkgver.tar.gz::$url/archive/refs/tags/v$pkgver.tar.gz")
sha256sums=('867c7a0462ee9fc00abe87b06c838541a4bc72c6bfe190cf8ae9072b394a3a49')

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
