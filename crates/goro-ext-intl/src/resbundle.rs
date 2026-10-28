//! ICU binary resource bundles (`.res` files, ResourceBundle): a data
//! header, then a root resource and the 32-bit resource words of format
//! 1.x/2.x (type in the top 4 bits: string, binary, table, int, array,
//! int vector; 16-bit tables and strings), read in the file's byte order.
//! The layout is the one of the files PHP's tests ship
//! (ext/intl/tests/_files/resourcebundle_*), checked against their source
//! text (resourcebundle.txt).

use std::rc::Rc;

/// A resource value.
#[derive(Debug, Clone)]
pub enum Res {
    Str(String),
    Bin(Vec<u8>),
    Int(i64),
    IntVector(Vec<i64>),
    Table(Vec<(String, Res)>),
    Array(Vec<Res>),
}

/// A parsed bundle file.
#[derive(Debug, Clone)]
pub struct Bundle {
    pub root: Res,
}

struct Reader<'a> {
    data: &'a [u8],
    be: bool,
    /// The 16-bit units area (format 2), in 16-bit units from the data
    /// start.
    pool16: usize,
}

impl Reader<'_> {
    fn u16(&self, off: usize) -> Option<u16> {
        let b = self.data.get(off..off + 2)?;
        Some(if self.be {
            u16::from_be_bytes([b[0], b[1]])
        } else {
            u16::from_le_bytes([b[0], b[1]])
        })
    }

    fn u32(&self, off: usize) -> Option<u32> {
        let b = self.data.get(off..off + 4)?;
        Some(if self.be {
            u32::from_be_bytes([b[0], b[1], b[2], b[3]])
        } else {
            u32::from_le_bytes([b[0], b[1], b[2], b[3]])
        })
    }

    fn key(&self, off: usize) -> Option<String> {
        let rest = self.data.get(off..)?;
        let end = rest.iter().position(|b| *b == 0)?;
        Some(String::from_utf8_lossy(&rest[..end]).into_owned())
    }

    fn utf16(&self, off: usize, n: usize) -> Option<String> {
        let mut v = Vec::with_capacity(n);
        for i in 0..n {
            v.push(self.u16(off + 2 * i)?);
        }
        Some(String::from_utf16_lossy(&v))
    }

    fn res(&self, word: u32, depth: usize) -> Option<Res> {
        if depth > 64 {
            return None;
        }
        let ty = word >> 28;
        let off = (word & 0x0FFF_FFFF) as usize;
        let at = off * 4;
        Some(match ty {
            0 => {
                if off == 0 {
                    return Some(Res::Str(String::new()));
                }
                let n = self.u32(at)? as usize;
                Res::Str(self.utf16(at + 4, n)?)
            }
            1 => {
                let n = self.u32(at)? as usize;
                Res::Bin(self.data.get(at + 4..at + 4 + n)?.to_vec())
            }
            2 => {
                if off == 0 {
                    return Some(Res::Table(Vec::new()));
                }
                let n = self.u16(at)? as usize;
                let keys_at = at + 2;
                let mut vals_at = keys_at + 2 * n;
                if !vals_at.is_multiple_of(4) {
                    vals_at += 2;
                }
                let mut t = Vec::with_capacity(n);
                for i in 0..n {
                    let k = self.key(self.u16(keys_at + 2 * i)? as usize)?;
                    let w = self.u32(vals_at + 4 * i)?;
                    t.push((k, self.res(w, depth + 1)?));
                }
                Res::Table(t)
            }
            4 => {
                let n = self.u32(at)? as usize;
                let mut t = Vec::with_capacity(n);
                for i in 0..n {
                    let k = self.key(self.u32(at + 4 + 4 * i)? as usize)?;
                    let w = self.u32(at + 4 + 4 * n + 4 * i)?;
                    t.push((k, self.res(w, depth + 1)?));
                }
                Res::Table(t)
            }
            5 => {
                // 16-bit table in the 16-bit area
                let base = (self.pool16 + off) * 2;
                let n = self.u16(base)? as usize;
                let mut t = Vec::with_capacity(n);
                for i in 0..n {
                    let k = self.key(self.u16(base + 2 + 2 * i)? as usize)?;
                    let v = self.u16(base + 2 + 2 * n + 2 * i)? as u32;
                    t.push((k, self.res16(v)?));
                }
                Res::Table(t)
            }
            6 => self.res16(off as u32 | 0x6000_0000)?,
            7 => {
                let v = (word & 0x0FFF_FFFF) as i64;
                Res::Int(if v & 0x0800_0000 != 0 {
                    v - 0x1000_0000
                } else {
                    v
                })
            }
            8 => {
                if off == 0 {
                    return Some(Res::Array(Vec::new()));
                }
                let n = self.u32(at)? as usize;
                let mut a = Vec::with_capacity(n);
                for i in 0..n {
                    a.push(self.res(self.u32(at + 4 + 4 * i)?, depth + 1)?);
                }
                Res::Array(a)
            }
            9 => {
                let base = (self.pool16 + off) * 2;
                let n = self.u16(base)? as usize;
                let mut a = Vec::with_capacity(n);
                for i in 0..n {
                    a.push(self.res16(self.u16(base + 2 + 2 * i)? as u32)?);
                }
                Res::Array(a)
            }
            14 => {
                if off == 0 {
                    return Some(Res::IntVector(Vec::new()));
                }
                let n = self.u32(at)? as usize;
                let mut v = Vec::with_capacity(n);
                for i in 0..n {
                    v.push(self.u32(at + 4 + 4 * i)? as i32 as i64);
                }
                Res::IntVector(v)
            }
            _ => return None,
        })
    }

    /// A 16-bit resource reference: a string in the 16-bit area.
    fn res16(&self, v: u32) -> Option<Res> {
        let off = (v & 0xFFFF) as usize;
        let base = (self.pool16 + off) * 2;
        // a NUL-terminated (or length-prefixed) UTF-16 string
        let first = self.u16(base)?;
        let (start, n) = if (0xDC00..=0xDFEF).contains(&first) {
            (base + 2, (first - 0xDC00) as usize)
        } else {
            let mut n = 0;
            while self.u16(base + 2 * n)? != 0 {
                n += 1;
            }
            (base, n)
        };
        Some(Res::Str(self.utf16(start, n)?))
    }
}

