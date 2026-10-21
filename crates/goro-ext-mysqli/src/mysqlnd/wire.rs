//! Packet encoding/decoding (mysqlnd_wireprotocol.c).

use super::conn::Conn;
use super::stats::*;
use super::vio::{Pfc, Vio};
use super::*;

pub const ERROR_MARKER: u8 = 0xFF;
pub const EODATA_MARKER: u8 = 0xFE;
const MARIADB_RPL_VERSION_HACK: &[u8] = b"5.5.5-";

/// enum mysqlnd_packet_type (for the statistics)
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PacketType {
    Greet,
    Auth,
    AuthResp,
    ChangeAuthResp,
    Ok,
    Eof,
    Cmd,
    RsetHeader,
    RsetFld,
    Row,
    Stats,
    PrepareResp,
    ChgUserResp,
    Sha256PkRequest,
    Sha256PkRequestResponse,
    CachedSha2Result,
}

fn stat_byte_count(t: PacketType) -> usize {
    match t {
        PacketType::AuthResp | PacketType::Ok => STAT_BYTES_RECEIVED_OK,
        PacketType::Eof => STAT_BYTES_RECEIVED_EOF,
        PacketType::RsetHeader => STAT_BYTES_RECEIVED_RSET_HEADER,
        PacketType::RsetFld => STAT_BYTES_RECEIVED_RSET_FIELD_META,
        PacketType::Row => STAT_BYTES_RECEIVED_RSET_ROW,
        PacketType::PrepareResp => STAT_BYTES_RECEIVED_PREPARE_RESPONSE,
        PacketType::ChgUserResp => STAT_BYTES_RECEIVED_CHANGE_USER,
        _ => STAT_LAST,
    }
}

fn stat_packet_count(t: PacketType) -> usize {
    match t {
        PacketType::AuthResp | PacketType::Ok => STAT_PACKETS_RECEIVED_OK,
        PacketType::Eof => STAT_PACKETS_RECEIVED_EOF,
        PacketType::RsetHeader => STAT_PACKETS_RECEIVED_RSET_HEADER,
        PacketType::RsetFld => STAT_PACKETS_RECEIVED_RSET_FIELD_META,
        PacketType::Row => STAT_PACKETS_RECEIVED_RSET_ROW,
        PacketType::PrepareResp => STAT_PACKETS_RECEIVED_PREPARE_RESPONSE,
        PacketType::ChgUserResp => STAT_PACKETS_RECEIVED_CHANGE_USER,
        _ => STAT_LAST,
    }
}

/// Reader over a packet body with BAIL_IF_NO_MORE_DATA semantics: reading
/// past the end yields zeros, `bail()` reports when `pos > size`.
pub struct Body<'a> {
    pub buf: &'a [u8],
    pub pos: usize,
    pub size: usize,
}

impl<'a> Body<'a> {
    pub fn new(buf: &'a [u8]) -> Body<'a> {
        Body {
            buf,
            pos: 0,
            size: buf.len(),
        }
    }
    pub fn at(&self, i: usize) -> u8 {
        self.buf.get(i).copied().unwrap_or(0)
    }
    pub fn cur(&self) -> u8 {
        self.at(self.pos)
    }
    pub fn rest(&self) -> &'a [u8] {
        self.buf.get(self.pos..).unwrap_or(&[])
    }
    pub fn u8(&mut self) -> u8 {
        let v = self.cur();
        self.pos += 1;
        v
    }
    pub fn u16(&mut self) -> u32 {
        let v = uint2korr(self.rest());
        self.pos += 2;
        v
    }
    pub fn u32(&mut self) -> u32 {
        let v = uint4korr(self.rest());
        self.pos += 4;
        v
    }
    pub fn lenenc(&mut self) -> u64 {
        net_field_length(self.buf, &mut self.pos)
    }
    pub fn lenenc_ll(&mut self) -> u64 {
        net_field_length_ll(self.buf, &mut self.pos)
    }
    /// BAIL_IF_NO_MORE_DATA
    pub fn bail(&self, line: u32) -> bool {
        if self.pos > self.size {
            php_error_docref(
                E_WARNING,
                format!("Premature end of data (mysqlnd_wireprotocol.c:{})", line),
            );
            true
        } else {
            false
        }
    }
    /// The C "p - begin - packet->header.size" (as size_t) of premature_end
    pub fn shortfall(&self) -> usize {
        self.pos.wrapping_sub(self.size)
    }
    /// bytes up to the next NUL (or the end)
    pub fn cstr(&mut self) -> &'a [u8] {
        let rest = self.rest();
        let n = rest.iter().position(|&c| c == 0).unwrap_or(rest.len());
        self.pos += n + 1;
        &rest[..n]
    }
}

