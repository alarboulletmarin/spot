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
sha256sums=('64cb4b25ded1ce0680609c46f94782d96bd55b85303e8ecc94b0afbe3eacbdd2')

package() {
    cd "$_name-$pkgver"
    make DESTDIR="$pkgdir" PREFIX=/usr install
}
