//! The global symbol table (EG(symbol_table)).
//!
//! Globals live in one `PhpArray` shared by every frame: it is also the value
//! of `$GLOBALS`, so `$GLOBALS[...]` reads and writes act on it directly.
//!
//! The main script (and files included / code eval'd at global scope) does
//! not keep private copies of its variables: each of its CVs is bound to the
//! table entry of the same name through a shared reference cell, the way
//! PHP's global CVs are IS_INDIRECT slots of the symbol table.  A frame
//! records which cell each CV is bound to (`GlobalBinding`), repairs the
//! binding after opcodes that replace a CV slot wholesale (`$a = &$b`,
//! `unset($a)`, ...), and rebinds every CV when another frame changed the
//! table's shape (`globals_gen`).

use std::cell::RefCell;
use std::rc::Rc;

use crate::array::{ArrayKey, PhpArray};
use crate::string::PhpString;
use crate::value::Value;
use crate::vm::Vm;

pub type Cell = Rc<RefCell<Value>>;

/// Symbol-table key for a variable name (numeric names use integer keys,
/// like array offsets).
pub fn gkey(name: &[u8]) -> ArrayKey {
    Vm::value_to_array_key(Value::String(PhpString::from_bytes_untracked(name)))
}

/// Per-frame record of the global cells a global-scope frame's CVs are bound to.
pub struct GlobalBinding {
    pub cells: Vec<Option<Cell>>,
    pub seen_gen: u64,
}

impl GlobalBinding {
    pub fn new(n: usize) -> Self {
        GlobalBinding {
            cells: vec![None; n],
            seen_gen: u64::MAX,
        }
    }
}

/// Table entries backing a global-scope CV are preallocated in PHP (the
/// CV slots themselves), so creating / removing them is not accounted as a
/// memory_get_usage() change.
fn cv_slot_entry_created() {
    crate::value::memory_free(PhpArray::ENTRY_SIZE);
}

fn cv_slot_entry_released() {
    crate::value::memory_alloc(PhpArray::ENTRY_SIZE);
}

fn skip_name(name: &[u8]) -> bool {
    name == b"GLOBALS" || name == b"this"
}

impl Vm {
    pub fn is_global_scope_flag(&self) -> bool {
        self.is_global_scope
    }

    pub fn set_global_scope_flag(&mut self, v: bool) {
        self.is_global_scope = v;
    }

    /// The shared `$GLOBALS` array.
    pub fn globals_array(&self) -> Rc<RefCell<PhpArray>> {
        self.globals.clone()
    }

    /// Current value of a global (dereferenced), None when unset.
    pub fn global_lookup(&self, name: &[u8]) -> Option<Value> {
        let g = self.globals.borrow();
        match g.get(&gkey(name))? {
            Value::Reference(r) => {
                let v = r.borrow().clone();
                if matches!(v, Value::Undef) {
                    None
                } else {
                    Some(v)
                }
            }
            Value::Undef => None,
            v => Some(v.clone()),
        }
    }

    pub fn global_exists(&self, name: &[u8]) -> bool {
        self.global_lookup(name).is_some()
    }

    /// The reference cell of a global, creating the global (as NULL) when
    /// missing — what `global $x` binds to.
    pub fn global_cell(&mut self, name: &[u8]) -> Cell {
        let key = gkey(name);
        let mut g = self.globals.borrow_mut();
        if let Some(slot) = g.get_mut(&key) {
            return match slot {
                Value::Reference(r) => {
                    if matches!(*r.borrow(), Value::Undef) {
                        *r.borrow_mut() = Value::Null;
                    }
                    r.clone()
                }
                other => {
                    let v = std::mem::replace(other, Value::Null);
                    let v = if matches!(v, Value::Undef) {
                        Value::Null
                    } else {
                        v
                    };
                    let r = Rc::new(RefCell::new(v));
                    *other = Value::Reference(r.clone());
                    r
                }
            };
        }
        let r = Rc::new(RefCell::new(Value::Null));
        g.set(key, Value::Reference(r.clone()));
        drop(g);
        self.globals_gen = self.globals_gen.wrapping_add(1);
        r
    }

