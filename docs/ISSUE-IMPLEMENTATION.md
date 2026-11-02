# Issues 3–10 implementation log

Current GitHub bodies and comments read on 2026-11-02. All eight open; no comments.
No applicable AGENTS.md found. Existing untracked `.claude/` and `spot` preserved.

Order: #3 ranking → #4 completion → #5 calculator → #6 suggestions → #9 scripting
→ #8 desktop modules → final #7 limits and #10 packaging/benchmarks.

Dependencies/conflicts:
- Ranking supplies completion/dmenu. Keep literal-prefix completion distinct from fuzzy matching.
- Calculator detection is a privacy gate for suggestions and suppresses Calculator provider duplicates.
- Suggestions must append after local results, preserve selection and cancel stale requests.
- Dmenu must use a separate nonresident process, preserve stdin/stdout, and not activate normal sources.
- Desktop modules must allocate/watch only after explicit opt-in; clipboard data stays in memory.
- #6 means #10's positioning must say no network **by default**, and disclose opt-in traffic.
- #7's “does not ship yet” wording must reflect #8's actual final support.
- Benchmarks run after changes; supplied comparison numbers must be attributed, not presented as a new run.

| Issue | Status | Validation / limits |
|---|---|---|
| #3 | Implemented | Tier ordering, initials, typo cases, Unicode positions and short-query rejection covered by unit tests; highlighting rendered in all six styles. |
| #4 | Implemented | Pure suffix and explicit key-precedence tests; GTK tests check Tab, mid-entry Right, selection changes and Enter activation; 440 px renders reviewed. |
| #5 | Implemented | Parser/precedence/unary/powers/hex/errors/format tests; real GTK clipboard decimal and hexadecimal actions checked. No external Calculator dependency. |
| #6 | Implemented | Default/config tests, privacy filter, JSON and prefix-cache tests; native HTTP loopback success, response cap and cancellation tested. GTK tests check append-only suggestion selection stability. Live provider availability is external and was not tested for every engine. |
| #7 | Implemented | Limits distinguishes GNOME bridge requirements from optional compositor protocols, with links. Separate documentation commit precedes feature work. |
| #8 | Implemented | History caps/dedup/sensitive hints tested. Isolated headless GNOME Shell 51 verifies actual window list/focus, clipboard notifications, size cap, sensitive exclusion and opt-out. Other declared GNOME versions and a live wlroots compositor remain untested. Disabled path creates no backend/watch/thread; benchmark confirms one resident, but no before/after memory-delta experiment is claimed. |
| #9 | Implemented | Metadata/interpreter/argument tests; actual Xvfb keyboard tests verify `b` stdout, cancellation code 1 with no output, and custom input. No Node/runtime added for these features. |
| #10 | Implemented | Release build and 20-sample benchmark run; raw JSON and attributed issue comparison published. Tagged-source AUR recipe, dependency metadata and release notes updated. AUR publication and a clean Arch chroot package build were not performed. |

Validation commands:

```bash
cargo test --locked
cargo clippy --locked --all-targets -- -D warnings
cargo fmt --check
msgfmt -c po/fr.po -o /dev/null
scripts/test-ui.sh
LANGUAGE=fr scripts/test-ui.sh
scripts/test-gnome.sh
cargo build --release --locked
bench/run.sh --runs 20
makepkg --printsrcinfo
(cd aur/spot && makepkg --printsrcinfo)
```

The GTK test is ignored by default because it requires a display. `scripts/test-ui.sh`
builds it, then runs it in an empty temporary HOME, private bus and Xvfb display.
Optional `SPOT_TEST_SCREENSHOTS=/absolute/directory` writes PNGs for every style and
light/dark theme. The French catalog must be compiled beside the test prefix
(`target/debug/share/locale/fr/LC_MESSAGES/spot.mo`) to test translated rendering.
English and French GTK runs passed; screenshots were inspected in default and mono
styles, with all twelve style/theme combinations generated at narrow width.

`test-gnome.sh` starts a separate headless Shell with a temporary profile; it does
not install or enable an extension in the active desktop. It needs GNOME Shell,
Python PyGObject with GTK4, and the Mutter remote-desktop API for a deterministic
fixture clipboard source. No real clipboard content is used or logged.

Dmenu stdout/cancellation/custom-input checks used a separate Xvfb session and
XTest-generated key events against the release launcher. Static checks also covered
Shell/JS syntax, Python compilation, translation catalog validity and Git whitespace.

The implementation adds libsoup 3 for HTTP and Rust Wayland protocol bindings;
neither feature adds a bundled interpreter or JS engine. Optional desktop runtime
resources are allocated only in the resident after explicit module activation;
dmenu never initializes those resources, even if the normal profile enables them.

The user subsequently authorized one local commit per issue with an English
conventional title below 50 characters and a `Closes #N` footer. `/commit` is not
available in this environment, so the same convention is applied with Git directly.
No pushes, releases, deployments, issue comments or direct closures were performed.
