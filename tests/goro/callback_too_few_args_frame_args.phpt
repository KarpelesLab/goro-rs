--TEST--
ArgumentCountError of an internally called callback: its frame shows the passed arguments
--FILE--
<?php
$a = ["t" => 1];
try { array_walk($a, function (&$v, $k, $x) {}); } catch (Error $e) { echo $e->getTraceAsString(), "\n"; }
try { array_map(function ($v, $k) {}, [1]); } catch (Error $e) { echo $e->getTraceAsString(), "\n"; }
try { call_user_func(function ($v, $k) {}, 5); } catch (Error $e) { echo $e->getTraceAsString(), "\n"; }
try { usort($a, function ($v, $k, $z) {}); } catch (Error $e) { echo $e->getTraceAsString(), "\n"; }
--EXPECTF--
#0 [internal function]: {closure:%s:3}(1, 't')
#1 %s(3): array_walk(Array, Object(Closure))
#2 {main}
#0 [internal function]: {closure:%s:4}(1)
#1 %s(4): array_map(Object(Closure), Array)
#2 {main}
#0 %s(5): {closure:%s:5}(5)
#1 {main}
