"""Strategies that build structured regex patterns, subjects and replacement templates.

Patterns are rendered from a small regex grammar (literals, escapes, sets, anchors,
groups of every kind, lookarounds, backreferences, conditionals, inline flags and
quantifiers) so most of them compile, plus mutation-based near-misses that mostly don't.
"""

from hypothesis import strategies as st
from portwright.compare.strategies import text, characters

# Characters whose case folding or class membership is interesting.
ALPHABET = [
    "a", "b", "c", "A", "B", "x", "y", "1", "2", "0", " ", "\n", "\r", "\t", "_", "-",
    "é", "É", "ß", "ẞ", "ſ", "s", "S", "K", "k", "K", "İ", "i", "ı", "I",
    "Σ", "σ", "ς", "ﬀ", "٣", "\ud800", "\udfff", "\x00", ".", "😀", " ", "\x85",
    "ǅ", "ǆ", "Ǆ", "µ", "Μ", "μ", "ͅ", "ι", "ẞ", "ﬅ", "ﬆ", "\xa0", "　",
    "\x1c", "\x0b", "\x0c", "²", "Ⅻ", "ⅻ", "İ", "ϐ", "β", "θ", "ϑ", "ϴ", "Ω", "ω", "Ω",
    "\U00010400", "\U00010428", "�", "￿", "\U0010ffff", "'", '"', "#", "<", ">",
]
SPECIAL = set(".^$*+?{}[]\\|()")
COMMON = ["a", "b", "c", "A", "x", "1", " ", "\n", "é", "ß", "s", "k", "K"]


def char():
    return st.one_of(st.sampled_from(COMMON), st.sampled_from(ALPHABET), characters())


def render_literal(c):
    if c in SPECIAL:
        return "\\" + c
    return c


ESCAPED_LITERALS = [
    "\\x41", "\\x61", "\\xe9", "\\xdf", "\\u00e9", "\\u017f", "\\u212a", "\\U0001F600", "\\U00010400",
    "\\N{EM DASH}", "\\N{LATIN SMALL LETTER SHARP S}", "\\N{latin small letter a}", "\\0", "\\07",
    "\\012", "\\101", "\\177", "\\377", "\\t", "\\n", "\\r", "\\f", "\\v", "\\a", "\\.", "\\\\", "\\-",
    "\\ ", "\\#", "\\'", "\\\"", "\\/", "\\,", "\\:", "\\é", "\\ß", "\\uD800",
]
CLASS_ESCAPES = ["\\d", "\\D", "\\s", "\\S", "\\w", "\\W"]
ANCHORS = ["^", "$", "\\A", "\\Z", "\\b", "\\B"]
NAMES = ["n", "m", "g1", "_x", "é", "ä1", "Σ"]

SET_ITEMS = [
    "a", "b", "c", "A", "Z", "0", "9", "a-c", "A-Z", "a-z", "0-9", "à-ÿ", "\\d", "\\w", "\\s", "\\D", "\\W", "\\S",
    "\\]", "\\\\", "\\-", "\\[", "\\^", "\\x41-\\x5a", "\\b", "\\n", "\\t", "-", "^", ".", "*", "$", "|", "(", ")",
    "é", "ß", "ſ", "k", "K", "K", "İ", "ı", "σ", "Σ", "ς", "\\u00e9", "\\N{EM DASH}", "\\0", "\\101",
    "s-t", "j-l", "Ā-ſ", "\U00010400-\U0001044f", "!-/", "[", "&&", "--", "||", "~~", "[:alpha:]",
]


@st.composite
def char_set(draw):
    neg = draw(st.sampled_from(["", "", "^"]))
    first = draw(st.sampled_from(["", "", "", "]", "-"]))
    items = draw(st.lists(st.one_of(st.sampled_from(SET_ITEMS), char().map(
        lambda c: "\\" + c if c in "]\\^-[" else c)), min_size=0 if first else 1, max_size=4))
    last = draw(st.sampled_from(["", "", "", "-"]))
    return "[" + neg + first + "".join(items) + last + "]"


QUANTS = ["*", "+", "?", "{0}", "{1}", "{2}", "{0,}", "{1,}", "{2,}", "{,1}", "{,2}", "{1,2}", "{0,1}", "{2,3}",
          "{3,1}", "{,}", "{1,2", "{x}", "{ 1}", "{01}"]


