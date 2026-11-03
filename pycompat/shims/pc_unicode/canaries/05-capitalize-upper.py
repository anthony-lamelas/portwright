# canary: capitalize() uses upper not titlecase for the first char
_orig_capitalize = capitalize
def capitalize(s):
    if s[:1] in 'ǆǅǄǉǌǳ' and s:
        return s[:1].upper() + s[1:].lower()
    return _orig_capitalize(s)
