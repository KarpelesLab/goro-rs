--TEST--
use_soap_error_handler(true): errors are not displayed outside of a SoapClient call
--FILE--
<?php
trigger_error("before");
var_dump(use_soap_error_handler(true));
trigger_error("after");
echo $undef;
var_dump(utf8_decode("a"));
use_soap_error_handler(false);
trigger_error("after2");
--EXPECTF--

Notice: before in %s on line 2
bool(false)
string(1) "a"

Notice: after2 in %s on line 8
