//! The `$options` / config-file settings shared by openssl_pkey_new(),
//! openssl_pkey_export(), openssl_csr_new() and openssl_csr_sign() (PHP
//! manual: openssl_csr_new() "options" table: digest_alg,
//! x509_extensions, req_extensions, private_key_bits, private_key_type,
//! encrypt_key, encrypt_key_cipher, curve_name, config,
//! config_section_name; each falls back to the config file's `req`
//! section key named in that table).

use crate::conf::Conf;
use crate::csr::StringMask;

/// Settings resolved from the options array and the config file.
#[derive(Debug, Clone)]
pub struct Req {
    pub conf: Conf,
    pub config_path: String,
    pub section: String,
    pub digest: Option<String>,
    pub x509_extensions: Option<String>,
    pub req_extensions: Option<String>,
    pub bits: i64,
    pub key_type: i64,
    pub encrypt_key: bool,
    pub cipher: Option<i64>,
    pub curve_name: Option<String>,
    pub string_mask: StringMask,
    /// Names from the config's oid_section.
    pub custom_oids: Vec<(String, String)>,
    /// The `oid_file` setting is not a usable path.
    pub oid_file_bad: bool,
}

/// An option value as the engine passes it.
#[derive(Debug, Clone)]
pub enum Opt {
    Str(String),
    Int(i64),
    Bool(bool),
}

impl Opt {
    pub fn as_str(&self) -> String {
        match self {
            Opt::Str(s) => s.clone(),
            Opt::Int(n) => n.to_string(),
            Opt::Bool(b) => {
                if *b {
                    "1".into()
                } else {
                    String::new()
                }
            }
        }
    }
}

/// The built-in configuration used when no file is given (written for
/// goro: a `req` section with the documented defaults).
pub const DEFAULT_CONF: &str = "[ req ]\ndefault_bits = 2048\ndefault_md = sha256\ndistinguished_name = req_distinguished_name\nstring_mask = utf8only\n[ req_distinguished_name ]\n[ v3_req ]\nbasicConstraints = CA:FALSE\n[ v3_ca ]\nsubjectKeyIdentifier = hash\nauthorityKeyIdentifier = keyid:always,issuer\nbasicConstraints = critical,CA:true\n[ usr_cert ]\nbasicConstraints = CA:FALSE\n";

#[derive(Debug, Clone)]
pub enum ReqError {
    /// The config file couldn't be read or parsed (path).
    Config(String),
    /// A named extension section is missing: (option key, section).
    MissingSection(&'static str, String),
}

impl Req {
    /// `get(key)` returns an options-array entry; `read_file` loads a
    /// config path.
    pub fn new(
        get: &dyn Fn(&str) -> Option<Opt>,
        read_file: &dyn Fn(&str) -> Option<Vec<u8>>,
    ) -> Result<Req, ReqError> {
        let (conf, path) = match get("config") {
            Some(p) => {
                let p = p.as_str();
                let data = read_file(&p).ok_or_else(|| ReqError::Config(p.clone()))?;
                (
                    Conf::parse(&data).map_err(|_| ReqError::Config(p.clone()))?,
                    p,
                )
            }
            None => (
                Conf::parse(DEFAULT_CONF.as_bytes()).expect("built-in config"),
                String::new(),
            ),
        };
        let section = get("config_section_name")
            .map(|o| o.as_str())
            .unwrap_or_else(|| "req".into());
        let cget = |k: &str| conf.get(&section, k).map(|s| s.to_string());
        let digest = get("digest_alg")
            .map(|o| o.as_str())
            .or_else(|| cget("default_md"));
        let x509_extensions = match get("x509_extensions") {
            Some(Opt::Str(s)) => Some(s),
            Some(_) => None,
            None => cget("x509_extensions"),
        };
        let req_extensions = match get("req_extensions") {
            Some(Opt::Str(s)) => Some(s),
            Some(_) => None,
            None => cget("req_extensions"),
        };
        let bits = match get("private_key_bits") {
            Some(Opt::Int(n)) => n,
            Some(o) => o.as_str().parse().unwrap_or(2048),
            None => cget("default_bits")
                .and_then(|s| s.trim().parse().ok())
                .unwrap_or(2048),
        };
        let key_type = match get("private_key_type") {
            Some(Opt::Int(n)) => n,
            Some(o) => o.as_str().parse().unwrap_or(0),
            None => 0,
        };
        let encrypt_key = match get("encrypt_key") {
            Some(Opt::Bool(b)) => b,
            Some(Opt::Int(n)) => n != 0,
            Some(Opt::Str(s)) => !s.is_empty() && s != "0",
            None => cget("encrypt_key").is_none_or(|s| s != "no"),
        };
        let cipher = match get("encrypt_key_cipher") {
            Some(Opt::Int(n)) => Some(n),
            Some(o) => o.as_str().parse().ok(),
            None => None,
        };
        let curve_name = get("curve_name").map(|o| o.as_str());
        let string_mask = StringMask::parse(cget("string_mask").as_deref());
        let mut custom_oids = Vec::new();
        if let Some(sec) = conf.get("default", "oid_section").map(|s| s.to_string())
            && let Some(e) = conf.section(&sec)
        {
            for (n, v) in e {
                custom_oids.push((n.clone(), v.trim().to_string()));
            }
        }
        if let Some(sec) = cget("oid_section")
            && let Some(e) = conf.section(&sec)
        {
            for (n, v) in e {
                custom_oids.push((n.clone(), v.trim().to_string()));
            }
        }
        let oid_file_bad = conf
            .get("default", "oid_file")
            .is_some_and(|p| p.len() >= 4096 || p.contains('\0'));
        let r = Req {
            conf,
            config_path: path,
            section,
            digest,
            x509_extensions,
            req_extensions,
            bits,
            key_type,
            encrypt_key,
            cipher,
            curve_name,
            string_mask,
            custom_oids,
            oid_file_bad,
        };
        if let Some(s) = &r.x509_extensions
            && r.conf.section(s).is_none()
        {
            return Err(ReqError::MissingSection("extensions_section", s.clone()));
        }
        if let Some(s) = &r.req_extensions
            && r.conf.section(s).is_none()
        {
            return Err(ReqError::MissingSection(
                "request_extensions_section",
                s.clone(),
            ));
        }
        Ok(r)
    }

    pub fn get(&self, key: &str) -> Option<&str> {
        self.conf.get(&self.section, key)
    }

    /// Resolves an attribute name: built-in names, the config's custom
    /// OIDs, or a dotted OID.
    pub fn resolve_oid(&self, name: &str) -> Option<String> {
        if let Some(o) = crate::oid::by_name(name) {
            return Some(o.oid.to_string());
        }
        if let Some((_, o)) = self.custom_oids.iter().find(|(n, _)| n == name) {
            return Some(o.clone());
        }
        None
    }
}
