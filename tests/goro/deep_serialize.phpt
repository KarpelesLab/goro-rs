--TEST--
serialize() checks the C stack like php-src (~5.4k levels); unserialize() of deep data does not overflow
--FILE--
<?php
function mk($n) { $a = []; for ($i = 0; $i < $n; $i++) $a = [$a]; return $a; }
function mko($n) { $o = new stdClass; for ($i = 0; $i < $n; $i++) { $p = new stdClass; $p->c = $o; $o = $p; } return $o; }
// php-src gets ~5.4k levels deep before running out of C stack
var_dump(strlen(serialize(mk(5000))));
var_dump(strlen(serialize(mko(5000))));
foreach ([mk(8000), mko(8000)] as $v) {
    try {
        serialize($v);
    } catch (Error $e) {
        echo get_class($e), ": ", $e->getMessage(), "\n";
    }
}
// unserialize() does not check the stack (bounded by max_depth)
$n = 20000;
$s = str_repeat('a:1:{i:0;', $n) . 'a:0:{}' . str_repeat('}', $n);
var_dump(unserialize($s, ['max_depth' => 0]) === mk($n));
$s = str_repeat('O:8:"stdClass":1:{s:1:"c";', $n) . 'N;' . str_repeat('}', $n);
$u = unserialize($s, ['max_depth' => 0]);
for ($d = 0; is_object($u); $d++) $u = $u->c;
var_dump($d);
--EXPECT--
int(50006)
int(135019)
Error: Maximum call stack size of 8339456 bytes (zend.max_allowed_stack_size - zend.reserved_stack_size) reached. Infinite recursion?
Error: Maximum call stack size of 8339456 bytes (zend.max_allowed_stack_size - zend.reserved_stack_size) reached. Infinite recursion?
bool(true)
int(20000)
