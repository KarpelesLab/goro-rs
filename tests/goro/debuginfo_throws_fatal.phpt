--TEST--
__debugInfo() throwing in var_dump() is fatal, reported after the exception as a warning
--INI--
zend.exception_ignore_args=0
--FILE--
<?php
class C { function __debugInfo() { throw new Exception("d"); } }
try {
    var_dump([1, new C]);
} catch (Exception $e) {
    echo "not reached\n";
}
echo "not reached\n";
?>
--EXPECTF--
array(2) {
  [0]=>
  int(1)
  [1]=>
  
Warning: Uncaught Exception: d in %s:2
Stack trace:
#0 [internal function]: C->__debugInfo()
#1 %s(4): var_dump(Array)
#2 {main}
  thrown in %s on line 2

Fatal error: __debuginfo() must return an array in %s on line 4
