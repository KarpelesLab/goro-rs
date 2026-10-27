--TEST--
var_dump(), debug_zval_dump(), var_export(), print_r() of deeply nested values
--INI--
zend.max_allowed_stack_size=256K
memory_limit=512M
--FILE--
<?php
// Deeply nested values dumped in a fiber (whose stack is small): the
// dumpers recurse without running out of stack.
function mk($n) { $a = []; for ($i = 0; $i < $n; $i++) $a = [$a]; return $a; }
function mko($n) { $o = new stdClass; for ($i = 0; $i < $n; $i++) { $p = new stdClass; $p->c = $o; $o = $p; } return $o; }
function dump($fn, $v) {
    ob_start();
    $fn($v);
    $s = ob_get_clean();
    echo $fn, ": ", strlen($s), " ", $fn === 'debug_zval_dump' ? '' : md5($s), "\n";
}
$f = new Fiber(function () {
    dump('var_dump', mk(1500));
    dump('var_dump', mko(1500));
    dump('debug_zval_dump', mko(1500));
    dump('var_export', mk(4500));
    dump('print_r', mk(3000));
});
$f->start();
// (zend.max_allowed_stack_size=256K) php_object_property_dump() checks the
// stack: deeper levels are not shown
ob_start();
var_dump(mko(3000));
$s = ob_get_clean();
$p = strpos($s, 'nesting level too deep');
$d = substr_count(substr($s, 0, (int)$p), "object(stdClass)");
var_dump($p !== false, $d > 1000 && $d < 2000);
// (debug_zval_dump() does not check)
ob_start();
debug_zval_dump(mko(3000));
var_dump(str_contains(ob_get_clean(), 'nesting level too deep'));
--EXPECT--
var_dump: 6783013 ca5f0fc1f42838b239e29eab1e21dd3c
var_dump: 6810429 c4404053809aa6965a48b8cb0822de2a
debug_zval_dump: 6826940 
var_export: 60840009 d74803469705d6c78042f175cd681222
print_r: 108078010 0f2d3ce1dd3df50ca2d3bddbf93938ed
bool(true)
bool(true)
bool(false)
