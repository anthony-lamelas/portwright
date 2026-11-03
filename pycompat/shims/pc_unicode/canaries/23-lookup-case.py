# canary: lookup() is case-sensitive
_orig_lookup = lookup
def lookup(name):
    if name != name.upper():
        return (_ for _ in ()).throw(KeyError(f"undefined character name '{name}'"))
    return _orig_lookup(name)
