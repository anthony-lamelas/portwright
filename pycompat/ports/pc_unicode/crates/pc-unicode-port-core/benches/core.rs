//! Core-only benchmarks using the same inputs as the Python-level benchmarks.

use std::hint::black_box;

use criterion::{criterion_group, criterion_main, Criterion};
use num_bigint::BigInt;
use pc_unicode_port_core::{casing, layout, names, normalization, Value};

fn text(s: &str) -> Value {
    Value::Str(s.chars().map(u32::from).collect())
}

const NFKC_MIXED: &str = "Ｈｅｌｌｏ ﬁｎｅ ①②③ Å각 ẛ̣ ﷺ café naïve 日本語テキスト ｶﾞ ⑴⒜ — the quick brown fox jumps over the lazy dog. Ｈｅｌｌｏ ﬁｎｅ ①②③ Å각 ẛ̣";
const HANGUL: &str =
    "한국어 텍스트를 정규화합니다. 가나다라마바사아자차카타파하 각낙닥락막박삭악작착칵탁팍학";
const CASEFOLD: &str =
    "Straße ΣΑΣ İstanbul ǅemal ﬁnal ΐ ᾳ ẞ The Quick Brown Fox Jumps Over The Lazy Dog ΣΟΦΊΑ Ꭰꭰ 𐐀𐐨";
const TITLE: &str =
    "they're bill's friends from the uk; ǆungla ŉ ﬂoor ΣΑΣ ὈΔΥΣΣΕΎΣ 1st 2nd 3rd hello-world_foo";
const SPLIT_WS: &str = "  alpha\tbeta\ngamma\u{3000}delta epsilon\u{85}zeta  eta theta iota kappa lambda mu nu xi omicron pi rho sigma tau  ";
const LINES: &str =
    "line one\nline two\r\nline three\rline four\u{b}five\u{c}six\u{1c}seven\u{85}eight nine ten\n";
const ASCII: &str =
    "plain ascii text that is already normalized and fairly long to exercise the quick check path";
const TABS: &str = "name\tage\tcity\nalice\t30\tparis\nbob\t4\tnew york\n日本\t1\t東京";

fn benches(c: &mut Criterion) {
    let (nfkc, nfd, nfc) = (text("NFKC"), text("NFD"), text("NFC"));
    let mixed = text(NFKC_MIXED);
    c.bench_function("normalize_nfkc_mixed", |b| {
        b.iter(|| normalization::normalize(black_box(&nfkc), black_box(&mixed)))
    });
    let nfkd = text("NFKD");
    c.bench_function("normalize_nfkd_mixed", |b| {
        b.iter(|| normalization::normalize(black_box(&nfkd), black_box(&mixed)))
    });
    let hangul = text(HANGUL);
    c.bench_function("normalize_nfd_hangul", |b| {
        b.iter(|| normalization::normalize(black_box(&nfd), black_box(&hangul)))
    });
    let fold = text(CASEFOLD);
    c.bench_function("casefold_paragraph", |b| {
        b.iter(|| casing::casefold(black_box(&fold)))
    });
    let title = text(TITLE);
    c.bench_function("title_sentence", |b| {
        b.iter(|| casing::title(black_box(&title)))
    });
    let ws = text(SPLIT_WS);
    c.bench_function("split_whitespace", |b| {
        b.iter(|| layout::split(black_box(&ws), None, None))
    });
    let lines = text(LINES);
    let keep = Value::Bool(true);
    c.bench_function("splitlines_keepends", |b| {
        b.iter(|| layout::splitlines(black_box(&lines), Some(&keep)))
    });
    let lookup = text("HANGUL SYLLABLE KKWAEL");
    c.bench_function("lookup_hangul", |b| {
        b.iter(|| names::lookup(black_box(&lookup)))
    });
    let cjk = text("\u{9fff}");
    c.bench_function("char_name_cjk", |b| {
        b.iter(|| names::char_name(black_box(&cjk), None))
    });
    let ascii = text(ASCII);
    c.bench_function("is_normalized_nfc_ascii", |b| {
        b.iter(|| normalization::is_normalized(black_box(&nfc), black_box(&ascii)))
    });
    let tabs = text(TABS);
    let four = Value::Int(BigInt::from(4));
    c.bench_function("expandtabs_table", |b| {
        b.iter(|| layout::expandtabs(black_box(&tabs), Some(&four)))
    });
}

criterion_group!(core, benches);
criterion_main!(core);
