--TEST--
iconv_mime_decode(): "Unknown error (%d)" prints the errno the last failing iconv call left
--FILE--
<?php
// a malformed quoted-printable word fails without touching errno, so the
// notice shows whatever iconv()/iconv_open() last set: 0 at first, then
// EILSEQ (84) after an unconvertible character, EINVAL (22) after an
// incomplete one or an unknown charset
var_dump(iconv_mime_decode("Subject: =?UTF-8?Q?=ZZ?=", 0, "UTF-8"));
var_dump(iconv("UTF-8", "ASCII", "é"));
var_dump(iconv_mime_decode("=?ISO-8859-1?Q?=ZZ?=", 0, "ISO-8859-1"));
var_dump(iconv("UTF-8", "UTF-16", "a\xc3"));
var_dump(iconv_mime_decode_headers("Subject: =?ISO-8859-1?Q?=Z?=", 0, "UTF-8"));
?>
--EXPECTF--
Notice: iconv_mime_decode(): Unknown error (0) in %s on line %d
bool(false)

Notice: iconv(): Detected an illegal character in input string in %s on line %d
bool(false)

Notice: iconv_mime_decode(): Unknown error (84) in %s on line %d
bool(false)

Notice: iconv(): Detected an incomplete multibyte character in input string in %s on line %d
bool(false)

Notice: iconv_mime_decode_headers(): Unknown error (22) in %s on line %d
bool(false)
