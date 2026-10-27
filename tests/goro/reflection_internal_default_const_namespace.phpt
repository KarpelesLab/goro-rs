--TEST--
ReflectionParameter::getDefaultValueConstantName(): stub class names resolve in the declaring class's namespace
--EXTENSIONS--
reflection
bcmath
--FILE--
<?php
// unqualified class names in namespaced stubs resolve to the namespaced class
// (Random\IntervalBoundary) and fall back to the global one (RoundingMode)
foreach ([['Random\Randomizer', 'getFloat', 2], ['BcMath\Number', 'round', 1]] as [$c, $m, $i]) {
  $p = (new ReflectionMethod($c, $m))->getParameters()[$i];
  var_dump($p->isDefaultValueConstant(), $p->getDefaultValueConstantName(), $p->getDefaultValue());
}
foreach ([['bcround', 2], ['mt_srand', 1], ['htmlspecialchars', 1]] as [$f, $i]) {
  var_dump((new ReflectionFunction($f))->getParameters()[$i]->getDefaultValueConstantName());
}
?>
--EXPECT--
bool(true)
string(35) "Random\IntervalBoundary::ClosedOpen"
enum(Random\IntervalBoundary::ClosedOpen)
bool(true)
string(30) "RoundingMode::HalfAwayFromZero"
enum(RoundingMode::HalfAwayFromZero)
string(30) "RoundingMode::HalfAwayFromZero"
string(15) "MT_RAND_MT19937"
NULL
