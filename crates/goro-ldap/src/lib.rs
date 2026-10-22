//! A pure-Rust LDAP client modelled on OpenLDAP's libldap/liblber, as PHP's
//! ext/ldap uses it: BER/DER codec, LDAPv3 operations, URL/DN parsing,
//! ldap_escape, ldap_err2string texts and the option semantics.

pub mod ber;
pub mod client;
pub mod consts;
pub mod controls;
pub mod dn;
pub mod error;
pub mod escape;
pub mod filter;
pub mod message;
pub mod tls;
pub mod url;

pub use client::{Conn, Options};
pub use error::err2string;
pub use message::{Control, Entry, LdapMessage, Reference};

/// MSG_NOSIGNAL for send(); 0 where unavailable.
#[inline]
pub(crate) fn msg_nosignal() -> i32 {
    #[cfg(target_os = "linux")]
    {
        libc::MSG_NOSIGNAL
    }
    #[cfg(not(target_os = "linux"))]
    {
        0
    }
}
