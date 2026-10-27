--TEST--
Destructor trace frames name the declaring class
--FILE--
<?php
class P { function __destruct() { foreach (debug_backtrace() as $f) echo $f["class"] ?? "", $f["type"] ?? "", $f["function"], "\n"; try { throw new Exception("x"); } catch (Exception $e) { echo $e->getTraceAsString(), "\n"; } } }
class S extends P {}
$s = new S; unset($s);
$t = new S;
echo "end\n";

?>
--EXPECTF--
P->__destruct
#0 %s(4): P->__destruct()
#1 {main}
end
P->__destruct
#0 [internal function]: P->__destruct()
#1 {main}
