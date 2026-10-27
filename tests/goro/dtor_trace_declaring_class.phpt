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
--EXPECT--
P->__destruct
#0 /tmp/claude-1000/-home-magicaltux-projects-goro-rs/3d2a04d8-60d8-47cf-8c66-cf536b5391ca/scratchpad/apps2/repro/p1/dtor3.php(4): P->__destruct()
#1 {main}
end
P->__destruct
#0 [internal function]: P->__destruct()
#1 {main}
