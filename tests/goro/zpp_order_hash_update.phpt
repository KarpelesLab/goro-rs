--TEST--
hash_update() and mb_ereg_replace_callback() parse their parameters in order
--FILE--
<?php
// zpp parses the parameters in order: $context's error comes first
try { hash_update("abc", null); } catch (TypeError $e) { echo $e->getMessage(), "\n"; }
try { hash_update("abc", NAN); } catch (TypeError $e) { echo $e->getMessage(), "\n"; }
$c = hash_init("md5");
var_dump(hash_update($c, null));
var_dump(hash_final($c));
try { mb_ereg_replace_callback("a", 1, null); } catch (TypeError $e) { echo $e->getMessage(), "\n"; }
var_dump(mb_ereg_replace_callback("a", fn($m) => "b", null));
--EXPECTF--
hash_update(): Argument #1 ($context) must be of type HashContext, string given
hash_update(): Argument #1 ($context) must be of type HashContext, string given

Deprecated: hash_update(): Passing null to parameter #2 ($data) of type string is deprecated in %s on line 6
bool(true)
string(32) "d41d8cd98f00b204e9800998ecf8427e"
mb_ereg_replace_callback(): Argument #2 ($callback) must be a valid callback, no array or string given

Deprecated: mb_ereg_replace_callback(): Passing null to parameter #3 ($string) of type string is deprecated in %s on line 9
string(0) ""
