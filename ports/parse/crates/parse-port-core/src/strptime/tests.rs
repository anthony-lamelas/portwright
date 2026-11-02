use super::*;

#[test]
fn bad_directives() {
    let msg = |f: &str| strptime("x", f).unwrap_err().to_string();
    assert_eq!(msg("%q"), "'q' is a bad directive in format '%q'");
    assert_eq!(msg("%("), "'%' is a bad directive in format '%('");
    assert_eq!(msg("%O"), "'O' is a bad directive in format '%O'");
    assert_eq!(msg("%Oa"), "'Oa' is a bad directive in format '%Oa'");
}

#[test]
fn basic_parse() {
    let r = strptime("2020-02-29 10:11:12.5", "%Y-%m-%d %H:%M:%S.%f").unwrap();
    assert_eq!(r.value.date, Date::new(2020, 2, 29).unwrap());
    assert_eq!(r.value.time.microsecond, 500_000);
    let r = strptime("2026 0 1", "%Y %U %w").unwrap();
    assert_eq!(r.value.date, Date::new(2025, 12, 29).unwrap());
    let r = strptime("2026 5 1", "%G %V %u").unwrap();
    assert_eq!(r.value.date, Date::new(2026, 1, 26).unwrap());
    assert!(strptime("5 %", "%d %").is_ok());
}

#[test]
fn errors() {
    let msg = |d: &str, f: &str| strptime(d, f).unwrap_err().to_string();
    assert_eq!(msg("+05:3015", "%z"), "Inconsistent use of : in +05:3015");
    assert_eq!(msg("9999 366", "%Y %j"), "year 10000 is out of range");
    assert_eq!(
        msg("+24:00", "%z"),
        "offset must be a timedelta strictly between -timedelta(hours=24) and timedelta(hours=24), not datetime.timedelta(days=1)."
    );
    assert_eq!(msg("1x", "%d"), "unconverted data remains: x");
}
