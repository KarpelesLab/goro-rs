--TEST--
var_export() of a Generator: an object without properties
--FILE--
<?php
$g = (function() { yield 1; })();
var_export($g); echo "\n"; var_export([$g, "k" => [$g]]); echo "\n";
var_export([function() {}, "k" => [fn() => 1]]); echo "\n"; var_export(function() {}); echo "\n";
--EXPECT--
\Generator::__set_state(array(
))
array (
  0 => 
  \Generator::__set_state(array(
  )),
  'k' => 
  array (
    0 => 
    \Generator::__set_state(array(
    )),
  ),
)
array (
  0 => 
  \Closure::__set_state(array(
  )),
  'k' => 
  array (
    0 => 
    \Closure::__set_state(array(
    )),
  ),
)
\Closure::__set_state(array(
))
