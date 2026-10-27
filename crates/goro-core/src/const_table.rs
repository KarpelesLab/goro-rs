//! The VM's global constants and its INI settings live in two separate
//! tables (`define("precision", 1)` must not touch the `precision` INI
//! entry, and a user constant may well be called `my.const`).
//!
//! Most of the code base still addresses both through `vm.constants` with
//! the plain map API: those generic accessors route a key to the INI table
//! when it is an INI-entry name ([`crate::vm::is_ini_entry_key`]) and to the
//! constants table otherwise. Code that resolves a *constant* by a name the
//! script chose (`constant()`, `defined()`, `define()`, constant fetches,
//! `get_defined_constants()`) uses the `const_*` accessors, which never
//! look at the INI table.

use crate::fxhash::FxIndexMap;
use crate::value::Value;
use crate::vm::is_ini_entry_key;

pub type Map = FxIndexMap<Vec<u8>, Value>;

#[derive(Clone, Default, Debug)]
pub struct ConstTable {
    consts: Map,
    ini: Map,
}

impl ConstTable {
    /// Room for `consts` constants and `ini` INI entries (the tables keep
    /// insertion order: their capacity is not observable).
    pub fn with_capacity(consts: usize, ini: usize) -> Self {
        Self {
            consts: Map::with_capacity_and_hasher(consts, Default::default()),
            ini: Map::with_capacity_and_hasher(ini, Default::default()),
        }
    }

    #[inline]
    fn route(&self, key: &[u8]) -> &Map {
        if is_ini_entry_key(key) {
            &self.ini
        } else {
            &self.consts
        }
    }

    #[inline]
    fn route_mut(&mut self, key: &[u8]) -> &mut Map {
        if is_ini_entry_key(key) {
            &mut self.ini
        } else {
            &mut self.consts
        }
    }

    // ---- generic (routed) accessors -------------------------------------

    #[inline]
    pub fn get(&self, key: &[u8]) -> Option<&Value> {
        self.route(key).get(key)
    }

    #[inline]
    pub fn get_mut(&mut self, key: &[u8]) -> Option<&mut Value> {
        self.route_mut(key).get_mut(key)
    }

    #[inline]
    pub fn contains_key(&self, key: &[u8]) -> bool {
        self.route(key).contains_key(key)
    }

    #[inline]
    pub fn insert(&mut self, key: Vec<u8>, val: Value) -> Option<Value> {
        self.route_mut(&key).insert(key, val)
    }

    #[inline]
    pub fn shift_remove(&mut self, key: &[u8]) -> Option<Value> {
        self.route_mut(key).shift_remove(key)
    }

    #[inline]
    pub fn swap_remove(&mut self, key: &[u8]) -> Option<Value> {
        self.route_mut(key).swap_remove(key)
    }

    #[inline]
    pub fn entry(&mut self, key: Vec<u8>) -> indexmap::map::Entry<'_, Vec<u8>, Value> {
        if is_ini_entry_key(&key) {
            self.ini.entry(key)
        } else {
            self.consts.entry(key)
        }
    }

    // ---- the constants table --------------------------------------------

    /// The constants table alone.
    #[inline]
    pub fn consts(&self) -> &Map {
        &self.consts
    }

    #[inline]
    pub fn consts_mut(&mut self) -> &mut Map {
        &mut self.consts
    }

    #[inline]
    pub fn const_get(&self, key: &[u8]) -> Option<&Value> {
        self.consts.get(key)
    }

    #[inline]
    pub fn const_contains(&self, key: &[u8]) -> bool {
        self.consts.contains_key(key)
    }

    #[inline]
    pub fn const_insert(&mut self, key: Vec<u8>, val: Value) -> Option<Value> {
        self.consts.insert(key, val)
    }

    #[inline]
    pub fn const_remove(&mut self, key: &[u8]) -> Option<Value> {
        self.consts.shift_remove(key)
    }

    // ---- the INI table ----------------------------------------------------

    /// The INI settings table alone.
    #[inline]
    pub fn ini(&self) -> &Map {
        &self.ini
    }

    #[inline]
    pub fn ini_mut(&mut self) -> &mut Map {
        &mut self.ini
    }
}
