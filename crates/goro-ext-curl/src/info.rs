//! What a transfer reports through curl_easy_getinfo(3) (libcurl's
//! CURLINFO_* documentation; values before a transfer observed on PHP 8.5
//! with libcurl 8.18).

/// Transfer information.
#[derive(Clone, Debug)]
pub struct Info {
    /// The last URL used (None before a transfer when the URL set has no
    /// scheme).
    pub effective_url: Option<Vec<u8>>,
    pub response_code: i64,
    pub http_connectcode: i64,
    pub header_size: i64,
    pub request_size: i64,
    pub filetime: i64,
    pub ssl_verify_result: i64,
    pub redirect_count: i64,
    /// Times in microseconds from the start of the transfer.
    pub queue_us: i64,
    pub namelookup_us: i64,
    pub connect_us: i64,
    pub appconnect_us: i64,
    pub pretransfer_us: i64,
    pub posttransfer_us: i64,
    pub starttransfer_us: i64,
    pub total_us: i64,
    pub redirect_us: i64,
    pub size_upload: i64,
    pub size_download: i64,
    pub download_content_length: i64,
    pub upload_content_length: i64,
    pub content_type: Option<Vec<u8>>,
    pub redirect_url: Option<Vec<u8>>,
    pub primary_ip: String,
    pub primary_port: i64,
    pub local_ip: String,
    pub local_port: i64,
    /// CURL_HTTP_VERSION_* of the last response (0 for none).
    pub http_version: i64,
    /// CURLPROTO_* of the last transfer.
    pub protocol: i64,
    pub scheme: Option<String>,
    pub effective_method: Vec<u8>,
    pub header_out: Option<Vec<u8>>,
    pub os_errno: i64,
    pub num_connects: i64,
    pub httpauth_avail: i64,
    pub proxyauth_avail: i64,
    pub httpauth_used: i64,
    pub proxyauth_used: i64,
    pub used_proxy: i64,
    pub conn_id: i64,
    pub referer: Option<Vec<u8>>,
    pub condition_unmet: i64,
    pub retry_after: i64,
    /// Certificate chain information (CURLOPT_CERTINFO): per certificate,
    /// (name, value) pairs.
    pub certinfo: Vec<Vec<(String, Vec<u8>)>>,
}

impl Default for Info {
    fn default() -> Info {
        Info {
            effective_url: None,
            response_code: 0,
            http_connectcode: 0,
            header_size: 0,
            request_size: 0,
            filetime: -1,
            ssl_verify_result: 0,
            redirect_count: 0,
            queue_us: 0,
            namelookup_us: 0,
            connect_us: 0,
            appconnect_us: 0,
            pretransfer_us: 0,
            posttransfer_us: 0,
            starttransfer_us: 0,
            total_us: 0,
            redirect_us: 0,
            size_upload: 0,
            size_download: 0,
            download_content_length: -1,
            upload_content_length: -1,
            content_type: None,
            redirect_url: None,
            primary_ip: String::new(),
            primary_port: -1,
            local_ip: String::new(),
            local_port: -1,
            http_version: 0,
            protocol: 0,
            scheme: None,
            effective_method: b"GET".to_vec(),
            header_out: None,
            os_errno: 0,
            num_connects: 0,
            httpauth_avail: 0,
            proxyauth_avail: 0,
            httpauth_used: 0,
            proxyauth_used: 0,
            used_proxy: 0,
            conn_id: -1,
            referer: None,
            condition_unmet: 0,
            retry_after: 0,
            certinfo: Vec::new(),
        }
    }
}

impl Info {
    /// Download speed in bytes per second.
    pub fn speed_download(&self) -> i64 {
        if self.total_us <= 0 {
            0
        } else {
            (self.size_download as i128 * 1_000_000 / self.total_us as i128) as i64
        }
    }

    /// Upload speed in bytes per second.
    pub fn speed_upload(&self) -> i64 {
        if self.total_us <= 0 {
            0
        } else {
            (self.size_upload as i128 * 1_000_000 / self.total_us as i128) as i64
        }
    }
}
