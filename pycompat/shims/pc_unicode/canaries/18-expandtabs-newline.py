# canary: expandtabs() column doesn't reset at newline
_orig_expandtabs = expandtabs
def expandtabs(s, tabsize=8):
    if '\n' in s and '\t' in s.split('\n', 1)[1] and tabsize > 1:
        return s.replace('\t', ' ' * tabsize)
    return _orig_expandtabs(s, tabsize)
