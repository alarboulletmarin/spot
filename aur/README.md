# AUR recipes

`spot/` builds a pinned tagged source release. `spot-launcher-git/` follows main.
Neither recipe's presence in this repository implies an AUR submission exists.
`spot` and `spot-launcher` conflict because they install the same binaries.

For a release:

1. Run the normal checks and `bench/run.sh`; inspect the new footprint.
2. Use the maintainer release process. `scripts/release.sh` updates `pkgver`,
   the GitHub archive SHA-256 and `.SRCINFO` for both root and `aur/spot`.
   Reset `pkgrel=1` for a new upstream version; increment it for packaging-only fixes.
3. Verify runtime dependencies (`gtk4`, `libadwaita`, `glib2`, `libsoup3`) and
   build dependencies (`cargo`, `gettext`). Test `makepkg` in a clean Arch chroot;
   validate the packaged launcher, resident, desktop files, icons and translations.
4. Review `makepkg --printsrcinfo` against the checked-in `.SRCINFO`.
5. In the separately maintained AUR checkout, copy `PKGBUILD` and `.SRCINFO`,
   inspect the diff, commit and push **only when authorized to publish**.

No AUR package is automatically submitted or pushed by the release script.