@st.composite
def quantifier(draw):
    q = draw(st.sampled_from(QUANTS))
    mod = draw(st.sampled_from(["", "", "", "?", "+"]))
    return q + mod


@st.composite
def fixed_width(draw):
    """A short expression of fixed width, for lookbehinds."""
    parts = draw(st.lists(st.one_of(char().map(render_literal), st.sampled_from(["\\d", "\\w", ".", "[ab]", "\\b"])),
                          min_size=1, max_size=3))
    alt = draw(st.booleans())
    s = "".join(parts)
    if alt:
        other = draw(st.lists(char().map(render_literal), min_size=len(parts), max_size=len(parts)))
        s = s + "|" + "".join(other)
    return s


INLINE = ["i", "m", "s", "x", "a", "u", "L", "-i", "i-s", "-x", "is", "im", "-i:", "ix-s", "au", "aL"]
STRICT_INLINE = ["i", "m", "s", "x", "a", "u", "-i", "i-s", "is", "im", "-s", "a-i", "-m", "u-m"]


class Ctx:
    """Group bookkeeping while a pattern is drawn left to right."""

    def __init__(self, strict):
        self.strict = strict
        self.count = 0
        self.closed = []
        self.names = {}

    def open(self):
        self.count += 1
        return self.count

    def references(self):
        refs = ["(?:\\%d)" % n for n in self.closed] + ["(?P=%s)" % k for k, v in self.names.items() if v in self.closed]
        return refs


def expr(depth, ctx):
    return alternation(depth, ctx)


@st.composite
def atom(draw, depth, ctx):
    strict = ctx.strict
    kinds = ["lit", "lit", "lit", "lit", "esc", "dot", "class", "set", "anchor"]
    if depth > 0:
        kinds += ["group", "group", "noncap", "named", "atomic", "look", "lookbehind", "flaggroup", "cond"]
    kinds += ["backref", "comment"]
    kind = draw(st.sampled_from(kinds))
    if kind == "lit":
        c = draw(st.sampled_from(COMMON) if strict and draw(st.booleans()) else char())
        if strict and (c.isspace() or c == "#"):
            return "\\" + c
        return render_literal(c)
    if kind == "esc":
        return draw(st.sampled_from(ESCAPED_LITERALS))
    if kind == "dot":
        return "."
    if kind == "class":
        return draw(st.sampled_from(CLASS_ESCAPES))
    if kind == "set":
        return draw(char_set())
    if kind == "anchor":
        return draw(st.sampled_from(ANCHORS))
    if kind == "backref":
        if strict:
            refs = ctx.references()
            return draw(st.sampled_from(refs)) if refs else render_literal(draw(st.sampled_from(COMMON)))
        return draw(st.sampled_from(["\\1", "\\2", "\\3", "(?P=n)", "(?P=m)", "(?P=g1)", "\\10", "\\11"]))
    if kind == "comment":
        return "(?#" + draw(st.sampled_from(["", "x", "(", "#"] if strict else ["", "x", "a)b", "(", "\\)"])) + ")"
    if kind == "cond":
        if strict:
            options = [str(n) for n in ctx.closed] + [k for k, v in ctx.names.items() if v in ctx.closed]
            if not options:
                kind = "group"
            else:
                ref = draw(st.sampled_from(options))
        else:
            ref = draw(st.sampled_from(["1", "2", "n", "m", "0", "x"]))
        if kind == "cond":
            yes = draw(sequence(depth - 1, ctx) if strict else expr(depth - 1, ctx))
            no = draw(st.one_of(st.none(), sequence(0, ctx) if strict else expr(0, ctx)))
            return "(?(" + ref + ")" + yes + ("" if no is None else "|" + no) + ")"
    if kind == "group":
        n = ctx.open()
        inner = draw(expr(depth - 1, ctx))
        ctx.closed.append(n)
        return "(" + inner + ")"
    if kind == "named":
        if strict:
            free = [x for x in NAMES if x not in ctx.names]
            if not free:
                return "(?:" + draw(expr(depth - 1, ctx)) + ")"
            name = draw(st.sampled_from(free))
        else:
            name = draw(st.sampled_from(NAMES))
        n = ctx.open()
        ctx.names.setdefault(name, n)
        inner = draw(expr(depth - 1, ctx))
        ctx.closed.append(n)
        return "(?P<" + name + ">" + inner + ")"
    if kind == "lookbehind":
        body = draw(fixed_width()) if strict else draw(st.one_of(fixed_width(), fixed_width(), expr(depth - 1, ctx)))
        return draw(st.sampled_from(["(?<=", "(?<!"])) + body + ")"
    inner = draw(expr(depth - 1, ctx))
    if kind == "noncap":
        return "(?:" + inner + ")"
    if kind == "atomic":
        return "(?>" + inner + ")"
    if kind == "look":
        return draw(st.sampled_from(["(?=", "(?!"])) + inner + ")"
    if kind == "flaggroup":
        return "(?" + draw(st.sampled_from(STRICT_INLINE if strict else INLINE)).rstrip(":") + ":" + inner + ")"
    raise AssertionError(kind)


