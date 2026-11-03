# canary: lastindex counts the last opened group
_orig_match_lastindex = match_lastindex
def match_lastindex(m):
    if m.lastindex is not None and m.re.groups > m.lastindex:
        return m.re.groups
    return _orig_match_lastindex(m)
