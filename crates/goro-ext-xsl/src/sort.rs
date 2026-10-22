//! xsl:sort (xsltutils.c xsltDefaultSortFunction)

use crate::transform::{State, Transform, XSLT_MAX_SORT};
use crate::xpath::{self, XObj};
use goro_ext_xml::tree::NodeId;

#[derive(Clone)]
enum Key {
    Num(f64),
    Str(Vec<u8>),
}

/// xsltComputeSortResultInternal
fn compute_sort_result(
    t: &mut Transform,
    sort: NodeId,
    number: bool,
    list: &[NodeId],
    locale: Option<&str>,
) -> Option<Vec<Option<(Key, usize)>>> {
    let comp = match t.sheet.precomp.get(&sort).cloned() {
        Some(c) => c,
        None => {
            t.generic_error("xsl:sort : compilation failed\n");
            return None;
        }
    };
    let c = comp.comp.clone()?;
    comp.select.as_ref()?;
    if list.len() <= 1 {
        return None;
    }
    let len = list.len();
    let old_inst = t.inst;
    let (old_node, old_pos, old_size, old_ns) =
        (t.xp.node, t.xp.pos, t.xp.size, t.xp.namespaces.clone());
    let mut results = Vec::with_capacity(len);
    for (i, &n) in list.iter().enumerate() {
        t.inst = Some(sort);
        t.xp.size = len as i32;
        t.xp.pos = i as i32 + 1;
        t.node = n;
        t.xp.node = Some(n);
        t.xp.namespaces = comp.ns_list.clone();
        let res = xpath::eval(t, &c);
        match res {
            Some(r) => {
                let s = match r {
                    XObj::Str(s) => s,
                    o => xpath::cast_to_string(&o),
                };
                if number {
                    results.push(Some((Key::Num(xpath::string_eval_number(&s)), i)));
                } else {
                    let k = match locale {
                        Some(l) => collation_key(&s, l),
                        None => s.into_bytes(),
                    };
                    results.push(Some((Key::Str(k), i)));
                }
            }
            None => {
                t.state = State::Stopped;
                results.push(None);
            }
        }
    }
    t.inst = old_inst;
    t.xp.node = old_node;
    t.xp.size = old_size;
    t.xp.pos = old_pos;
    t.xp.namespaces = old_ns;
    Some(results)
}

/// The base letters and accent rank of a Latin character (a partial
/// ISO 14651 decomposition).
fn fold_char(c: char) -> (&'static str, u8) {
    const TABLE: &[(char, &str, u8)] = &[
        ('à', "a", 2),
        ('á', "a", 1),
        ('â', "a", 4),
        ('ã', "a", 6),
        ('ä', "a", 5),
        ('å', "a", 7),
        ('æ', "ae", 1),
        ('ç', "c", 1),
        ('è', "e", 2),
        ('é', "e", 1),
        ('ê', "e", 4),
        ('ë', "e", 5),
        ('ì', "i", 2),
        ('í', "i", 1),
        ('î', "i", 4),
        ('ï', "i", 5),
        ('ð', "d", 1),
        ('ñ', "n", 6),
        ('ò', "o", 2),
        ('ó', "o", 1),
        ('ô', "o", 4),
        ('õ', "o", 6),
        ('ö', "o", 5),
        ('ø', "o", 8),
        ('ù', "u", 2),
        ('ú', "u", 1),
        ('û', "u", 4),
        ('ü', "u", 5),
        ('ý', "y", 1),
        ('ÿ', "y", 5),
        ('þ', "th", 1),
        ('ß', "ss", 1),
        ('ā', "a", 3),
        ('ă', "a", 9),
        ('ą', "a", 10),
        ('ć', "c", 2),
        ('č', "c", 11),
        ('ď', "d", 11),
        ('đ', "d", 12),
        ('ē', "e", 3),
        ('ė', "e", 13),
        ('ę', "e", 10),
        ('ě', "e", 11),
        ('ğ', "g", 9),
        ('ī', "i", 3),
        ('į', "i", 10),
        ('ı', "i", 14),
        ('ł', "l", 12),
        ('ľ', "l", 11),
        ('ĺ', "l", 1),
        ('ń', "n", 1),
        ('ň', "n", 11),
        ('ō', "o", 3),
        ('ő', "o", 15),
        ('œ', "oe", 1),
        ('ř', "r", 11),
        ('ŕ', "r", 1),
        ('ś', "s", 1),
        ('š', "s", 11),
        ('ş', "s", 16),
        ('ť', "t", 11),
        ('ţ', "t", 16),
        ('ū', "u", 3),
        ('ů', "u", 7),
        ('ű', "u", 15),
        ('ų', "u", 10),
        ('ź', "z", 1),
        ('ż', "z", 13),
        ('ž', "z", 11),
    ];
    let lc = c.to_lowercase().next().unwrap_or(c);
    for &(ch, base, rank) in TABLE {
        if ch == lc {
            return (base, rank);
        }
    }
    ("", 0)
}

