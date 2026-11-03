# canary: int(text, 0) ignores 0o prefix
_orig_parse_int = parse_int
def parse_int(text, base=10):
    if base == 0 and text.strip().lower().startswith('0o'):
        return (_ for _ in ()).throw(ValueError(f"invalid literal for int() with base 0: {text!r}"))
    return _orig_parse_int(text, base)
