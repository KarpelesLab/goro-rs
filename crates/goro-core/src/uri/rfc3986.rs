//! RFC 3986 URI references: a port of the uriparser library bundled with
//! php-src (ext/uri/uriparser, version 1.0.0) as far as ext/uri uses it:
//! parsing (UriParse.c), syntax normalization (UriNormalize.c),
//! recomposition (UriRecompose.c), reference resolution (UriResolve.c),
//! the component setters (UriSet*.c) and their helpers (UriCommon.c).
//!
//! uriparser's text ranges become `Option<Vec<u8>>` (None: NULL range,
//! Some(empty): an empty range such as `SafeToPointTo`), its path segment
//! list a `Vec` (empty: `pathHead == NULL`).

/// UriUriA
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Uri {
    pub scheme: Option<Vec<u8>>,
    pub user_info: Option<Vec<u8>>,
    /// .hostText: the host as written (without the brackets of an IP literal)
    pub host_text: Option<Vec<u8>>,
    /// .hostData.ip4
    pub ip4: Option<[u8; 4]>,
    /// .hostData.ip6
    pub ip6: Option<[u8; 16]>,
    /// .hostData.ipFuture is set (it is the same range as .hostText)
    pub ip_future: bool,
    pub port_text: Option<Vec<u8>>,
    /// pathHead .. pathTail
    pub path: Vec<Vec<u8>>,
    pub absolute_path: bool,
    pub query: Option<Vec<u8>>,
    pub fragment: Option<Vec<u8>>,
}

/// Errors of the uriparser API (URI_ERROR_*).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Error {
    Syntax,
    AddBaseRelBase,
    SetHostPortSet,
    SetHostUserInfoSet,
    SetUserInfoHostNotSet,
    SetPortHostNotSet,
}

fn is_digit(c: u8) -> bool {
    c.is_ascii_digit()
}
fn is_hexdig(c: u8) -> bool {
    c.is_ascii_hexdigit()
}
fn is_alpha(c: u8) -> bool {
    c.is_ascii_alphabetic()
}
fn is_sub_delim(c: u8) -> bool {
    matches!(
        c,
        b'!' | b'$' | b'&' | b'\'' | b'(' | b')' | b'*' | b'+' | b',' | b';' | b'='
    )
}
fn is_unreserved(c: u8) -> bool {
    is_alpha(c) || is_digit(c) || matches!(c, b'-' | b'.' | b'_' | b'~')
}
fn is_pchar_without_percent(c: u8) -> bool {
    is_unreserved(c) || is_sub_delim(c) || c == b':' || c == b'@'
}
fn is_pchar(c: u8) -> bool {
    is_pchar_without_percent(c) || c == b'%'
}

fn hexdig_to_int(c: u8) -> u8 {
    match c {
        b'0'..=b'9' => c - b'0',
        b'a'..=b'f' => c - b'a' + 10,
        b'A'..=b'F' => c - b'A' + 10,
        _ => 0,
    }
}

fn hex_to_letter(v: u8, upper: bool) -> u8 {
    let d = if upper {
        b"0123456789ABCDEF"
    } else {
        b"0123456789abcdef"
    };
    d[(v & 15) as usize]
}

/// The parser state (UriParserStateA) with text ranges as offsets.
struct Parser<'a> {
    s: &'a [u8],
    scheme: (Option<usize>, Option<usize>),
    user_info: (Option<usize>, Option<usize>),
    host: (Option<usize>, Option<usize>),
    ip4: Option<[u8; 4]>,
    ip6: Option<[u8; 16]>,
    ip_future: (Option<usize>, Option<usize>),
    port: (Option<usize>, Option<usize>),
    path: Vec<(usize, usize)>,
    absolute_path: bool,
    query: Option<(usize, usize)>,
    fragment: Option<(usize, usize)>,
}

type R = Option<usize>;

impl<'a> Parser<'a> {
    fn new(s: &'a [u8]) -> Self {
        Parser {
            s,
            scheme: (None, None),
            user_info: (None, None),
            host: (None, None),
            ip4: None,
            ip6: None,
            ip_future: (None, None),
            port: (None, None),
            path: Vec::new(),
            absolute_path: false,
            query: None,
            fragment: None,
        }
    }

    fn c(&self, i: usize) -> u8 {
        self.s[i]
    }

    fn push_path_segment(&mut self, first: usize, after: usize) {
        self.path.push((first, after));
    }

    fn has_host(&self) -> bool {
        self.host.0.is_some() || self.ip4.is_some() || self.ip6.is_some()
    }

    /// FixEmptyTrailSegment() during parsing
    fn fix_empty_trail_segment(&mut self) {
        if !self.absolute_path
            && !self.has_host()
            && self.path.len() == 1
            && self.path[0].0 == self.path[0].1
        {
            self.path.clear();
        }
    }

    fn parse_authority(&mut self, first: usize, after: usize) -> R {
        if first >= after {
            // "" regname host
            self.host = (Some(first), Some(first));
            return Some(after);
        }
        match self.c(first) {
            b'[' => {
                let after_ip_lit2 = self.parse_ip_lit2(first + 1, after)?;
                self.host.0 = Some(first + 1);
                self.parse_authority_two(after_ip_lit2, after)
            }
            c if is_pchar(c) => {
                self.user_info.0 = Some(first);
                self.parse_own_host_user_info_nz(first, after)
            }
            _ => {
                self.host = (Some(first), Some(first));
                Some(first)
            }
        }
    }

    fn parse_authority_two(&mut self, first: usize, after: usize) -> R {
        if first >= after {
            return Some(after);
        }
        match self.c(first) {
            b':' => {
                let after_port = self.parse_port(first + 1, after);
                self.port = (Some(first + 1), Some(after_port));
                Some(after_port)
            }
            _ => Some(first),
        }
    }

    fn parse_hex_zero(&self, mut first: usize, after: usize) -> usize {
        while first < after && is_hexdig(self.c(first)) {
            first += 1;
        }
        first.min(after)
    }

    fn parse_hier_part(&mut self, first: usize, after: usize) -> R {
        if first >= after {
            return Some(after);
        }
        match self.c(first) {
            c if is_pchar(c) => self.parse_path_rootless(first, after),
            b'/' => self.parse_part_helper_two(first + 1, after),
            _ => Some(first),
        }
    }

    fn parse_ip_fut_loop(&mut self, mut first: usize, after: usize) -> R {
        let original = first;
        while first < after {
            let c = self.c(first);
            if c == b':' || is_sub_delim(c) || is_unreserved(c) {
                first += 1;
            } else {
                break;
            }
        }
        if first == original {
            return None;
        }
        Some(first)
    }