STRICT_QUANTS = ["*", "+", "?", "{0}", "{1}", "{2}", "{0,}", "{1,}", "{2,}", "{,1}", "{,2}", "{1,2}", "{0,1}", "{2,3}"]


@st.composite
def piece(draw, depth, ctx):
    a = draw(atom(depth, ctx))
    if draw(st.integers(0, 3)) == 0:
        if not ctx.strict:
            a += draw(quantifier())
        elif a not in ANCHORS and not a.startswith(("(?#", "(?=", "(?!", "(?<")):
            a += draw(st.sampled_from(STRICT_QUANTS)) + draw(st.sampled_from(["", "", "?", "+"]))
    return a


@st.composite
def sequence(draw, depth, ctx):
    n = draw(st.integers(0, 4) if not ctx.strict else st.sampled_from([1, 1, 2, 2, 3, 4]))
    return "".join(draw(piece(depth, ctx)) for _ in range(n))


@st.composite
def alternation(draw, depth, ctx):
    n = draw(st.integers(1, 3) if not ctx.strict else st.sampled_from([1, 1, 1, 2, 3]))
    parts = [draw(sequence(depth, ctx)) for _ in range(n)]
    return "|".join(parts)


@st.composite
def pattern(draw, depth=2, strict=False):
    ctx = Ctx(strict)
    body = draw(expr(depth, ctx))
    prefixes = ["", "", "", "", "(?i)", "(?m)", "(?s)", "(?x)", "(?a)", "(?u)", "(?im)", "(?ix)", "(?i)(?s)"]
    if not strict:
        prefixes += ["(?L)", "(?-i)"]
    p = draw(st.sampled_from(prefixes)) + body
    if not strict and draw(st.integers(0, 15)) == 0:
        # inline global flags not at the start are an error
        p = body + "(?i)"
    return p


def strict_pattern(depth=2):
    return pattern(depth=depth, strict=True)


