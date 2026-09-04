# Single install recipe, used by the PKGBUILD and by `sudo make install` on any distro.
PREFIX ?= /usr/local
DESTDIR ?=
PO := $(wildcard po/*.po)

install:
	install -Dm755 spot.py $(DESTDIR)$(PREFIX)/bin/spot
	install -Dm644 dev.andrea.Spot.desktop $(DESTDIR)$(PREFIX)/share/applications/dev.andrea.Spot.desktop
	install -Dm644 dev.andrea.Spot-daemon.desktop $(DESTDIR)/etc/xdg/autostart/dev.andrea.Spot-daemon.desktop
	install -Dm644 dev.andrea.Spot.svg $(DESTDIR)$(PREFIX)/share/icons/hicolor/scalable/apps/dev.andrea.Spot.svg
	install -Dm644 LICENSE $(DESTDIR)$(PREFIX)/share/licenses/spot-launcher/LICENSE
	for po in $(PO); do \
	  lang=$$(basename $$po .po); \
	  install -d $(DESTDIR)$(PREFIX)/share/locale/$$lang/LC_MESSAGES; \
	  msgfmt -c $$po -o $(DESTDIR)$(PREFIX)/share/locale/$$lang/LC_MESSAGES/spot.mo; \
	done

test:
	python3 test_spot.py
	for po in $(PO); do msgfmt -c $$po -o /dev/null; done

.PHONY: install test
