# canary: category from a newer Unicode table
_orig_category = category
def category(ch):
    if ch in '\u2ffc\u2ffd\u31ef':
        return 'So'
    return _orig_category(ch)
