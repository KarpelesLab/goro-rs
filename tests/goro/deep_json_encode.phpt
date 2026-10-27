--TEST--
json_encode() of deeply nested values: the C stack budget of php-src (~21.6k levels)
--FILE--
<?php
function mk($n) { $a = []; for ($i = 0; $i < $n; $i++) $a = [$a]; return $a; }
function mko($n) { $o = new stdClass; for ($i = 0; $i < $n; $i++) { $p = new stdClass; $p->c = $o; $o = $p; } return $o; }
// php-src gets ~21.6k levels deep before running out of C stack
var_dump(strlen(json_encode(mk(10000), 0, 20000)));
var_dump(strlen(json_encode(mko(10000), 0, 20000)));
var_dump(json_encode(mk(30000), 0, 40000), json_last_error_msg());
var_dump(json_encode(mk(30000), JSON_PARTIAL_OUTPUT_ON_ERROR, 40000) !== false);
try {
    json_encode(mko(30000), JSON_THROW_ON_ERROR, 40000);
} catch (JsonException $e) {
    echo get_class($e), ": ", $e->getMessage(), "\n";
}
// the depth limit still applies first
var_dump(json_encode(mk(100), 0, 50), json_last_error_msg());
--EXPECT--
int(20002)
int(60002)
bool(false)
string(28) "Maximum stack depth exceeded"
bool(true)
JsonException: Maximum stack depth exceeded
bool(false)
string(28) "Maximum stack depth exceeded"
