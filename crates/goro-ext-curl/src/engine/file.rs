//! The file:// protocol (lib/file.c)

use std::io::{Read, Seek, SeekFrom, Write};
use std::os::unix::fs::{MetadataExt, OpenOptionsExt};

use super::consts::{code, opt};
use super::easy::{Easy, Timer};
use super::http::{MONTH, WKDAY, gmtime};
use super::options::HttpReq;
use super::transfer::Target;
use super::{Callbacks, CurlCode, ReadResult, WRITEFUNC_ERROR, WRITEFUNC_PAUSE};

impl Easy {
    /// file_connect() + file_do() + done
    pub(crate) fn file_transfer(&mut self, cb: &mut dyn Callbacks, t: &Target) -> CurlCode {
        let upload = self.st.httpreq == HttpReq::Put;
        let path = super::urlapi::easy_unescape(&t.tu.path);
        if path.contains(&0) {
            return code::URL_MALFORMAT;
        }
        let pstr = std::ffi::OsStr::from_bytes_ext(&path);
        let file = std::fs::File::open(pstr);
        if !upload && file.is_err() {
            self.failf(
                cb,
                &format!("Couldn't open file {}", String::from_utf8_lossy(&t.tu.path)),
            );
            self.infof(cb, &format!("closing connection #{}", -1));
            return code::FILE_COULDNT_READ_FILE;
        }
        // the "connection" gets an id now
        let id = {
            let pool = self.conn_pool();
            let mut p = pool.0.borrow_mut();
            let id = p.next_conn_id;
            p.next_conn_id += 1;
            id
        };
        self.st.recent_conn_id = id;
        self.progress.time(Timer::PostQueue);
        if self.set.callbacks.contains(&opt::PREREQFUNCTION) {
            let rc = cb.prereq("", "", -1, -1);
            if rc != 0 {
                self.failf(cb, "operation aborted by pre-request callback");
                self.infof(cb, &format!("shutting down connection #{id}"));
                return code::ABORTED_BY_CALLBACK;
            }
        }
        self.progress.time(Timer::PreTransfer);
        let r = if upload {
            self.file_upload(cb, &path)
        } else {
            self.file_do(cb, file.unwrap(), &path)
        };
        self.progress.calc();
        let r = match r {
            Ok(()) => {
                if self.pgrs_update(cb) {
                    code::ABORTED_BY_CALLBACK
                } else {
                    code::OK
                }
            }
            Err(e) => e,
        };
        let premature = matches!(
            r,
            code::ABORTED_BY_CALLBACK | code::READ_ERROR | code::WRITE_ERROR
        );
        self.infof(
            cb,
            &format!(
                "{} connection #{id}",
                if premature {
                    "closing"
                } else {
                    "shutting down"
                }
            ),
        );
        r
    }

    fn file_write(
        &mut self,
        cb: &mut dyn Callbacks,
        data: &[u8],
        header: bool,
    ) -> Result<(), CurlCode> {
        if !header && self.set.verbose {
            self.tracer().debug(cb, super::consts::info::DATA_IN, data);
        }
        if header {
            if self.set.include_header {
                self.file_cb_write(cb, data, false)?;
            }
            self.file_cb_write(cb, data, true)
        } else {
            self.file_cb_write(cb, data, false)?;
            self.progress.dl_cur += data.len() as i64;
            Ok(())
        }
    }

    fn file_cb_write(
        &mut self,
        cb: &mut dyn Callbacks,
        data: &[u8],
        hds: bool,
    ) -> Result<(), CurlCode> {
        let mut data = data;
        while !data.is_empty() {
            let wlen = if hds {
                data.len()
            } else {
                data.len().min(16384)
            };
            let n = if hds {
                cb.header(&data[..wlen])
            } else {
                cb.write(&data[..wlen])
            };
            if n == WRITEFUNC_PAUSE {
                self.failf(cb, "Write callback asked for PAUSE when not supported");
                return Err(code::WRITE_ERROR);
            }
            if n == WRITEFUNC_ERROR {
                self.failf(
                    cb,
                    &format!("client returned ERROR on write of {wlen} bytes"),
                );
                return Err(code::WRITE_ERROR);
            }
            if n != wlen {
                self.failf(
                    cb,
                    &format!(
                        "Failure writing output to destination, passed {wlen} returned {}",
                        n as isize
                    ),
                );
                return Err(code::WRITE_ERROR);
            }
            data = &data[wlen..];
        }
        Ok(())
    }

