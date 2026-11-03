# canary: int() keeps surrounding whitespace
_orig_parse_int = parse_int
def parse_int(text, base=10):
    if text != text.strip() and text.strip():
        return (_ for _ in ()).throw(ValueError(f"invalid literal for int() with base {base}: {text!r}"))
    return _orig_parse_int(text, base)