    fn parse_ip_future(&mut self, first: usize, after: usize) -> R {
        if first >= after {
            return None;
        }
        if after - first < 2 {
            return None;
        }
        if !is_hexdig(self.c(first + 1)) {
            return None;
        }
        let after_hex_zero = self.parse_hex_zero(first + 2, after);
        if after_hex_zero >= after {
            return None;
        }
        if self.c(after_hex_zero) != b'.' {
            return None;
        }
        self.host.0 = Some(first);
        self.ip_future.0 = Some(first);
        let after_loop = self.parse_ip_fut_loop(after_hex_zero + 1, after)?;
        self.host.1 = Some(after_loop);
        self.ip_future.1 = Some(after_loop);
        Some(after_loop)
    }

    fn parse_ip_lit2(&mut self, first: usize, after: usize) -> R {
        if first >= after {
            return None;
        }
        match self.c(first) {
            b'v' | b'V' => {
                let after_ip_future = self.parse_ip_future(first, after)?;
                if after_ip_future >= after {
                    return None;
                }
                if self.c(after_ip_future) != b']' {
                    return None;
                }
                Some(after_ip_future + 1)
            }
            c if c == b':' || c == b']' || is_hexdig(c) => {
                self.ip6 = Some([0u8; 16]);
                self.parse_ipv6_address2(first, after)
            }
            _ => None,
        }
    }

    fn parse_ipv6_address2(&mut self, mut first: usize, after: usize) -> R {
        let mut zipper_ever = 0usize;
        let mut quads_done = 0usize;
        let mut digit_count = 0usize;
        let mut digit_history = [0u8; 4];
        let mut ip4_octets_done = 0usize;
        let mut quads_after_zipper = [0u8; 14];
        let mut quads_after_zipper_count = 0usize;
        let mut ip6 = self.ip6.unwrap_or([0u8; 16]);
        loop {
            if first >= after {
                return None;
            }
            if ip4_octets_done > 0 {
                // Eat rest of IPv4 address
                loop {
                    let c = self.c(first);
                    match c {
                        b'0'..=b'9' => {
                            if digit_count == 4 {
                                return None;
                            }
                            digit_history[digit_count] = c - b'0';
                            digit_count += 1;
                        }
                        b'.' => {
                            if ip4_octets_done == 4
                                || digit_count == 0
                                || digit_count == 4
                                || (digit_count > 1 && digit_history[0] == 0)
                                || (digit_count == 3
                                    && 100 * digit_history[0] as u32
                                        + 10 * digit_history[1] as u32
                                        + digit_history[2] as u32
                                        > 255)
                            {
                                return None;
                            }
                            ip6[16 - 4 + ip4_octets_done] =
                                get_octet_value(&digit_history, digit_count);
                            digit_count = 0;
                            ip4_octets_done += 1;
                        }
                        b']' => {
                            if ip4_octets_done != 3
                                || digit_count == 0
                                || digit_count == 4
                                || (digit_count > 1 && digit_history[0] == 0)
                                || (digit_count == 3
                                    && 100 * digit_history[0] as u32
                                        + 10 * digit_history[1] as u32
                                        + digit_history[2] as u32
                                        > 255)
                            {
                                return None;
                            }
                            self.host.1 = Some(first);
                            let off = 16 - 4 - 2 * quads_after_zipper_count;
                            ip6[off..off + 2 * quads_after_zipper_count].copy_from_slice(
                                &quads_after_zipper[..2 * quads_after_zipper_count],
                            );
                            ip6[16 - 4 + 3] = get_octet_value(&digit_history, digit_count);
                            self.ip6 = Some(ip6);
                            return Some(first + 1);
                        }
                        _ => return None,
                    }
                    first += 1;
                    if first >= after {
                        return None;
                    }
                }
            } else {
                // Eat while no dot in sight
                let mut letter_among = false;
                let mut walking = true;
                loop {
                    let c = self.c(first);
                    match c {
                        b'a'..=b'f' | b'A'..=b'F' | b'0'..=b'9' => {
                            if !c.is_ascii_digit() {
                                letter_among = true;
                            }
                            if digit_count == 4 {
                                return None;
                            }
                            digit_history[digit_count] = hexdig_to_int(c);
                            digit_count += 1;
                        }
                        b':' => {
                            let mut set_zipper = false;
                            if digit_count > 0 {
                                if zipper_ever != 0 {
                                    let o = 2 * quads_after_zipper_count;
                                    write_quad(
                                        &digit_history,
                                        digit_count,
                                        &mut quads_after_zipper[o..o + 2],
                                    );
                                    quads_after_zipper_count += 1;
                                } else {
                                    let o = 2 * quads_done;
                                    write_quad(&digit_history, digit_count, &mut ip6[o..o + 2]);
                                }
                                quads_done += 1;
                                digit_count = 0;
                            }
                            letter_among = false;
                            // Too many quads?
                            if quads_done >= 8 - zipper_ever {
                                return None;
                            }
                            // "::"?
                            if after - first < 2 {
                                return None;
                            }
                            if self.c(first + 1) == b':' {
                                let reset_offset = 2 * (quads_done + (digit_count > 0) as usize);
                                first += 1;
                                if zipper_ever != 0 {
                                    return None; // "::.+::"
                                }
                                // Zero everything after zipper
                                for b in ip6.iter_mut().skip(reset_offset) {
                                    *b = 0;
                                }
                                set_zipper = true;
                                // ":::+"?
                                if after - first < 2 {
                                    return None;
                                }
                                if self.c(first + 1) == b':' {
                                    return None;
                                }
                            } else if quads_done == 0 || self.c(first + 1) == b']' {
                                // Single leading or trailing ":"
                                return None;
                            }
                            if set_zipper {
                                zipper_ever = 1;
                            }
                        }
                        b'.' => {
                            if quads_done + zipper_ever > 6
                                || (zipper_ever == 0 && quads_done < 6)
                                || letter_among
                                || digit_count == 0
                                || digit_count == 4
                                || (digit_count > 1 && digit_history[0] == 0)
                                || (digit_count == 3
                                    && 100 * digit_history[0] as u32
                                        + 10 * digit_history[1] as u32
                                        + digit_history[2] as u32
                                        > 255)
                            {
                                return None;
                            }
                            ip6[16 - 4] = get_octet_value(&digit_history, digit_count);
                            digit_count = 0;
                            ip4_octets_done = 1;
                            walking = false;
                        }
                        b']' => {
                            // Too little quads?
                            if zipper_ever == 0 && !(quads_done == 7 && digit_count > 0) {
                                return None;
                            }
                            if digit_count > 0 {
                                if zipper_ever != 0 {
                                    if quads_done >= 7 {
                                        return None;
                                    }
                                    let o = 2 * quads_after_zipper_count;
                                    write_quad(
                                        &digit_history,
                                        digit_count,
                                        &mut quads_after_zipper[o..o + 2],
                                    );
                                    quads_after_zipper_count += 1;
                                } else {
                                    let o = 2 * quads_done;
                                    write_quad(&digit_history, digit_count, &mut ip6[o..o + 2]);
                                }
                            }
                            // Copy missing quads to the end
                            let off = 16 - 2 * quads_after_zipper_count;
                            ip6[off..].copy_from_slice(
                                &quads_after_zipper[..2 * quads_after_zipper_count],
                            );
                            self.host.1 = Some(first);
                            self.ip6 = Some(ip6);
                            return Some(first + 1);
                        }
                        _ => return None,
                    }
                    first += 1;
                    if first >= after {
                        return None;
                    }
                    if !walking {
                        break;
                    }
                }
            }
        }
    }

