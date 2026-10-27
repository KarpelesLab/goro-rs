--TEST--
ReflectionGenerator::getExecutingLine()/File() of the running generator: where it called the code running
--FILE--
<?php
function g() { yield 1; }
$gen = (function() use (&$gen) {
  $r = new ReflectionGenerator($gen);
  echo $r->getExecutingLine(), "\n";
  (function() use ($r) { echo $r->getExecutingLine(), " ", basename($r->getExecutingFile()), "\n"; })();
  yield 1;
})();
$gen->current();
--EXPECT--
5
6 reflection_generator_running_line.php
