--TEST--
include of a php://filter URL reports the filters it cannot create
--FILE--
<?php
$inc = __DIR__ . '/include_php_filter_warnings.inc';
file_put_contents($inc, '<?php return 42;');
function f($inc) {
    return include 'php://filter/read=no.such.filter/resource=' . $inc;
}
var_dump(f($inc));
class K { function m($inc) { return include 'php://filter/read=nope/resource=' . $inc; } }
var_dump((new K)->m($inc));
?>
--CLEAN--
<?php @unlink(__DIR__ . '/include_php_filter_warnings.inc'); ?>
--EXPECTF--
Warning: include(): Unable to locate filter "no.such.filter" in %s on line %d

Warning: include(): Unable to create filter (no.such.filter) in %s on line %d
int(42)

Warning: include(): Unable to locate filter "nope" in %s on line %d

Warning: include(): Unable to create filter (nope) in %s on line %d
int(42)