    /// Assign a global (writes through an existing binding).
    pub fn global_assign(&mut self, name: &[u8], value: Value) {
        let key = gkey(name);
        let value = value.deref();
        let mut g = self.globals.borrow_mut();
        if let Some(slot) = g.get_mut(&key) {
            match slot {
                Value::Reference(r) => {
                    let r = r.clone();
                    drop(g);
                    let old = std::mem::replace(&mut *r.borrow_mut(), value);
                    drop(old);
                }
                other => {
                    let old = std::mem::replace(other, value);
                    drop(g);
                    drop(old);
                }
            }
            return;
        }
        g.set(key, value);
        drop(g);
        self.globals_gen = self.globals_gen.wrapping_add(1);
    }

    /// Bind a global name to the given reference cell.
    pub fn global_bind_ref(&mut self, name: &[u8], r: Cell) {
        self.globals
            .borrow_mut()
            .set(gkey(name), Value::Reference(r));
        self.globals_gen = self.globals_gen.wrapping_add(1);
    }

    /// Remove a global variable.
    pub fn global_unset(&mut self, name: &[u8]) {
        let old = self.globals.borrow_mut().remove(&gkey(name));
        if old.is_some() {
            self.globals_gen = self.globals_gen.wrapping_add(1);
        }
        drop(old);
    }

    /// Names and (dereferenced) values of all set globals, in table order.
    pub fn global_entries(&self) -> Vec<(Vec<u8>, Value)> {
        let g = self.globals.borrow();
        let mut out = Vec::with_capacity(g.len());
        for (k, v) in g.iter() {
            let v = v.deref();
            if matches!(v, Value::Undef) {
                continue;
            }
            let name = match k {
                ArrayKey::String(s) => s.as_bytes().to_vec(),
                ArrayKey::Int(n) => n.to_string().into_bytes(),
            };
            out.push((name, v));
        }
        out
    }

    /// Bind (or re-bind) every CV of a global-scope frame to the symbol table.
    pub(crate) fn symtab_rebind_all(
        &mut self,
        cvs: &mut [Value],
        names: &[Vec<u8>],
        b: &mut GlobalBinding,
    ) {
        b.seen_gen = self.globals_gen;
        for i in 0..cvs.len().min(names.len()) {
            if skip_name(&names[i]) {
                continue;
            }
            let entry = {
                let g = self.globals.borrow();
                match g.get(&gkey(&names[i])) {
                    None => None,
                    Some(Value::Reference(r)) => Some(Some(r.clone())),
                    Some(_) => Some(None),
                }
            };
            match entry {
                Some(Some(r)) => {
                    let same = matches!(&cvs[i], Value::Reference(c) if Rc::ptr_eq(c, &r));
                    if !same {
                        cvs[i] = Value::Reference(r.clone());
                    }
                    b.cells[i] = Some(r);
                }
                Some(None) => {
                    // Plain table value: wrap it into a cell shared with the CV.
                    let key = gkey(&names[i]);
                    let mut g = self.globals.borrow_mut();
                    if let Some(slot) = g.get_mut(&key) {
                        let v = std::mem::replace(slot, Value::Null);
                        let r = Rc::new(RefCell::new(v));
                        *slot = Value::Reference(r.clone());
                        cvs[i] = Value::Reference(r.clone());
                        b.cells[i] = Some(r);
                    }
                }
                None => {
                    if let Some(old) = b.cells[i].take() {
                        // Removed from the table by someone else.
                        let same = matches!(&cvs[i], Value::Reference(c) if Rc::ptr_eq(c, &old));
                        if same {
                            cvs[i] = Value::Undef;
                        }
                    } else if !matches!(cvs[i], Value::Undef) {
                        self.symtab_fix_cv(i, cvs, names, b);
                    }
                }
            }
        }
        b.seen_gen = self.globals_gen;
    }

