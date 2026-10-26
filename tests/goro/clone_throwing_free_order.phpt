--TEST--
A clone whose __clone() throws is released after the cloned operand
--FILE--
<?php
class B { public $x = 1; public function __clone() { throw new Exception("x"); } }
try { $b = clone new B; } catch (Exception $e) { echo $e->getMessage(), "\n"; }
$p = new stdClass; $q = new stdClass;
var_dump(spl_object_id($p), spl_object_id($q));
$o = new B;
try { $b = clone $o; } catch (Exception $e) { echo $e->getMessage(), "\n"; }
$r = new stdClass;
var_dump(spl_object_id($r));
class B2 { public $n; function __construct($n) { $this->n = $n; } public function __clone() { $this->n = "clone"; throw new Exception("x"); } function __destruct() { echo "dtor {$this->n}\n"; } }
try { $b = clone new B2("orig"); } catch (Exception $e) { echo $e->getMessage(), "\n"; }
$o = new B2("o2");
try { $b = clone $o; } catch (Exception $e) { echo $e->getMessage(), "\n"; }
try { $b = clone(new B2("fn"), ['n' => 'w']); } catch (Exception $e) { echo $e->getMessage(), "\n"; }
echo "end\n";
--EXPECTF--
x
int(2)
int(1)
x
int(3)
dtor orig
dtor clone
x
dtor clone
x
dtor clone
x
end
dtor o2
dtor fn
