//! Generated character set tables (tools/charset/gen_tables.py).

#[rustfmt::skip]
pub(crate) mod big5;
#[rustfmt::skip]
pub(crate) mod case;
#[rustfmt::skip]
pub(crate) mod detect;
#[rustfmt::skip]
pub(crate) mod gb;
#[rustfmt::skip]
pub(crate) mod html;
#[rustfmt::skip]
pub(crate) mod jis;
#[rustfmt::skip]
pub(crate) mod kana;
#[rustfmt::skip]
pub(crate) mod kr;
#[rustfmt::skip]
pub(crate) mod mobile;
#[rustfmt::skip]
pub(crate) mod single;

/// Looks `cp` up in a paged reverse table (`NAME_IDX`, `NAME_DATA`): the
/// stored value, 0 when unmapped.
#[inline]
pub(crate) fn rev<T: Copy + Into<usize>>(idx: &[T], data: &[u16], cp: u32) -> u16 {
    let page = (cp >> 8) as usize;
    match idx.get(page) {
        Some(&p) => data[(p.into() << 8) | (cp & 0xFF) as usize],
        None => 0,
    }
}

/// The code points of a sequence-table value (`NAME_SEQ`, `NAME_SEQ_OFF`):
/// a single code point, or a flagged index into the sequence list.
#[inline]
pub(crate) fn seq<'a>(v: &'a u32, seqs: &'a [u32], offs: &'a [u16]) -> &'a [u32] {
    if *v & 0x8000_0000 != 0 {
        let i = (*v & 0x7FFF_FFFF) as usize;
        &seqs[offs[i] as usize..offs[i + 1] as usize]
    } else {
        std::slice::from_ref(v)
    }
}
