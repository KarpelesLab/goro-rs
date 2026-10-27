//! The VM's list of objects awaiting destruction (Vm::destructible_objects)
//! and the bookkeeping that keeps the release points' scan of its tail
//! cheap when the tail holds long-lived objects.

use crate::value::Value;

/// After this many scans of the tail, the entries already listed when they
/// began are long-lived ("old").
const SCANS_TO_OLD: u32 = 1024;
/// Old entries of the tail are looked at by one scan in this many.
const OLD_SCAN_EVERY: u32 = 64;

/// A Vec of the listed objects, in creation order. Pushes keep track of
/// which entries are long-lived; anything else that changes the list (a
/// removal moves entries) makes every entry young again.
#[derive(Default)]
pub(crate) struct DtorList {
    list: Vec<Value>,
    /// Entries below this index have been listed for SCANS_TO_OLD scans.
    old_below: usize,
    /// The list's length when the current SCANS_TO_OLD round of scans
    /// began, and how many scans it has had.
    round_len: usize,
    round_scans: u32,
}

impl DtorList {
    #[inline]
    pub(crate) fn push(&mut self, v: Value) {
        self.list.push(v);
    }

    /// The first index the release points' scan of the last `window`
    /// entries looks at this time: old entries are skipped but by one scan
    /// in OLD_SCAN_EVERY (a release no holder reported is noticed late).
    #[inline]
    pub(crate) fn tail_scan_start(&mut self, window: usize) -> usize {
        let len = self.list.len();
        let start = len - len.min(window);
        self.round_scans += 1;
        if self.round_scans >= SCANS_TO_OLD {
            self.old_below = self.round_len.min(len);
            self.round_len = len;
            self.round_scans = 0;
        }
        if self.old_below > start && !self.round_scans.is_multiple_of(OLD_SCAN_EVERY) {
            self.old_below
        } else {
            start
        }
    }
}

impl std::ops::Deref for DtorList {
    type Target = Vec<Value>;
    #[inline]
    fn deref(&self) -> &Vec<Value> {
        &self.list
    }
}

impl std::ops::DerefMut for DtorList {
    /// (any change but a push: every entry is young again)
    #[inline]
    fn deref_mut(&mut self) -> &mut Vec<Value> {
        self.old_below = 0;
        self.round_len = 0;
        self.round_scans = 0;
        &mut self.list
    }
}
