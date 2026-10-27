--TEST--
mb_encode_mimeheader() in strict mode: $charset/$transfer_encoding are Z_PARAM_STR (type string, null rejected)
--FILE--
<?php declare(strict_types=1);
function t($c) { try { var_dump($c()); } catch (Throwable $e) { echo get_class($e), ': ', $e->getMessage(), "\n"; } }
t(fn() => mb_encode_mimeheader("x", null));
t(fn() => mb_encode_mimeheader("x", 5));
t(fn() => mb_encode_mimeheader("x", "UTF-8", 1.5));
t(fn() => mb_encode_mimeheader("x", "UTF-8", null));
t(fn() => mb_encode_mimeheader("x", []));
t(fn() => mb_encode_mimeheader("x", "UTF-8", "B", 5));
t(fn() => mb_encode_mimeheader(5));
t(fn() => mb_encode_mimeheader("x", "UTF-8", "B", "\n", "1"));
t(fn() => mb_encode_mimeheader("é", "UTF-8", "Q"));
--EXPECT--
TypeError: mb_encode_mimeheader(): Argument #2 ($charset) must be of type string, null given
TypeError: mb_encode_mimeheader(): Argument #2 ($charset) must be of type string, int given
TypeError: mb_encode_mimeheader(): Argument #3 ($transfer_encoding) must be of type string, float given
TypeError: mb_encode_mimeheader(): Argument #3 ($transfer_encoding) must be of type string, null given
TypeError: mb_encode_mimeheader(): Argument #2 ($charset) must be of type string, array given
TypeError: mb_encode_mimeheader(): Argument #4 ($newline) must be of type string, int given
TypeError: mb_encode_mimeheader(): Argument #1 ($string) must be of type string, int given
TypeError: mb_encode_mimeheader(): Argument #5 ($indent) must be of type int, string given
string(18) "=?UTF-8?Q?=C3=A9?="
