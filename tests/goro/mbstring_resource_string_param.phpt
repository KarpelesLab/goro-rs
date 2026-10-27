--TEST--
mbstring string parameters reject resources with a TypeError
--FILE--
<?php
$r = STDERR;
foreach ([
  fn() => mb_encode_mimeheader("x", $r),
  fn() => mb_encode_mimeheader("x", "UTF-8", $r),
  fn() => mb_encode_mimeheader($r),
  fn() => mb_strlen($r),
  fn() => mb_substr("abc", 0, 1, $r),
  fn() => mb_convert_case("abc", MB_CASE_UPPER, $r),
  fn() => mb_strtoupper($r),
  fn() => mb_str_pad("a", 3, $r),
  fn() => mb_convert_encoding("a", $r),
  fn() => mb_check_encoding($r),
  fn() => mb_strwidth($r),
  fn() => mb_substr_count("a", $r),
] as $f) {
  try { var_dump($f()); } catch (Throwable $e) { echo get_class($e), ": ", $e->getMessage(), "\n"; }
}
--EXPECT--
TypeError: mb_encode_mimeheader(): Argument #2 ($charset) must be of type string, resource given
TypeError: mb_encode_mimeheader(): Argument #3 ($transfer_encoding) must be of type string, resource given
TypeError: mb_encode_mimeheader(): Argument #1 ($string) must be of type string, resource given
TypeError: mb_strlen(): Argument #1 ($string) must be of type string, resource given
TypeError: mb_substr(): Argument #4 ($encoding) must be of type ?string, resource given
TypeError: mb_convert_case(): Argument #3 ($encoding) must be of type ?string, resource given
TypeError: mb_strtoupper(): Argument #1 ($string) must be of type string, resource given
TypeError: mb_str_pad(): Argument #3 ($pad_string) must be of type string, resource given
TypeError: mb_convert_encoding(): Argument #2 ($to_encoding) must be of type string, resource given
TypeError: mb_check_encoding(): Argument #1 ($value) must be of type array|string|null, resource given
TypeError: mb_strwidth(): Argument #1 ($string) must be of type string, resource given
TypeError: mb_substr_count(): Argument #2 ($needle) must be of type string, resource given