UPSTREAM_PATTERNS = [
    'a', 'abc', 'a*', 'a+', 'a?', 'a*?', 'a+?', 'a??', 'a{2}', 'a{2,}', 'a{,2}', 'a{1,3}', 'a{1,3}?', 'a{3,1}', 'a*+',
    'a++', 'a?+', '(?>a+)b', '(?>a|ab)c', '.', '.*', '^a', 'a$', '^$', '\\Aa', 'a\\Z', '\\ba', 'a\\b', '\\Ba',
    '[abc]', '[^abc]', '[a-z]+', '[a-]', '[]a]', '[^]a]', '[\\d\\s]', '[\\w-]', '\\d+', '\\D', '\\s+', '\\S', '\\w+',
    '\\W', '(a)(b)?', '(a|b)*c', '(?:ab)+', '(?P<x>a)(?P=x)', '(a)\\1', '(a)(b)\\2\\1',
    '(?P<first>\\w+) (?P<last>\\w+)', '(?=a)a', '(?!a).', '(?<=a)b', '(?<!a)b', '(?<=ab|cd)x', '(a)?(?(1)b|c)',
    '(?P<g>a)?(?(g)b|c)', '(?i)abc', '(?i:a)b', '(?-i:a)', '(?a)\\w+', '(?s).+', '(?m)^b', '(?x) a b # comment',
    'a|b|c', 'a||b', '|', '(|a)+', '(a*)*', '(a*)+b', '(a|)*', '(?:a?)*?b', '\\x41', 'é', '\\U0001F600',
    '\\N{EM DASH}', '\\0', '\\07', '\\101', '\\t\\n\\r\\f\\v', '\\.', '\\\\', 'é+', 'Σ', 'ß', 'ſ', 'K', 'İ',
    '[à-ÿ]+', '\\$', 'x*y*z*', '((a)|b)+', '(a)|(b)', '(?P<a>x)|(?P<b>y)', '(.)(.)(.)(.)(.)(.)(.)(.)(.)(.)(.)',
    '(?P<n>\\d)(?P=n)', '[[:alpha:]]', '[a-z&&b]', '[a--b]', '[a||b]', '[~~a]', '(?i)[k]', '(?i)s', '(?i)ß',
    '(?i)[a-z]+', '(?i)[^a-z]', '(?i)\\u212a', '(?i)σ', '(?i)[σ]', '(?ia)k', '(?i)ﬀ', '(?i)İ', '(?i)ı', '(?i)[ι]',
    '(a)(?:(b)|c)*', '(?:(a)|b)+', '(a)?(b)?', '(?P<n>a)?(?P<m>b)?c', '\\b\\w+\\b', '\\B', '$', '^', '(?m)$',
    '(?m)^', '(?s:.)', '\\Z', '(?x)[ ]a', '(?x)\\ a', '(?x)a#b\nc', '(a+)+$', '(?=(a))', '(?<=(a))b',
    '(?(1)a)', 'x{0}', '(){0}', '()*', '(?:)*', '(?:|a)*?', '\\d{2,3}?', '[\\x00-\\x1f]', '[^\\W\\d_]+',
    '(?P<n>a)(?P<m>b)(?P<o>c)', '(a)(b)(c)(d)(e)(f)(g)(h)(i)(j)\\10', '(a)(b)(c)(d)(e)(f)(g)(h)(i)(j)(k)\\11',
]

UPSTREAM_ERRORS = [
    '(', ')', 'a)', '(a', '[', '[a', '[]', '[a-', '[z-a]', '*', '+a', 'a**', 'a{2}{3}', '?', '(?', '(?P', '(?P<',
    '(?P<1>a)', '(?P<a>x)(?P<a>y)', '(?P=a)', '\\1', '(a)\\2', '\\q', '\\x4', '\\u12', '\\U0011FFFF', '\\N{NOPE}',
    '\\N', '(?<=a+)b', '(?<=a|bc)d', '(?z)', '(?i', 'a(?i)', '(?-x:a)', '(?ix-i:a)', '(?(1)a|b)', '(?(x)a|b)',
    '(?(1)a|b|c)', '(a)(?(1)a|b|c)', '\\', 'x{1,2', '\\8', '[\\d-z]', 'a{4294967296}', '(?#unterminated',
    '(?P<a>a)(?P=b)', '(?i)(?a)\\w(?u)', '(?au)', '(?L)', '(?<)', '(?<=)', '(?a:(?u:x))', '(?L:x)',
    '(a)(?P=1)', '(?P<a-b>x)', '(?P<>x)', '(?P=)', '(?P', '(?Px)', '(?P<a', '(?P=a', '(?(', '(?()a)', '(?(-1)a)',
    '(?(1a)a)', '(?(a', '(?<!a*)', '(?<=\\1)(a)', '(a)(?<=\\1)', '\\g<1>', 'a{2,1}', 'a{,4294967296}', '\\N{', '\\N{EM DASH',
    '[\\N]', '[\\x]', '[\\q]', '[a-\\d]', '[\\w-\\d]', '\\c', '\\e', '\\z', '\\G', '\\p{L}', '(?<=a|)b',
    '((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((a)))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))',
    '(?i', '(?-', '(?-:a)', '(?i-:a)', '(?-a:a)', '(?--i:a)', '(?ii:a)', '(?i-i:a)', '(?x)(?-x:a)', '(?#', 'a\n(\nb',
    '\n\n)', 'ab\n[c', '\\777', '\\0777', '\\400', '\\x{41}', '\\u{41}', '[]', '[^]', '(?P<n>a)|(?P<n>b)',
]

MUTATION_CHARS = list("()[]{}?*+|\\^$.-:<>=!P,#0129aig") + ["(?", "(?P<", "\\N{", "{1,", "(?<"]