    fn parse_must_be_segment_nz_nc(&mut self, mut first: usize, after: usize) -> R {
        loop {
            if first >= after {
                let sf = self.scheme.0.unwrap_or(first);
                self.push_path_segment(sf, first);
                self.scheme.0 = None;
                return Some(after);
            }
            let c = self.c(first);
            match c {
                b'%' => {
                    first = self.parse_pct_encoded(first, after)?;
                }
                c if c == b'@' || is_sub_delim(c) || is_unreserved(c) => {
                    first += 1;
                }
                b'/' => {
                    let sf = self.scheme.0.unwrap_or(first);
                    self.push_path_segment(sf, first);
                    self.scheme.0 = None;
                    let after_segment = self.parse_segment(first + 1, after)?;
                    self.push_path_segment(first + 1, after_segment);
                    let after_zero_more = self.parse_zero_more_slash_segs(after_segment, after)?;
                    return self.parse_uri_tail(after_zero_more, after);
                }
                _ => {
                    let sf = self.scheme.0.unwrap_or(first);
                    self.push_path_segment(sf, first);
                    self.scheme.0 = None;
                    return self.parse_uri_tail(first, after);
                }
            }
        }
    }

    fn parse_own_host(&mut self, first: usize, after: usize) -> R {
        if first >= after {
            self.host.1 = Some(after);
            return Some(after);
        }
        match self.c(first) {
            b'[' => {
                let after_ip_lit2 = self.parse_ip_lit2(first + 1, after)?;
                self.host.0 = Some(first + 1);
                self.parse_authority_two(after_ip_lit2, after)
            }
            _ => self.parse_own_host2(first, after),
        }
    }

    fn detect_ip4(&mut self) {
        let (Some(f), Some(a)) = self.host else {
            self.ip4 = None;
            return;
        };
        self.ip4 = if f <= a {
            parse_ip_four_address(&self.s[f..a])
        } else {
            None
        };
    }

    fn on_exit_own_host2(&mut self, first: usize) {
        self.host.1 = Some(first);
        self.detect_ip4();
    }

    fn parse_own_host2(&mut self, mut first: usize, after: usize) -> R {
        loop {
            if first >= after {
                self.on_exit_own_host2(first);
                return Some(after);
            }
            let c = self.c(first);
            if c == b'%' || is_sub_delim(c) || is_unreserved(c) {
                first = self.parse_pct_sub_unres(first, after)?;
            } else {
                self.on_exit_own_host2(first);
                return self.parse_authority_two(first, after);
            }
        }
    }

    fn on_exit_own_host_user_info(&mut self, first: usize) {
        self.host.0 = self.user_info.0;
        self.user_info.0 = None;
        self.host.1 = Some(first);
        self.detect_ip4();
    }

    fn parse_own_host_user_info_nz(&mut self, mut first: usize, after: usize) -> R {
        let original = first;
        while first < after {
            let c = self.c(first);
            if c == b'%' || is_sub_delim(c) || is_unreserved(c) {
                first = self.parse_pct_sub_unres(first, after)?;
            } else {
                break;
            }
        }
        if first < after {
            match self.c(first) {
                b':' => {
                    self.host.1 = Some(first);
                    self.port.0 = Some(first + 1);
                    return self.parse_own_port_user_info(first + 1, after);
                }
                b'@' => {
                    self.user_info.1 = Some(first);
                    self.host.0 = Some(first + 1);
                    return self.parse_own_host(first + 1, after);
                }
                _ => {}
            }
        }
        if first == original {
            return None;
        }
        self.on_exit_own_host_user_info(first);
        Some(first)
    }

    fn on_exit_own_port_user_info(&mut self, first: usize) {
        self.host.0 = self.user_info.0;
        self.user_info.0 = None;
        self.port.1 = Some(first);
        self.detect_ip4();
    }

    fn parse_own_port_user_info(&mut self, mut first: usize, after: usize) -> R {
        loop {
            if first >= after {
                self.on_exit_own_port_user_info(first);
                return Some(after);
            }
            let c = self.c(first);
            match c {
                c if is_sub_delim(c)
                    || matches!(c, b'-' | b'.' | b'_' | b'~' | b':')
                    || is_alpha(c) =>
                {
                    self.host.1 = None;
                    self.port.0 = None;
                    return self.parse_own_user_info(first + 1, after);
                }
                b'0'..=b'9' => {
                    first += 1;
                }
                b'%' => {
                    self.port.0 = None;
                    let after_pct = self.parse_pct_encoded(first, after)?;
                    return self.parse_own_user_info(after_pct, after);
                }
                b'@' => {
                    self.host.1 = None;
                    self.port.0 = None;
                    self.user_info.1 = Some(first);
                    self.host.0 = Some(first + 1);
                    return self.parse_own_host(first + 1, after);
                }
                _ => {
                    self.on_exit_own_port_user_info(first);
                    return Some(first);
                }
            }
        }
    }

    fn parse_own_user_info(&mut self, mut first: usize, after: usize) -> R {
        loop {
            if first >= after {
                return None;
            }
            let c = self.c(first);
            match c {
                c if c == b'%' || is_sub_delim(c) || is_unreserved(c) => {
                    first = self.parse_pct_sub_unres(first, after)?;
                }
                b':' => first += 1,
                b'@' => {
                    self.user_info.1 = Some(first);
                    self.host.0 = Some(first + 1);
                    return self.parse_own_host(first + 1, after);
                }
                _ => return None,
            }
        }
    }

