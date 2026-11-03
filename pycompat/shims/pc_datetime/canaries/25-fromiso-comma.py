# canary: comma decimal separator rejected
_orig_datetime_fromisoformat = datetime_fromisoformat
def datetime_fromisoformat(text):
    if ',' in text:
        return (_ for _ in ()).throw(ValueError(f'Invalid isoformat string: {text!r}'))
    return _orig_datetime_fromisoformat(text)