    /// Repair the binding of CV `i` after its slot may have been replaced.
    #[inline]
    pub(crate) fn symtab_fix_cv(
        &mut self,
        i: usize,
        cvs: &mut [Value],
        names: &[Vec<u8>],
        b: &mut GlobalBinding,
    ) {
        let Some(slot) = cvs.get(i) else { return };
        match (slot, &b.cells[i]) {
            (Value::Reference(r), Some(c)) if Rc::ptr_eq(r, c) => {}
            (Value::Undef, None) => {}
            _ => self.symtab_fix_cv_slow(i, cvs, names, b),
        }
    }

    fn symtab_fix_cv_slow(
        &mut self,
        i: usize,
        cvs: &mut [Value],
        names: &[Vec<u8>],
        b: &mut GlobalBinding,
    ) {
        let Some(name) = names.get(i) else { return };
        if skip_name(name) {
            return;
        }
        let key = gkey(name);
        match std::mem::replace(&mut cvs[i], Value::Undef) {
            Value::Undef => {
                // unset($x): drop the table entry if it is still ours.
                if let Some(c) = b.cells[i].take() {
                    let ours = matches!(self.globals.borrow().get(&key), Some(Value::Reference(r)) if Rc::ptr_eq(r, &c));
                    if ours {
                        let old = self.globals.borrow_mut().remove(&key);
                        self.globals_gen = self.globals_gen.wrapping_add(1);
                        cv_slot_entry_released();
                        drop(old);
                    }
                }
            }
            Value::Reference(r) => {
                // The CV was bound to another reference ($x = &$y).
                cvs[i] = Value::Reference(r.clone());
                let same = matches!(self.globals.borrow().get(&key), Some(Value::Reference(t)) if Rc::ptr_eq(t, &r));
                if !same {
                    self.globals
                        .borrow_mut()
                        .set(key, Value::Reference(r.clone()));
                    self.globals_gen = self.globals_gen.wrapping_add(1);
                }
                b.cells[i] = Some(r);
            }
            v => {
                // A plain value was stored into the slot: assign it to the
                // bound variable (or create the global).
                let cell = match b.cells[i].clone() {
                    Some(c) => Some(c),
                    None => match self.globals.borrow().get(&key) {
                        Some(Value::Reference(r)) => Some(r.clone()),
                        _ => None,
                    },
                };
                let cell = match cell {
                    Some(c) => {
                        let old = std::mem::replace(&mut *c.borrow_mut(), v);
                        drop(old);
                        c
                    }
                    None => {
                        let c = Rc::new(RefCell::new(v));
                        self.globals
                            .borrow_mut()
                            .set(key, Value::Reference(c.clone()));
                        self.globals_gen = self.globals_gen.wrapping_add(1);
                        cv_slot_entry_created();
                        c
                    }
                };
                cvs[i] = Value::Reference(cell.clone());
                b.cells[i] = Some(cell);
            }
        }
    }

    /// Detach CV `i` from the symbol table before it is unset: returns the
    /// slot to a plain value when nothing else shares the cell.
    pub(crate) fn symtab_unbind_cv(
        &mut self,
        i: usize,
        cvs: &mut [Value],
        names: &[Vec<u8>],
        b: &mut GlobalBinding,
    ) {
        let Some(c) = b.cells.get_mut(i).and_then(|c| c.take()) else {
            return;
        };
        if let Some(name) = names.get(i) {
            let key = gkey(name);
            let ours = matches!(self.globals.borrow().get(&key), Some(Value::Reference(r)) if Rc::ptr_eq(r, &c));
            if ours {
                let old = self.globals.borrow_mut().remove(&key);
                self.globals_gen = self.globals_gen.wrapping_add(1);
                cv_slot_entry_released();
                drop(old);
            }
        }
        if matches!(&cvs[i], Value::Reference(r) if Rc::ptr_eq(r, &c)) && Rc::strong_count(&c) == 2
        {
            let inner = std::mem::replace(&mut *c.borrow_mut(), Value::Undef);
            cvs[i] = inner;
        }
    }
}
