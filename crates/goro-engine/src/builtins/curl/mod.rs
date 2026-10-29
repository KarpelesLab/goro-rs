//! ext/curl (PHP manual: Client URL Library): `CurlHandle`,
//! `CurlMultiHandle`, `CurlShareHandle`, `CurlSharePersistentHandle`,
//! `CURLFile`, `CURLStringFile` and the `curl_*()` functions, on the
//! transfer engine of goro-ext-curl.
//!
//! Signatures and class shapes from Reflection on PHP 8.5; the option
//! checks, warnings and exceptions from the manual, the ext/curl tests and
//! behaviour observed on PHP 8.5 (linked with libcurl 8.18).

mod consts;
mod easy;
mod multi;

use crate::registry::Module;

const CURL_HANDLE: &str = r#"<?php
#[NotSerializable]
final class CurlHandle {
    public function __construct() {}
}"#;

const CURL_MULTI_HANDLE: &str = r#"<?php
#[NotSerializable]
#[NotCloneable]
final class CurlMultiHandle {
    public function __construct() {}
}"#;

const CURL_SHARE_HANDLE: &str = r#"<?php
#[NotSerializable]
#[NotCloneable]
final class CurlShareHandle {
    public function __construct() {}
}"#;

const CURL_SHARE_PERSISTENT_HANDLE: &str = r#"<?php
#[NotSerializable]
#[NotCloneable]
final class CurlSharePersistentHandle {
    public readonly array $options;
    public function __construct() {}
}"#;

const CURL_FILE: &str = r#"<?php
class CURLFile {
    public string $name = "";
    public string $mime = "";
    public string $postname = "";
    public function __construct(string $filename, ?string $mime_type = null, ?string $posted_filename = null) {
        $this->name = $filename;
        $this->mime = $mime_type ?? "";
        $this->postname = $posted_filename ?? "";
    }
    public function getFilename() { return $this->name; }
    public function getMimeType() { return $this->mime; }
    public function getPostFilename() { return $this->postname; }
    public function setMimeType(string $mime_type) { $this->mime = $mime_type; }
    public function setPostFilename(string $posted_filename) { $this->postname = $posted_filename; }
}"#;

const CURL_STRING_FILE: &str = r#"<?php
class CURLStringFile {
    public string $data;
    public string $postname;
    public string $mime;
    public function __construct(string $data, string $postname, string $mime = "application/octet-stream") {
        $this->data = $data;
        $this->postname = $postname;
        $this->mime = $mime;
    }
}"#;

