//! `Uri\WhatWg\UrlValidationErrorType` and the error list of a parse.

/// The cases of `Uri\WhatWg\UrlValidationErrorType`, in declaration order
/// (observed on PHP 8.5 via reflection; the URL Standard's validation
/// errors).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ErrorType {
    DomainToAscii,
    DomainToUnicode,
    DomainInvalidCodePoint,
    HostInvalidCodePoint,
    Ipv4EmptyPart,
    Ipv4TooManyParts,
    Ipv4NonNumericPart,
    Ipv4NonDecimalPart,
    Ipv4OutOfRangePart,
    Ipv6Unclosed,
    Ipv6InvalidCompression,
    Ipv6TooManyPieces,
    Ipv6MultipleCompression,
    Ipv6InvalidCodePoint,
    Ipv6TooFewPieces,
    Ipv4InIpv6TooManyPieces,
    Ipv4InIpv6InvalidCodePoint,
    Ipv4InIpv6OutOfRangePart,
    Ipv4InIpv6TooFewParts,
    InvalidUrlUnit,
    SpecialSchemeMissingFollowingSolidus,
    MissingSchemeNonRelativeUrl,
    /// (sic: the PHP case name)
    InvalidReverseSoldius,
    InvalidCredentials,
    HostMissing,
    PortOutOfRange,
    PortInvalid,
    FileInvalidWindowsDriveLetter,
    FileInvalidWindowsDriveLetterHost,
}

impl ErrorType {
    pub const ALL: [ErrorType; 29] = [
        ErrorType::DomainToAscii,
        ErrorType::DomainToUnicode,
        ErrorType::DomainInvalidCodePoint,
        ErrorType::HostInvalidCodePoint,
        ErrorType::Ipv4EmptyPart,
        ErrorType::Ipv4TooManyParts,
        ErrorType::Ipv4NonNumericPart,
        ErrorType::Ipv4NonDecimalPart,
        ErrorType::Ipv4OutOfRangePart,
        ErrorType::Ipv6Unclosed,
        ErrorType::Ipv6InvalidCompression,
        ErrorType::Ipv6TooManyPieces,
        ErrorType::Ipv6MultipleCompression,
        ErrorType::Ipv6InvalidCodePoint,
        ErrorType::Ipv6TooFewPieces,
        ErrorType::Ipv4InIpv6TooManyPieces,
        ErrorType::Ipv4InIpv6InvalidCodePoint,
        ErrorType::Ipv4InIpv6OutOfRangePart,
        ErrorType::Ipv4InIpv6TooFewParts,
        ErrorType::InvalidUrlUnit,
        ErrorType::SpecialSchemeMissingFollowingSolidus,
        ErrorType::MissingSchemeNonRelativeUrl,
        ErrorType::InvalidReverseSoldius,
        ErrorType::InvalidCredentials,
        ErrorType::HostMissing,
        ErrorType::PortOutOfRange,
        ErrorType::PortInvalid,
        ErrorType::FileInvalidWindowsDriveLetter,
        ErrorType::FileInvalidWindowsDriveLetterHost,
    ];

    /// The enum case name.
    pub fn name(self) -> &'static str {
        match self {
            ErrorType::DomainToAscii => "DomainToAscii",
            ErrorType::DomainToUnicode => "DomainToUnicode",
            ErrorType::DomainInvalidCodePoint => "DomainInvalidCodePoint",
            ErrorType::HostInvalidCodePoint => "HostInvalidCodePoint",
            ErrorType::Ipv4EmptyPart => "Ipv4EmptyPart",
            ErrorType::Ipv4TooManyParts => "Ipv4TooManyParts",
            ErrorType::Ipv4NonNumericPart => "Ipv4NonNumericPart",
            ErrorType::Ipv4NonDecimalPart => "Ipv4NonDecimalPart",
            ErrorType::Ipv4OutOfRangePart => "Ipv4OutOfRangePart",
            ErrorType::Ipv6Unclosed => "Ipv6Unclosed",
            ErrorType::Ipv6InvalidCompression => "Ipv6InvalidCompression",
            ErrorType::Ipv6TooManyPieces => "Ipv6TooManyPieces",
            ErrorType::Ipv6MultipleCompression => "Ipv6MultipleCompression",
            ErrorType::Ipv6InvalidCodePoint => "Ipv6InvalidCodePoint",
            ErrorType::Ipv6TooFewPieces => "Ipv6TooFewPieces",
            ErrorType::Ipv4InIpv6TooManyPieces => "Ipv4InIpv6TooManyPieces",
            ErrorType::Ipv4InIpv6InvalidCodePoint => "Ipv4InIpv6InvalidCodePoint",
            ErrorType::Ipv4InIpv6OutOfRangePart => "Ipv4InIpv6OutOfRangePart",
            ErrorType::Ipv4InIpv6TooFewParts => "Ipv4InIpv6TooFewParts",
            ErrorType::InvalidUrlUnit => "InvalidUrlUnit",
            ErrorType::SpecialSchemeMissingFollowingSolidus => {
                "SpecialSchemeMissingFollowingSolidus"
            }
            ErrorType::MissingSchemeNonRelativeUrl => "MissingSchemeNonRelativeUrl",
            ErrorType::InvalidReverseSoldius => "InvalidReverseSoldius",
            ErrorType::InvalidCredentials => "InvalidCredentials",
            ErrorType::HostMissing => "HostMissing",
            ErrorType::PortOutOfRange => "PortOutOfRange",
            ErrorType::PortInvalid => "PortInvalid",
            ErrorType::FileInvalidWindowsDriveLetter => "FileInvalidWindowsDriveLetter",
            ErrorType::FileInvalidWindowsDriveLetterHost => "FileInvalidWindowsDriveLetterHost",
        }
    }

    /// The case for a name (`UrlValidationErrorType::from` style lookups
    /// by the engine).
    pub fn from_name(name: &str) -> Option<ErrorType> {
        ErrorType::ALL.iter().copied().find(|t| t.name() == name)
    }

    /// The `failure` flag PHP gives errors of this type: the URL
    /// Standard's "Failure" column for the type, whether or not the parse
    /// failed (observed on PHP 8.5: `http://192.0x00A80001` succeeds with an
    /// Ipv4OutOfRangePart error whose failure is true).
    pub fn is_failure(self) -> bool {
        !matches!(
            self,
            ErrorType::DomainToUnicode
                | ErrorType::Ipv4EmptyPart
                | ErrorType::Ipv4NonDecimalPart
                | ErrorType::InvalidUrlUnit
                | ErrorType::SpecialSchemeMissingFollowingSolidus
                | ErrorType::InvalidReverseSoldius
                | ErrorType::InvalidCredentials
                | ErrorType::FileInvalidWindowsDriveLetter
                | ErrorType::FileInvalidWindowsDriveLetterHost
        )
    }
}