    fn parse_part_helper_two(&mut self, first: usize, after: usize) -> R {
        if first >= after {
            self.absolute_path = true;
            return Some(after);
        }
        match self.c(first) {
            b'/' => {
                let after_authority = self.parse_authority(first + 1, after)?;
                let after_path_abs_empty = self.parse_path_abs_empty(after_authority, after);
                self.fix_empty_trail_segment();
                after_path_abs_empty
            }
            _ => {
                self.absolute_path = true;
                self.parse_path_abs_no_lead_slash(first, after)
            }
        }
    }

    fn parse_path_abs_empty(&mut self, mut first: usize, after: usize) -> R {
        loop {
            if first >= after {
                return Some(after);
            }
            match self.c(first) {
                b'/' => {
                    let after_segment = self.parse_segment(first + 1, after)?;
                    self.push_path_segment(first + 1, after_segment);
                    first = after_segment;
                }
                _ => return Some(first),
            }
        }
    }

    fn parse_path_abs_no_lead_slash(&mut self, first: usize, after: usize) -> R {
        if first >= after {
            return Some(after);
        }
        if is_pchar(self.c(first)) {
            let after_segment_nz = self.parse_segment_nz(first, after)?;
            self.push_path_segment(first, after_segment_nz);
            return self.parse_zero_more_slash_segs(after_segment_nz, after);
        }
        Some(first)
    }

    fn parse_path_rootless(&mut self, first: usize, after: usize) -> R {
        let after_segment_nz = self.parse_segment_nz(first, after)?;
        self.push_path_segment(first, after_segment_nz);
        self.parse_zero_more_slash_segs(after_segment_nz, after)
    }

    fn parse_pchar(&mut self, first: usize, after: usize) -> R {
        if first >= after {
            return None;
        }
        match self.c(first) {
            b'%' => self.parse_pct_encoded(first, after),
            c if is_pchar_without_percent(c) => Some(first + 1),
            _ => None,
        }
    }

    fn parse_pct_encoded(&mut self, first: usize, after: usize) -> R {
        if first >= after || after - first < 2 {
            return None;
        }
        if !is_hexdig(self.c(first + 1)) {
            return None;
        }
        if after - first < 3 {
            return None;
        }
        if !is_hexdig(self.c(first + 2)) {
            return None;
        }
        Some(first + 3)
    }

    fn parse_pct_sub_unres(&mut self, first: usize, after: usize) -> R {
        if first >= after {
            return None;
        }
        match self.c(first) {
            b'%' => self.parse_pct_encoded(first, after),
            c if is_sub_delim(c) || is_unreserved(c) => Some(first + 1),
            _ => None,
        }
    }

    fn parse_port(&self, mut first: usize, after: usize) -> usize {
        while first < after && is_digit(self.c(first)) {
            first += 1;
        }
        first.min(after)
    }

    fn parse_query_frag(&mut self, mut first: usize, after: usize) -> R {
        loop {
            if first >= after {
                return Some(after);
            }
            match self.c(first) {
                c if is_pchar(c) => first = self.parse_pchar(first, after)?,
                b'/' | b'?' => first += 1,
                _ => return Some(first),
            }
        }
    }

    fn parse_segment(&mut self, mut first: usize, after: usize) -> R {
        loop {
            if first >= after {
                return Some(after);
            }
            if is_pchar(self.c(first)) {
                first = self.parse_pchar(first, after)?;
            } else {
                return Some(first);
            }
        }
    }

    fn parse_segment_nz(&mut self, first: usize, after: usize) -> R {
        let after_pchar = self.parse_pchar(first, after)?;
        self.parse_segment(after_pchar, after)
    }

    fn on_exit_segment_nz_nc_or_scheme2(&mut self, first: usize) {
        let sf = self.scheme.0.unwrap_or(first);
        self.push_path_segment(sf, first);
        self.scheme.0 = None;
    }

    fn parse_segment_nz_nc_or_scheme2(&mut self, mut first: usize, after: usize) -> R {
        loop {
            if first >= after {
                self.on_exit_segment_nz_nc_or_scheme2(first);
                return Some(after);
            }
            let c = self.c(first);
            match c {
                b'.' | b'+' | b'-' => first += 1,
                c if is_alpha(c) || is_digit(c) => first += 1,
                b'%' => {
                    let after_pct = self.parse_pct_encoded(first, after)?;
                    return self.parse_must_be_segment_nz_nc(after_pct, after);
                }
                b'!' | b'$' | b'&' | b'(' | b')' | b'*' | b',' | b';' | b'@' | b'_' | b'~'
                | b'=' | b'\'' => {
                    return self.parse_must_be_segment_nz_nc(first + 1, after);
                }
                b'/' => {
                    let after_segment = self.parse_segment(first + 1, after)?;
                    let sf = self.scheme.0.unwrap_or(first);
                    self.push_path_segment(sf, first);
                    self.scheme.0 = None;
                    self.push_path_segment(first + 1, after_segment);
                    let after_zero_more = self.parse_zero_more_slash_segs(after_segment, after)?;
                    return self.parse_uri_tail(after_zero_more, after);
                }
                b':' => {
                    let after_hier_part = self.parse_hier_part(first + 1, after);
                    self.scheme.1 = Some(first);
                    let after_hier_part = after_hier_part?;
                    return self.parse_uri_tail(after_hier_part, after);
                }
                _ => {
                    self.on_exit_segment_nz_nc_or_scheme2(first);
                    return self.parse_uri_tail(first, after);
                }
            }
        }
    }

    fn parse_uri_reference(&mut self, first: usize, after: usize) -> R {
        if first >= after {
            return Some(after);
        }
        let c = self.c(first);
        match c {
            c if is_alpha(c) => {
                self.scheme.0 = Some(first);
                self.parse_segment_nz_nc_or_scheme2(first + 1, after)
            }
            c if is_digit(c)
                || is_sub_delim(c)
                || matches!(c, b'.' | b'_' | b'~' | b'-' | b'@') =>
            {
                self.scheme.0 = Some(first);
                self.parse_must_be_segment_nz_nc(first + 1, after)
            }
            b'%' => {
                let after_pct = self.parse_pct_encoded(first, after)?;
                self.scheme.0 = Some(first);
                self.parse_must_be_segment_nz_nc(after_pct, after)
            }
            b'/' => {
                let after_part_helper_two = self.parse_part_helper_two(first + 1, after)?;
                self.parse_uri_tail(after_part_helper_two, after)
            }
            _ => self.parse_uri_tail(first, after),
        }
    }

