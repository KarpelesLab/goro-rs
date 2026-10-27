--TEST--
parse_ini_string(): an unterminated single-quoted string is an unexpected end of file
--FILE--
<?php
foreach (["foo = '\nbar = bar\n", "foo = '\n", "foo = 'x\n", "a = \"x\nb = 2\n", "foo = 'abc'\n"] as $s) {
    var_dump(@parse_ini_string($s, true));
    var_dump(@parse_ini_string($s, true, INI_SCANNER_RAW));
}

?>
--EXPECT--
bool(false)
array(2) {
  ["foo"]=>
  string(1) "'"
  ["bar"]=>
  string(3) "bar"
}
bool(false)
array(1) {
  ["foo"]=>
  string(1) "'"
}
bool(false)
array(1) {
  ["foo"]=>
  string(2) "'x"
}
bool(false)
array(2) {
  ["a"]=>
  string(2) ""x"
  ["b"]=>
  string(1) "2"
}
array(1) {
  ["foo"]=>
  string(3) "abc"
}
array(1) {
  ["foo"]=>
  string(5) "'abc'"
}