    fn file_do(
        &mut self,
        cb: &mut dyn Callbacks,
        mut f: std::fs::File,
        path: &[u8],
    ) -> Result<(), CurlCode> {
        let md = f.metadata().ok();
        let mut expected: i64 = -1;
        let is_dir = md.as_ref().is_some_and(|m| m.is_dir());
        if let Some(m) = &md {
            if !m.is_dir() {
                expected = m.len() as i64;
            }
            self.info.filetime = m.mtime();
            if self.set.timecondition != 0
                && !self.set.strs.contains_key(&opt::RANGE)
                && !self.meets_timecondition(cb, self.info.filetime)
            {
                return Ok(());
            }
            self.progress.time(Timer::StartTransfer);
            if expected >= 0 {
                self.file_write(
                    cb,
                    format!("Content-Length: {expected}\r\n").as_bytes(),
                    true,
                )?;
                self.file_write(cb, b"Accept-ranges: bytes\r\n", true)?;
            }
            let (y, mon, d, h, mi, s, wday) = gmtime(m.mtime());
            let wd = WKDAY[if wday > 0 { wday - 1 } else { 6 }];
            let lm = format!(
                "Last-Modified: {wd}, {d:02} {} {y:4} {h:02}:{mi:02}:{s:02} GMT\r\n",
                MONTH[mon]
            );
            self.file_write(cb, lm.as_bytes(), true)?;
            self.file_write(cb, b"\r\n", true)?;
            self.progress.set_download_size(expected);
            if self.set.opt_no_body {
                return Ok(());
            }
        }
        // Curl_range()
        let mut maxdownload: i64 = -1;
        if let Some(r) = self.set.strs.get(&opt::RANGE).cloned() {
            let s = String::from_utf8_lossy(&r).into_owned();
            if let Some((a, b)) = s.split_once('-') {
                let a = a.trim();
                let b = b.trim();
                if a.is_empty() {
                    if let Ok(n) = b.parse::<i64>() {
                        self.st.resume_from = -n;
                    }
                } else if let Ok(from) = a.parse::<i64>() {
                    self.st.resume_from = from;
                    if let Ok(to) = b.parse::<i64>() {
                        maxdownload = to - from + 1;
                    }
                }
            }
        }
        if self.st.resume_from < 0 {
            if md.is_none() {
                self.failf(cb, "cannot get the size of file.");
                return Err(code::READ_ERROR);
            }
            self.st.resume_from += expected.max(0);
        }
        if self.st.resume_from > 0 {
            if self.st.resume_from <= expected {
                expected -= self.st.resume_from;
            } else {
                self.failf(cb, "failed to resume file:// transfer");
                return Err(code::BAD_DOWNLOAD_RESUME);
            }
        }
        if maxdownload > 0 {
            expected = maxdownload;
        }
        let size_known = md.is_some() && expected > 0;
        if size_known {
            self.progress.set_download_size(expected);
        }
        if self.st.resume_from > 0 {
            if is_dir || f.seek(SeekFrom::Start(self.st.resume_from as u64)).is_err() {
                return Err(code::BAD_DOWNLOAD_RESUME);
            }
        }
        let blen = self.set.buffer_size as usize;
        if !is_dir {
            let mut buf = vec![0u8; blen];
            loop {
                let want = if size_known {
                    (expected as usize).min(blen - 1)
                } else {
                    blen - 1
                };
                let n = f.read(&mut buf[..want]).unwrap_or(0);
                if n == 0 || (size_known && expected == 0) {
                    break;
                }
                if size_known {
                    expected -= n as i64;
                }
                self.file_write(cb, &buf[..n], false)?;
                if self.pgrs_update(cb) {
                    return Err(code::ABORTED_BY_CALLBACK);
                }
            }
        } else {
            let dir = std::fs::read_dir(std::ffi::OsStr::from_bytes_ext(path))
                .map_err(|_| code::READ_ERROR)?;
            for e in dir.flatten() {
                use std::os::unix::ffi::OsStrExt;
                let name = e.file_name();
                let nb = name.as_bytes();
                if nb.first() != Some(&b'.') {
                    self.file_write(cb, nb, false)?;
                    self.file_write(cb, b"\n", false)?;
                }
            }
        }
        Ok(())
    }

