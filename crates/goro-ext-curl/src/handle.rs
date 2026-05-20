use std::cell::{Cell, RefCell};
use std::collections::HashMap;

use goro_core::value::Value;

/// Represents a PHP curl handle with all its options and response data.
#[derive(Clone)]
pub struct CurlHandle {
    // URL and request
    pub url: String,
    pub method: String,
    pub headers: Vec<(String, String)>,
    pub post_fields: Option<Vec<u8>>,
    pub custom_request: Option<String>,

    // Options
    pub return_transfer: bool,
    pub follow_location: bool,
    pub max_redirects: i64,
    pub timeout: u64,
    pub connect_timeout: u64,
    pub user_agent: String,
    pub include_header: bool,
    pub nobody: bool,
    pub ssl_verify_peer: bool,
    pub ssl_verify_host: i64,
    pub encoding: Option<String>,
    pub cookie: Option<String>,
    pub userpwd: Option<String>,
    pub http_auth: i64,
    pub fail_on_error: bool,

    /// CURLOPT_PRIVATE - holds arbitrary user-provided value
    pub private: Value,

    // Response (filled after curl_exec)
    pub response_code: i64,
    pub response_headers: Vec<(String, String)>,
    pub response_body: Vec<u8>,
    pub header_size: usize,
    pub error_message: String,
    pub error_number: i64,
    pub content_type: String,
    pub effective_url: String,
    pub redirect_count: i64,
    pub total_time: f64,
}

impl CurlHandle {
    pub fn new() -> Self {
        Self {
            url: String::new(),
            method: "GET".to_string(),
            headers: Vec::new(),
            post_fields: None,
            custom_request: None,

            return_transfer: false,
            follow_location: false,
            max_redirects: -1,
            timeout: 0,
            connect_timeout: 0,
            user_agent: String::new(),
            include_header: false,
            nobody: false,
            ssl_verify_peer: true,
            ssl_verify_host: 2,
            encoding: None,
            cookie: None,
            userpwd: None,
            http_auth: 0,
            fail_on_error: false,

            private: Value::Null,

            response_code: 0,
            response_headers: Vec::new(),
            response_body: Vec::new(),
            header_size: 0,
            error_message: String::new(),
            error_number: 0,
            content_type: String::new(),
            effective_url: String::new(),
            redirect_count: 0,
            total_time: 0.0,
        }
    }

    /// Reset all options to defaults (like curl_reset)
    pub fn reset(&mut self) {
        self.url.clear();
        self.method = "GET".to_string();
        self.headers.clear();
        self.post_fields = None;
        self.custom_request = None;

        self.return_transfer = false;
        self.follow_location = false;
        self.max_redirects = -1;
        self.timeout = 0;
        self.connect_timeout = 0;
        self.user_agent.clear();
        self.include_header = false;
        self.nobody = false;
        self.ssl_verify_peer = true;
        self.ssl_verify_host = 2;
        self.encoding = None;
        self.cookie = None;
        self.userpwd = None;
        self.http_auth = 0;
        self.fail_on_error = false;
        self.private = Value::Null;

        self.response_code = 0;
        self.response_headers.clear();
        self.response_body.clear();
        self.header_size = 0;
        self.error_message.clear();
        self.error_number = 0;
        self.content_type.clear();
        self.effective_url.clear();
        self.redirect_count = 0;
        self.total_time = 0.0;
    }
}

/// Wrapper that clears the handle's `private` Value fields on Drop.
/// This avoids the "thread local panicked on drop" issue when a CurlHandle
/// stored in CURL_HANDLES outlives FREED_OBJECT_IDS during shutdown.
pub struct HandleStore {
    pub handles: RefCell<HashMap<i64, CurlHandle>>,
}

impl Default for HandleStore {
    fn default() -> Self {
        Self { handles: RefCell::new(HashMap::new()) }
    }
}

impl Drop for HandleStore {
    fn drop(&mut self) {
        // Best-effort: forget the inner HashMap to avoid running drop on the
        // CurlHandles. This deliberately leaks at shutdown, but avoids a
        // thread-local-after-destruction panic when Value::Object inside a
        // CurlHandle tries to access FREED_OBJECT_IDS during teardown.
        let map = std::mem::take(&mut *self.handles.borrow_mut());
        std::mem::forget(map);
    }
}

thread_local! {
    pub static CURL_HANDLES: HandleStore = HandleStore::default();
    pub static NEXT_CURL_ID: Cell<i64> = const { Cell::new(1) };
}
