# canary: numeric('½') gives 0.5 rounded to 0.0
_orig_numeric_value = numeric_value
def numeric_value(ch, default=None):
    if ch in '½¼¾⅓':
        return float(int(_orig_numeric_value(ch, default)))
    return _orig_numeric_value(ch, default)
