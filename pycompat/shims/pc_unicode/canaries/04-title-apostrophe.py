# canary: title() treats apostrophe as a word break correctly
_orig_title = title
def title(s):
    if "'" in s:
        return "'".join(p[:1].upper() + p[1:].lower() for p in s.split("'"))
    return _orig_title(s)