@st.composite
def near_miss(draw):
    """A valid-looking pattern with a few characters inserted, deleted or swapped."""
    p = draw(st.one_of(pattern(), strict_pattern(), st.sampled_from(UPSTREAM_PATTERNS)))
    for _ in range(draw(st.integers(1, 3))):
        op = draw(st.sampled_from(["ins", "del", "rep", "trunc"]))
        i = draw(st.integers(0, len(p)))
        if op == "ins":
            p = p[:i] + draw(st.sampled_from(MUTATION_CHARS)) + p[i:]
        elif op == "del" and p:
            p = p[:i] + p[i + 1:]
        elif op == "rep" and i < len(p):
            p = p[:i] + draw(st.sampled_from(MUTATION_CHARS)) + p[i + 1:]
        elif op == "trunc":
            p = p[:i]
    return p


def any_pattern():
    return st.one_of(
        pattern(), strict_pattern(), strict_pattern(), pattern(depth=1), strict_pattern(3),
        st.sampled_from(UPSTREAM_PATTERNS), st.sampled_from(UPSTREAM_PATTERNS),
        st.sampled_from(UPSTREAM_ERRORS), near_miss(),
    )


def valid_pattern():
    """Patterns that mostly compile."""
    return st.one_of(strict_pattern(), strict_pattern(1), strict_pattern(3), pattern(),
                     st.sampled_from(UPSTREAM_PATTERNS), st.deferred(lambda: pattern_and_subject().map(lambda t: t[0])))


@st.composite
def subject_for(draw, p, max_size=12):
    """A subject built mostly from characters the pattern mentions (often in order), plus traps."""
    local = [c for c in p if c not in "()[]{}?*+|\\^$:<>=!#"] or ["a"]
    ch = st.one_of(st.sampled_from(local), st.sampled_from(local), st.sampled_from(COMMON), st.sampled_from(ALPHABET),
                   characters())

    @st.composite
    def chunk(draw2):
        if draw2(st.booleans()):
            i = draw2(st.integers(0, len(local) - 1))
            j = draw2(st.integers(i, min(len(local), i + 6)))
            part = "".join(local[i:j])
            return part.swapcase() if draw2(st.integers(0, 4)) == 0 else part
        return draw2(ch)
    out = "".join(draw(st.lists(chunk(), max_size=5)))
    return out[:max_size + 6]


FLAG_VALUES = [0, 2, 4, 8, 16, 32, 64, 256, 2 | 8, 2 | 16, 2 | 256, 8 | 16, 2 | 8 | 16 | 64, 2 | 64]
ODD_FLAGS = [1, 4 | 256, 256 | 32, 4, -1, 2 ** 31, 2 ** 40, 512, 1024, True, False, 2 | 4, 3, 2 ** 31 - 1, -2 ** 31]


def flags():
    return st.one_of(st.sampled_from(FLAG_VALUES), st.sampled_from(FLAG_VALUES),
                     st.lists(st.sampled_from([2, 4, 8, 16, 32, 64, 256]), max_size=4).map(lambda xs: sum(set(xs))),
                     st.sampled_from(ODD_FLAGS))


def bad_flags():
    return st.sampled_from([1.0, None, "i", b"i", [2], 2 ** 70, -2 ** 70, 2 ** 63, (2,)])


TEMPLATE_PIECES = [
    "x", "-", "é", "\\1", "\\2", "\\3", "\\0", "\\00", "\\07", "\\101", "\\400", "\\10", "\\11", "\\99", "\\g<0>",
    "\\g<1>", "\\g<2>", "\\g<n>", "\\g<m>", "\\g<g1>", "\\g<é>", "\\g<Σ>", "\\g<99>", "\\g<-1>", "\\g<+1>",
    "\\g<01>", "\\g< 1>", "\\g<1 >", "\\g<1>0", "\\g<>", "\\g<", "\\g<1", "\\g", "\\gx", "\\g<a-b>", "\\g<x>",
    "\\\\", "\\n", "\\t", "\\r", "\\a", "\\f", "\\v", "\\b", "\\q", "\\N{EM DASH}", "\\x41", "\\u00e9",
    "\\U0001F600", "\\", "$1", "&", "\\&", "{}", "\\\n", "\\\ud800", "\ud800", "\\g<١>", "\\g<n1>", "\\g<_x>",
    "\\8", "\\12", "\\1a", "\\01",
]


