--TEST--
A lazy initializer run by a builtin is called from [internal function]; var_export() writes what it built before the failed init
--FILE--
<?php
class A { public $x = 1; }
$r = new ReflectionClass('A');
$g = $r->newLazyGhost(function($o) { throw new Exception("init failed"); });
try { var_dump($g->x); } catch (Exception $e) { echo $e->getMessage(), "\n"; }
var_dump($r->isUninitializedLazyObject($g));
try { var_dump(json_encode($g)); } catch (Exception $e) { echo $e->getMessage(), "\n"; }
try { var_dump(serialize($g)); } catch (Exception $e) { echo $e->getMessage(), "\n"; }
var_dump($g);
print_r($g); echo "\n";
var_export($g); echo "\n";
try { foreach ($g as $k => $v) echo "$k\n"; } catch (Exception $e) { echo $e->getMessage(), "\n"; }
try { var_dump((array)$g); } catch (Exception $e) { echo $e->getMessage(), "\n"; }
try { var_dump(get_object_vars($g)); } catch (Exception $e) { echo $e->getMessage(), "\n"; }
--EXPECTF--
init failed
bool(true)
init failed
init failed
lazy ghost object(A)#3 (0) {
}
A Object
(
)

\A::__set_state(array(
))
Fatal error: Uncaught Exception: init failed in %s:4
Stack trace:
#0 [internal function]: {closure:%s:4}(Object(A))
#1 %s(11): var_export(Object(A))
#2 {main}
  thrown in %s on line 4
