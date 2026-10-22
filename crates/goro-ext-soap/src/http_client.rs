//! The HTTP request/response exchange of make_http_soap_request().

use goro_core::value::Value;
use goro_core::vm::Vm;

use crate::http::Url;
use crate::php::*;
use crate::types::*;

#[allow(clippy::too_many_arguments)]
pub fn request(
    vm: &mut Vm,
    this: &Obj,
    _buf: &[u8],
    _uri: Url,
    _use_ssl: bool,
    _soapaction: &str,
    _soap_version: i64,
    _want_response: bool,
) -> SR<Option<Value>> {
    add_soap_fault(vm, this, "HTTP", "Could not connect to host");
    Ok(None)
}
