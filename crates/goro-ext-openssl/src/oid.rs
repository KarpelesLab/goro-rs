//! Object identifiers and the short/long names PHP prints for them.
//!
//! The OIDs come from their defining standards (RFC 5280 and X.520 for
//! name attributes and extensions, RFC 8017 / RFC 5758 / RFC 8410 for
//! algorithms, RFC 5652 / RFC 2985 for PKCS #7 and #9, NIST CSOR for the
//! AES and SHA-2/3 arcs). The short and long names and the numeric
//! identifiers (`signatureTypeNID`) are the ones PHP 8.5 reports (observed
//! on PHP 8.5 with OpenSSL 3.5: openssl_csr_get_subject() with both name
//! styles, openssl_x509_parse()).

#[derive(Debug, Clone, Copy)]
pub struct Obj {
    pub oid: &'static str,
    pub sn: &'static str,
    pub ln: &'static str,
    pub nid: i64,
}

const fn o(oid: &'static str, sn: &'static str, ln: &'static str, nid: i64) -> Obj {
    Obj { oid, sn, ln, nid }
}

pub static OBJS: &[Obj] = &[
    // X.520 / RFC 4519 name attributes.
    o("2.5.4.3", "CN", "commonName", 13),
    o("2.5.4.4", "SN", "surname", 100),
    o("2.5.4.5", "serialNumber", "serialNumber", 105),
    o("2.5.4.6", "C", "countryName", 14),
    o("2.5.4.7", "L", "localityName", 15),
    o("2.5.4.8", "ST", "stateOrProvinceName", 16),
    o("2.5.4.9", "street", "streetAddress", 660),
    o("2.5.4.10", "O", "organizationName", 17),
    o("2.5.4.11", "OU", "organizationalUnitName", 18),
    o("2.5.4.12", "title", "title", 106),
    o("2.5.4.13", "description", "description", 107),
    o("2.5.4.15", "businessCategory", "businessCategory", 860),
    o("2.5.4.17", "postalCode", "postalCode", 661),
    o("2.5.4.18", "postOfficeBox", "postOfficeBox", 862),
    o("2.5.4.20", "telephoneNumber", "telephoneNumber", 864),
    o(
        "2.5.4.23",
        "facsimileTelephoneNumber",
        "facsimileTelephoneNumber",
        867,
    ),
    o("2.5.4.41", "name", "name", 173),
    o("2.5.4.42", "GN", "givenName", 99),
    o("2.5.4.43", "initials", "initials", 101),
    o(
        "2.5.4.44",
        "generationQualifier",
        "generationQualifier",
        509,
    ),
    o(
        "2.5.4.45",
        "x500UniqueIdentifier",
        "x500UniqueIdentifier",
        503,
    ),
    o("2.5.4.46", "dnQualifier", "dnQualifier", 174),
    o("2.5.4.51", "houseIdentifier", "houseIdentifier", 889),
    o("2.5.4.65", "pseudonym", "pseudonym", 510),
    o("2.5.4.72", "role", "role", 400),
    o(
        "2.5.4.97",
        "organizationIdentifier",
        "organizationIdentifier",
        1089,
    ),
    o("0.9.2342.19200300.100.1.1", "UID", "userId", 458),
    o("0.9.2342.19200300.100.1.3", "mail", "rfc822Mailbox", 460),
    o("0.9.2342.19200300.100.1.25", "DC", "domainComponent", 391),
    o("0.9.2342.19200300.100.1.44", "uid", "uniqueIdentifier", 102),
    o(
        "1.3.6.1.4.1.311.60.2.1.1",
        "jurisdictionL",
        "jurisdictionLocalityName",
        955,
    ),
    o(
        "1.3.6.1.4.1.311.60.2.1.2",
        "jurisdictionST",
        "jurisdictionStateOrProvinceName",
        956,
    ),
    o(
        "1.3.6.1.4.1.311.60.2.1.3",
        "jurisdictionC",
        "jurisdictionCountryName",
        957,
    ),
    // PKCS #9 (RFC 2985).
    o("1.2.840.113549.1.9.1", "emailAddress", "emailAddress", 48),
    o(
        "1.2.840.113549.1.9.2",
        "unstructuredName",
        "unstructuredName",
        49,
    ),
    o("1.2.840.113549.1.9.3", "contentType", "contentType", 50),
    o("1.2.840.113549.1.9.4", "messageDigest", "messageDigest", 51),
    o("1.2.840.113549.1.9.5", "signingTime", "signingTime", 52),
    o(
        "1.2.840.113549.1.9.6",
        "countersignature",
        "countersignature",
        53,
    ),
    o(
        "1.2.840.113549.1.9.7",
        "challengePassword",
        "challengePassword",
        54,
    ),
    o(
        "1.2.840.113549.1.9.8",
        "unstructuredAddress",
        "unstructuredAddress",
        55,
    ),
    o("1.2.840.113549.1.9.14", "extReq", "Extension Request", 172),
    o(
        "1.2.840.113549.1.9.15",
        "SMIME-CAPS",
        "S/MIME Capabilities",
        167,
    ),
    o("1.2.840.113549.1.9.20", "friendlyName", "friendlyName", 156),
    o("1.2.840.113549.1.9.21", "localKeyID", "localKeyID", 157),
    o(
        "1.2.840.113549.1.9.22.1",
        "x509Certificate",
        "x509Certificate",
        158,
    ),
    o(
        "1.2.840.113549.1.9.16.2.47",
        "id-smime-aa-signingCertificateV2",
        "id-smime-aa-signingCertificateV2",
        1086,
    ),
    // Public key algorithms.
    o("1.2.840.113549.1.1.1", "rsaEncryption", "rsaEncryption", 6),
    o("1.2.840.113549.1.1.10", "RSASSA-PSS", "rsassaPss", 912),
    o("1.2.840.113549.1.1.7", "RSAES-OAEP", "rsaesOaep", 919),
    o("1.2.840.113549.1.1.8", "MGF1", "mgf1", 911),
    o("1.2.840.10040.4.1", "DSA", "dsaEncryption", 116),
    o("1.2.840.10046.2.1", "dhpublicnumber", "X9.42 DH", 920),
    o(
        "1.2.840.113549.1.3.1",
        "dhKeyAgreement",
        "dhKeyAgreement",
        28,
    ),
    o("1.2.840.10045.2.1", "id-ecPublicKey", "id-ecPublicKey", 408),
    o("1.3.101.110", "X25519", "X25519", 1034),
    o("1.3.101.111", "X448", "X448", 1035),
    o("1.3.101.112", "ED25519", "ED25519", 1087),
    o("1.3.101.113", "ED448", "ED448", 1088),
    o("1.2.156.10197.1.301", "SM2", "sm2", 1172),
    // Signature algorithms (NIDs observed on PHP 8.5).
    o("1.2.840.113549.1.1.2", "RSA-MD2", "md2WithRSAEncryption", 7),
    o(
        "1.2.840.113549.1.1.3",
        "RSA-MD4",
        "md4WithRSAEncryption",
        396,
    ),
    o("1.2.840.113549.1.1.4", "RSA-MD5", "md5WithRSAEncryption", 8),
    o(
        "1.2.840.113549.1.1.5",
        "RSA-SHA1",
        "sha1WithRSAEncryption",
        65,
    ),
    o(
        "1.2.840.113549.1.1.11",
        "RSA-SHA256",
        "sha256WithRSAEncryption",
        668,
    ),
    o(
        "1.2.840.113549.1.1.12",
        "RSA-SHA384",
        "sha384WithRSAEncryption",
        669,
    ),
    o(
        "1.2.840.113549.1.1.13",
        "RSA-SHA512",
        "sha512WithRSAEncryption",
        670,
    ),
    o(
        "1.2.840.113549.1.1.14",
        "RSA-SHA224",
        "sha224WithRSAEncryption",
        671,
    ),
    o(
        "1.2.840.113549.1.1.15",
        "RSA-SHA512/224",
        "sha512-224WithRSAEncryption",
        1145,
    ),
    o(
        "1.2.840.113549.1.1.16",
        "RSA-SHA512/256",
        "sha512-256WithRSAEncryption",
        1146,
    ),
    o("1.3.36.3.3.1.2", "RSA-RIPEMD160", "ripemd160WithRSA", 119),
    o(
        "2.16.840.1.101.3.4.3.13",
        "id-rsassa-pkcs1-v1_5-with-sha3-224",
        "RSA-SHA3-224",
        1116,
    ),
    o(
        "2.16.840.1.101.3.4.3.14",
        "id-rsassa-pkcs1-v1_5-with-sha3-256",
        "RSA-SHA3-256",
        1117,
    ),
    o(
        "2.16.840.1.101.3.4.3.15",
        "id-rsassa-pkcs1-v1_5-with-sha3-384",
        "RSA-SHA3-384",
        1118,
    ),
    o(
        "2.16.840.1.101.3.4.3.16",
        "id-rsassa-pkcs1-v1_5-with-sha3-512",
        "RSA-SHA3-512",
        1119,
    ),
    o(
        "1.2.840.10045.4.1",
        "ecdsa-with-SHA1",
        "ecdsa-with-SHA1",
        416,
    ),
    o(
        "1.2.840.10045.4.3.1",
        "ecdsa-with-SHA224",
        "ecdsa-with-SHA224",
        793,
    ),
    o(
        "1.2.840.10045.4.3.2",
        "ecdsa-with-SHA256",
        "ecdsa-with-SHA256",
        794,
    ),
    o(
        "1.2.840.10045.4.3.3",
        "ecdsa-with-SHA384",
        "ecdsa-with-SHA384",
        795,
    ),
    o(
        "1.2.840.10045.4.3.4",
        "ecdsa-with-SHA512",
        "ecdsa-with-SHA512",
        796,
    ),
    o(
        "2.16.840.1.101.3.4.3.9",
        "id-ecdsa-with-sha3-224",
        "ecdsa_with_SHA3-224",
        1112,
    ),
    o(
        "2.16.840.1.101.3.4.3.10",
        "id-ecdsa-with-sha3-256",
        "ecdsa_with_SHA3-256",
        1113,
    ),
    o(
        "2.16.840.1.101.3.4.3.11",
        "id-ecdsa-with-sha3-384",
        "ecdsa_with_SHA3-384",
        1114,
    ),
    o(
        "2.16.840.1.101.3.4.3.12",
        "id-ecdsa-with-sha3-512",
        "ecdsa_with_SHA3-512",
        1115,
    ),
    o("1.2.840.10040.4.3", "DSA-SHA1", "dsaWithSHA1", 113),
    o(
        "2.16.840.1.101.3.4.3.1",
        "dsa_with_SHA224",
        "dsa_with_SHA224",
        802,
    ),
    o(
        "2.16.840.1.101.3.4.3.2",
        "dsa_with_SHA256",
        "dsa_with_SHA256",
        803,
    ),
    o("1.2.156.10197.1.501", "SM2-SM3", "SM2-with-SM3", 1204),
    // Digests.
    o("1.2.840.113549.2.2", "MD2", "md2", 3),
    o("1.2.840.113549.2.4", "MD4", "md4", 257),
    o("1.2.840.113549.2.5", "MD5", "md5", 4),
    o("1.3.14.3.2.26", "SHA1", "sha1", 64),
    o("2.16.840.1.101.3.4.2.1", "SHA256", "sha256", 672),
    o("2.16.840.1.101.3.4.2.2", "SHA384", "sha384", 673),
    o("2.16.840.1.101.3.4.2.3", "SHA512", "sha512", 674),
    o("2.16.840.1.101.3.4.2.4", "SHA224", "sha224", 675),
    o("2.16.840.1.101.3.4.2.5", "SHA512-224", "sha512-224", 1094),
    o("2.16.840.1.101.3.4.2.6", "SHA512-256", "sha512-256", 1095),
    o("2.16.840.1.101.3.4.2.7", "SHA3-224", "sha3-224", 1096),
    o("2.16.840.1.101.3.4.2.8", "SHA3-256", "sha3-256", 1097),
    o("2.16.840.1.101.3.4.2.9", "SHA3-384", "sha3-384", 1098),
    o("2.16.840.1.101.3.4.2.10", "SHA3-512", "sha3-512", 1099),
    o("2.16.840.1.101.3.4.2.11", "SHAKE128", "shake128", 1100),
    o("2.16.840.1.101.3.4.2.12", "SHAKE256", "shake256", 1101),
    o("1.3.36.3.2.1", "RIPEMD160", "ripemd160", 117),
    o("1.2.156.10197.1.401", "SM3", "sm3", 1143),
    o("1.2.840.113549.2.7", "hmacWithSHA1", "hmacWithSHA1", 163),
    o(
        "1.2.840.113549.2.8",
        "hmacWithSHA224",
        "hmacWithSHA224",
        798,
    ),
    o(
        "1.2.840.113549.2.9",
        "hmacWithSHA256",
        "hmacWithSHA256",
        799,
    ),
    o(
        "1.2.840.113549.2.10",
        "hmacWithSHA384",
        "hmacWithSHA384",
        800,
    ),
    o(
        "1.2.840.113549.2.11",
        "hmacWithSHA512",
        "hmacWithSHA512",
        801,
    ),
    // Curves (RFC 5480, SEC 2, RFC 5639).
    o("1.2.840.10045.3.1.1", "prime192v1", "prime192v1", 409),
    o("1.2.840.10045.3.1.7", "prime256v1", "prime256v1", 415),
    o("1.3.132.0.33", "secp224r1", "secp224r1", 713),
    o("1.3.132.0.34", "secp384r1", "secp384r1", 715),
    o("1.3.132.0.35", "secp521r1", "secp521r1", 716),
    o("1.3.132.0.10", "secp256k1", "secp256k1", 714),
    o("1.3.132.0.31", "secp192k1", "secp192k1", 711),
    o("1.3.132.0.32", "secp224k1", "secp224k1", 712),
    o("1.3.132.0.9", "secp160k1", "secp160k1", 708),
    o("1.3.132.0.8", "secp160r1", "secp160r1", 709),
    o("1.3.132.0.30", "secp160r2", "secp160r2", 710),
    o(
        "1.3.36.3.3.2.8.1.1.5",
        "brainpoolP224r1",
        "brainpoolP224r1",
        925,
    ),
    o(
        "1.3.36.3.3.2.8.1.1.7",
        "brainpoolP256r1",
        "brainpoolP256r1",
        927,
    ),
    o(
        "1.3.36.3.3.2.8.1.1.9",
        "brainpoolP320r1",
        "brainpoolP320r1",
        929,
    ),
    o(
        "1.3.36.3.3.2.8.1.1.11",
        "brainpoolP384r1",
        "brainpoolP384r1",
        931,
    ),
    o(
        "1.3.36.3.3.2.8.1.1.13",
        "brainpoolP512r1",
        "brainpoolP512r1",
        933,
    ),
    // X.509v3 extensions (RFC 5280 §4.2).
    o(
        "2.5.29.9",
        "subjectDirectoryAttributes",
        "X509v3 Subject Directory Attributes",
        769,
    ),
    o(
        "2.5.29.14",
        "subjectKeyIdentifier",
        "X509v3 Subject Key Identifier",
        82,
    ),
    o("2.5.29.15", "keyUsage", "X509v3 Key Usage", 83),
    o(
        "2.5.29.16",
        "privateKeyUsagePeriod",
        "X509v3 Private Key Usage Period",
        84,
    ),
    o(
        "2.5.29.17",
        "subjectAltName",
        "X509v3 Subject Alternative Name",
        85,
    ),
    o(
        "2.5.29.18",
        "issuerAltName",
        "X509v3 Issuer Alternative Name",
        86,
    ),
    o(
        "2.5.29.19",
        "basicConstraints",
        "X509v3 Basic Constraints",
        87,
    ),
    o("2.5.29.20", "crlNumber", "X509v3 CRL Number", 88),
    o("2.5.29.21", "CRLReason", "X509v3 CRL Reason Code", 141),
    o(
        "2.5.29.30",
        "nameConstraints",
        "X509v3 Name Constraints",
        666,
    ),
    o(
        "2.5.29.31",
        "crlDistributionPoints",
        "X509v3 CRL Distribution Points",
        103,
    ),
    o(
        "2.5.29.32",
        "certificatePolicies",
        "X509v3 Certificate Policies",
        89,
    ),
    o("2.5.29.32.0", "anyPolicy", "X509v3 Any Policy", 746),
    o("2.5.29.33", "policyMappings", "X509v3 Policy Mappings", 747),
    o(
        "2.5.29.35",
        "authorityKeyIdentifier",
        "X509v3 Authority Key Identifier",
        90,
    ),
    o(
        "2.5.29.36",
        "policyConstraints",
        "X509v3 Policy Constraints",
        401,
    ),
    o(
        "2.5.29.37",
        "extendedKeyUsage",
        "X509v3 Extended Key Usage",
        126,
    ),
    o("2.5.29.46", "freshestCRL", "X509v3 Freshest CRL", 857),
    o(
        "2.5.29.54",
        "inhibitAnyPolicy",
        "X509v3 Inhibit Any Policy",
        748,
    ),
    o(
        "1.3.6.1.5.5.7.1.1",
        "authorityInfoAccess",
        "Authority Information Access",
        177,
    ),
    o(
        "1.3.6.1.5.5.7.1.11",
        "subjectInfoAccess",
        "Subject Information Access",
        398,
    ),
    o("1.3.6.1.5.5.7.1.24", "tlsfeature", "TLS Feature", 1020),
    o(
        "1.3.6.1.4.1.11129.2.4.2",
        "ct_precert_scts",
        "CT Precertificate SCTs",
        951,
    ),
    o(
        "1.3.6.1.4.1.11129.2.4.3",
        "ct_precert_poison",
        "CT Precertificate Poison",
        952,
    ),
    o(
        "2.16.840.1.113730.1.1",
        "nsCertType",
        "Netscape Cert Type",
        71,
    ),
    o(
        "2.16.840.1.113730.1.2",
        "nsBaseUrl",
        "Netscape Base Url",
        72,
    ),
    o(
        "2.16.840.1.113730.1.3",
        "nsRevocationUrl",
        "Netscape Revocation Url",
        73,
    ),
    o(
        "2.16.840.1.113730.1.4",
        "nsCaRevocationUrl",
        "Netscape CA Revocation Url",
        74,
    ),
    o(
        "2.16.840.1.113730.1.7",
        "nsRenewalUrl",
        "Netscape Renewal Url",
        75,
    ),
    o(
        "2.16.840.1.113730.1.8",
        "nsCaPolicyUrl",
        "Netscape CA Policy Url",
        76,
    ),
    o(
        "2.16.840.1.113730.1.12",
        "nsSslServerName",
        "Netscape SSL Server Name",
        77,
    ),
    o(
        "2.16.840.1.113730.1.13",
        "nsComment",
        "Netscape Comment",
        78,
    ),
    o(
        "1.3.6.1.4.1.311.20.2",
        "msCertificateTemplateName",
        "Microsoft Certificate Template Name",
        0,
    ),
    o(
        "1.3.6.1.4.1.311.21.7",
        "msCertificateTemplate",
        "Microsoft Certificate Template",
        0,
    ),
    // Extended key usages and access methods (RFC 5280 §4.2.1.12, §4.2.2.1).
    o(
        "1.3.6.1.5.5.7.3.1",
        "serverAuth",
        "TLS Web Server Authentication",
        129,
    ),
    o(
        "1.3.6.1.5.5.7.3.2",
        "clientAuth",
        "TLS Web Client Authentication",
        130,
    ),
    o("1.3.6.1.5.5.7.3.3", "codeSigning", "Code Signing", 131),
    o(
        "1.3.6.1.5.5.7.3.4",
        "emailProtection",
        "E-mail Protection",
        132,
    ),
    o("1.3.6.1.5.5.7.3.8", "timeStamping", "Time Stamping", 133),
    o("1.3.6.1.5.5.7.3.9", "OCSPSigning", "OCSP Signing", 180),
    o(
        "1.3.6.1.4.1.311.2.1.21",
        "msCodeInd",
        "Microsoft Individual Code Signing",
        134,
    ),
    o(
        "1.3.6.1.4.1.311.2.1.22",
        "msCodeCom",
        "Microsoft Commercial Code Signing",
        135,
    ),
    o(
        "1.3.6.1.4.1.311.10.3.1",
        "msCTLSign",
        "Microsoft Trust List Signing",
        136,
    ),
    o(
        "1.3.6.1.4.1.311.10.3.4",
        "msEFS",
        "Microsoft Encrypted File System",
        138,
    ),
    o(
        "1.3.6.1.4.1.311.20.2.2",
        "msSmartcardLogin",
        "Microsoft Smartcard Login",
        648,
    ),
    o(
        "2.16.840.1.113730.4.1",
        "nsSGC",
        "Netscape Server Gated Crypto",
        139,
    ),
    o(
        "1.3.6.1.4.1.311.10.3.3",
        "msSGC",
        "Microsoft Server Gated Crypto",
        137,
    ),
    o(
        "2.5.29.37.0",
        "anyExtendedKeyUsage",
        "Any Extended Key Usage",
        910,
    ),
    o("1.3.6.1.5.5.7.48.1", "OCSP", "OCSP", 178),
    o("1.3.6.1.5.5.7.48.2", "caIssuers", "CA Issuers", 179),
    o("1.3.6.1.5.5.7.48.5", "caRepository", "CA Repository", 785),
    o(
        "1.3.6.1.5.5.7.2.1",
        "id-qt-cps",
        "Policy Qualifier CPS",
        164,
    ),
    o(
        "1.3.6.1.5.5.7.2.2",
        "id-qt-unotice",
        "Policy Qualifier User Notice",
        165,
    ),
    // PKCS #7 / CMS content types (RFC 5652, RFC 5083).
    o("1.2.840.113549.1.7.1", "pkcs7-data", "pkcs7-data", 21),
    o(
        "1.2.840.113549.1.7.2",
        "pkcs7-signedData",
        "pkcs7-signedData",
        22,
    ),
    o(
        "1.2.840.113549.1.7.3",
        "pkcs7-envelopedData",
        "pkcs7-envelopedData",
        23,
    ),
    o(
        "1.2.840.113549.1.7.4",
        "pkcs7-signedAndEnvelopedData",
        "pkcs7-signedAndEnvelopedData",
        24,
    ),
    o(
        "1.2.840.113549.1.7.5",
        "pkcs7-digestData",
        "pkcs7-digestData",
        25,
    ),
    o(
        "1.2.840.113549.1.7.6",
        "pkcs7-encryptedData",
        "pkcs7-encryptedData",
        26,
    ),
    o(
        "1.2.840.113549.1.9.16.1.23",
        "id-smime-ct-authEnvelopedData",
        "id-smime-ct-authEnvelopedData",
        1059,
    ),
    // Ciphers (NIST CSOR, RFC 8018).
    o("1.2.840.113549.3.7", "DES-EDE3-CBC", "des-ede3-cbc", 44),
    o("1.3.14.3.2.7", "DES-CBC", "des-cbc", 31),
    o("1.2.840.113549.3.2", "RC2-CBC", "rc2-cbc", 37),
    o("2.16.840.1.101.3.4.1.2", "AES-128-CBC", "aes-128-cbc", 419),
    o("2.16.840.1.101.3.4.1.22", "AES-192-CBC", "aes-192-cbc", 423),
    o("2.16.840.1.101.3.4.1.42", "AES-256-CBC", "aes-256-cbc", 427),
    o(
        "2.16.840.1.101.3.4.1.6",
        "id-aes128-GCM",
        "aes-128-gcm",
        895,
    ),
    o(
        "2.16.840.1.101.3.4.1.26",
        "id-aes192-GCM",
        "aes-192-gcm",
        898,
    ),
    o(
        "2.16.840.1.101.3.4.1.46",
        "id-aes256-GCM",
        "aes-256-gcm",
        901,
    ),
    o(
        "2.16.840.1.101.3.4.1.5",
        "id-aes128-wrap",
        "id-aes128-wrap",
        788,
    ),
    o(
        "2.16.840.1.101.3.4.1.25",
        "id-aes192-wrap",
        "id-aes192-wrap",
        789,
    ),
    o(
        "2.16.840.1.101.3.4.1.45",
        "id-aes256-wrap",
        "id-aes256-wrap",
        790,
    ),
    o("1.2.840.113549.1.5.12", "PBKDF2", "PBKDF2", 69),
    o("1.2.840.113549.1.5.13", "PBES2", "PBES2", 161),
    o(
        "1.2.840.113549.1.5.3",
        "PBE-MD5-DES",
        "pbeWithMD5AndDES-CBC",
        10,
    ),
    o(
        "1.2.840.113549.1.5.10",
        "PBE-SHA1-DES",
        "pbeWithSHA1AndDES-CBC",
        170,
    ),
    o(
        "1.2.840.113549.1.12.1.3",
        "PBE-SHA1-3DES",
        "pbeWithSHA1And3-KeyTripleDES-CBC",
        146,
    ),
    o(
        "1.2.840.113549.1.12.1.6",
        "PBE-SHA1-RC2-40",
        "pbeWithSHA1And40BitRC2-CBC",
        149,
    ),
    o(
        "1.2.840.113549.1.12.1.5",
        "PBE-SHA1-RC2-128",
        "pbeWithSHA1And128BitRC2-CBC",
        148,
    ),
    o("1.2.840.113549.1.12.10.1.1", "keyBag", "keyBag", 150),
    o(
        "1.2.840.113549.1.12.10.1.2",
        "pkcs8ShroudedKeyBag",
        "pkcs8ShroudedKeyBag",
        151,
    ),
    o("1.2.840.113549.1.12.10.1.3", "certBag", "certBag", 152),
    o("1.2.840.113549.1.12.10.1.4", "crlBag", "crlBag", 153),
    o("1.2.840.113549.1.12.10.1.5", "secretBag", "secretBag", 154),
    o(
        "1.2.840.113549.1.12.10.1.6",
        "safeContentsBag",
        "safeContentsBag",
        155,
    ),
];

