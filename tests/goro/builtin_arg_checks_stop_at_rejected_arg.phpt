--TEST--
A builtin's argument conversions stop at an argument its zpp rejects; parse_ini_string() scanner mode as a C int
--FILE--
<?php
try { mb_strlen(new stdClass, NAN); } catch (Throwable $e) { echo $e->getMessage(), "\n"; }
try { mb_encode_mimeheader(new stdClass, NAN); } catch (Throwable $e) { echo $e->getMessage(), "\n"; }
try { mb_encode_mimeheader(new stdClass, NAN, "x", "y", 1); } catch (Throwable $e) { echo $e->getMessage(), "\n"; }
try { strpos(new stdClass, NAN); } catch (Throwable $e) { echo $e->getMessage(), "\n"; }
try { str_pad(new stdClass, 1, NAN); } catch (Throwable $e) { echo $e->getMessage(), "\n"; }
try { mb_str_pad(new stdClass, 1, NAN); } catch (Throwable $e) { echo $e->getMessage(), "\n"; }
try { var_dump(parse_ini_string("a=1", false, PHP_INT_MIN), parse_ini_string("a=1", false, 4294967296 + 1)); } catch (Throwable $e) { echo $e->getMessage(), "\n"; }
--EXPECT--
mb_strlen(): Argument #1 ($string) must be of type string, stdClass given
mb_encode_mimeheader(): Argument #1 ($string) must be of type string, stdClass given
mb_encode_mimeheader(): Argument #1 ($string) must be of type string, stdClass given
strpos(): Argument #1 ($haystack) must be of type string, stdClass given
str_pad(): Argument #1 ($string) must be of type string, stdClass given
mb_str_pad(): Argument #1 ($string) must be of type string, stdClass given
array(1) {
  ["a"]=>
  string(1) "1"
}
array(1) {
  ["a"]=>
  string(1) "1"
}
