//! curl_easy_getinfo() (lib/getinfo.c)

use super::InfoVal;
use super::consts::{httpver, info, opt};
use super::easy::Easy;
use super::options::{DEFAULT_CA_BUNDLE, DEFAULT_CA_PATH, HttpReq};

const CURLINFO_STRING: u32 = 0x100000;
const CURLINFO_LONG: u32 = 0x200000;
const CURLINFO_DOUBLE: u32 = 0x300000;
const CURLINFO_SLIST: u32 = 0x400000;
const CURLINFO_PTR: u32 = 0x400000;
const CURLINFO_SOCKET: u32 = 0x500000;
const CURLINFO_OFF_T: u32 = 0x600000;
const CURLINFO_TYPEMASK: u32 = 0xf00000;

fn secs(us: i64) -> f64 {
    us as f64 / 1_000_000.0
}

impl Easy {
    /// curl_easy_getinfo(); None = CURLE_UNKNOWN_OPTION (or not available)
    pub fn getinfo(&self, inf: u32) -> Option<InfoVal> {
        let i = &self.info;
        let p = &self.progress;
        match inf & CURLINFO_TYPEMASK {
            CURLINFO_STRING => {
                let s: Option<Vec<u8>> = match inf {
                    info::EFFECTIVE_URL => Some(self.st.url.clone().unwrap_or_default()),
                    info::EFFECTIVE_METHOD => {
                        let m: &[u8] = match self.set.strs.get(&opt::CUSTOMREQUEST) {
                            Some(c) => c,
                            None => {
                                if self.set.opt_no_body {
                                    b"HEAD"
                                } else {
                                    match self.st.httpreq {
                                        HttpReq::Post | HttpReq::PostForm | HttpReq::PostMime => {
                                            b"POST"
                                        }
                                        HttpReq::Put => b"PUT",
                                        HttpReq::Head => b"HEAD",
                                        HttpReq::Get => b"GET",
                                    }
                                }
                            }
                        };
                        Some(m.to_vec())
                    }
                    info::CONTENT_TYPE => i.contenttype.clone(),
                    info::PRIVATE => None,
                    info::FTP_ENTRY_PATH => None,
                    info::REDIRECT_URL => i.wouldredirect.clone(),
                    info::REFERER => self.st.referer.clone(),
                    info::PRIMARY_IP => Some(i.primary_ip.clone().into_bytes()),
                    info::LOCAL_IP => Some(i.local_ip.clone().into_bytes()),
                    info::RTSP_SESSION_ID => self.set.strs.get(&opt::RTSP_SESSION_ID).cloned(),
                    info::SCHEME => i.conn_scheme.clone().map(String::into_bytes),
                    info::CAPATH => Some(DEFAULT_CA_PATH.as_bytes().to_vec()),
                    info::CAINFO => Some(DEFAULT_CA_BUNDLE.as_bytes().to_vec()),
                    _ => return None,
                };
                Some(InfoVal::Str(s))
            }
            CURLINFO_LONG => {
                let v: i64 = match inf {
                    info::RESPONSE_CODE => i.httpcode,
                    info::HTTP_CONNECTCODE => i.httpproxycode,
                    info::FILETIME => i.filetime,
                    info::HEADER_SIZE => i.header_size,
                    info::REQUEST_SIZE => i.request_size,
                    info::SSL_VERIFYRESULT => i.ssl_verifyresult,
                    info::PROXY_SSL_VERIFYRESULT => i.proxy_ssl_verifyresult,
                    info::REDIRECT_COUNT => self.st.followlocation,
                    info::HTTPAUTH_AVAIL => i.httpauthavail as i64,
                    info::PROXYAUTH_AVAIL => i.proxyauthavail as i64,
                    info::HTTPAUTH_USED => i.httpauthpicked as i64,
                    info::PROXYAUTH_USED => i.proxyauthpicked as i64,
                    info::OS_ERRNO => i.os_errno,
                    info::NUM_CONNECTS => i.numconnects,
                    info::LASTSOCKET => self.last_socket(),
                    info::PRIMARY_PORT => i.primary_port,
                    info::LOCAL_PORT => i.local_port,
                    info::PROXY_ERROR => 0,
                    info::CONDITION_UNMET => {
                        if i.httpcode == 304 {
                            1
                        } else {
                            i.timecond as i64
                        }
                    }
                    info::RTSP_CLIENT_CSEQ | info::RTSP_SERVER_CSEQ | info::RTSP_CSEQ_RECV => 0,
                    info::HTTP_VERSION => match i.httpversion {
                        10 => httpver::_1_0,
                        11 => httpver::_1_1,
                        20 => httpver::_2_0,
                        30 => httpver::_3,
                        _ => httpver::NONE,
                    },
                    info::PROTOCOL => i.conn_protocol,
                    info::USED_PROXY => i.used_proxy as i64,
                    _ => return None,
                };
                Some(InfoVal::Long(v))
            }
            CURLINFO_DOUBLE => {
                let v = match inf {
                    info::TOTAL_TIME => secs(p.timespent),
                    info::NAMELOOKUP_TIME => secs(p.t_nslookup),
                    info::CONNECT_TIME => secs(p.t_connect),
                    info::APPCONNECT_TIME => secs(p.t_appconnect),
                    info::PRETRANSFER_TIME => secs(p.t_pretransfer),
                    info::STARTTRANSFER_TIME => secs(p.t_starttransfer),
                    info::SIZE_UPLOAD => p.ul_cur as f64,
                    info::SIZE_DOWNLOAD => p.dl_cur as f64,
                    info::SPEED_DOWNLOAD => p.dl_speed as f64,
                    info::SPEED_UPLOAD => p.ul_speed as f64,
                    info::CONTENT_LENGTH_DOWNLOAD => {
                        if p.dl_size_known {
                            p.dl_total as f64
                        } else {
                            -1.0
                        }
                    }
                    info::CONTENT_LENGTH_UPLOAD => {
                        if p.ul_size_known {
                            p.ul_total as f64
                        } else {
                            -1.0
                        }
                    }
                    info::REDIRECT_TIME => secs(p.t_redirect),
                    _ => return None,
                };
                Some(InfoVal::Double(v))
            }
            CURLINFO_OFF_T => {
                let v = match inf {
                    info::FILETIME_T => i.filetime,
                    info::SIZE_UPLOAD_T => p.ul_cur,
                    info::SIZE_DOWNLOAD_T => p.dl_cur,
                    info::SPEED_DOWNLOAD_T => p.dl_speed,
                    info::SPEED_UPLOAD_T => p.ul_speed,
                    info::CONTENT_LENGTH_DOWNLOAD_T => {
                        if p.dl_size_known {
                            p.dl_total
                        } else {
                            -1
                        }
                    }
                    info::CONTENT_LENGTH_UPLOAD_T => {
                        if p.ul_size_known {
                            p.ul_total
                        } else {
                            -1
                        }
                    }
                    info::TOTAL_TIME_T => p.timespent,
                    info::NAMELOOKUP_TIME_T => p.t_nslookup,
                    info::CONNECT_TIME_T => p.t_connect,
                    info::APPCONNECT_TIME_T => p.t_appconnect,
                    info::PRETRANSFER_TIME_T => p.t_pretransfer,
                    info::POSTTRANSFER_TIME_T => p.t_posttransfer,
                    info::STARTTRANSFER_TIME_T => p.t_starttransfer,
                    info::QUEUE_TIME_T => p.t_postqueue,
                    info::REDIRECT_TIME_T => p.t_redirect,
                    info::RETRY_AFTER => i.retry_after,
                    info::XFER_ID => self.id,
                    info::CONN_ID => self.st.recent_conn_id,
                    info::EARLYDATA_SENT_T => 0,
                    _ => return None,
                };
                Some(InfoVal::OffT(v))
            }
            CURLINFO_SLIST => match inf {
                info::SSL_ENGINES => Some(InfoVal::Slist(Vec::new())),
                info::COOKIELIST => {
                    let lines = match &self.cookies {
                        Some(c) => c.borrow().netscape_lines(),
                        None => Vec::new(),
                    };
                    Some(InfoVal::Slist(lines))
                }
                info::CERTINFO => Some(InfoVal::CertInfo(i.certinfo.clone())),
                _ => None,
            },
            CURLINFO_SOCKET => match inf {
                info::ACTIVESOCKET => Some(InfoVal::Long(-1)),
                _ => None,
            },
            _ => {
                let _ = CURLINFO_PTR;
                None
            }
        }
    }
}