/// A stand-in for strxfrm_l() with a glibc (ISO 14651) locale: letters and
/// digits compare first by base letter, then by accent, then by case
/// (lowercase first); punctuation and spaces only break ties.
fn collation_key(s: &str, _lang: &str) -> Vec<u8> {
    let mut primary = String::new();
    let mut secondary = Vec::new();
    let mut tertiary = Vec::new();
    for c in s.chars() {
        if !c.is_alphanumeric() {
            continue;
        }
        let (base, rank) = fold_char(c);
        if base.is_empty() {
            primary.extend(c.to_lowercase());
            secondary.push(1u8);
        } else {
            primary.push_str(base);
            secondary.extend(std::iter::repeat_n(rank + 1, base.len()));
        }
        tertiary.push(if c.is_uppercase() { 2u8 } else { 1u8 });
    }
    let mut key = primary.into_bytes();
    key.push(0);
    key.extend(secondary);
    key.push(0);
    key.extend(tertiary);
    key.push(0);
    key.extend_from_slice(s.as_bytes());
    key
}

fn cmp_keys(a: &Key, b: &Key) -> i32 {
    match (a, b) {
        (Key::Num(x), Key::Num(y)) => {
            if x.is_nan() {
                if y.is_nan() { 0 } else { -1 }
            } else if y.is_nan() {
                1
            } else if x == y {
                0
            } else if x > y {
                1
            } else {
                -1
            }
        }
        (Key::Str(x), Key::Str(y)) => match x.cmp(y) {
            std::cmp::Ordering::Less => -1,
            std::cmp::Ordering::Equal => 0,
            std::cmp::Ordering::Greater => 1,
        },
        _ => 0,
    }
}

