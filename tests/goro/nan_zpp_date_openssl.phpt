--TEST--
NAN given to string/bool parameters of ext/date and ext/openssl functions warns
--FILE--
<?php
// NAN coerced to a string/bool parameter warns (zpp of ext/date, ext/openssl)
var_dump(timezone_open(NAN));
var_dump(@strftime(NAN, 0));
var_dump(@gmstrftime(NAN, 0));
var_dump(openssl_x509_parse(NAN));
var_dump(openssl_x509_read(NAN));
var_dump(openssl_x509_fingerprint(NAN));
--EXPECTF--

Warning: unexpected NAN value was coerced to string in %s on line 3

Warning: timezone_open(): Unknown or bad timezone (NAN) in %s on line 3
bool(false)
string(3) "NAN"
string(3) "NAN"

Warning: unexpected NAN value was coerced to string in %s on line 6
bool(false)

Warning: unexpected NAN value was coerced to string in %s on line 7

Warning: openssl_x509_read(): X.509 Certificate cannot be retrieved in %s on line 7
bool(false)

Warning: unexpected NAN value was coerced to string in %s on line 8

Warning: openssl_x509_fingerprint(): X.509 Certificate cannot be retrieved in %s on line 8
bool(false)
