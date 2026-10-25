--TEST--
mb_encode_mimeheader() argument parsing order, mb_substitute_character() numeric strings
--FILE--
<?php
function t($f) { try { var_dump($f()); } catch (\Throwable $e) { echo get_class($e), ": ", $e->getMessage(), "\n"; } }
// every argument is parsed before the charset is looked up
t(fn() => mb_encode_mimeheader("x", "nope", []));
t(fn() => mb_encode_mimeheader("x", "nope", "B", new stdClass));
t(fn() => mb_encode_mimeheader("x", "nope", "B", "\r\n", "abc"));
t(fn() => mb_encode_mimeheader("x", "BASE64", "B", "\r\n", 1));
// a numeric string is no codepoint
t(fn() => mb_substitute_character("12"));
t(fn() => mb_substitute_character(" 63"));
t(fn() => mb_substitute_character(63));
t(fn() => mb_substitute_character());
--EXPECTF--
TypeError: mb_encode_mimeheader(): Argument #3 ($transfer_encoding) must be of type string, array given
TypeError: mb_encode_mimeheader(): Argument #4 ($newline) must be of type string, stdClass given
TypeError: mb_encode_mimeheader(): Argument #5 ($indent) must be of type int, string given

Deprecated: mb_encode_mimeheader(): Handling Base64 via mbstring is deprecated; use base64_encode/base64_decode instead in %s on line 7
string(1) "x"
ValueError: mb_substitute_character(): Argument #1 ($substitute_character) must be "none", "long", "entity" or a valid codepoint
ValueError: mb_substitute_character(): Argument #1 ($substitute_character) must be "none", "long", "entity" or a valid codepoint
bool(true)
int(63)