    fn file_upload(&mut self, cb: &mut dyn Callbacks, path: &[u8]) -> Result<(), CurlCode> {
        let Some(slash) = path.iter().position(|&c| c == b'/') else {
            return Err(code::FILE_COULDNT_READ_FILE);
        };
        if slash + 1 >= path.len() {
            return Err(code::FILE_COULDNT_READ_FILE);
        }
        let perms = self
            .set
            .longs
            .get(&opt::NEW_FILE_PERMS)
            .copied()
            .unwrap_or(0o644) as u32;
        let mut oo = std::fs::OpenOptions::new();
        oo.write(true).create(true).mode(perms);
        if self.st.resume_from != 0 {
            oo.append(true);
        } else {
            oo.truncate(true);
        }
        let pstr = std::ffi::OsStr::from_bytes_ext(path);
        let mut f = match oo.open(pstr) {
            Ok(f) => f,
            Err(_) => {
                self.failf(
                    cb,
                    &format!("cannot open {} for writing", String::from_utf8_lossy(path)),
                );
                return Err(code::WRITE_ERROR);
            }
        };
        if self.st.infilesize != -1 {
            self.progress.set_upload_size(self.st.infilesize);
        }
        if self.st.resume_from < 0 {
            match f.metadata() {
                Ok(m) => self.st.resume_from = m.len() as i64,
                Err(_) => {
                    self.failf(
                        cb,
                        &format!("cannot get the size of {}", String::from_utf8_lossy(path)),
                    );
                    return Err(code::WRITE_ERROR);
                }
            }
        }
        let total = self.st.infilesize;
        let mut readn: i64 = 0;
        let mut bytecount: i64 = 0;
        let ulb = self.set.upload_buffer_size as usize;
        loop {
            let mut want = ulb;
            if total >= 0 {
                let remain = total - readn;
                if remain <= 0 {
                    break;
                }
                want = want.min(remain as usize);
            }
            let data = match cb.read(want) {
                ReadResult::Data(d) => d,
                ReadResult::Abort => {
                    self.failf(cb, "operation aborted by callback");
                    return Err(code::ABORTED_BY_CALLBACK);
                }
                ReadResult::Pause => {
                    self.failf(cb, "Read callback asked for PAUSE when not supported");
                    return Err(code::READ_ERROR);
                }
            };
            if data.is_empty() {
                if total >= 0 && readn < total {
                    self.failf(
                        cb,
                        &format!("client read function EOF fail, only {readn}/{total} of needed bytes read"),
                    );
                    return Err(code::READ_ERROR);
                }
                break;
            }
            if data.len() > want {
                self.failf(cb, "read function returned funny value");
                return Err(code::READ_ERROR);
            }
            readn += data.len() as i64;
            let mut d: &[u8] = &data;
            if self.st.resume_from > 0 {
                if (d.len() as i64) <= self.st.resume_from {
                    self.st.resume_from -= d.len() as i64;
                    d = &[];
                } else {
                    d = &d[self.st.resume_from as usize..];
                    self.st.resume_from = 0;
                }
            }
            if f.write_all(d).is_err() {
                return Err(code::SEND_ERROR);
            }
            bytecount += d.len() as i64;
            self.progress.ul_cur = bytecount;
            if self.pgrs_update(cb) {
                return Err(code::ABORTED_BY_CALLBACK);
            }
        }
        Ok(())
    }
}

trait FromBytesExt {
    fn from_bytes_ext(b: &[u8]) -> &std::ffi::OsStr;
}

impl FromBytesExt for std::ffi::OsStr {
    fn from_bytes_ext(b: &[u8]) -> &std::ffi::OsStr {
        use std::os::unix::ffi::OsStrExt;
        std::ffi::OsStr::from_bytes(b)
    }
}
