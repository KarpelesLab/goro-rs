//! Stubs: Phar::createDefaultStub() (its text as PHP 8.5 outputs it,
//! test phar_createdefaultstub.phpt) and the normalization of the stubs
//! given to Phar::setStub().

const DEFAULT: &[u8] = include_bytes!("default_stub.php.txt");

/// Phar::createDefaultStub($index, $webIndex): the default stub running
/// `index` (CLI) and `web` (web SAPIs), "index.php" when none.
pub fn default_stub(index: Option<&[u8]>, web: Option<&[u8]>) -> Result<Vec<u8>, String> {
    let index = index.unwrap_or(b"index.php");
    let web = web.unwrap_or(b"index.php");
    if index.len() > 400 {
        return Err(format!(
            "Illegal filename passed in for stub creation, was {} characters long, and only 400 or less is allowed",
            index.len()
        ));
    }
    if web.len() > 400 {
        return Err(format!(
            "Illegal web filename passed in for stub creation, was {} characters long, and only 400 or less is allowed",
            web.len()
        ));
    }
    let s = replace_once(
        DEFAULT,
        b"$web = 'index.php';",
        &[b"$web = '", web, b"';"].concat(),
    );
    let s = replace_once(
        &s,
        b"const START = 'index.php';",
        &[b"const START = '", index, b"';"].concat(),
    );
    // LEN is the stub's length plus 2 (6643 for the 6641-byte default)
    let len = s.len() + 2;
    Ok(replace_once(
        &s,
        b"const LEN = 6643;",
        format!("const LEN = {len};").as_bytes(),
    ))
}

fn replace_once(hay: &[u8], from: &[u8], to: &[u8]) -> Vec<u8> {
    match find(hay, from) {
        Some(p) => [&hay[..p], to, &hay[p + from.len()..]].concat(),
        None => hay.to_vec(),
    }
}

pub(crate) fn find(hay: &[u8], needle: &[u8]) -> Option<usize> {
    if needle.is_empty() || hay.len() < needle.len() {
        return None;
    }
    hay.windows(needle.len()).position(|w| w == needle)
}

/// The position after the first `__HALT_COMPILER();` (any case).
pub fn find_halt(data: &[u8]) -> Option<usize> {
    const H: &[u8] = b"__halt_compiler();";
    if data.len() < H.len() {
        return None;
    }
    data.windows(H.len())
        .position(|w| w.eq_ignore_ascii_case(H))
        .map(|p| p + H.len())
}

/// A stub as Phar::setStub() stores it: up to `__HALT_COMPILER();`, then
/// " ?>\r\n" (observed on PHP 8.5); none without the call.
pub fn normalize(stub: &[u8]) -> Option<Vec<u8>> {
    let end = find_halt(stub)?;
    let mut out = stub[..end].to_vec();
    out.extend_from_slice(b" ?>\r\n");
    Some(out)
}

/// The end of a stub in a phar file: after `__HALT_COMPILER();`, an
/// optional " ?>" and one line break.
pub fn stub_end(data: &[u8], halt: usize) -> usize {
    let mut p = halt;
    if data[p..].starts_with(b" ?>") {
        p += 3;
        if data[p..].starts_with(b"\r\n") {
            p += 2;
        } else if data[p..].starts_with(b"\n") {
            p += 1;
        }
    }
    p
}

#[cfg(test)]
mod tests {
    #[test]
    fn lens() {
        let s = super::default_stub(None, None).unwrap();
        assert_eq!(s.len(), 6641);
        let s = super::default_stub(Some(b"my/custom/thingy.php"), Some(b"the/web.php")).unwrap();
        assert!(super::find(&s, b"const LEN = 6656;").is_some());
    }
}