/// php_mysqlnd_read_error_from_line: (error_no, sqlstate, message)
pub fn read_error_from_line(buf: &[u8]) -> (u32, Vec<u8>, Vec<u8>) {
    let mut error_no = CR_UNKNOWN_ERROR;
    let mut sqlstate = UNKNOWN_SQLSTATE.as_bytes().to_vec();
    let mut error = Vec::new();
    let buf_len = buf.len();
    if buf_len > 2 {
        error_no = uint2korr(buf);
        let mut p = 2;
        if buf[p] == b'#' {
            p += 1;
            if buf_len - p >= 5 {
                sqlstate = buf[p..p + 5].to_vec();
                p += 5;
            } else {
                return (error_no, sqlstate, error);
            }
        }
        if buf_len > p {
            let n = (buf_len - p).min(MYSQLND_ERRMSG_SIZE - 1);
            error = buf[p..p + n].to_vec();
        }
    }
    // the C buffers are NUL terminated strings
    if let Some(z) = error.iter().position(|&c| c == 0) {
        error.truncate(z);
    }
    (error_no, sqlstate, error)
}

fn set_raw_error(ei: &mut ErrorInfo, e: (u32, Vec<u8>, Vec<u8>)) {
    ei.error_no = e.0;
    ei.sqlstate = e.1;
    ei.error = e.2;
}

/// mysqlnd_read_header: (size, packet_no). On failure, an error packet read
/// in place of an out-of-order one is stored in `ei` (without the list).
pub fn read_header(
    pfc: &mut Pfc,
    vio: &mut Vio,
    stats: &mut Stats,
    ei: &mut ErrorInfo,
) -> Result<(usize, u8), ()> {
    let mut buffer = [0u8; MYSQLND_HEADER_SIZE];
    if !pfc.receive(vio, &mut buffer, stats) {
        return Err(());
    }
    let size = uint3korr(&buffer) as usize;
    let packet_no = buffer[3];
    stats.inc2(
        STAT_PROTOCOL_OVERHEAD_IN,
        MYSQLND_HEADER_SIZE as u64,
        STAT_PACKETS_RECEIVED,
        1,
    );
    if pfc.compressed || pfc.packet_no == packet_no {
        pfc.packet_no = pfc.packet_no.wrapping_add(1);
        return Ok((size, packet_no));
    }
    if size > 0 {
        let mut buf = vec![0u8; size];
        if pfc.receive(vio, &mut buf, stats) && buf[0] == ERROR_MARKER {
            set_raw_error(ei, read_error_from_line(&buf[1..]));
            return Err(());
        }
    }
    php_error(
        E_WARNING,
        format!(
            "Packets out of order. Expected {} received {}. Packet size={}",
            pfc.packet_no, packet_no, size
        ),
    );
    Err(())
}

// ── Packets ───────────────────────────────────────────────────────────

#[derive(Default, Debug)]
pub struct Greet {
    pub protocol_version: u8,
    pub server_version: Vec<u8>,
    pub thread_id: u32,
    pub auth_plugin_data: Vec<u8>,
    pub server_capabilities: u32,
    pub charset_no: u32,
    pub server_status: u32,
    pub auth_protocol: Option<Vec<u8>>,
    pub pre41: bool,
    pub error_no: u32,
    pub error: Vec<u8>,
    pub sqlstate: Vec<u8>,
}

#[derive(Default, Debug)]
pub struct AuthResponse {
    pub response_code: u8,
    pub error_no: u32,
    pub error: Vec<u8>,
    pub sqlstate: Vec<u8>,
    pub new_auth_protocol: Option<Vec<u8>>,
    pub new_auth_protocol_data: Option<Vec<u8>>,
    pub affected_rows: u64,
    pub last_insert_id: u64,
    pub server_status: u32,
    pub warning_count: u32,
    pub message: Option<Vec<u8>>,
}

#[derive(Default, Debug)]
pub struct OkPacket {
    pub field_count: u8,
    pub error_no: u32,
    pub error: Vec<u8>,
    pub sqlstate: Vec<u8>,
    pub affected_rows: u64,
    pub last_insert_id: u64,
    pub server_status: u32,
    pub warning_count: u32,
    pub message: Option<Vec<u8>>,
}

#[derive(Default, Debug)]
pub struct EofPacket {
    pub field_count: u8,
    pub error_no: u32,
    pub error: Vec<u8>,
    pub sqlstate: Vec<u8>,
    pub warning_count: u32,
    pub server_status: u32,
}

#[derive(Default, Debug)]
pub struct RsetHeader {
    pub error_info: ErrorInfo,
    pub field_count: u64,
    pub affected_rows: u64,
    pub last_insert_id: u64,
    pub server_status: u32,
    pub warning_count: u32,
    pub info_or_local_file: Option<Vec<u8>>,
}

#[derive(Default, Debug)]
pub struct PrepareResponse {
    pub error_code: u8,
    pub error_info: ErrorInfo,
    pub stmt_id: u32,
    pub field_count: u32,
    pub param_count: u32,
    pub warning_count: u32,
}

#[derive(Default, Debug)]
pub struct ChgUserResponse {
    pub response_code: u8,
    pub error_info: ErrorInfo,
    pub server_asked_323_auth: bool,
    pub new_auth_protocol: Option<Vec<u8>>,
    pub new_auth_protocol_data: Option<Vec<u8>>,
}

#[derive(Default, Debug)]
pub struct CachedSha2Result {
    pub response_code: u8,
    pub result: u8,
    pub error_no: u32,
    pub error: Vec<u8>,
    pub sqlstate: Vec<u8>,
    pub new_auth_protocol: Option<Vec<u8>>,
    pub new_auth_protocol_data: Option<Vec<u8>>,
}

