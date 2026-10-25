--TEST--
Argument temporaries move to the callee: object handles recycle as php-src's
--FILE--
<?php
function t($f){ echo "f=", spl_object_id($f), "\n"; try{ $o = $f(); echo "o=", spl_object_id($o), "\n"; }catch(Throwable $e){ echo "caught ", spl_object_id($e), "\n"; } }
t(function(){ throw new Exception; });
t(fn()=>new stdClass);
t(fn()=>new stdClass);
$a = new stdClass; $b = new stdClass; echo spl_object_id($a), spl_object_id($b), "\n";
--EXPECTF--
f=1
caught 2
f=2
o=1
f=1
o=2
21