pub fn by_oid(oid: &str) -> Option<&'static Obj> {
    OBJS.iter().find(|x| x.oid == oid)
}

/// By short or long name (case-sensitive first, then ignoring case).
pub fn by_name(name: &str) -> Option<&'static Obj> {
    OBJS.iter()
        .find(|x| x.sn == name || x.ln == name)
        .or_else(|| {
            OBJS.iter()
                .find(|x| x.sn.eq_ignore_ascii_case(name) || x.ln.eq_ignore_ascii_case(name))
        })
}

pub fn by_nid(nid: i64) -> Option<&'static Obj> {
    if nid <= 0 {
        return None;
    }
    OBJS.iter().find(|x| x.nid == nid)
}

/// The short name of an OID, or the dotted text.
pub fn sn(oid: &str) -> String {
    by_oid(oid).map_or_else(|| oid.to_string(), |x| x.sn.to_string())
}

/// The long name of an OID, or the dotted text.
pub fn ln(oid: &str) -> String {
    by_oid(oid).map_or_else(|| oid.to_string(), |x| x.ln.to_string())
}

/// Resolves a name or dotted OID to a dotted OID.
pub fn resolve(name: &str) -> Option<String> {
    if let Some(o) = by_name(name) {
        return Some(o.oid.to_string());
    }
    if crate::der::oid_from_string(name).is_some() {
        return Some(name.to_string());
    }
    None
}