/// st_mysqlnd_packet_row: the state kept between row reads.
#[derive(Default, Debug)]
pub struct RowPacket {
    /// packet->header.size as left by the previous read (the statistics
    /// for a row use the previous row's size, as in php-src)
    pub header_size: usize,
    pub binary_protocol: bool,
    pub eof: bool,
    pub warning_count: u32,
    pub server_status: u32,
    pub error_info: ErrorInfo,
}

/// (auth, field metadata...) the outcome of a packet read
pub type Res<T> = Result<T, ()>;

impl Conn {
    /// mysqlnd_read_packet_header_and_body into the command buffer.
    fn read_packet_header_and_body(&mut self, name: &str, ptype: PacketType) -> Res<Vec<u8>> {
        let (size, _) = match read_header(
            &mut self.pfc,
            &mut self.vio,
            &mut self.stats,
            &mut self.error_info,
        ) {
            Ok(h) => h,
            Err(()) => {
                self.state = ConnState::QuitSent;
                if self.error_info.error_no == 0 {
                    self.error_info.set_str(
                        CR_SERVER_GONE_ERROR,
                        UNKNOWN_SQLSTATE,
                        MYSQLND_SERVER_GONE,
                    );
                }
                let _ = name;
                return Err(());
            }
        };
        if self.pfc.cmd_buffer_length < size {
            self.error_info.set_str(
                CR_INVALID_BUFFER_USE,
                UNKNOWN_SQLSTATE,
                "Packet buffer wasn't big enough; as a workaround consider increasing value of net_cmd_buffer_size",
            );
            return Err(());
        }
        let mut buf = vec![0u8; size];
        if !self.pfc.receive(&mut self.vio, &mut buf, &mut self.stats) {
            self.state = ConnState::QuitSent;
            self.error_info
                .set_str(CR_SERVER_GONE_ERROR, UNKNOWN_SQLSTATE, MYSQLND_SERVER_GONE);
            return Err(());
        }
        self.stats.inc2(
            stat_byte_count(ptype),
            (MYSQLND_HEADER_SIZE + size) as u64,
            stat_packet_count(ptype),
            1,
        );
        Ok(buf)
    }

    /// php_mysqlnd_greet_read
    pub fn read_greet(&mut self) -> Res<Greet> {
        let buf = self.read_packet_header_and_body("greeting", PacketType::Greet)?;
        let mut g = Greet::default();
        let mut b = Body::new(&buf);
        macro_rules! bail {
            ($l:expr) => {
                if b.bail($l) {
                    php_error_docref(
                        E_WARNING,
                        format!("GREET packet {} bytes shorter than expected", b.shortfall()),
                    );
                    return Err(());
                }
            };
        }
        bail!(358);
        g.auth_plugin_data = vec![0u8; SCRAMBLE_LENGTH];
        g.protocol_version = b.u8();
        bail!(365);
        if g.protocol_version == ERROR_MARKER {
            let (no, st, er) = read_error_from_line(b.buf.get(1..).unwrap_or(&[]));
            g.error_no = no;
            g.sqlstate = if no == 1040 { b"08004".to_vec() } else { st };
            g.error = er;
            return Ok(g);
        }
        if b.rest().starts_with(MARIADB_RPL_VERSION_HACK) {
            b.pos += MARIADB_RPL_VERSION_HACK.len();
        }
        g.server_version = b.cstr().to_vec();
        bail!(391);
        g.thread_id = b.u32();
        bail!(395);
        for i in 0..SCRAMBLE_LENGTH_323 {
            g.auth_plugin_data[i] = b.at(b.pos + i);
        }
        b.pos += SCRAMBLE_LENGTH_323;
        bail!(399);
        b.pos += 1; // pad1
        bail!(403);
        g.server_capabilities = b.u16();
        bail!(407);
        g.charset_no = b.u8() as u32;
        bail!(412);
        g.server_status = b.u16();
        bail!(416);
        let pad_start = b.pos;
        b.pos += 13;
        bail!(421);
        if b.pos < b.size {
            for i in 0..(SCRAMBLE_LENGTH - SCRAMBLE_LENGTH_323) {
                g.auth_plugin_data[SCRAMBLE_LENGTH_323 + i] = b.at(b.pos + i);
            }
            b.pos += SCRAMBLE_LENGTH - SCRAMBLE_LENGTH_323;
            b.pos += 1;
        } else {
            g.pre41 = true;
        }
        if b.pos < b.size {
            b.pos -= 1;
            g.server_capabilities |= uint2korr(&buf[pad_start..]) << 16;
            let l = b.at(pad_start + 2) as usize;
            if l > SCRAMBLE_LENGTH {
                for i in 0..(l - SCRAMBLE_LENGTH) {
                    g.auth_plugin_data.push(b.at(b.pos + i));
                }
                b.pos += l - SCRAMBLE_LENGTH;
            }
            // authentication_plugin_data.l = l (may be < SCRAMBLE_LENGTH)
            if l < SCRAMBLE_LENGTH {
                g.auth_plugin_data.truncate(l);
            }
        }
        if g.server_capabilities & CLIENT_PLUGIN_AUTH != 0 {
            bail!(456);
            let remaining = b.size.saturating_sub(b.pos);
            if remaining == 0 {
                g.auth_protocol = Some(Vec::new());
            } else {
                let rest = &buf[b.pos..b.size];
                let n = rest.iter().position(|&c| c == 0).unwrap_or(rest.len());
                g.auth_protocol = Some(rest[..n].to_vec());
            }
        }
        Ok(g)
    }

