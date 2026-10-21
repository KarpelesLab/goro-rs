//! Symbol tables: the global one (EG(symbol_table)) and the lazily built
//! per-frame tables of functions (EX(symbol_table)).
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
//! table's shape (the table's generation counter).
//!
//! A function frame has no table until it needs one (zend_rebuild_symbol_table):
//! a dynamic variable write (`$$n = v`, `${$n}[k] = v`, `extract()`, ...),
//! or `include` / `eval` (which share the function's variables).  The table
//! is then created from the frame's CVs, in declaration order, and the frame
//! is bound to it exactly like a global-scope frame.  In a local table, an
//! undefined CV keeps its position as a `Value::Undef` placeholder (php-src's
//! IS_INDIRECT entry pointing at an UNDEF slot), so get_defined_vars() lists
//! variables in php-src's order.

use std::cell::RefCell;
use std::rc::Rc;

use crate::array::{ArrayKey, PhpArray};
use crate::opcode::OpArray;
use crate::string::PhpString;
use crate::value::Value;
use crate::vm::Vm;

pub type Cell = Rc<RefCell<Value>>;

/// Symbol-table key for a variable name (numeric names use integer keys,
/// like array offsets).
pub fn gkey(name: &[u8]) -> ArrayKey {
    Vm::value_to_array_key(Value::String(PhpString::from_bytes_untracked(name)))
}

/// Name of a symbol-table key.
pub fn key_name(k: &ArrayKey) -> Vec<u8> {
    match k {
        ArrayKey::String(s) => s.as_bytes().to_vec(),
        ArrayKey::Int(n) => n.to_string().into_bytes(),
    }
}

/// A symbol table and its shape generation.
#[derive(Clone)]
pub struct SymTab {
    pub arr: Rc<RefCell<PhpArray>>,
    /// Bumped whenever an entry is added, removed or rebound to another
    /// cell; frames bound to the table rebind their CVs when it changed.
    pub generation: Rc<std::cell::Cell<u64>>,
    /// The global symbol table.
    pub global: bool,
}

impl SymTab {
    pub fn new_local() -> Self {
        SymTab {
            arr: Rc::new(RefCell::new(PhpArray::new())),
            generation: Rc::new(std::cell::Cell::new(0)),
            global: false,
        }
    }

    #[inline]
    pub fn bump(&self) {
        self.generation.set(self.generation.get().wrapping_add(1));
    }