/// Well-known dotted OIDs used across the crate.
pub mod ids {
    pub const RSA: &str = "1.2.840.113549.1.1.1";
    pub const RSA_PSS: &str = "1.2.840.113549.1.1.10";
    pub const DSA: &str = "1.2.840.10040.4.1";
    pub const DH: &str = "1.2.840.113549.1.3.1";
    pub const DHX: &str = "1.2.840.10046.2.1";
    pub const EC: &str = "1.2.840.10045.2.1";
    pub const X25519: &str = "1.3.101.110";
    pub const X448: &str = "1.3.101.111";
    pub const ED25519: &str = "1.3.101.112";
    pub const ED448: &str = "1.3.101.113";
    pub const SM2: &str = "1.2.156.10197.1.301";
    pub const DATA: &str = "1.2.840.113549.1.7.1";
    pub const SIGNED_DATA: &str = "1.2.840.113549.1.7.2";
    pub const ENVELOPED_DATA: &str = "1.2.840.113549.1.7.3";
    pub const ENCRYPTED_DATA: &str = "1.2.840.113549.1.7.6";
    pub const AUTH_ENVELOPED_DATA: &str = "1.2.840.113549.1.9.16.1.23";
    pub const CONTENT_TYPE: &str = "1.2.840.113549.1.9.3";
    pub const MESSAGE_DIGEST: &str = "1.2.840.113549.1.9.4";
    pub const SIGNING_TIME: &str = "1.2.840.113549.1.9.5";
    pub const SMIME_CAPS: &str = "1.2.840.113549.1.9.15";
    pub const EXT_REQ: &str = "1.2.840.113549.1.9.14";
    pub const CHALLENGE_PASSWORD: &str = "1.2.840.113549.1.9.7";
    pub const FRIENDLY_NAME: &str = "1.2.840.113549.1.9.20";
    pub const LOCAL_KEY_ID: &str = "1.2.840.113549.1.9.21";
    pub const X509_CERT_TYPE: &str = "1.2.840.113549.1.9.22.1";
    pub const PBES2: &str = "1.2.840.113549.1.5.13";
    pub const PBKDF2: &str = "1.2.840.113549.1.5.12";
    pub const EMAIL: &str = "1.2.840.113549.1.9.1";
    pub const CN: &str = "2.5.4.3";
}