    /// php_mysqlnd_auth_write (the handshake response; not change user).
    #[allow(clippy::too_many_arguments)]
    pub fn write_auth(
        &mut self,
        client_flags: u32,
        max_packet_size: u32,
        charset_no: u32,
        user: &[u8],
        auth_data: &[u8],
        db: &[u8],
        auth_plugin_name: Option<&[u8]>,
        connect_attr: Option<&[(Vec<u8>, Vec<u8>)]>,
    ) -> usize {
        let p = self.build_auth_payload(
            false,
            client_flags,
            max_packet_size,
            charset_no,
            user,
            auth_data,
            db,
            auth_plugin_name,
            connect_attr,
        );
        let p = match p {
            Some(p) => p,
            None => return 0,
        };
        let sent = self
            .pfc
            .send(&mut self.vio, &p, &mut self.stats, &mut self.error_info);
        if sent == 0 {
            self.state = ConnState::QuitSent;
        }
        sent
    }

    /// The payload of php_mysqlnd_auth_write.
    #[allow(clippy::too_many_arguments)]
    pub fn build_auth_payload(
        &mut self,
        is_change_user: bool,
        client_flags: u32,
        max_packet_size: u32,
        charset_no: u32,
        user: &[u8],
        auth_data: &[u8],
        db: &[u8],
        auth_plugin_name: Option<&[u8]>,
        connect_attr: Option<&[(Vec<u8>, Vec<u8>)]>,
    ) -> Option<Vec<u8>> {
        const AUTH_WRITE_BUFFER_LEN: usize = MYSQLND_HEADER_SIZE
            + MYSQLND_MAX_ALLOWED_USER_LEN
            + SCRAMBLE_LENGTH
            + MYSQLND_MAX_ALLOWED_DB_LEN
            + 1
            + 4096;
        let mut p = Vec::new();
        if !is_change_user {
            int4store(&mut p, client_flags);
            int4store(&mut p, max_packet_size);
            p.push(charset_no as u8);
            p.extend_from_slice(&[0u8; 23]);
        }
        // send_auth_data is always set
        let ulen = user
            .iter()
            .position(|&c| c == 0)
            .unwrap_or(user.len())
            .min(MYSQLND_MAX_ALLOWED_USER_LEN);
        p.extend_from_slice(&user[..ulen]);
        p.push(0);
        if auth_data.len() > 0xFF {
            let msg = "Authentication data too long. Won't fit into the buffer and will be truncated. Authentication will thus fail";
            self.error_info
                .set_str(CR_UNKNOWN_ERROR, UNKNOWN_SQLSTATE, msg);
            php_error_docref(E_WARNING, msg);
            return None;
        }
        p.push(auth_data.len() as u8);
        if AUTH_WRITE_BUFFER_LEN < auth_data.len() + p.len() + MYSQLND_HEADER_SIZE {
            return None;
        }
        p.extend_from_slice(auth_data);
        if !db.is_empty() {
            let n = db.len().min(MYSQLND_MAX_ALLOWED_DB_LEN);
            p.extend_from_slice(&db[..n]);
            p.push(0);
        } else if is_change_user {
            p.push(0);
        }
        if is_change_user && charset_no != 0 {
            int2store(&mut p, charset_no);
        }
        if let Some(name) = auth_plugin_name {
            let n = name.iter().position(|&c| c == 0).unwrap_or(name.len());
            let room = AUTH_WRITE_BUFFER_LEN - (p.len() + MYSQLND_HEADER_SIZE) - 1;
            p.extend_from_slice(&name[..n.min(room)]);
            p.push(0);
        }
        if let Some(attrs) = connect_attr {
            if !attrs.is_empty() {
                let mut ca_payload_len = 0usize;
                for (k, v) in attrs {
                    ca_payload_len += net_store_length_size(k.len() as u64) + k.len();
                    ca_payload_len += net_store_length_size(v.len() as u64) + v.len();
                }
                if AUTH_WRITE_BUFFER_LEN
                    >= ca_payload_len
                        + net_store_length_size(ca_payload_len as u64)
                        + p.len()
                        + MYSQLND_HEADER_SIZE
                {
                    net_store_length(&mut p, ca_payload_len as u64);
                    for (k, v) in attrs {
                        net_store_length(&mut p, k.len() as u64);
                        p.extend_from_slice(k);
                        net_store_length(&mut p, v.len() as u64);
                        p.extend_from_slice(v);
                    }
                }
            }
        }
        Some(p)
    }

