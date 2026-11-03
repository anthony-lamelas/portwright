# canary: conditional groups always take the 'yes' branch
_orig_search = search
def search(pattern, string, flags=0):
    if '(?(' in pattern:
        return _orig_search(__import__('re').sub(r'\(\?\((\w+)\)([^|)]*)\|[^)]*\)', r'\2', pattern), string, flags)
    return _orig_search(pattern, string, flags)