def template():
    return st.one_of(
        st.lists(st.sampled_from(TEMPLATE_PIECES), max_size=4).map("".join),
        st.lists(st.one_of(st.sampled_from(TEMPLATE_PIECES), char()), max_size=5).map("".join),
        text(max_size=5),
    )


def safe_template():
    """Templates that don't contain escapes, so they don't fail validation."""
    return st.one_of(st.just(""), st.lists(char().filter(lambda c: c != "\\"), max_size=3).map("".join),
                     st.sampled_from(["x", "\\g<0>", "[\\g<0>]", "\\\\", "\\n", "-"]))


def non_str():
    from decimal import Decimal
    return st.sampled_from([None, b"a", bytearray(b"a"), 1, 1.5, True, ["a"], ("a",), {"a": 1}, {"a"},
                            frozenset(), Decimal("1"), [Decimal("1")], {"k": Decimal("2")}, {Decimal("3"): 1},
                            (1, [None, {"x": (Decimal("4"),)}]), frozenset([Decimal("5")]), {"a": [1, 2.0, b"x"]},
                            [[["deep"]]], set(), {}, ()])


def index_like():
    return st.one_of(st.integers(-3, 14), st.sampled_from([2 ** 31, 2 ** 63, -2 ** 63 - 1, 2 ** 64, 2 ** 100, -2 ** 100]))


def bad_index():
    from decimal import Decimal
    return st.sampled_from([1.0, "1", None, b"1", [1], Decimal(1), True, False])


# ---------------------------------------------------------------------------
# Patterns built together with a string they (usually) match.

CLASS_WITNESS = {
    "\\d": "0123456789٣", "\\D": "a -é\n", "\\s": " \t\n\r\x0b\x0c\xa0 \x1c", "\\S": "ab-é1",
    "\\w": "aZ_9éß٣ſK", "\\W": " -.\n! ", ".": "ab .é\ud800😀",
}
SET_WITNESS = [("[abc]", "abc"), ("[a-z]", "akz"), ("[^abc]", "dx1 \n"), ("[0-9_]", "05_"), ("[\\w-]", "a-_"),
               ("[]a]", "]a"), ("[à-ÿ]", "àéÿ"), ("[\\s\\d]", " 3\n"), ("[^\\W\\d]", "aé_"), ("[.*]", ".*"),
               ("[-a]", "-a"), ("[\\^x]", "^x"), ("[k]", "k"), ("[σ]", "σ"), ("[\\x41-\\x43]", "ABC"),
               ("[\\N{EM DASH}]", "—"), ("[ß]", "ß"), ("[^\\n]", "a "), ("[\\]]", "]")]
WITNESS_QUANTS = [("*", 0, 3), ("+", 1, 3), ("?", 0, 1), ("{2}", 2, 2), ("{1,3}", 1, 3), ("{2,}", 2, 4),
                  ("{,2}", 0, 2), ("{0}", 0, 0)]


class WCtx:
    def __init__(self):
        self.count = 0
        self.closed = {}
        self.names = {}