    fn parse_uri_tail(&mut self, first: usize, after: usize) -> R {
        if first >= after {
            return Some(after);
        }
        match self.c(first) {
            b'#' => {
                let after_qf = self.parse_query_frag(first + 1, after)?;
                self.fragment = Some((first + 1, after_qf));
                Some(after_qf)
            }
            b'?' => {
                let after_qf = self.parse_query_frag(first + 1, after)?;
                self.query = Some((first + 1, after_qf));
                self.parse_uri_tail_two(after_qf, after)
            }
            _ => Some(first),
        }
    }

    fn parse_uri_tail_two(&mut self, first: usize, after: usize) -> R {
        if first >= after {
            return Some(after);
        }
        match self.c(first) {
            b'#' => {
                let after_qf = self.parse_query_frag(first + 1, after)?;
                self.fragment = Some((first + 1, after_qf));
                Some(after_qf)
            }
            _ => Some(first),
        }
    }

    fn parse_zero_more_slash_segs(&mut self, mut first: usize, after: usize) -> R {
        loop {
            if first >= after {
                return Some(after);
            }
            match self.c(first) {
                b'/' => {
                    let after_segment = self.parse_segment(first + 1, after)?;
                    self.push_path_segment(first + 1, after_segment);
                    first = after_segment;
                }
                _ => return Some(first),
            }
        }
    }

    fn range(&self, r: (Option<usize>, Option<usize>)) -> Option<Vec<u8>> {
        match r {
            (Some(f), Some(a)) if f <= a => Some(self.s[f..a].to_vec()),
            (Some(_), Some(_)) => Some(Vec::new()),
            _ => None,
        }
    }

    fn into_uri(self) -> Uri {
        let ip_future = self.ip_future.0.is_some();
        Uri {
            scheme: self.range(self.scheme),
            user_info: self.range(self.user_info),
            host_text: self.range(self.host),
            ip4: self.ip4,
            ip6: self.ip6,
            ip_future,
            port_text: self.range(self.port),
            path: self
                .path
                .iter()
                .map(|&(f, a)| self.s[f..a.max(f)].to_vec())
                .collect(),
            absolute_path: self.absolute_path,
            query: self.query.map(|(f, a)| self.s[f..a].to_vec()),
            fragment: self.fragment.map(|(f, a)| self.s[f..a].to_vec()),
        }
    }
}

fn get_octet_value(digits: &[u8; 4], count: usize) -> u8 {
    match count {
        1 => digits[0],
        2 => 10 * digits[0] + digits[1],
        _ => (100u32 * digits[0] as u32 + 10 * digits[1] as u32 + digits[2] as u32) as u8,
    }
}

fn write_quad(hex: &[u8; 4], count: usize, out: &mut [u8]) {
    match count {
        1 => {
            out[0] = 0;
            out[1] = hex[0];
        }
        2 => {
            out[0] = 0;
            out[1] = 16 * hex[0] + hex[1];
        }
        3 => {
            out[0] = hex[0];
            out[1] = 16 * hex[1] + hex[2];
        }
        4 => {
            out[0] = 16 * hex[0] + hex[1];
            out[1] = 16 * hex[2] + hex[3];
        }
        _ => {}
    }
}

/// ParseIpFourAddress(): a dotted-decimal IPv4 address (dec-octets).
pub fn parse_ip_four_address(s: &[u8]) -> Option<[u8; 4]> {
    if s.is_empty() {
        return None;
    }
    let mut out = [0u8; 4];
    let mut pos = 0usize;
    for (n, o) in out.iter_mut().enumerate() {
        let (v, after) = parse_dec_octet(s, pos)?;
        *o = v;
        if n < 3 {
            if after >= s.len() || s[after] != b'.' {
                return None;
            }
            pos = after + 1;
        } else if after != s.len() {
            return None;
        }
    }
    Some(out)
}

/// ParseDecOctet() and its helpers: the longest dec-octet at `i`.
fn parse_dec_octet(s: &[u8], i: usize) -> Option<(u8, usize)> {
    let c = *s.get(i)?;
    let d = |j: usize| s.get(j).copied().filter(|c| c.is_ascii_digit());
    match c {
        b'0' => Some((0, i + 1)),
        b'1' => match d(i + 1) {
            Some(c2) => match d(i + 2) {
                Some(c3) => Some((100 + (c2 - b'0') * 10 + (c3 - b'0'), i + 3)),
                None => Some((10 + (c2 - b'0'), i + 2)),
            },
            None => Some((1, i + 1)),
        },
        b'2' => match s.get(i + 1).copied() {
            Some(c2 @ b'0'..=b'4') => match d(i + 2) {
                Some(c3) => Some((200 + (c2 - b'0') * 10 + (c3 - b'0'), i + 3)),
                None => Some((20 + (c2 - b'0'), i + 2)),
            },
            Some(b'5') => match s.get(i + 2).copied() {
                Some(c3 @ b'0'..=b'5') => Some((250 + (c3 - b'0'), i + 3)),
                _ => Some((25, i + 2)),
            },
            Some(c2 @ b'6'..=b'9') => Some((20 + (c2 - b'0'), i + 2)),
            _ => Some((2, i + 1)),
        },
        b'3'..=b'9' => match d(i + 1) {
            Some(c2) => Some(((c - b'0') * 10 + (c2 - b'0'), i + 2)),
            None => Some((c - b'0', i + 1)),
        },
        _ => None,
    }
}

/// uriParseSingleUriExMm(): parse a complete URI reference.
pub fn parse(s: &[u8]) -> Result<Uri, Error> {
    let mut p = Parser::new(s);
    match p.parse_uri_reference(0, s.len()) {
        Some(after) if after == s.len() => Ok(p.into_uri()),
        _ => Err(Error::Syntax),
    }
}

impl Uri {
    /// HasHost()
    pub fn has_host(&self) -> bool {
        self.host_text.is_some() || self.ip4.is_some() || self.ip6.is_some()
    }

    // ------------------------------------------------------------------
    // UriCommon.c