impl Bundle {
    /// Parses a `.res` file.
    pub fn parse(file: &[u8]) -> Option<Bundle> {
        if file.len() < 32 || file[2] != 0xDA || file[3] != 0x27 {
            return None;
        }
        let be = file[8] != 0;
        let hsize = if be {
            u16::from_be_bytes([file[0], file[1]])
        } else {
            u16::from_le_bytes([file[0], file[1]])
        } as usize;
        if file.get(12..16)? != b"ResB" {
            return None;
        }
        let format = file[16];
        let data = file.get(hsize..)?;
        let mut r = Reader {
            data,
            be,
            pool16: 0,
        };
        let root = r.u32(0)?;
        if format >= 2 {
            let n = (r.u32(4)? & 0xFF) as usize;
            if n > 6 {
                // the 16-bit area follows the keys
                r.pool16 = r.u32(4 + 4)? as usize * 2;
            }
        }
        let root = r.res(root, 0)?;
        Some(Bundle { root })
    }
}

/// A bundle loaded from a directory with its locale fallback.
#[derive(Debug, Clone)]
pub struct Loaded {
    pub bundle: Rc<Bundle>,
    /// The locale of the file found.
    pub locale: String,
    /// The error code of the load (U_USING_FALLBACK_WARNING,
    /// U_USING_DEFAULT_WARNING or 0).
    pub warning: i64,
}

/// Loads `<dir>/<locale>.res`, falling back to the parent locales and
/// root (with `fallback`).
pub fn load(dir: &str, locale: &str, fallback: bool) -> Option<Loaded> {
    let p = crate::locale::parse(locale);
    let mut chain = Vec::new();
    let base = p.base_id();
    if !base.is_empty() {
        let mut cur = base.clone();
        loop {
            chain.push(cur.clone());
            match cur.rfind('_') {
                Some(i) => cur.truncate(i),
                None => break,
            }
        }
    }
    let first = chain.first().cloned();
    if !fallback {
        chain.truncate(1);
    } else {
        chain.push("root".into());
    }
    for (i, l) in chain.iter().enumerate() {
        let path = format!("{}/{}.res", dir.trim_end_matches('/'), l);
        if let Ok(bytes) = std::fs::read(&path)
            && let Some(b) = Bundle::parse(&bytes)
        {
            let warning = if i == 0 && Some(l) == first.as_ref() {
                0
            } else if l == "root" {
                crate::error::U_USING_DEFAULT_WARNING
            } else {
                crate::error::U_USING_FALLBACK_WARNING
            };
            return Some(Loaded {
                bundle: Rc::new(b),
                locale: l.clone(),
                warning,
            });
        }
    }
    None
}

/// The locales of a bundle directory (`res_index.res` InstalledLocales).
pub fn locales(dir: &str) -> Option<Vec<String>> {
    let path = format!("{}/res_index.res", dir.trim_end_matches('/'));
    let b = Bundle::parse(&std::fs::read(path).ok()?)?;
    let Res::Table(t) = &b.root else { return None };
    let (_, inst) = t.iter().find(|(k, _)| k == "InstalledLocales")?;
    match inst {
        Res::Table(t) => Some(t.iter().map(|(k, _)| k.clone()).collect()),
        _ => None,
    }
}