/// A `Uri\WhatWg\UrlValidationError`: `$type`, `$context` (the input from
/// the point the error was found, cut at its first NUL byte, as PHP
/// reports it) and `$failure` ([`ErrorType::is_failure`]).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ValidationError {
    pub kind: ErrorType,
    pub context: Vec<u8>,
}

impl ValidationError {
    pub fn failure(&self) -> bool {
        self.kind.is_failure()
    }
}

/// Errors in the order the parser met them.
#[derive(Clone, Debug, Default)]
pub(crate) struct Errors {
    pub list: Vec<ValidationError>,
    /// The IPv4 errors of the last host parsed, for [`Errors::fix_ipv4`].
    pub ipv4: Option<Ipv4Errors>,
}

/// Where the IPv4 errors of a host point.
#[derive(Clone, Debug, Default)]
pub(crate) struct Ipv4Errors {
    /// The host as the IPv4 parser read it.
    pub domain: Vec<u8>,
    /// Whether it came straight from the input (no percent-decoding or
    /// IDNA mapping).
    pub plain: bool,
    /// (index in `list`, offset in `domain`) of each error.
    pub at: Vec<(usize, usize)>,
}

impl Errors {
    #[cold]
    pub(crate) fn push(&mut self, kind: ErrorType, buf: &[u8], pos: usize) {
        let rest = buf.get(pos..).unwrap_or(&[]);
        let end = rest.iter().position(|&c| c == 0).unwrap_or(rest.len());
        self.list.push(ValidationError {
            kind,
            context: rest[..end].to_vec(),
        });
    }

    /// PHP reads the contexts of the IPv4 errors of a successfully parsed
    /// host from the host's buffer after freeing it, so the next string of
    /// the same allocation size class (8 bytes: a host of up to 6 bytes,
    /// a string of up to 7) written for the URL, the path, else the query,
    /// else the fragment, shows through (observed on PHP 8.5:
    /// `http://256/x` reports its Ipv4OutOfRangePart error with the
    /// context "/x", `http://256/abcdefg` with "256"). Only modelled for
    /// hosts taken from the input as they are, and for the first parse of
    /// a process; PHP's result depends on its allocator's history.
    pub(crate) fn fix_ipv4(&mut self, strings: &[&[u8]]) {
        let Some(v) = self.ipv4.take() else { return };
        if !v.plain {
            return;
        }
        let class = |n: usize| n.div_ceil(8) * 8;
        let host_class = class(v.domain.len() + 2);
        let Some(s) = strings.iter().find(|s| class(s.len() + 1) == host_class) else {
            return;
        };
        let mut block = v.domain.clone();
        if block.len() < s.len() + 1 {
            block.resize(s.len() + 1, 0);
        }
        block[..s.len()].copy_from_slice(s);
        block[s.len()] = 0;
        for &(i, off) in &v.at {
            let rest = &block[off.min(block.len())..];
            let end = rest.iter().position(|&c| c == 0).unwrap_or(rest.len());
            self.list[i].context = rest[..end].to_vec();
        }
    }

    /// The first error (in parse order) whose type is a failure type.
    pub(crate) fn first_failure(&self) -> Option<ErrorType> {
        self.list.iter().find(|e| e.failure()).map(|e| e.kind)
    }
}
