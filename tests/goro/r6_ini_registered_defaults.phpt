--TEST--
INI entries registered with NULL or core defaults exist for ini_get()/ini_set()
--FILE--
<?php
// Registered entries (several with a NULL default) exist for ini_get()
// and can be set.
foreach (['arg_separator.input', 'assert.bail', 'assert.callback', 'assert.warning', 'fiber.stack_size', 'mail.force_extra_parameters', 'mail.log', 'openssl.cafile', 'openssl.capath', 'openssl.libctx', 'syslog.facility', 'syslog.filter', 'syslog.ident', 'request_order'] as $k) {
    echo "$k: ", var_export(ini_get($k), true), "\n";
}
var_dump(ini_set('fiber.stack_size', '1M'), ini_get('fiber.stack_size'));
var_dump(@ini_set('assert.callback', 'strlen'), ini_get('assert.callback'));
var_dump(ini_set('syslog.filter', 'raw'), ini_get('syslog.filter'));
--EXPECT--
arg_separator.input: '&'
assert.bail: '0'
assert.callback: ''
assert.warning: '1'
fiber.stack_size: ''
mail.force_extra_parameters: ''
mail.log: ''
openssl.cafile: ''
openssl.capath: ''
openssl.libctx: 'custom'
syslog.facility: 'LOG_USER'
syslog.filter: 'no-ctrl'
syslog.ident: 'php'
request_order: ''
string(0) ""
string(2) "1M"
string(0) ""
string(6) "strlen"
string(7) "no-ctrl"
string(3) "raw"
