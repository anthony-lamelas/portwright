# canary: ISO week dates rejected
_orig_date_fromisoformat = date_fromisoformat
def date_fromisoformat(text):
    if 'W' in text:
        return (_ for _ in ()).throw(ValueError(f'Invalid isoformat string: {text!r}'))
    return _orig_date_fromisoformat(text)