    /// RemoveDotSegmentsEx()
    fn remove_dot_segments_ex(&mut self, relative: bool) {
        if self.path.is_empty() {
            return;
        }
        // The segments still in the list, and the index of the walker.
        let mut segs: Vec<Vec<u8>> = std::mem::take(&mut self.path);
        let mut i = 0usize;
        while i < segs.len() {
            let seg = segs[i].clone();
            if seg.as_slice() == b"." {
                let mut remove_segment = true;
                if i == 0 && segs.len() > 1 {
                    // Detect case "/.//" (with or without scheme)
                    if segs[1].is_empty() && !self.has_host() {
                        remove_segment = false;
                    } else if relative && segs[1].contains(&b':') {
                        // Detect case "./withcolon:"
                        remove_segment = false;
                    }
                }
                if remove_segment {
                    if i + 1 < segs.len() {
                        segs.remove(i);
                    } else if i == 0 {
                        // Last and first
                        if self.has_host() {
                            segs[i] = Vec::new();
                            i += 1;
                        } else {
                            segs.clear();
                        }
                    } else {
                        // Last but not first: empty segment for the trailing slash
                        segs[i] = Vec::new();
                        i += 1;
                    }
                    continue;
                }
            } else if seg.as_slice() == b".." {
                let has_prev = i > 0;
                let remove_segment = !(relative && (!has_prev || segs[i - 1].as_slice() == b".."));
                if remove_segment {
                    if has_prev {
                        let prev = i - 1;
                        if prev > 0 {
                            // Not even prev is the first one
                            if i + 1 < segs.len() {
                                segs.drain(prev..=i);
                                i = prev;
                            } else {
                                // Last segment: "" for the trailing slash
                                segs.drain(prev..=i);
                                segs.push(Vec::new());
                                i = segs.len();
                            }
                        } else {
                            // Prev is the first segment
                            if i + 1 < segs.len() {
                                segs.drain(0..=i);
                                i = 0;
                            } else {
                                segs.clear();
                                segs.push(Vec::new());
                                i = 1;
                            }
                        }
                    } else {
                        // First segment
                        if i + 1 < segs.len() {
                            segs.remove(0);
                            i = 0;
                        } else if self.absolute_path {
                            segs.clear();
                            i = 0;
                        } else {
                            segs[0] = Vec::new();
                            i = 1;
                        }
                    }
                    continue;
                }
            }
            i += 1;
        }
        self.path = segs;
    }

    /// RemoveDotSegmentsAbsolute()
    fn remove_dot_segments_absolute(&mut self) {
        self.remove_dot_segments_ex(false);
    }

    /// FixAmbiguity()
    fn fix_ambiguity(&mut self) {
        let case1 = self.absolute_path && self.path.first().is_some_and(|s| s.is_empty());
        let case2 = !self.absolute_path
            && self.path.len() >= 2
            && self.path[0].is_empty()
            && self.path[1].is_empty();
        if case1 || case2 {
            self.path.insert(0, b".".to_vec());
        }
    }

    /// FixPathNoScheme()
    fn fix_path_no_scheme(&mut self) {
        if self.absolute_path || self.path.is_empty() || self.scheme.is_some() || self.has_host() {
            return;
        }
        if self.path[0].contains(&b':') {
            self.path.insert(0, b".".to_vec());
        }
    }

    /// EnsureThatPathIsNotMistakenForHost()
    fn ensure_path_not_mistaken_for_host(&mut self) {
        if self.has_host() || !self.absolute_path || self.path.len() < 2 || !self.path[0].is_empty()
        {
            return;
        }
        self.path.insert(0, b".".to_vec());
    }

    /// FixEmptyTrailSegment()
    fn fix_empty_trail_segment(&mut self) {
        if !self.absolute_path
            && !self.has_host()
            && self.path.len() == 1
            && self.path[0].is_empty()
        {
            self.path.clear();
        }
    }

    // ------------------------------------------------------------------
    // UriNormalize.c

    /// uriNormalizeSyntaxExMmA(uri, (unsigned int)-1) on an owner URI.
    pub fn normalize_syntax(&mut self) {
        // Scheme
        if let Some(s) = &mut self.scheme {
            s.make_ascii_lowercase();
        }
        // Host
        if self.ip_future {
            if let Some(h) = &mut self.host_text {
                h.make_ascii_lowercase();
            }
        } else if self.host_text.is_some() && self.ip4.is_none() {
            if let Some(h) = &mut self.host_text {
                fix_percent_encoding(h);
                lowercase_except_percent_encoding(h);
            }
        }
        // Port: drop leading zeros
        if let Some(p) = &mut self.port_text {
            if !p.is_empty() {
                let mut i = 0;
                while i < p.len() && p[i] == b'0' {
                    i += 1;
                }
                if i == p.len() {
                    i -= 1;
                }
                p.drain(..i);
            }
        }
        // User info
        if let Some(u) = &mut self.user_info {
            fix_percent_encoding(u);
        }
        // Path
        let relative = self.scheme.is_none() && !self.absolute_path;
        for seg in &mut self.path {
            fix_percent_encoding(seg);
        }
        self.remove_dot_segments_ex(relative);
        self.fix_empty_trail_segment();
        // Query, fragment
        if let Some(q) = &mut self.query {
            fix_percent_encoding(q);
        }
        if let Some(f) = &mut self.fragment {
            fix_percent_encoding(f);
        }
    }

    // ------------------------------------------------------------------
    // UriRecompose.c

    /// uriToStringA()
    pub fn to_string_bytes(&self) -> Vec<u8> {
        let mut out = Vec::new();
        if let Some(s) = &self.scheme {
            out.extend_from_slice(s);
            out.push(b':');
        }
        if self.has_host() {
            out.extend_from_slice(b"//");
            if let Some(u) = &self.user_info {
                out.extend_from_slice(u);
                out.push(b'@');
            }
            if let Some(ip4) = &self.ip4 {
                for (i, v) in ip4.iter().enumerate() {
                    out.extend_from_slice(v.to_string().as_bytes());
                    if i < 3 {
                        out.push(b'.');
                    }
                }
            } else if let Some(ip6) = &self.ip6 {
                out.push(b'[');
                for (i, v) in ip6.iter().enumerate() {
                    out.push(hex_to_letter(v / 16, false));
                    out.push(hex_to_letter(v % 16, false));
                    if (i & 1) == 1 && i < 15 {
                        out.push(b':');
                    }
                }
                out.push(b']');
            } else if self.ip_future {
                out.push(b'[');
                if let Some(h) = &self.host_text {
                    out.extend_from_slice(h);
                }
                out.push(b']');
            } else if let Some(h) = &self.host_text {
                out.extend_from_slice(h);
            }
            if let Some(p) = &self.port_text {
                out.push(b':');
                out.extend_from_slice(p);
            }
        }
        if self.absolute_path || (!self.path.is_empty() && self.has_host()) {
            out.push(b'/');
        }
        for (i, seg) in self.path.iter().enumerate() {
            out.extend_from_slice(seg);
            if i + 1 < self.path.len() {
                out.push(b'/');
            }
        }
        if let Some(q) = &self.query {
            out.push(b'?');
            out.extend_from_slice(q);
        }
        if let Some(f) = &self.fragment {
            out.push(b'#');
            out.extend_from_slice(f);
        }
        out
    }

