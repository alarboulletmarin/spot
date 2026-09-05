# Maintainer: Andrea Larboullet Marin <a.larboulletmarin@gmail.com>
pkgname=spot-launcher
_name=spot
pkgver=0.3.0
pkgrel=1
pkgdesc="Application and file launcher for GNOME"
arch=('any')
url="https://github.com/alarboulletmarin/spot"
license=('MIT')
depends=('python' 'python-gobject' 'gtk4' 'libadwaita' 'glib2')
optdepends=('plocate: indexed file search')
source=("$pkgname-$pkgver.tar.gz::$url/archive/refs/tags/v$pkgver.tar.gz")
sha256sums=('SKIP')

package() {
    cd "$_name-$pkgver"
    make DESTDIR="$pkgdir" PREFIX=/usr install
}
