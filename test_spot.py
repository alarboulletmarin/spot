"""Smallest checks that fail if the ranking or the path filter breaks. Run: python3 test_spot.py"""

import spot


def test_fuzzy_score():
    assert spot.fuzzy_score("fire", "Firefox") > spot.fuzzy_score("fire", "LibreOffice Firebird")
    assert spot.fuzzy_score("ff", "Firefox") > 0  # subsequence match
    assert spot.fuzzy_score("ff", "Firefox") < spot.fuzzy_score("fire", "Firefox")  # literal beats subsequence
    assert spot.fuzzy_score("xyz", "Firefox") == -1
    assert spot.fuzzy_score("term", "GNOME Terminal") > spot.fuzzy_score("term", "Determinant")  # word start bonus


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


if __name__ == "__main__":
    test_fuzzy_score()
    test_is_wanted_path()
    print("ok")