    // ------------------------------------------------------------------
    // UriResolve.c

    /// CopyAuthority()
    fn copy_authority_from(&mut self, src: &Uri) {
        self.user_info = src.user_info.clone();
        self.host_text = src.host_text.clone();
        self.ip4 = src.ip4;
        self.ip6 = if src.ip4.is_some() { None } else { src.ip6 };
        self.ip_future = src.ip4.is_none() && src.ip6.is_none() && src.ip_future;
        self.port_text = src.port_text.clone();
    }

    /// CopyPath()
    fn copy_path_from(&mut self, src: &Uri) {
        self.path = src.path.clone();
        self.absolute_path = src.absolute_path;
    }

    /// MergePath()
    fn merge_path(&mut self, rel: &Uri) {
        if rel.path.is_empty() {
            return;
        }
        if self.path.is_empty() {
            self.path.push(Vec::new());
        }
        let last = self.path.len() - 1;
        self.path[last] = rel.path[0].clone();
        self.path.extend(rel.path[1..].iter().cloned());
    }

    /// ResolveAbsolutePathFlag()
    fn resolve_absolute_path_flag(&mut self) {
        if self.has_host() && self.absolute_path {
            if self.path.is_empty() {
                self.path.push(Vec::new());
            }
            self.absolute_path = false;
        }
    }

    /// uriAddBaseUriExMmA(dest, rel, base, URI_RESOLVE_STRICTLY)
    pub fn add_base(rel: &Uri, base: &Uri) -> Result<Uri, Error> {
        if base.scheme.is_none() {
            return Err(Error::AddBaseRelBase);
        }
        let mut dest = Uri::default();
        if rel.scheme.is_some() {
            dest.scheme = rel.scheme.clone();
            dest.copy_authority_from(rel);
            dest.copy_path_from(rel);
            dest.remove_dot_segments_absolute();
            dest.query = rel.query.clone();
        } else {
            if rel.has_host() {
                dest.copy_authority_from(rel);
                dest.copy_path_from(rel);
                dest.remove_dot_segments_absolute();
                dest.query = rel.query.clone();
            } else {
                dest.copy_authority_from(base);
                if rel.path.is_empty() && !rel.absolute_path {
                    dest.copy_path_from(base);
                    if rel.query.is_some() {
                        dest.query = rel.query.clone();
                    } else {
                        dest.query = base.query.clone();
                    }
                } else {
                    if rel.absolute_path {
                        dest.copy_path_from(rel);
                        dest.resolve_absolute_path_flag();
                        dest.remove_dot_segments_absolute();
                    } else {
                        dest.copy_path_from(base);
                        dest.merge_path(rel);
                        dest.remove_dot_segments_absolute();
                        dest.fix_ambiguity();
                    }
                    dest.query = rel.query.clone();
                }
                dest.fix_empty_trail_segment();
            }
            dest.scheme = base.scheme.clone();
        }
        dest.fragment = rel.fragment.clone();
        Ok(dest)
    }

    // ------------------------------------------------------------------
    // UriSet*.c

    /// uriSetSchemeMmA()
    pub fn set_scheme(&mut self, v: Option<&[u8]>) -> Result<(), Error> {
        if let Some(v) = v {
            if !is_well_formed_scheme(v) {
                return Err(Error::Syntax);
            }
        }
        self.scheme = None;
        match v {
            None => {
                self.fix_path_no_scheme();
            }
            Some(v) => self.scheme = Some(v.to_vec()),
        }
        Ok(())
    }

    /// uriSetUserInfoMmA()
    pub fn set_user_info(&mut self, v: Option<&[u8]>) -> Result<(), Error> {
        if v.is_some() && !self.has_host() {
            return Err(Error::SetUserInfoHostNotSet);
        }
        if let Some(v) = v {
            if !is_well_formed_user_info(v) {
                return Err(Error::Syntax);
            }
        }
        self.user_info = v.map(|v| v.to_vec());
        Ok(())
    }

    /// uriSetPortTextMmA()
    pub fn set_port_text(&mut self, v: Option<&[u8]>) -> Result<(), Error> {
        if v.is_some() && !self.has_host() {
            return Err(Error::SetPortHostNotSet);
        }
        if let Some(v) = v {
            if !v.iter().all(|c| c.is_ascii_digit()) {
                return Err(Error::Syntax);
            }
        }
        self.port_text = v.map(|v| v.to_vec());
        Ok(())
    }

    /// uriSetHostAutoMmA()
    pub fn set_host_auto(&mut self, v: Option<&[u8]>) -> Result<(), Error> {
        let Some(s) = v else {
            return self.internal_set_host(HostType::RegName, None);
        };
        if s.is_empty() {
            return self.internal_set_host(HostType::RegName, Some(s));
        }
        if s[0] == b'[' {
            if s.len() < 2 || s[s.len() - 1] != b']' {
                return Err(Error::Syntax);
            }
            let inner = &s[1..s.len() - 1];
            if inner.is_empty() {
                return Err(Error::Syntax);
            }
            let t = match inner[0] {
                b'v' | b'V' => HostType::IpFuture,
                _ => HostType::Ip6,
            };
            return self.internal_set_host(t, Some(inner));
        }
        if parse_ip_four_address(s).is_some() {
            return self.internal_set_host(HostType::Ip4, Some(s));
        }
        self.internal_set_host(HostType::RegName, Some(s))
    }

    /// InternalSetHostMm()
    fn internal_set_host(&mut self, t: HostType, v: Option<&[u8]>) -> Result<(), Error> {
        if v.is_none() {
            if self.user_info.is_some() {
                return Err(Error::SetHostUserInfoSet);
            } else if self.port_text.is_some() {
                return Err(Error::SetHostPortSet);
            }
        }
        let mut ip6 = None;
        if let Some(s) = v {
            match t {
                HostType::Ip4 => {
                    if parse_ip_four_address(s).is_none() {
                        return Err(Error::Syntax);
                    }
                }
                HostType::Ip6 => {
                    ip6 = Some(parse_ip_six_address(s).ok_or(Error::Syntax)?);
                }
                HostType::IpFuture => {
                    if !is_well_formed_host_ip_future(s) {
                        return Err(Error::Syntax);
                    }
                }
                HostType::RegName => {
                    if !is_well_formed_reg_name(s) {
                        return Err(Error::Syntax);
                    }
                }
            }
        }
        let had_host_before = self.has_host();
        self.host_text = None;
        self.ip_future = false;
        self.ip4 = None;
        self.ip6 = None;
        let Some(s) = v else {
            if had_host_before {
                if !self.path.is_empty() {
                    self.absolute_path = true;
                }
                self.ensure_path_not_mistaken_for_host();
            }
            return Ok(());
        };
        self.host_text = Some(s.to_vec());
        self.absolute_path = false;
        match t {
            HostType::Ip4 => self.ip4 = parse_ip_four_address(s),
            HostType::Ip6 => self.ip6 = ip6,
            HostType::IpFuture => self.ip_future = true,
            HostType::RegName => {}
        }
        Ok(())
    }

