//! curl_share (lib/share.c)

use std::cell::RefCell;
use std::rc::Rc;

use super::consts::{lock, shcode, shopt};
use super::cookie::CookieInfo;
use super::net::{DnsCache, Pool};
use super::{ConnPool, CurlSHcode};

/// A share handle: data shared between easy handles
#[derive(Default)]
pub struct Share {
    pub(crate) specifier: u32,
    /// number of easy handles using this share
    pub(crate) dirty: u32,
    pub(crate) cookies: Option<Rc<RefCell<CookieInfo>>>,
    pub(crate) dns: Option<Rc<RefCell<DnsCache>>>,
    pub(crate) cpool: Option<ConnPool>,
    pub(crate) ssl_session: bool,
}

impl Share {
    /// curl_share_init()
    pub fn new() -> Share {
        Share::default()
    }

    /// curl_share_setopt() for CURLSHOPT_SHARE / CURLSHOPT_UNSHARE (and the
    /// lock function options, which are accepted and ignored)
    pub fn setopt(&mut self, option: u32, value: i64) -> CurlSHcode {
        if self.dirty > 0 {
            return shcode::IN_USE;
        }
        let ty = value;
        let mut res = shcode::OK;
        match option {
            shopt::SHARE => {
                match ty {
                    x if x == lock::DNS as i64 => {
                        if self.dns.is_none() {
                            self.dns = Some(Rc::new(RefCell::new(DnsCache::default())));
                        }
                    }
                    x if x == lock::COOKIE as i64 => {
                        if self.cookies.is_none() {
                            self.cookies =
                                Some(Rc::new(RefCell::new(CookieInfo::new_initialized(true))));
                        }
                    }
                    x if x == lock::HSTS as i64 => {}
                    x if x == lock::SSL_SESSION as i64 => self.ssl_session = true,
                    x if x == lock::CONNECT as i64 => {
                        if self.cpool.is_none() {
                            self.cpool = Some(ConnPool(Rc::new(RefCell::new(Pool::default()))));
                        }
                    }
                    x if x == lock::PSL as i64 => res = shcode::NOT_BUILT_IN,
                    _ => res = shcode::BAD_OPTION,
                }
                if res == shcode::OK {
                    self.specifier |= 1u32.wrapping_shl(ty as u32);
                }
            }
            shopt::UNSHARE => {
                self.specifier &= !(1u32.wrapping_shl(ty as u32));
                match ty {
                    x if x == lock::DNS as i64 => self.dns = None,
                    x if x == lock::COOKIE as i64 => self.cookies = None,
                    x if x == lock::HSTS as i64 => {}
                    x if x == lock::SSL_SESSION as i64 => self.ssl_session = false,
                    x if x == lock::CONNECT as i64 => {}
                    _ => res = shcode::BAD_OPTION,
                }
            }
            shopt::LOCKFUNC | shopt::UNLOCKFUNC | shopt::USERDATA => {}
            _ => res = shcode::BAD_OPTION,
        }
        res
    }

    pub(crate) fn shares(&self, what: u32) -> bool {
        self.specifier & (1 << what) != 0
    }
}