    /// php_mysqlnd_auth_response_read
    pub fn read_auth_response(&mut self) -> Res<AuthResponse> {
        let buf = self.read_packet_header_and_body("OK", PacketType::Ok)?;
        let mut r = AuthResponse::default();
        let mut b = Body::new(&buf);
        macro_rules! bail {
            ($l:expr) => {
                if b.bail($l) {
                    php_error_docref(
                        E_WARNING,
                        format!(
                            "AUTH_RESPONSE packet {} bytes shorter than expected",
                            b.shortfall()
                        ),
                    );
                    return Err(());
                }
            };
        }
        bail!(690);
        r.response_code = b.u8();
        bail!(695);
        if r.response_code == ERROR_MARKER {
            let (no, st, er) = read_error_from_line(b.buf.get(1..).unwrap_or(&[]));
            r.error_no = no;
            r.sqlstate = st;
            r.error = er;
            return Ok(r);
        }
        if r.response_code == 0xFE {
            if b.size > b.pos {
                let name = b.cstr().to_vec();
                r.new_auth_protocol = Some(name);
                let dlen = b.size.saturating_sub(b.pos);
                if dlen > 0 {
                    r.new_auth_protocol_data = Some(buf[b.pos..b.pos + dlen].to_vec());
                }
            }
        } else {
            r.affected_rows = b.lenenc_ll();
            bail!(723);
            r.last_insert_id = b.lenenc_ll();
            bail!(726);
            r.server_status = b.u16();
            bail!(730);
            r.warning_count = b.u16();
            bail!(734);
            if b.size > b.pos {
                let net_len = b.lenenc();
                if net_len != 0 {
                    if b.pos > b.size || ((b.size - b.pos) as u64) < net_len {
                        php_error_docref(
                            E_WARNING,
                            "OK packet message length is past the packet size",
                        );
                        return Err(());
                    }
                    r.message = Some(buf[b.pos..b.pos + net_len as usize].to_vec());
                }
            }
        }
        Ok(r)
    }

    /// php_mysqlnd_change_auth_response_write
    pub fn write_change_auth_response(&mut self, auth_data: &[u8]) -> usize {
        let sent = self.pfc.send(
            &mut self.vio,
            auth_data,
            &mut self.stats,
            &mut self.error_info,
        );
        if sent == 0 {
            self.state = ConnState::QuitSent;
        }
        sent
    }

    /// php_mysqlnd_ok_read
    pub fn read_ok(&mut self) -> Res<OkPacket> {
        let buf = self.read_packet_header_and_body("OK", PacketType::Ok)?;
        let mut r = OkPacket::default();
        let mut b = Body::new(&buf);
        macro_rules! bail {
            ($l:expr) => {
                if b.bail($l) {
                    php_error_docref(
                        E_WARNING,
                        format!("OK packet {} bytes shorter than expected", b.shortfall()),
                    );
                    return Err(());
                }
            };
        }
        bail!(850);
        r.field_count = b.u8();
        bail!(855);
        if r.field_count == ERROR_MARKER {
            let (no, st, er) = read_error_from_line(b.buf.get(1..).unwrap_or(&[]));
            r.error_no = no;
            r.sqlstate = st;
            r.error = er;
            return Ok(r);
        }
        r.affected_rows = b.lenenc_ll();
        bail!(866);
        r.last_insert_id = b.lenenc_ll();
        bail!(869);
        r.server_status = b.u16();
        bail!(873);
        r.warning_count = b.u16();
        bail!(877);
        if b.size > b.pos {
            let net_len = b.lenenc();
            if net_len != 0 {
                let avail = self.pfc.cmd_buffer_length.saturating_sub(b.pos);
                let n = (net_len as usize).min(avail);
                let end = (b.pos + n).min(buf.len());
                let mut m = buf.get(b.pos..end).unwrap_or(&[]).to_vec();
                m.resize(n.min(m.len().max(n)), 0);
                r.message = Some(m);
            }
        }
        bail!(892);
        Ok(r)
    }

    /// php_mysqlnd_eof_read
    pub fn read_eof(&mut self) -> Res<EofPacket> {
        let buf = self.read_packet_header_and_body("EOF", PacketType::Eof)?;
        let mut r = EofPacket::default();
        let mut b = Body::new(&buf);
        macro_rules! bail {
            ($l:expr) => {
                if b.bail($l) {
                    php_error_docref(
                        E_WARNING,
                        format!("EOF packet {} bytes shorter than expected", b.shortfall()),
                    );
                    return Err(());
                }
            };
        }
        bail!(943);
        r.field_count = b.u8();
        bail!(948);
        if r.field_count == ERROR_MARKER {
            let (no, st, er) = read_error_from_line(b.buf.get(1..).unwrap_or(&[]));
            r.error_no = no;
            r.sqlstate = st;
            r.error = er;
            return Ok(r);
        }
        if b.size > 1 {
            r.warning_count = b.u16();
            bail!(966);
            r.server_status = b.u16();
            bail!(970);
        }
        bail!(976);
        Ok(r)
    }

    /// php_mysqlnd_cmd_write
    pub fn write_cmd(&mut self, command: u8, arg: &[u8]) -> usize {
        self.pfc.reset();
        self.stats.inc(STAT_PACKETS_SENT_CMD);
        let mut p = Vec::with_capacity(arg.len() + 1);
        p.push(command);
        p.extend_from_slice(arg);
        if arg.len() + 1 + MYSQLND_HEADER_SIZE > self.pfc.cmd_buffer_length {
            self.stats.inc(STAT_CMD_BUFFER_TOO_SMALL);
        }
        let sent = self
            .pfc
            .send(&mut self.vio, &p, &mut self.stats, &mut self.error_info);
        if sent == 0 {
            self.state = ConnState::QuitSent;
        }
        sent
    }