pub static MODULE: Module = Module {
    name: "curl",
    functions: &[
        (
            "function curl_close(CurlHandle $handle): void",
            easy::curl_close,
        ),
        (
            "function curl_copy_handle(CurlHandle $handle): CurlHandle|false",
            easy::curl_copy_handle,
        ),
        (
            "function curl_errno(CurlHandle $handle): int",
            easy::curl_errno,
        ),
        (
            "function curl_error(CurlHandle $handle): string",
            easy::curl_error,
        ),
        (
            "function curl_escape(CurlHandle $handle, string $string): string|false",
            easy::curl_escape,
        ),
        (
            "function curl_unescape(CurlHandle $handle, string $string): string|false",
            easy::curl_unescape,
        ),
        (
            "function curl_multi_setopt(CurlMultiHandle $multi_handle, int $option, mixed $value): bool",
            multi::curl_multi_setopt,
        ),
        (
            "function curl_exec(CurlHandle $handle): string|bool",
            easy::curl_exec,
        ),
        (
            "function curl_file_create(string $filename, ?string $mime_type = null, ?string $posted_filename = null): CURLFile",
            easy::curl_file_create,
        ),
        (
            "function curl_getinfo(CurlHandle $handle, ?int $option = null): mixed",
            easy::curl_getinfo,
        ),
        (
            "function curl_init(?string $url = null): CurlHandle|false",
            easy::curl_init,
        ),
        (
            "function curl_upkeep(CurlHandle $handle): bool",
            easy::curl_upkeep,
        ),
        (
            "function curl_multi_add_handle(CurlMultiHandle $multi_handle, CurlHandle $handle): int",
            multi::curl_multi_add_handle,
        ),
        (
            "function curl_multi_get_handles(CurlMultiHandle $multi_handle): array",
            multi::curl_multi_get_handles,
        ),
        (
            "function curl_multi_close(CurlMultiHandle $multi_handle): void",
            multi::curl_multi_close,
        ),
        (
            "function curl_multi_errno(CurlMultiHandle $multi_handle): int",
            multi::curl_multi_errno,
        ),
        (
            "function curl_multi_exec(CurlMultiHandle $multi_handle, &$still_running): int",
            multi::curl_multi_exec,
        ),
        (
            "function curl_multi_getcontent(CurlHandle $handle): ?string",
            easy::curl_multi_getcontent,
        ),
        (
            "function curl_multi_info_read(CurlMultiHandle $multi_handle, &$queued_messages = null): array|false",
            multi::curl_multi_info_read,
        ),
        (
            "function curl_multi_init(): CurlMultiHandle",
            multi::curl_multi_init,
        ),
        (
            "function curl_multi_remove_handle(CurlMultiHandle $multi_handle, CurlHandle $handle): int",
            multi::curl_multi_remove_handle,
        ),
        (
            "function curl_multi_select(CurlMultiHandle $multi_handle, float $timeout = 1.0): int",
            multi::curl_multi_select,
        ),
        (
            "function curl_multi_strerror(int $error_code): ?string",
            multi::curl_multi_strerror,
        ),
        (
            "function curl_pause(CurlHandle $handle, int $flags): int",
            easy::curl_pause,
        ),
        (
            "function curl_reset(CurlHandle $handle): void",
            easy::curl_reset,
        ),
        (
            "function curl_setopt_array(CurlHandle $handle, array $options): bool",
            easy::curl_setopt_array,
        ),
        (
            "function curl_setopt(CurlHandle $handle, int $option, mixed $value): bool",
            easy::curl_setopt,
        ),
        (
            "function curl_share_close(CurlShareHandle $share_handle): void",
            multi::curl_share_close,
        ),
        (
            "function curl_share_errno(CurlShareHandle $share_handle): int",
            multi::curl_share_errno,
        ),
        (
            "function curl_share_init(): CurlShareHandle",
            multi::curl_share_init,
        ),
        (
            "function curl_share_setopt(CurlShareHandle $share_handle, int $option, mixed $value): bool",
            multi::curl_share_setopt,
        ),
        (
            "function curl_share_strerror(int $error_code): ?string",
            multi::curl_share_strerror,
        ),
        (
            "function curl_share_init_persistent(array $share_options): CurlSharePersistentHandle",
            multi::curl_share_init_persistent,
        ),
        (
            "function curl_strerror(int $error_code): ?string",
            easy::curl_strerror,
        ),
        ("function curl_version(): array|false", easy::curl_version),
    ],
    classes: &[
        ("curlhandle", CURL_HANDLE),
        ("curlmultihandle", CURL_MULTI_HANDLE),
        ("curlsharehandle", CURL_SHARE_HANDLE),
        ("curlsharepersistenthandle", CURL_SHARE_PERSISTENT_HANDLE),
        ("curlfile", CURL_FILE),
        ("curlstringfile", CURL_STRING_FILE),
    ],
    methods: &[
        ("curlhandle::__construct", easy::no_construct),
        ("curlmultihandle::__construct", multi::no_construct_multi),
        ("curlsharehandle::__construct", multi::no_construct_share),
        (
            "curlsharepersistenthandle::__construct",
            multi::no_construct_persistent,
        ),
    ],
    hooks: &[
        ("curlhandle", easy::hooks),
        ("curlmultihandle", multi::multi_hooks),
        ("curlsharehandle", multi::share_hooks),
        ("curlsharepersistenthandle", multi::share_hooks),
    ],
    constants: consts::CONSTANTS,
    ini: &[("curl.cainfo", "")],
    info: Some(info),
    ..Module::EMPTY
};

/// The phpinfo() section (its shape observed on PHP 8.5 and in test
/// ext/curl/tests/check_win_config.phpt).
fn info(_vm: &mut crate::vm::Vm) -> String {
    use goro_ext_curl::version as v;
    let mut s = format!(
        "cURL support => enabled\ncURL Information => {}\nAge => {}\nFeatures\n",
        v::VERSION,
        v::AGE
    );
    for (name, _, on) in v::FEATURES {
        s.push_str(&format!("{name} => {}\n", if *on { "Yes" } else { "No" }));
    }
    s.push_str(&format!(
        "Protocols => {}\nHost => {}\nSSL Version => {}\nZLib Version => {}\n",
        v::PROTOCOLS.join(", "),
        v::HOST,
        v::SSL_VERSION,
        v::LIBZ_VERSION
    ));
    s
}
