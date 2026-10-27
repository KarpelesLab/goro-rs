--TEST--
mb_encode_mimeheader(): $newline / $indent are parsed after the charset arguments
--FILE--
<?php
try { mb_encode_mimeheader("x", [], "B", "\n", -2.5); } catch (Throwable $e) { echo get_class($e), ": ", $e->getMessage(), "\n"; }
try { mb_encode_mimeheader("x", "UTF-8", [], "\n", -2.5); } catch (Throwable $e) { echo get_class($e), ": ", $e->getMessage(), "\n"; }
try { mb_encode_mimeheader("x", "UTF-8", "B", [], 1.5); } catch (Throwable $e) { echo get_class($e), ": ", $e->getMessage(), "\n"; }
try { mb_encode_mimeheader([], "UTF-8", "B", "\n", 1.5); } catch (Throwable $e) { echo get_class($e), ": ", $e->getMessage(), "\n"; }
try { var_dump(mb_encode_mimeheader("x", "UTF-8", "B", "\n", 1.5)); } catch (Throwable $e) { echo get_class($e), ": ", $e->getMessage(), "\n"; }
try { var_dump(mb_encode_mimeheader("x", "bogus", "B", "\n", 1.5)); } catch (Throwable $e) { echo get_class($e), ": ", $e->getMessage(), "\n"; }
foreach ([[null, 1], ["\n", null], [5, "abc"], ["\n", "5"], ["\n", "5.5"], [[], 1], ["\n", []], ["\n", true], [1.5, 2], ["\n", NAN], ["\n", 1e30], ["\n", "1e3"], ["\n", " 5"], ["\n", "5 "]] as [$nl, $ind]) {
  try { var_dump(mb_encode_mimeheader("x", "UTF-8", "B", $nl, $ind)); } catch (Throwable $e) { echo get_class($e), ": ", $e->getMessage(), "\n"; }
}
--EXPECTF--
TypeError: mb_encode_mimeheader(): Argument #2 ($charset) must be of type string, array given
TypeError: mb_encode_mimeheader(): Argument #3 ($transfer_encoding) must be of type string, array given
TypeError: mb_encode_mimeheader(): Argument #4 ($newline) must be of type string, array given
TypeError: mb_encode_mimeheader(): Argument #1 ($string) must be of type string, array given

Deprecated: Implicit conversion from float 1.5 to int loses precision in %s on line 6
string(1) "x"

Deprecated: Implicit conversion from float 1.5 to int loses precision in %s on line 7
ValueError: mb_encode_mimeheader(): Argument #2 ($charset) must be a valid encoding, "bogus" given

Deprecated: mb_encode_mimeheader(): Passing null to parameter #4 ($newline) of type string is deprecated in %s on line 9
string(1) "x"

Deprecated: mb_encode_mimeheader(): Passing null to parameter #5 ($indent) of type int is deprecated in %s on line 9
string(1) "x"
TypeError: mb_encode_mimeheader(): Argument #5 ($indent) must be of type int, string given
string(1) "x"

Deprecated: Implicit conversion from float-string "5.5" to int loses precision in %s on line 9
string(1) "x"
TypeError: mb_encode_mimeheader(): Argument #4 ($newline) must be of type string, array given
TypeError: mb_encode_mimeheader(): Argument #5 ($indent) must be of type int, array given
string(1) "x"
string(1) "x"
TypeError: mb_encode_mimeheader(): Argument #5 ($indent) must be of type int, float given
TypeError: mb_encode_mimeheader(): Argument #5 ($indent) must be of type int, float given
string(1) "x"
string(1) "x"
string(1) "x"
