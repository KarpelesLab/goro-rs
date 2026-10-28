//! Long and repetitive strings parse without deep recursion.

use goro_ext_date::parse;

#[test]
fn long_separator_runs() {
    let mut s = b"1".to_vec();
    s.extend(std::iter::repeat_n(b' ', 200_000));
    s.extend_from_slice(b"Jan 2020");
    let p = parse::parse(&s);
    assert_eq!((p.y, p.m, p.d), (Some(2020), Some(1), Some(1)));

    let mut s = b"Jan".to_vec();
    s.extend(std::iter::repeat_n(b'.', 100_000));
    s.extend_from_slice(b"5");
    let p = parse::parse(&s);
    assert_eq!((p.m, p.d), (Some(1), Some(5)));
}

#[test]
fn long_identifier() {
    let mut s = b"Aa".to_vec();
    for _ in 0..50_000 {
        s.extend_from_slice(b"/a");
    }
    let p = parse::parse(&s);
    assert_eq!(p.errors.len(), 1);
    assert_eq!(
        p.errors[0].1,
        "The timezone could not be found in the database"
    );
}

#[test]
fn long_garbage() {
    let s: Vec<u8> = b"+-:/()".iter().copied().cycle().take(20_000).collect();
    let p = parse::parse(&s);
    assert!(!p.errors.is_empty());
}
