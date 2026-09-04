# Maintainer: Andrea Larboullet Marin <a.larboulletmarin@gmail.com>
pkgname=spot-launcher
_name=spot
pkgver=0.2.0
pkgrel=1
pkgdesc="Application and file launcher for GNOME"
arch=('any')
url="https://github.com/alarboulletmarin/spot"
license=('MIT')
depends=('python' 'python-gobject' 'gtk4' 'libadwaita' 'glib2')
optdepends=('plocate: indexed file search')
source=("$pkgname-$pkgver.tar.gz::$url/archive/refs/tags/v$pkgver.tar.gz")
sha256sums=('644466fefd1c4cc086906adb3bc625208ad8d46300e4312c707b4bf5f8784aea')

package() {
    cd "$_name-$pkgver"
    make DESTDIR="$pkgdir" PREFIX=/usr install
}
