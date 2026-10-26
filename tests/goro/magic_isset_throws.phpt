--TEST--
An exception thrown by __isset() propagates from isset() / empty() / ??
--FILE--
<?php
class C { public function __isset($n) { throw new Exception("isset $n"); } }
try { var_dump(isset((new C)->x)); } catch (Exception $e) { echo $e->getMessage(), "\n"; }
$c = new C;
try { var_dump(isset($c->x)); } catch (Exception $e) { echo $e->getMessage(), "\n"; }
try { var_dump(empty($c->x)); } catch (Exception $e) { echo $e->getMessage(), "\n"; }
try { var_dump($c->x ?? 1); } catch (Exception $e) { echo $e->getMessage(), "\n"; }
try { var_dump(isset($c->x->y)); } catch (Exception $e) { echo $e->getMessage(), "\n"; }
function f() { return new C; }
try { var_dump(isset(f()->x)); } catch (Exception $e) { echo $e->getMessage(), "\n"; }
?>
--EXPECT--
isset x
isset x
isset x
isset x
isset x
isset x