    /// Current value of a variable (dereferenced), None when undefined.
    pub fn lookup(&self, name: &[u8]) -> Option<Value> {
        let g = self.arr.borrow();
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

    pub fn exists(&self, name: &[u8]) -> bool {
        let g = self.arr.borrow();
        match g.get(&gkey(name)) {
            None | Some(Value::Undef) => false,
            Some(Value::Reference(r)) => !matches!(*r.borrow(), Value::Undef),
            Some(_) => true,
        }
    }

    /// The reference cell of a variable, creating it (as NULL) when missing
    /// (FETCH_W).  Returns the cell and whether the variable was undefined.
    pub fn cell(&self, name: &[u8]) -> (Cell, bool) {
        let key = gkey(name);
        let mut g = self.arr.borrow_mut();
        if let Some(slot) = g.get_mut(&key) {
            return match slot {
                Value::Reference(r) => {
                    let undef = matches!(*r.borrow(), Value::Undef);
                    if undef {
                        *r.borrow_mut() = Value::Null;
                    }
                    (r.clone(), undef)
                }
                other => {
                    let undef = matches!(other, Value::Undef);
                    let v = std::mem::replace(other, Value::Null);
                    let v = if undef { Value::Null } else { v };
                    let r = Rc::new(RefCell::new(v));
                    *other = Value::Reference(r.clone());
                    drop(g);
                    self.bump();
                    (r, undef)
                }
            };
        }
        let r = Rc::new(RefCell::new(Value::Null));
        g.set(key, Value::Reference(r.clone()));
        drop(g);
        self.bump();
        (r, true)
    }

    /// Assign a variable (writes through an existing binding).
    pub fn assign(&self, name: &[u8], value: Value) {
        let key = gkey(name);
        let value = value.deref();
        let mut g = self.arr.borrow_mut();
        if let Some(slot) = g.get_mut(&key) {
            match slot {
                Value::Reference(r) => {
                    let r = r.clone();
                    drop(g);
                    let old = std::mem::replace(&mut *r.borrow_mut(), value);
                    drop(old);
                }
                other => {
                    let was_undef = matches!(other, Value::Undef);
                    let old = std::mem::replace(other, value);
                    drop(g);
                    drop(old);
                    if was_undef {
                        self.bump();
                    }
                }
            }
            return;
        }
        g.set(key, value);
        drop(g);
        self.bump();
    }

    /// Bind a variable name to the given reference cell.
    pub fn bind_ref(&self, name: &[u8], r: Cell) {
        let old = replace_entry(&self.arr, gkey(name), Value::Reference(r));
        self.bump();
        drop(old);
    }

    /// Remove a variable (zend_hash_del_ind).  In a local table, a variable
    /// bound to a CV keeps its position as a placeholder.
    pub fn unset(&self, name: &[u8]) {
        let key = gkey(name);
        let old = if self.global {
            self.arr.borrow_mut().remove(&key)
        } else {
            let mut g = self.arr.borrow_mut();
            match g.get_mut(&key) {
                Some(slot @ Value::Reference(_)) => Some(std::mem::replace(slot, Value::Undef)),
                Some(Value::Undef) | None => None,
                Some(_) => {
                    drop(g);
                    self.arr.borrow_mut().remove(&key)
                }
            }
        };
        if old.is_some() {
            self.bump();
        }
        drop(old);
    }

    /// Names and (dereferenced) values of all defined variables, in table order.
    pub fn entries(&self) -> Vec<(Vec<u8>, Value)> {
        let g = self.arr.borrow();
        let mut out = Vec::with_capacity(g.len());
        for (k, v) in g.iter() {
            let v = v.deref();
            if matches!(v, Value::Undef) {
                continue;
            }
            out.push((key_name(k), v));
        }
        out
    }
}

/// Per-frame record of the table cells a bound frame's CVs are bound to.
#[derive(Clone)]
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

/// A frame's symbol table and the binding of its CVs to it.
#[derive(Clone)]
pub struct FrameSymtab {
    pub tab: SymTab,
    pub bind: GlobalBinding,
    /// The frame built this (local) table: it is released with the frame.
    pub owner: bool,
}

impl std::fmt::Debug for FrameSymtab {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("FrameSymtab")
    }
}

impl FrameSymtab {
    pub fn new(tab: SymTab, ncvs: usize) -> Self {
        FrameSymtab {
            tab,
            bind: GlobalBinding::new(ncvs),
            owner: false,
        }
    }
}