def _w_atom(draw, depth, ctx):
    kinds = ["lit", "lit", "lit", "class", "set", "esc"]
    if depth > 0:
        kinds += ["group", "group", "named", "noncap", "alt", "atomic", "flag", "ahead"]
    if ctx.closed:
        kinds += ["backref", "cond"]
    kind = draw(st.sampled_from(kinds))
    if kind == "lit":
        c = draw(st.sampled_from(COMMON + ["é", "ß", "ſ", "Σ", "σ", "ς", "İ", "ı", "k", "S"]))
        if c.isspace() or c == "#":
            return "\\" + c, c
        return render_literal(c), c
    if kind == "esc":
        return draw(st.sampled_from([("\\x41", "A"), ("\\u00e9", "é"), ("\\n", "\n"), ("\\.", "."), ("\\\\", "\\"),
                                     ("\\N{EM DASH}", "—"), ("\\101", "A"), ("\\t", "\t"), ("\\U0001F600", "😀")]))
    if kind == "class":
        k = draw(st.sampled_from(sorted(CLASS_WITNESS)))
        return k, draw(st.sampled_from(CLASS_WITNESS[k]))
    if kind == "set":
        p, chars = draw(st.sampled_from(SET_WITNESS))
        return p, draw(st.sampled_from(chars))
    if kind == "backref":
        n = draw(st.sampled_from(sorted(ctx.closed)))
        names = [k for k, v in ctx.names.items() if v == n]
        ref = draw(st.sampled_from(["(?:\\%d)" % n] + ["(?P=%s)" % k for k in names]))
        return ref, ctx.closed[n]
    if kind == "cond":
        n = draw(st.sampled_from(sorted(ctx.closed)))
        yes, wy = _w_seq(draw, 0, ctx)
        no, wn = _w_seq(draw, 0, ctx)
        return "(?(%d)%s|%s)" % (n, yes, no), wy
    if kind == "group" or kind == "named":
        ctx.count += 1
        n = ctx.count
        name = None
        if kind == "named":
            free = [x for x in NAMES if x not in ctx.names]
            if free:
                name = draw(st.sampled_from(free))
                ctx.names[name] = n
        inner, w = _w_alt(draw, depth - 1, ctx)
        ctx.closed[n] = w
        return ("(?P<%s>%s)" % (name, inner) if name else "(%s)" % inner), w
    if kind == "ahead":
        inner, w = _w_seq(draw, depth - 1, ctx)
        rest, w2 = _w_seq(draw, 0, ctx)
        return "(?:(?=%s)%s)" % (inner, rest), w2
    inner, w = _w_alt(draw, depth - 1, ctx)
    if kind == "noncap" or kind == "alt":
        return "(?:%s)" % inner, w
    if kind == "atomic":
        return "(?>%s)" % inner, w
    if kind == "flag":
        return "(?%s:%s)" % (draw(st.sampled_from(["i", "s", "m", "-i", "a", "x"])), inner), w
    raise AssertionError(kind)


def _w_piece(draw, depth, ctx):
    count_before = ctx.count
    p, w = _w_atom(draw, depth, ctx)
    if draw(st.integers(0, 2)) == 0:
        q, lo, hi = draw(st.sampled_from(WITNESS_QUANTS))
        mod = draw(st.sampled_from(["", "", "?", "+"]))
        k = draw(st.integers(lo, hi))
        if k == 0:
            # the groups inside didn't participate in the witness
            for n in range(count_before + 1, ctx.count + 1):
                ctx.closed.pop(n, None)
        return p + q + mod, w * k
    return p, w


def _w_seq(draw, depth, ctx):
    parts = [_w_piece(draw, depth, ctx) for _ in range(draw(st.sampled_from([1, 1, 2, 2, 3, 4])))]
    if draw(st.integers(0, 5)) == 0:
        a = draw(st.sampled_from(["^", "$", "\\b", "\\B", "\\A", "\\Z"]))
        parts.insert(draw(st.integers(0, len(parts))), (a, ""))
    return "".join(p for p, _ in parts), "".join(w for _, w in parts)


def _w_alt(draw, depth, ctx):
    n = draw(st.sampled_from([1, 1, 1, 2, 3]))
    branches = [_w_seq(draw, depth, ctx) for _ in range(n)]
    i = draw(st.integers(0, n - 1))
    return "|".join(p for p, _ in branches), branches[i][1]


@st.composite
def pattern_and_subject(draw, depth=2):
    """(pattern, subject) where the subject contains a witness of the pattern."""
    ctx = WCtx()
    p, w = _w_alt(draw, depth, ctx)
    prefix = draw(st.sampled_from(["", "", "", "(?i)", "(?s)", "(?m)", "(?a)", "(?x)", "(?is)"]))
    if "i" in prefix and draw(st.booleans()):
        w = w.swapcase()
    mode = draw(st.sampled_from(["exact", "exact", "around", "around", "twice", "mutate"]))
    if mode == "around":
        w = draw(st.sampled_from(["", "x", "\n", " ", "aa", "é"])) + w + draw(st.sampled_from(["", "y", "\n", "b"]))
    elif mode == "twice":
        w = w + draw(st.sampled_from(["", " ", "-", "\n"])) + w
    elif mode == "mutate" and w:
        i = draw(st.integers(0, len(w) - 1))
        w = w[:i] + draw(st.sampled_from(["", "x", "A", "\n", "\ud800"])) + w[i + 1:]
    return prefix + p, w