    /// php_mysqlnd_rset_header_read
    pub fn read_rset_header(&mut self) -> Res<RsetHeader> {
        let buf = self.read_packet_header_and_body("resultset header", PacketType::RsetHeader)?;
        let mut r = RsetHeader::default();
        let mut b = Body::new(&buf);
        macro_rules! bail {
            ($l:expr) => {
                if b.bail($l) {
                    php_error_docref(
                        E_WARNING,
                        format!(
                            "RSET_HEADER packet {} bytes shorter than expected",
                            b.shortfall()
                        ),
                    );
                    return Err(());
                }
            };
        }
        bail!(1072);
        if b.cur() == ERROR_MARKER {
            b.pos += 1;
            bail!(1081);
            let (no, st, er) = read_error_from_line(b.buf.get(1..).unwrap_or(&[]));
            r.error_info.error_no = no;
            r.error_info.sqlstate = st;
            r.error_info.error = er;
            return Ok(r);
        }
        r.field_count = b.lenenc();
        bail!(1090);
        match r.field_count {
            MYSQLND_NULL_LENGTH => {
                let len = b.size - 1;
                r.info_or_local_file = Some(buf[b.pos..b.pos + len].to_vec());
            }
            0 => {
                r.affected_rows = b.lenenc_ll();
                bail!(1111);
                r.last_insert_id = b.lenenc_ll();
                bail!(1114);
                r.server_status = b.u16();
                bail!(1118);
                r.warning_count = b.u16();
                bail!(1122);
                if b.size > b.pos {
                    let len = b.lenenc();
                    if len != 0 {
                        if b.pos > b.size || ((b.size - b.pos) as u64) < len {
                            let over = (b.pos as u64).wrapping_sub(b.size as u64).wrapping_add(len);
                            php_error_docref(
                                E_WARNING,
                                format!(
                                    "RSET_HEADER packet additional data length is past {} bytes the packet size",
                                    over
                                ),
                            );
                            return Err(());
                        }
                        r.info_or_local_file = Some(buf[b.pos..b.pos + len as usize].to_vec());
                    }
                }
            }
            _ => {}
        }
        bail!(1150);
        Ok(r)
    }

    /// php_mysqlnd_rset_field_read; `skip_parsing` for the parameter
    /// metadata of a prepare response. Ok(None) when an error packet was
    /// read (stored in `ei`).
    pub fn read_field(
        &mut self,
        skip_parsing: bool,
        ei: &mut ErrorInfo,
    ) -> Res<Option<super::result::Field>> {
        let buf = self.read_packet_header_and_body("field", PacketType::RsetFld)?;
        if skip_parsing {
            return Ok(None);
        }
        let mut b = Body::new(&buf);
        macro_rules! premature {
            () => {{
                php_error_docref(
                    E_WARNING,
                    format!(
                        "Result set field packet {} bytes shorter than expected",
                        b.shortfall()
                    ),
                );
                return Err(());
            }};
        }
        if b.bail(1219) {
            premature!();
        }
        if b.cur() == ERROR_MARKER {
            b.pos += 1;
            if b.bail(1223) {
                premature!();
            }
            let (no, st, er) = read_error_from_line(b.buf.get(1..).unwrap_or(&[]));
            ei.error_no = no;
            ei.sqlstate = st;
            ei.error = er;
            return Ok(None);
        } else if b.cur() == EODATA_MARKER && b.size < 8 {
            php_error_docref(E_WARNING, "Premature EOF in result field metadata");
            return Err(());
        }
        let mut f = super::result::Field::default();
        macro_rules! read_field {
            ($dst:expr) => {{
                let len = b.lenenc();
                if len == MYSQLND_NULL_LENGTH {
                    php_error_docref(
                        E_WARNING,
                        "Protocol error. Server sent NULL_LENGTH. The server is faulty",
                    );
                    return Err(());
                } else if len != 0 {
                    let start = b.pos.min(buf.len());
                    let end = (b.pos + len as usize).min(buf.len());
                    let mut v = buf[start..end].to_vec();
                    v.resize(len as usize, 0);
                    $dst = v;
                    b.pos += len as usize;
                }
            }};
        }
        read_field!(f.catalog);
        read_field!(f.db);
        read_field!(f.table);
        read_field!(f.org_table);
        read_field!(f.name);
        read_field!(f.org_name);
        if b.cur() != 12 {
            php_error_docref(
                E_WARNING,
                "Protocol error. Server sent false length. Expected 12",
            );
        }
        if b.pos + 12 > b.size {
            php_error_docref(
                E_WARNING,
                "Premature end of data (mysqlnd_wireprotocol.c:1253)",
            );
            premature!();
        }
        b.pos += 1;
        f.charsetnr = b.u16();
        f.length = b.u32();
        f.ftype = b.u8();
        f.flags = b.u16();
        f.decimals = b.u8();
        b.pos += 2;
        if (f.ftype <= MYSQL_TYPE_INT24
            && (f.ftype != MYSQL_TYPE_TIMESTAMP || f.length == 14 || f.length == 8))
            || f.ftype == MYSQL_TYPE_YEAR
        {
            f.flags |= NUM_FLAG;
        }
        if b.size > b.pos {
            let len = b.lenenc();
            if len != 0 && len != MYSQLND_NULL_LENGTH {
                php_error_docref(
                    E_WARNING,
                    "Protocol error. Server sent default for unsupported field list (mysqlnd_wireprotocol.c:1293)",
                );
                return Err(());
            }
        }
        Ok(Some(f))
    }

