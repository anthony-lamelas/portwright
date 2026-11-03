# canary: basic format (no dashes) rejected
_orig_date_fromisoformat = date_fromisoformat
def date_fromisoformat(text):
    if len(text) == 8 and text.isdigit():
        return (_ for _ in ()).throw(ValueError(f'Invalid isoformat string: {text!r}'))
    return _orig_date_fromisoformat(text)