/// xsltDoSortFunction / xsltDefaultSortFunction: sorts `list` in place.
pub fn do_sort(t: &mut Transform, sorts: &[NodeId], list: &mut [NodeId]) {
    let nbsorts = sorts.len();
    if nbsorts == 0 || nbsorts >= XSLT_MAX_SORT {
        return;
    }
    if !t.sheet.precomp.contains_key(&sorts[0]) {
        return;
    }
    if list.len() <= 1 {
        return;
    }
    let mut number = vec![false; nbsorts];
    let mut desc = vec![false; nbsorts];
    let mut locale: Vec<Option<String>> = vec![None; nbsorts];
    for j in 0..nbsorts {
        let comp = match t.sheet.precomp.get(&sorts[j]).cloned() {
            Some(c) => c,
            None => continue,
        };
        if comp.stype.is_none() && comp.has_stype {
            number[j] = false;
            if let Some(st) = t.eval_attr_value_template(sorts[j], "data-type", None) {
                if st == "text" {
                } else if st == "number" {
                    number[j] = true;
                } else {
                    t.error(
                        Some(sorts[j]),
                        &format!("xsltDoSortFunction: no support for data-type = {}\n", st),
                    );
                }
            }
        } else {
            number[j] = comp.number;
        }
        if comp.order.is_none() && comp.has_order {
            desc[j] = false;
            if let Some(o) = t.eval_attr_value_template(sorts[j], "order", None) {
                if o == "ascending" {
                } else if o == "descending" {
                    desc[j] = true;
                } else {
                    t.error(
                        Some(sorts[j]),
                        &format!("xsltDoSortFunction: invalid value {} for order\n", o),
                    );
                }
            }
        } else {
            desc[j] = comp.descending;
        }
        let lang = if comp.lang.is_none() && comp.has_lang {
            t.eval_attr_value_template(sorts[j], "lang", None)
        } else {
            comp.lang.clone()
        };
        locale[j] = lang.filter(|l| valid_locale(l));
    }
    let len = list.len();
    let mut results_tab: Vec<Option<Vec<Option<(Key, usize)>>>> = vec![None; XSLT_MAX_SORT];
    let snapshot: Vec<NodeId> = list.to_vec();
    results_tab[0] = compute_sort_result(t, sorts[0], number[0], &snapshot, locale[0].as_deref());
    if results_tab[0].is_none() {
        return;
    }
    let mut incr = len / 2;
    while incr > 0 {
        for i in incr..len {
            let mut j = i as isize - incr as isize;
            if results_tab[0].as_ref().unwrap()[i].is_none() {
                continue;
            }
            while j >= 0 {
                let ju = j as usize;
                let mut tst: i32;
                {
                    let r0 = results_tab[0].as_ref().unwrap();
                    match (&r0[ju], &r0[ju + incr]) {
                        (None, _) => tst = 1,
                        (Some(a), Some(b)) => {
                            tst = cmp_keys(&a.0, &b.0);
                            if desc[0] {
                                tst = -tst;
                            }
                        }
                        (Some(_), None) => tst = 0,
                    }
                }
                if tst == 0 {
                    let mut depth = 1;
                    while depth < nbsorts {
                        if !t.sheet.precomp.contains_key(&sorts[depth]) {
                            break;
                        }
                        if results_tab[depth].is_none() {
                            results_tab[depth] = compute_sort_result(
                                t,
                                sorts[depth],
                                number[depth],
                                &snapshot,
                                locale[depth].as_deref(),
                            );
                        }
                        let res = match &results_tab[depth] {
                            Some(r) => r,
                            None => break,
                        };
                        match (&res[ju], &res[ju + incr]) {
                            (None, Some(_)) => tst = 1,
                            (None, None) => {}
                            (Some(_), None) => tst = -1,
                            (Some(a), Some(b)) => {
                                tst = cmp_keys(&a.0, &b.0);
                                if desc[depth] {
                                    tst = -tst;
                                }
                            }
                        }
                        if tst != 0 {
                            break;
                        }
                        depth += 1;
                    }
                }
                if tst == 0 {
                    let r0 = results_tab[0].as_ref().unwrap();
                    let ia = r0[ju].as_ref().map(|x| x.1).unwrap_or(0);
                    let ib = r0[ju + incr].as_ref().map(|x| x.1).unwrap_or(0);
                    tst = (ia > ib) as i32;
                }
                if tst > 0 {
                    results_tab[0].as_mut().unwrap().swap(ju, ju + incr);
                    list.swap(ju, ju + incr);
                    for r in results_tab.iter_mut().take(nbsorts).skip(1) {
                        match r {
                            Some(r) => r.swap(ju, ju + incr),
                            None => break,
                        }
                    }
                    j -= incr as isize;
                } else {
                    break;
                }
            }
        }
        incr /= 2;
    }
}

/// xsltNewLocale's tag syntax check (no system locale data here).
fn valid_locale(l: &str) -> bool {
    let b = l.as_bytes();
    let mut i = 0;
    while i < b.len() && i < 8 && b[i].is_ascii_alphabetic() {
        i += 1;
    }
    if i == 0 {
        return false;
    }
    if i < b.len() {
        if b[i] != b'-' {
            return false;
        }
        i += 1;
        let st = i;
        while i < b.len() && i - st < 8 && b[i].is_ascii_alphabetic() {
            i += 1;
        }
        if i == st || i < b.len() {
            return false;
        }
    }
    true
}
