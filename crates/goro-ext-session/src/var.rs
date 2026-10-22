//! Session (un)serialization state. The session serializers use
//! ext/standard's serializer and unserializer with one shared var_hash
//! (serialize::serialize_scope_hook, serialize::UnserializeScope).

thread_local! {
    /// Set when unserialization hit a fatal error (zend_bailout).
    pub static BAILOUT: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}
