//! The seam between socket streams (the standard extension's xp_socket
//! layer) and the TLS implementation of the openssl extension
//! (ext/openssl/xp_ssl.c): PHP_STREAM_OPTION_CRYPTO_API and the
//! php_openssl_sockop_* I/O of an encrypted socket stream.

use crate::array::PhpArray;
use crate::value::Value;
use crate::vm::Vm;

/// STREAM_XPORT_CRYPTO_OP_SETUP inputs.
pub struct CryptoSetup<'a> {
    /// STREAM_CRYPTO_IS_CLIENT was set in the method.
    pub is_client: bool,
    /// The requested STREAM_CRYPTO_METHOD_* flags, without the client bit.
    pub method: i64,
    /// The host name derived from the stream URL (sslsock->url_name).
    pub url_name: Option<&'a [u8]>,
    /// The stream context's "ssl" options.
    pub opts: Option<&'a PhpArray>,
    /// The socket is in blocking mode (sslsock->s.is_blocked).
    pub blocked: bool,
    /// The PHP function warnings are reported for.
    pub func: &'a str,
}

/// Result of STREAM_XPORT_CRYPTO_OP_ENABLE.
pub struct CryptoEnable {
    /// -1 failure, 0 "would block" (non-blocking handshake in progress),
    /// 1 success.
    pub result: i32,
    /// Options to store into the stream context's "ssl" options
    /// (peer_certificate / peer_certificate_chain).
    pub captured: Vec<(Vec<u8>, Value)>,
}

/// An encrypted socket stream (php_openssl_netstream_data_t without the
/// php_netstream_data_t part, which the socket layer keeps).
pub trait CryptoStream {
    /// STREAM_XPORT_CRYPTO_OP_ENABLE with activate=true: drive the
    /// handshake on `fd`. `timeout_ms` < 0 means no limit.
    fn enable(
        &mut self,
        vm: &mut Vm,
        func: &str,
        fd: i32,
        blocked: bool,
        timeout_ms: i32,
        opts: Option<&PhpArray>,
    ) -> CryptoEnable;
    /// STREAM_XPORT_CRYPTO_OP_ENABLE with activate=false (SSL_shutdown()).
    /// Returns false when crypto was not active.
    fn disable(&mut self, fd: i32) -> bool;
    /// sslsock->ssl_active
    fn is_active(&self) -> bool;
    /// php_openssl_sockop_io(read). Ok(0) at EOF, Err(WouldBlock) when a
    /// non-blocking read has no data, Err(TimedOut) on timeout.
    fn read(
        &mut self,
        fd: i32,
        buf: &mut [u8],
        blocked: bool,
        timeout_ms: i32,
    ) -> std::io::Result<usize>;
    /// php_openssl_sockop_io(write). Err(WouldBlock) when nothing could be
    /// written without blocking, Err(TimedOut) on timeout.
    fn write(
        &mut self,
        fd: i32,
        data: &[u8],
        blocked: bool,
        timeout_ms: i32,
    ) -> std::io::Result<usize>;
    /// SSL_pending(): decrypted bytes buffered inside the TLS layer.
    fn pending(&self) -> usize;
    /// The "crypto" element of stream_get_meta_data().
    fn meta(&self) -> Option<PhpArray>;
    /// Warnings raised by read/write (emitted by the caller, which has the VM).
    fn take_warnings(&mut self) -> Vec<String>;
    /// Callbacks a read made due (the "reneg_limit_callback" of a server
    /// stream whose client renegotiated too often), each to be called by
    /// the caller with the stream as its argument.
    fn take_callbacks(&mut self) -> Vec<Value> {
        Vec::new()
    }
    /// PHP_STREAM_OPTION_CHECK_LIVENESS on an active TLS stream (SSL_peek()).
    fn alive(&mut self, fd: i32) -> bool;
    /// php_openssl_sockop_close(): send close_notify if active.
    fn close(&mut self, fd: i32);
}

/// Installed by the openssl extension.
#[derive(Clone, Copy)]
pub struct CryptoHooks {
    /// php_openssl_setup_crypto(): None on failure (after warnings).
    pub setup: fn(&mut Vm, &CryptoSetup<'_>) -> Option<Box<dyn CryptoStream>>,
}
