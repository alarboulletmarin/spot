# Maintainer: Andrea Larboullet Marin <a.larboulletmarin@gmail.com>
pkgname=spot
pkgver=0.2.0
pkgrel=1
pkgdesc="Application and file launcher for GNOME"
arch=('any')
url="https://github.com/alarboulletmarin/spot"
license=('MIT')
depends=('python' 'python-gobject' 'gtk4' 'libadwaita' 'glib2')
optdepends=('plocate: indexed file search')
source=('spot.py'
        'dev.andrea.Spot.desktop'
        'dev.andrea.Spot-daemon.desktop'
        'dev.andrea.Spot.svg'
        'LICENSE')
sha256sums=('SKIP' 'SKIP' 'SKIP' 'SKIP' 'SKIP')

package() {
    install -Dm755 "$srcdir/spot.py" "$pkgdir/usr/bin/spot"
    install -Dm644 "$srcdir/dev.andrea.Spot.desktop" \
        "$pkgdir/usr/share/applications/dev.andrea.Spot.desktop"
    install -Dm644 "$srcdir/dev.andrea.Spot-daemon.desktop" \
        "$pkgdir/etc/xdg/autostart/dev.andrea.Spot-daemon.desktop"
    install -Dm644 "$srcdir/dev.andrea.Spot.svg" \
        "$pkgdir/usr/share/icons/hicolor/scalable/apps/dev.andrea.Spot.svg"
    install -Dm644 "$srcdir/LICENSE" \
        "$pkgdir/usr/share/licenses/$pkgname/LICENSE"
}