    /// php_mysqlnd_rowp_read (php_mysqlnd_read_row_ex + markers). Returns
    /// the row buffer (or None at EOF); Err with the packet error set.
    pub fn read_row(&mut self, rp: &mut RowPacket) -> Res<Option<Vec<u8>>> {
        let mut data: Vec<u8> = Vec::new();
        let (mut size, _) = match read_header(
            &mut self.pfc,
            &mut self.vio,
            &mut self.stats,
            &mut rp.error_info,
        ) {
            Ok(h) => h,
            Err(()) => {
                self.state = ConnState::QuitSent;
                set_packet_error(
                    &mut rp.error_info,
                    CR_SERVER_GONE_ERROR,
                    UNKNOWN_SQLSTATE,
                    MYSQLND_SERVER_GONE,
                );
                return Err(());
            }
        };
        while size >= MYSQLND_MAX_PACKET_SIZE {
            let off = data.len();
            data.resize(off + size, 0);
            if !self
                .pfc
                .receive(&mut self.vio, &mut data[off..], &mut self.stats)
            {
                self.state = ConnState::QuitSent;
                set_packet_error(
                    &mut rp.error_info,
                    CR_SERVER_GONE_ERROR,
                    UNKNOWN_SQLSTATE,
                    MYSQLND_SERVER_GONE,
                );
                return Err(());
            }
            match read_header(
                &mut self.pfc,
                &mut self.vio,
                &mut self.stats,
                &mut rp.error_info,
            ) {
                Ok((s, _)) => size = s,
                Err(()) => return Err(()),
            }
        }
        let off = data.len();
        data.resize(off + size, 0);
        if !self
            .pfc
            .receive(&mut self.vio, &mut data[off..], &mut self.stats)
        {
            self.state = ConnState::QuitSent;
            set_packet_error(
                &mut rp.error_info,
                CR_SERVER_GONE_ERROR,
                UNKNOWN_SQLSTATE,
                MYSQLND_SERVER_GONE,
            );
            return Err(());
        }
        self.stats.inc2(
            STAT_BYTES_RECEIVED_RSET_ROW,
            (MYSQLND_HEADER_SIZE + rp.header_size) as u64,
            STAT_PACKETS_RECEIVED_RSET_ROW,
            1,
        );
        let data_size = data.len();
        rp.header_size = data_size;
        let first = data.first().copied().unwrap_or(0);
        if first == ERROR_MARKER {
            let e = read_error_from_line(data.get(1..).unwrap_or(&[]));
            rp.error_info.error_no = e.0;
            rp.error_info.sqlstate = e.1;
            rp.error_info.error = e.2;
            return Err(());
        } else if first == EODATA_MARKER && data_size < 8 {
            rp.eof = true;
            if data_size > 1 {
                rp.warning_count = uint2korr(&data[1..]);
                rp.server_status = uint2korr(data.get(3..).unwrap_or(&[]));
            }
            return Ok(None);
        }
        rp.eof = false;
        self.stats.inc(if rp.binary_protocol {
            STAT_ROWS_FETCHED_FROM_SERVER_PS
        } else {
            STAT_ROWS_FETCHED_FROM_SERVER_NORMAL
        });
        Ok(Some(data))
    }

    /// php_mysqlnd_stats_read
    pub fn read_stats(&mut self) -> Res<Vec<u8>> {
        self.read_packet_header_and_body("statistics", PacketType::Stats)
    }

    /// php_mysqlnd_prepare_read
    pub fn read_prepare_response(&mut self) -> Res<PrepareResponse> {
        let buf = self.read_packet_header_and_body("prepare", PacketType::PrepareResp)?;
        let mut r = PrepareResponse::default();
        let mut b = Body::new(&buf);
        macro_rules! bail {
            ($l:expr) => {
                if b.bail($l) {
                    php_error_docref(
                        E_WARNING,
                        format!(
                            "PREPARE packet {} bytes shorter than expected",
                            b.shortfall()
                        ),
                    );
                    return Err(());
                }
            };
        }
        bail!(1846);
        let data_size = b.size;
        r.error_code = b.u8();
        bail!(1851);
        if r.error_code == ERROR_MARKER {
            let (no, st, er) = read_error_from_line(b.buf.get(1..).unwrap_or(&[]));
            r.error_info.error_no = no;
            r.error_info.sqlstate = st;
            r.error_info.error = er;
            return Ok(r);
        }
        if data_size != 9 && data_size != 12 && data_size <= 12 {
            php_error(
                E_WARNING,
                format!(
                    "Wrong COM_STMT_PREPARE response size. Received {}",
                    data_size
                ),
            );
            return Err(());
        }
        r.stmt_id = b.u32();
        bail!(1873);
        r.field_count = b.u16();
        bail!(1878);
        r.param_count = b.u16();
        bail!(1882);
        if data_size > 9 {
            b.pos += 1;
            bail!(1887);
            r.warning_count = uint2korr(b.rest());
        }
        bail!(1895);
        Ok(r)
    }

