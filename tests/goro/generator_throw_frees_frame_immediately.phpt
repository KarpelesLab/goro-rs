--TEST--
An exception thrown into a generator without handler frees its frame before the caller catches it
--FILE--
<?php
class D { function __construct(public $n) {} function __destruct() { echo "dtor {$this->n}\n"; } }
function g() { $o = new D(1); yield 1; throw new Exception("x"); }
$gen = g();
try { foreach ($gen as $v) { echo "v=$v\n"; } } catch (Exception $e) { echo "caught\n"; }
echo "after1\n";
function g2() { $o = new D(2); yield 1; }
$gen2 = g2();
$gen2->current();
try { $gen2->throw(new Exception("y")); } catch (Exception $e) { echo "caught2\n"; }
echo "after2\n";
function g3() { $o = new D(3); try { yield 1; } finally { echo "finally3\n"; } }
$gen3 = g3(); $gen3->current();
try { $gen3->throw(new Exception("z")); } catch (Exception $e) { echo "caught3\n"; }
echo "after3\n";
function g4() { $o = new D(4); yield 1; return 5; }
$gen4 = g4(); foreach ($gen4 as $v) {} echo "done4\n"; var_dump($gen4->getReturn());
echo "end\n";
--EXPECT--
v=1
dtor 1
caught
after1
dtor 2
caught2
after2
finally3
dtor 3
caught3
after3
dtor 4
done4
int(5)
end
