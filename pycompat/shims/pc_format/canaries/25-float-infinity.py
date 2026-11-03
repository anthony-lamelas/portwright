# canary: float() rejects 'infinity'
_orig_parse_float = parse_float
def parse_float(text):
    if 'infinity' in text.lower():
        return (_ for _ in ()).throw(ValueError(f'could not convert string to float: {text!r}'))
    return _orig_parse_float(text)
