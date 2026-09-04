"""Smallest checks that fail if the ranking or the path filter breaks. Run: python3 test_spot.py"""

import spot


def test_fuzzy_score():
    assert spot.fuzzy_score("fire", "Firefox") > spot.fuzzy_score("fire", "LibreOffice Firebird")
    assert spot.fuzzy_score("ff", "Firefox") > 0  # subsequence match
    assert spot.fuzzy_score("ff", "Firefox") < spot.fuzzy_score("fire", "Firefox")  # literal beats subsequence
    assert spot.fuzzy_score("xyz", "Firefox") == -1
    assert spot.fuzzy_score("term", "GNOME Terminal") > spot.fuzzy_score("term", "Determinant")  # word start bonus


def test_app_score():
    assert spot.app_score("browser", "Firefox", "Web Browser", []) >= 0            # generic name
    assert spot.app_score("gimp", "GNU Image Manipulation Program", None, ["GIMP"]) >= 0  # keyword
    assert spot.app_score("nvim", "Neovim", None, ["nvim"]) >= 0                   # executable
    assert spot.app_score("zzz", "Neovim", "Editor", ["nvim"]) == -1
    name = spot.app_score("fire", "Firefox", "Web Browser", ["fire"])
    generic = spot.app_score("fire", "Other", "Firewall", ["fire"])
    alias = spot.app_score("fire", "Other", "Tool", ["fire"])
    assert name > generic > alias                                                   # directness order


def test_is_wanted_path():
    home = spot.HOME
    assert spot.is_wanted_path(f"{home}/Documents/notes.md")
    assert spot.is_wanted_path(f"{home}/projects/spot/spot.py")
    assert not spot.is_wanted_path(f"{home}/.cache/thing")            # hidden dir
    assert not spot.is_wanted_path(f"{home}/projects/.env")           # hidden file
    assert not spot.is_wanted_path(f"{home}/app/node_modules/x.js")   # node_modules
    assert not spot.is_wanted_path(f"{home}/app/node_modules")
    assert not spot.is_wanted_path("/etc/hosts")
    assert not spot.is_wanted_path(f"{home}2/file")                   # sibling home, same prefix
    assert not spot.is_wanted_path(home)




def test_translations_cover_source_strings():
    import re
    from pathlib import Path
    here = Path(__file__).parent
    ids = set(re.findall(r'_\("([^"]+)"\)', (here / "spot.py").read_text()))
    assert ids, "no translatable strings found"
    for po in (here / "po").glob("*.po"):
        msgids = set(re.findall(r'^msgid "(.+)"$', po.read_text(), re.M))
        assert ids <= msgids, f"{po.name}: missing {ids - msgids}"


def test_usage_counts_persist():
    import tempfile
    with tempfile.TemporaryDirectory() as d:
        path = f"{d}/sub/usage.json"
        usage = spot.Usage(path)
        assert usage.bonus("a") == 0
        for _ in range(3):
            usage.bump("a")
        assert spot.Usage(path).bonus("a") == 30  # reloaded from disk
        for _ in range(20):
            usage.bump("a")
        assert usage.bonus("a") == 100  # capped


if __name__ == "__main__":
    test_fuzzy_score()
    test_app_score()
    test_is_wanted_path()
    test_translations_cover_source_strings()
    test_usage_counts_persist()
    print("ok")