    /// uriSetPathMmA()
    pub fn set_path(&mut self, v: Option<&[u8]>) -> Result<(), Error> {
        if let Some(s) = v {
            if !is_well_formed_path(s, self.has_host()) {
                return Err(Error::Syntax);
            }
        }
        self.path.clear();
        self.absolute_path = false;
        let Some(mut s) = v else {
            return Ok(());
        };
        // InternalSetPath()
        if self.has_host() {
            s = &s[1..];
        } else if s.is_empty() {
            return Ok(());
        }
        let mut candidate = b"///".to_vec();
        candidate.extend_from_slice(s);
        let temp = parse(&candidate)?;
        self.path = temp.path;
        self.absolute_path = false;
        // TransformEmptyLeadPathSegments()
        if !self.has_host() && self.path.first().is_some_and(|s| s.is_empty()) {
            self.path.remove(0);
            self.absolute_path = true;
        }
        self.fix_path_no_scheme();
        self.ensure_path_not_mistaken_for_host();
        Ok(())
    }

    /// uriSetQueryMmA()
    pub fn set_query(&mut self, v: Option<&[u8]>) -> Result<(), Error> {
        if let Some(s) = v {
            if !is_well_formed_query_or_fragment(s) {
                return Err(Error::Syntax);
            }
        }
        self.query = v.map(|v| v.to_vec());
        Ok(())
    }

    /// uriSetFragmentMmA()
    pub fn set_fragment(&mut self, v: Option<&[u8]>) -> Result<(), Error> {
        if let Some(s) = v {
            if !is_well_formed_query_or_fragment(s) {
                return Err(Error::Syntax);
            }
        }
        self.fragment = v.map(|v| v.to_vec());
        Ok(())
    }
}

#[derive(Clone, Copy)]
enum HostType {
    Ip4,
    Ip6,
    IpFuture,
    RegName,
}

/// IsWellFormedScheme()
fn is_well_formed_scheme(s: &[u8]) -> bool {
    match s.first() {
        Some(c) if is_alpha(*c) => {}
        _ => return false,
    }
    s[1..]
        .iter()
        .all(|&c| is_alpha(c) || is_digit(c) || matches!(c, b'+' | b'-' | b'.'))
}

/// A pct-encoded triplet at `s[i]` (which is '%').
fn pct_ok(s: &[u8], i: usize) -> bool {
    s.len() - i >= 3 && is_hexdig(s[i + 1]) && is_hexdig(s[i + 2])
}

fn well_formed_with(s: &[u8], ok: impl Fn(u8) -> bool) -> bool {
    let mut i = 0;
    while i < s.len() {
        let c = s[i];
        if c == b'%' {
            if !pct_ok(s, i) {
                return false;
            }
            i += 3;
            continue;
        }
        if !ok(c) {
            return false;
        }
        i += 1;
    }
    true
}

/// IsWellFormedUserInfo()
fn is_well_formed_user_info(s: &[u8]) -> bool {
    well_formed_with(s, |c| is_unreserved(c) || is_sub_delim(c) || c == b':')
}

/// IsWellFormedHostRegName()
fn is_well_formed_reg_name(s: &[u8]) -> bool {
    well_formed_with(s, |c| is_unreserved(c) || is_sub_delim(c))
}

/// IsWellFormedPath()
fn is_well_formed_path(s: &[u8], has_host: bool) -> bool {
    if has_host && (s.is_empty() || s[0] != b'/') {
        return false;
    }
    well_formed_with(s, |c| is_pchar_without_percent(c) || c == b'/')
}

/// IsWellFormedQuery() / IsWellFormedFragment()
fn is_well_formed_query_or_fragment(s: &[u8]) -> bool {
    well_formed_with(s, |c| is_pchar_without_percent(c) || c == b'/' || c == b'?')
}

/// ParseIpSixAddressMm(): the address bytes of an IPv6 literal (no brackets).
fn parse_ip_six_address(s: &[u8]) -> Option<[u8; 16]> {
    if matches!(s.first(), Some(b'v' | b'V')) {
        return None;
    }
    if s.len() > 8 * 4 + 7 {
        return None;
    }
    let mut candidate = b"//[".to_vec();
    candidate.extend_from_slice(s);
    candidate.push(b']');
    let u = parse(&candidate).ok()?;
    u.ip6
}

/// IsWellFormedHostIpFutureMm()
fn is_well_formed_host_ip_future(s: &[u8]) -> bool {
    if !s.is_empty() && !matches!(s[0], b'v' | b'V') {
        return false;
    }
    let mut candidate = b"//[".to_vec();
    candidate.extend_from_slice(s);
    candidate.push(b']');
    parse(&candidate).is_ok()
}

/// FixPercentEncodingEngine(): decode percent-encoded unreserved
/// characters, uppercase the other percent-encodings.
fn fix_percent_encoding(s: &mut Vec<u8>) {
    let input = std::mem::take(s);
    let len = input.len();
    let mut out = Vec::with_capacity(len);
    let mut i = 0usize;
    // All but last two
    while i + 2 < len {
        if input[i] != b'%' {
            out.push(input[i]);
        } else {
            let left = hexdig_to_int(input[i + 1]);
            let right = hexdig_to_int(input[i + 2]);
            let code = 16 * left + right;
            if is_unreserved(code) {
                out.push(code);
            } else {
                out.push(b'%');
                out.push(hex_to_letter(left, true));
                out.push(hex_to_letter(right, true));
            }
            i += 2;
        }
        i += 1;
    }
    // Last two
    while i < len {
        out.push(input[i]);
        i += 1;
    }
    *s = out;
}

/// LowercaseInplaceExceptPercentEncoding()
fn lowercase_except_percent_encoding(s: &mut [u8]) {
    let len = s.len();
    let mut i = 0usize;
    while i < len {
        if s[i].is_ascii_uppercase() {
            s[i] = s[i].to_ascii_lowercase();
        } else if s[i] == b'%' {
            if i + 3 >= len {
                return;
            }
            i += 2;
        }
        i += 1;
    }
}