/// Store an entry, returning the value it replaced (dropped by the caller
/// once the table is no longer borrowed).
fn replace_entry(arr: &RefCell<PhpArray>, key: ArrayKey, v: Value) -> Option<Value> {
    let mut g = arr.borrow_mut();
    if let Some(slot) = g.get_mut(&key) {
        return Some(std::mem::replace(slot, v));
    }
    g.set(key, v);
    None
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

/// The CVs of `op_array` in php-src's order.  goro's CV layout is
/// [$this][closure uses][params][locals]; php-src's is params, then uses,
/// then locals.
pub(crate) fn php_cv_order(op_array: &OpArray) -> impl Iterator<Item = usize> {
    let n = op_array.cv_names.len();
    let nparams = (op_array.param_count as usize).min(n);
    let hidden = nparams.saturating_sub(op_array.param_attributes.len());
    (hidden..nparams).chain(0..hidden).chain(nparams..n)
}

/// Hidden compiler CVs (`\0...`) hold the variables fetched by dynamic
/// variable writes; they are never part of a symbol table.
#[inline]
pub fn is_hidden_cv(name: &[u8]) -> bool {
    name.first() == Some(&0)
}

/// CVs that are not entries of the given kind of symbol table.
#[inline]
pub fn skip_name(name: &[u8], global: bool) -> bool {
    is_hidden_cv(name)
        || name == b"GLOBALS"
        || name == b"this"
        || (!global && crate::vm::is_auto_global_name(name))
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

    /// The global symbol table.
    pub fn globals_tab(&self) -> SymTab {
        SymTab {
            arr: self.globals.clone(),
            generation: self.globals_gen.clone(),
            global: true,
        }
    }

    #[inline]
    pub(crate) fn bump_globals_gen(&self) {
        self.globals_gen.set(self.globals_gen.get().wrapping_add(1));
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
        if !self.global_ref_names.contains(name) {
            self.global_ref_names.insert(name.to_vec());
        }
        self.globals_tab().cell(name).0
    }

    /// Assign a global (writes through an existing binding).
    pub fn global_assign(&mut self, name: &[u8], value: Value) {
        self.globals_tab().assign(name, value);
    }

    /// Bind a global name to the given reference cell.
    pub fn global_bind_ref(&mut self, name: &[u8], r: Cell) {
        self.globals_tab().bind_ref(name, r);
    }

    /// Remove a global variable.
    pub fn global_unset(&mut self, name: &[u8]) {
        self.globals_tab().unset(name);
    }

    /// Names and (dereferenced) values of all set globals, in table order.
    pub fn global_entries(&self) -> Vec<(Vec<u8>, Value)> {
        self.globals_tab().entries()
    }

    /// Raise an exception of `class` with `msg` (the caller unwinds).
    pub(crate) fn throw_error_at(&mut self, class: &[u8], msg: &str, line: u32) {
        let exc = self.create_exception(class, msg, line);
        self.current_exception = Some(exc);
    }

    /// The name of a dynamic variable (zval_try_get_tmp_string()); None
    /// when the conversion threw.
    pub(crate) fn varvar_name(&mut self, v: &Value) -> Option<Vec<u8>> {
        match v.deref() {
            Value::String(s) => Some(s.as_bytes().to_vec()),
            Value::Undef => Some(Vec::new()),
            other => match self.value_to_string_checked(&other) {
                Ok(s) => Some(s.as_bytes().to_vec()),
                Err(_) => {
                    if self.current_exception.is_some() {
                        None
                    } else {
                        Some(Vec::new())
                    }
                }
            },
        }
    }

    /// A dynamic variable of a frame without symbol table: one of its CVs.
    pub(crate) fn varvar_cv_lookup(cvs: &[Value], names: &[Vec<u8>], name: &[u8]) -> Option<Value> {
        if skip_name(name, false) {
            return None;
        }
        let i = names.iter().position(|n| n.as_slice() == name)?;
        match cvs.get(i)?.deref() {
            Value::Undef => None,
            v => Some(v),
        }
    }

    /// `$this` of a frame (ZEND_FETCH_R of "this" falls back to EX(This)).
    pub(crate) fn frame_this(cvs: &[Value], names: &[Vec<u8>]) -> Option<Value> {
        let i = names.iter().position(|n| n == b"this")?;
        match cvs.get(i)?.deref() {
            v @ Value::Object(_) => Some(v),
            _ => None,
        }
    }

    /// The frame's symbol table, built from its CVs when it has none yet.
    pub(crate) fn symtab_ensure<'a>(
        &mut self,
        fsym: &'a mut Option<FrameSymtab>,
        cvs: &mut [Value],
        op_array: &OpArray,
    ) -> &'a mut FrameSymtab {
        if fsym.is_none() {
            *fsym = Some(self.symtab_build_local(cvs, op_array));
        }
        fsym.as_mut().unwrap()
    }

    /// Assign a variable of `tab`; a variable that is a reference held by
    /// typed properties checks (and coerces) the value first.  Returns the
    /// TypeError message when the value does not fit.
    pub(crate) fn symtab_assign_checked(
        &mut self,
        tab: &SymTab,
        name: &[u8],
        mut value: Value,
        strict: bool,
    ) -> Result<(), String> {
        if crate::vm::typed_ref::any() {
            let cell = match tab.arr.borrow().get(&gkey(name)) {
                Some(Value::Reference(r)) => Some(r.clone()),
                _ => None,
            };
            if let Some(cell) = cell {
                self.typed_ref_write_check(&cell, &mut value, strict, None)?;
            }
        }
        match &value {
            Value::Object(o) => crate::gc::track_object(o),
            Value::Generator(g) => crate::gc::track_generator(g),
            _ => {}
        }
        let (cell, _) = tab.cell(name);
        let old = std::mem::replace(&mut *cell.borrow_mut(), value);
        crate::gc::on_value_replaced(&old, &cell.borrow());
        drop(old);
        Ok(())
    }

    /// include / eval: the code about to run shares the caller's variables.
    /// Its CVs will be bound to the caller's symbol table (built first in a
    /// function), and it sees the caller's `$this`.
    pub(crate) fn share_scope(
        &mut self,
        fsym: &mut Option<FrameSymtab>,
        cvs: &mut [Value],
        op_array: &OpArray,
        child_cvs: &mut [Value],
        child_names: &[Vec<u8>],
    ) {
        let names = &op_array.cv_names;
        let fs = self.symtab_ensure(fsym, cvs, op_array);
        if fs.tab.global {
            self.bind_next_frame = true;
        } else {
            self.bind_next_tab = Some(fs.tab.clone());
        }
        if let Some(i) = child_names.iter().position(|n| n == b"this") {
            if let Some(t) = names
                .iter()
                .position(|n| n == b"this")
                .and_then(|p| cvs.get(p))
            {
                child_cvs[i] = t.clone();
            }
        }
    }

    /// get_defined_vars(): a copy of the scope's symbol table (or of its
    /// defined CVs when it has none).  Variables that are PHP references
    /// (shared with another variable) stay references in the copy.
    pub(crate) fn defined_vars(
        &self,
        fsym: Option<&FrameSymtab>,
        cvs: &[Value],
        op_array: &OpArray,
    ) -> Value {
        let names = &op_array.cv_names;
        let mut out = PhpArray::new();
        match fsym {
            Some(fs) => {
                // A CV-bound cell is held by the table, the CV slot and the
                // frame's binding.
                let g = fs.tab.arr.borrow();
                for (k, v) in g.iter() {
                    let v = match v {
                        Value::Undef => continue,
                        Value::Reference(r) => {
                            if matches!(*r.borrow(), Value::Undef) {
                                continue;
                            }
                            if Rc::strong_count(r) > 3 {
                                Value::Reference(r.clone())
                            } else {
                                self.dup_array_value(r.borrow().clone())
                            }
                        }
                        v => self.dup_array_value(v.clone()),
                    };
                    // Symbol tables have string keys only: `${5}` is "5".
                    out.set(ArrayKey::String(PhpString::from_vec(key_name(k))), v);
                }
            }
            None => {
                for i in php_cv_order(op_array) {
                    let name = &names[i];
                    if skip_name(name, false) {
                        continue;
                    }
                    let v = match cvs.get(i) {
                        None | Some(Value::Undef) => continue,
                        Some(Value::Reference(r)) => {
                            if matches!(*r.borrow(), Value::Undef) {
                                continue;
                            }
                            if Rc::strong_count(r) > 1 {
                                Value::Reference(r.clone())
                            } else {
                                self.dup_array_value(r.borrow().clone())
                            }
                        }
                        Some(v) => self.dup_array_value(v.clone()),
                    };
                    out.set(ArrayKey::String(PhpString::from_vec(name.clone())), v);
                }
            }
        }
        Value::Array(Rc::new(RefCell::new(out)))
    }

    /// zend_rebuild_symbol_table(): create a function frame's table from its
    /// CVs (in declaration order) and bind the CVs to it.
    pub(crate) fn symtab_build_local(
        &mut self,
        cvs: &mut [Value],
        op_array: &OpArray,
    ) -> FrameSymtab {
        let names = &op_array.cv_names;
        let tab = SymTab::new_local();
        {
            let mut g = tab.arr.borrow_mut();
            for i in php_cv_order(op_array) {
                if i >= cvs.len() {
                    continue;
                }
                let name = &names[i];
                if skip_name(name, false) {
                    continue;
                }
                let entry = match &cvs[i] {
                    Value::Undef => Value::Undef,
                    Value::Reference(r) => Value::Reference(r.clone()),
                    _ => {
                        let v = std::mem::replace(&mut cvs[i], Value::Undef);
                        let r = Rc::new(RefCell::new(v));
                        cvs[i] = Value::Reference(r.clone());
                        Value::Reference(r)
                    }
                };
                g.set(gkey(name), entry);
            }
        }
        let mut fs = FrameSymtab::new(tab, cvs.len());
        fs.owner = true;
        self.symtab_rebind_all(cvs, names, &mut fs);
        fs
    }

    /// zend_attach_symbol_table() for a frame running in another frame's
    /// scope (include / eval in a function): its CVs missing from the table
    /// are added as placeholders, then every CV is bound.
    pub(crate) fn symtab_attach(
        &mut self,
        cvs: &mut [Value],
        names: &[Vec<u8>],
        tab: SymTab,
    ) -> FrameSymtab {
        if !tab.global {
            let mut g = tab.arr.borrow_mut();
            for name in names.iter().take(cvs.len()) {
                if skip_name(name, false) {
                    continue;
                }
                let key = gkey(name);
                if g.get(&key).is_none() {
                    g.set(key, Value::Undef);
                }
            }
        }
        let mut fs = FrameSymtab::new(tab, cvs.len());
        self.symtab_rebind_all(cvs, names, &mut fs);
        fs
    }

    /// A function frame owning its symbol table is left: its CVs get their
    /// values back (zend_detach_symbol_table), so they are released in
    /// declaration order before the table's other variables.
    pub(crate) fn symtab_release_local(
        &mut self,
        cvs: &mut [Value],
        names: &[Vec<u8>],
        fs: &mut FrameSymtab,
    ) {
        if fs.tab.global || !fs.owner {
            return;
        }
        for (i, slot) in cvs.iter_mut().enumerate() {
            let Some(c) = fs.bind.cells.get_mut(i).and_then(|c| c.take()) else {
                continue;
            };
            if !matches!(&*slot, Value::Reference(r) if Rc::ptr_eq(r, &c)) {
                continue;
            }
            if let Some(name) = names.get(i) {
                let key = gkey(name);
                let mut g = fs.tab.arr.borrow_mut();
                if let Some(e) = g.get_mut(&key) {
                    if matches!(e, Value::Reference(r) if Rc::ptr_eq(r, &c)) {
                        *e = Value::Undef;
                    }
                }
            }
            if Rc::strong_count(&c) == 2 {
                let inner = std::mem::replace(&mut *c.borrow_mut(), Value::Undef);
                *slot = inner;
            }
        }
    }

    /// Bind (or re-bind) every CV of a bound frame to its symbol table.
    pub(crate) fn symtab_rebind_all(
        &mut self,
        cvs: &mut [Value],
        names: &[Vec<u8>],
        fs: &mut FrameSymtab,
    ) {
        let global = fs.tab.global;
        fs.bind.seen_gen = fs.tab.generation.get();
        for i in 0..cvs.len().min(names.len()) {
            if skip_name(&names[i], global) {
                continue;
            }
            let entry = {
                let g = fs.tab.arr.borrow();
                match g.get(&gkey(&names[i])) {
                    None | Some(Value::Undef) => None,
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
                    fs.bind.cells[i] = Some(r);
                }
                Some(None) => {
                    // Plain table value: wrap it into a cell shared with the CV.
                    let key = gkey(&names[i]);
                    let mut g = fs.tab.arr.borrow_mut();
                    if let Some(slot) = g.get_mut(&key) {
                        let v = std::mem::replace(slot, Value::Null);
                        let r = Rc::new(RefCell::new(v));
                        *slot = Value::Reference(r.clone());
                        cvs[i] = Value::Reference(r.clone());
                        fs.bind.cells[i] = Some(r);
                    }
                }
                None => {
                    if let Some(old) = fs.bind.cells[i].take() {
                        // Removed from the table by someone else.
                        let same = matches!(&cvs[i], Value::Reference(c) if Rc::ptr_eq(c, &old));
                        if same {
                            cvs[i] = Value::Undef;
                        }
                    } else if !matches!(cvs[i], Value::Undef) {
                        self.symtab_fix_cv(i, cvs, names, fs);
                    }
                }
            }
        }
        fs.bind.seen_gen = fs.tab.generation.get();
    }

    /// Repair the binding of CV `i` after its slot may have been replaced.
    #[inline]
    pub(crate) fn symtab_fix_cv(
        &mut self,
        i: usize,
        cvs: &mut [Value],
        names: &[Vec<u8>],
        fs: &mut FrameSymtab,
    ) {
        let Some(slot) = cvs.get(i) else { return };
        match (slot, &fs.bind.cells[i]) {
            (Value::Reference(r), Some(c)) if Rc::ptr_eq(r, c) => {}
            (Value::Undef, None) => {}
            _ => self.symtab_fix_cv_slow(i, cvs, names, fs),
        }
    }

    /// Repair the binding of every CV.
    pub(crate) fn symtab_fix_all(
        &mut self,
        cvs: &mut [Value],
        names: &[Vec<u8>],
        fs: &mut FrameSymtab,
    ) {
        for i in 0..cvs.len() {
            self.symtab_fix_cv(i, cvs, names, fs);
        }
        if fs.bind.seen_gen != fs.tab.generation.get() {
            self.symtab_rebind_all(cvs, names, fs);
        }
    }

    fn symtab_fix_cv_slow(
        &mut self,
        i: usize,
        cvs: &mut [Value],
        names: &[Vec<u8>],
        fs: &mut FrameSymtab,
    ) {
        let Some(name) = names.get(i) else { return };
        let global = fs.tab.global;
        if skip_name(name, global) {
            return;
        }
        let key = gkey(name);
        let tab = fs.tab.clone();
        let b = &mut fs.bind;
        match std::mem::replace(&mut cvs[i], Value::Undef) {
            Value::Undef => {
                // unset($x): drop the table entry if it is still ours.
                if let Some(c) = b.cells[i].take() {
                    let ours = matches!(tab.arr.borrow().get(&key), Some(Value::Reference(r)) if Rc::ptr_eq(r, &c));
                    if ours {
                        let old = if global {
                            cv_slot_entry_released();
                            tab.arr.borrow_mut().remove(&key)
                        } else {
                            tab.arr
                                .borrow_mut()
                                .get_mut(&key)
                                .map(|s| std::mem::replace(s, Value::Undef))
                        };
                        tab.bump();
                        drop(old);
                    }
                }
            }
            Value::Reference(r) => {
                // The CV was bound to another reference ($x = &$y).
                cvs[i] = Value::Reference(r.clone());
                let same = matches!(tab.arr.borrow().get(&key), Some(Value::Reference(t)) if Rc::ptr_eq(t, &r));
                if !same {
                    let old = replace_entry(&tab.arr, key, Value::Reference(r.clone()));
                    tab.bump();
                    drop(old);
                }
                b.cells[i] = Some(r);
            }
            v => {
                // A plain value was stored into the slot: assign it to the
                // bound variable (or create the entry).
                let cell = match b.cells[i].clone() {
                    Some(c) => Some(c),
                    None => match tab.arr.borrow().get(&key) {
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
                        let old = replace_entry(&tab.arr, key, Value::Reference(c.clone()));
                        tab.bump();
                        if global {
                            cv_slot_entry_created();
                        }
                        drop(old);
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
        fs: &mut FrameSymtab,
    ) {
        let Some(c) = fs.bind.cells.get_mut(i).and_then(|c| c.take()) else {
            return;
        };
        let global = fs.tab.global;
        if let Some(name) = names.get(i) {
            let key = gkey(name);
            let ours = matches!(fs.tab.arr.borrow().get(&key), Some(Value::Reference(r)) if Rc::ptr_eq(r, &c));
            if ours {
                let old = if global {
                    cv_slot_entry_released();
                    fs.tab.arr.borrow_mut().remove(&key)
                } else {
                    fs.tab
                        .arr
                        .borrow_mut()
                        .get_mut(&key)
                        .map(|s| std::mem::replace(s, Value::Undef))
                };
                fs.tab.bump();
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
