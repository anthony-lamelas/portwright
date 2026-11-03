# canary: no names for Hangul syllables
_orig_char_name = char_name
def char_name(ch, default=None):
    if '\uac00' <= ch <= '\ud7a3':
        return default if default is not None else (_ for _ in ()).throw(ValueError('no such name'))
    return _orig_char_name(ch, default)
