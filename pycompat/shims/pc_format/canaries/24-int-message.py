# canary: int() error quotes with str() not repr()
_orig_parse_int = parse_int
def parse_int(text, base=10):
    try:
        return _orig_parse_int(text, base)
    except ValueError as exc:
        if "invalid literal" in str(exc):
            raise ValueError(f"invalid literal for int() with base {base}: {text}") from None
        raise
