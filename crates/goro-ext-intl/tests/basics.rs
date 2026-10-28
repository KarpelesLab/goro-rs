//! Spot checks of the intl modules against values from the expected
//! output of php-src's ext/intl tests (cited per case).

use goro_ext_intl::{collator, grapheme, idna, locale, msgfmt, numfmt, translit, uconv};

#[test]
fn locale_parts() {
    // locale_get_region.phpt, locale_get_script.phpt
    let p = locale::parse("zh-Hans-CN");
    assert_eq!(
        (p.language.as_str(), p.script.as_str(), p.region.as_str()),
        ("zh", "Hans", "CN")
    );
    assert_eq!(locale::parse("zh-min-nan-Hant-CN").region, "MIN");
    // locale_get_keywords2.phpt
    let k = locale::parse("en-US-u-islamCal").keywords;
    assert_eq!(k.get("attribute").map(String::as_str), Some("islamcal"));
    // locale_filter_matches_icu70.phpt
    assert_eq!(locale::canonicalize("art-lojban"), "jbo");
    assert_eq!(locale::canonicalize("i-enochian"), "@x=i-enochian");
    // locale_get_display_name8.phpt
    assert_eq!(
        locale::display_name("sr-Latn-CS", "en").as_deref(),
        Some("Serbian (Latin, Serbia)")
    );
}

#[test]
fn number_formats() {
    // formatter_format8.phpt
    let f = numfmt::NumberFormat::new("en_US", numfmt::DECIMAL, None).unwrap();
    assert_eq!(f.format_f64(1234567.891234567).unwrap(), "1,234,567.891");
    let f = numfmt::NumberFormat::new("de", numfmt::PERCENT, None).unwrap();
    assert_eq!(
        f.format_f64(1234567.891234567).unwrap(),
        "123.456.789\u{a0}%"
    );
    let f = numfmt::NumberFormat::new("en_US", numfmt::CURRENCY, None).unwrap();
    assert_eq!(f.format_f64(1234567.891).unwrap(), "$1,234,567.89");
    // formatter/currencies.phpt: Indian grouping
    let f = numfmt::NumberFormat::new("hi-IN", numfmt::CURRENCY, None).unwrap();
    assert_eq!(f.format_f64(123456789.42).unwrap(), "₹12,34,56,789.42");
}

#[test]
fn messages() {
    // msgfmt_format_subpatterns.phpt
    let pat =
        msgfmt::MessagePattern::parse("{0, plural, offset:1 =0 {none} =1 {one} other {# others}}")
            .unwrap();
    let args = |n: &msgfmt::ArgName| match n {
        msgfmt::ArgName::Index(0) => Some(msgfmt::MsgValue::Int(27)),
        _ => None,
    };
    let none = |_: &str, _: Option<&str>, _: f64| String::new();
    assert_eq!(msgfmt::format("en_US", &pat, &args, &none), "26 others");
    // msgfmt_fail2.phpt
    assert!(matches!(
        msgfmt::MessagePattern::parse("{0,choice}"),
        Err(msgfmt::MsgError::Syntax(1))
    ));
}

#[test]
fn collation() {
    // collator_sort_variant2.phpt: Lithuanian y sorts with i
    let c = collator::Collator::new("lt_LT");
    assert_eq!(c.compare("y", "k"), std::cmp::Ordering::Less);
}

#[test]
fn graphemes_and_idna() {
    // grapheme.phpt
    assert_eq!(grapheme::strlen("a\u{30a}bc"), 3);
    // idn_uts46_basic.phpt
    assert_eq!(
        idna::to_ascii("www.fußball.com", 0).result,
        "www.fussball.com"
    );
    assert_eq!(
        idna::to_ascii("www.fußball.com", idna::NONTRANSITIONAL_TO_ASCII).result,
        "www.xn--fuball-cta.com"
    );
}

#[test]
fn transliteration_and_conversion() {
    // transliterator_transliterate_basic.phpt
    let t = translit::Translit::create("Latin; Title", false).unwrap();
    assert_eq!(
        t.apply("Κοντογιαννάτος, Βασίλης"),
        "Kontogiannátos, Basílēs"
    );
    // uconverter_oop_algo.phpt
    assert_eq!(uconv::lookup("koi8-r").unwrap().ty, uconv::SBCS);
    assert_eq!(uconv::lookup("latin1").unwrap().name, "ISO-8859-1");
}
