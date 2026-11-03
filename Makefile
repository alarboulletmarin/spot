# Single install recipe, used by the PKGBUILD and by `sudo make install` on any distro.
PREFIX ?= /usr/local
DESTDIR ?=
PO := $(wildcard po/*.po)

all:
	cargo build --release --locked

install:
	install -Dm755 target/release/spot $(DESTDIR)$(PREFIX)/bin/spot
	install -Dm755 target/release/spot-resident $(DESTDIR)$(PREFIX)/bin/spot-resident
	install -Dm644 dev.andrea.Spot.desktop $(DESTDIR)$(PREFIX)/share/applications/dev.andrea.Spot.desktop
	install -Dm644 dev.andrea.Spot-daemon.desktop $(DESTDIR)/etc/xdg/autostart/dev.andrea.Spot-daemon.desktop
	install -Dm644 dev.andrea.Spot.svg $(DESTDIR)$(PREFIX)/share/icons/hicolor/scalable/apps/dev.andrea.Spot.svg
	install -Dm644 LICENSE $(DESTDIR)$(PREFIX)/share/licenses/spot-launcher/LICENSE
	for po in $(PO); do \
	  lang=$$(basename $$po .po); \
	  install -d $(DESTDIR)$(PREFIX)/share/locale/$$lang/LC_MESSAGES; \
	  msgfmt -c $$po -o $(DESTDIR)$(PREFIX)/share/locale/$$lang/LC_MESSAGES/spot.mo; \
	done

uninstall:
	rm -f $(DESTDIR)$(PREFIX)/bin/spot $(DESTDIR)$(PREFIX)/bin/spot-resident \
	  $(DESTDIR)$(PREFIX)/share/applications/dev.andrea.Spot.desktop \
	  $(DESTDIR)/etc/xdg/autostart/dev.andrea.Spot-daemon.desktop \
	  $(DESTDIR)$(PREFIX)/share/icons/hicolor/scalable/apps/dev.andrea.Spot.svg \
	  $(DESTDIR)$(PREFIX)/share/locale/*/LC_MESSAGES/spot.mo
	rm -rf $(DESTDIR)$(PREFIX)/share/licenses/spot-launcher

test:
	cargo test --locked
	$(MAKE) translations

translations:
	set -e; for po in $(PO); do msgfmt -c $$po -o /dev/null; done

# The same checks run locally and on every push / pull request.
verify:
	cargo fmt --check
	cargo clippy --locked --all-targets -- -D warnings
	$(MAKE) test
	cargo build --release --locked

# Needs a display server only inside Xvfb, never the active desktop.
test-ui:
	scripts/test-ui.sh

.PHONY: all install uninstall test translations verify test-ui