    /// php_mysqlnd_chg_user_read: (PASS?, packet)
    pub fn read_chg_user_response(&mut self) -> (bool, ChgUserResponse) {
        let mut r = ChgUserResponse::default();
        let buf = match self
            .read_packet_header_and_body("change user response", PacketType::ChgUserResp)
        {
            Ok(b) => b,
            Err(()) => return (false, r),
        };
        let mut b = Body::new(&buf);
        macro_rules! bail {
            ($l:expr) => {
                if b.bail($l) {
                    php_error_docref(
                        E_WARNING,
                        format!(
                            "CHANGE_USER packet {} bytes shorter than expected",
                            b.shortfall()
                        ),
                    );
                    return (false, r);
                }
            };
        }
        bail!(1928);
        r.response_code = b.u8();
        if b.size == 1
            && buf.first() == Some(&EODATA_MARKER)
            && self.server_capabilities & CLIENT_SECURE_CONNECTION != 0
        {
            r.server_asked_323_auth = true;
            return (false, r);
        }
        if r.response_code == ERROR_MARKER {
            let (no, st, er) = read_error_from_line(b.buf.get(1..).unwrap_or(&[]));
            r.error_info.error_no = no;
            r.error_info.sqlstate = st;
            r.error_info.error = er;
        }
        bail!(1953);
        if r.response_code == 0xFE && b.size > b.pos {
            r.new_auth_protocol = Some(b.cstr().to_vec());
            let dlen = b.size.saturating_sub(b.pos);
            if dlen > 0 {
                r.new_auth_protocol_data = Some(buf[b.pos..b.pos + dlen].to_vec());
            }
        }
        (true, r)
    }

    /// php_mysqlnd_sha256_pk_request_write (`req` is 1 for sha256_password
    /// and 2 for caching_sha2_password's public key request)
    pub fn write_pk_request(&mut self, req: u8) -> usize {
        self.pfc
            .send(&mut self.vio, &[req], &mut self.stats, &mut self.error_info)
    }

    /// php_mysqlnd_cached_sha2_result_write (password)
    pub fn write_raw(&mut self, data: &[u8]) -> usize {
        self.pfc
            .send(&mut self.vio, data, &mut self.stats, &mut self.error_info)
    }

    /// php_mysqlnd_sha256_pk_request_response_read
    pub fn read_pk_response(&mut self) -> Res<Vec<u8>> {
        let buf = self.read_packet_header_and_body(
            "SHA256_PK_REQUEST_RESPONSE",
            PacketType::Sha256PkRequestResponse,
        )?;
        let mut b = Body::new(&buf);
        macro_rules! bail {
            ($l:expr) => {
                if b.bail($l) {
                    php_error_docref(
                        E_WARNING,
                        format!(
                            "SHA256_PK_REQUEST_RESPONSE packet {} bytes shorter than expected",
                            b.shortfall()
                        ),
                    );
                    return Err(());
                }
            };
        }
        bail!(2040);
        b.pos += 1;
        bail!(2043);
        Ok(buf[b.pos..].to_vec())
    }

    /// php_mysqlnd_cached_sha2_result_read
    pub fn read_cached_sha2_result(&mut self) -> Res<CachedSha2Result> {
        let buf = self.read_packet_header_and_body(
            "PROT_CACHED_SHA2_RESULT_PACKET",
            PacketType::CachedSha2Result,
        )?;
        let mut r = CachedSha2Result::default();
        let mut b = Body::new(&buf);
        macro_rules! bail {
            ($l:expr) => {
                if b.bail($l) {
                    php_error_docref(
                        E_WARNING,
                        format!(
                            "SHA256_PK_REQUEST_RESPONSE packet {} bytes shorter than expected",
                            b.shortfall()
                        ),
                    );
                    return Err(());
                }
            };
        }
        bail!(2120);
        r.response_code = b.u8();
        bail!(2124);
        if r.response_code == ERROR_MARKER {
            let (no, st, er) = read_error_from_line(b.buf.get(1..).unwrap_or(&[]));
            r.error_no = no;
            r.sqlstate = st;
            r.error = er;
            return Ok(r);
        }
        if r.response_code == 0xFE {
            if b.size > b.pos {
                r.new_auth_protocol = Some(b.cstr().to_vec());
                let dlen = b.size.saturating_sub(b.pos);
                if dlen > 0 {
                    r.new_auth_protocol_data = Some(buf[b.pos..b.pos + dlen].to_vec());
                }
            }
            return Ok(r);
        }
        r.response_code = b.u8();
        bail!(2158);
        r.result = b.cur();
        bail!(2161);
        Ok(r)
    }
}

/// set_packet_error (no error list)
pub fn set_packet_error(ei: &mut ErrorInfo, err_no: u32, sqlstate: &str, error: &str) {
    ei.error_no = err_no;
    ei.sqlstate = sqlstate.as_bytes().to_vec();
    ei.error = error.as_bytes().to_vec();
}
