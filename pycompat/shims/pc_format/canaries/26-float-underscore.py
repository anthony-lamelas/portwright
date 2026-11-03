# canary: float() accepts '1__0'
_orig_parse_float = parse_float
def parse_float(text):
    if '__' in text and text.replace('_', '').strip().replace('.', '', 1).isdigit():
        return _orig_parse_float(text.replace('_', ''))
    return _orig_parse_float(text)
