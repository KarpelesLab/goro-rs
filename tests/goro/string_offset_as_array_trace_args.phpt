--TEST--
Cannot use string offset as an array: the variable keeps its string for trace arguments
--FILE--
<?php
function d($x) { try { $x[1][] = 7; } catch (Error $e) { echo $e->getMessage(), "\n"; var_dump($x); } }
d('none');
function d2($x) { $x[1][] = 7; }
try { d2('abc'); } catch (Error $e) { echo $e->getTraceAsString(), "\n"; }
$s = 'xyz';
try { $s[0][] = 1; } catch (Error $e) { echo $e->getMessage(), "\n"; }
var_dump($s);
try { $s[0]['k'] = 1; } catch (Error $e) { echo $e->getMessage(), "\n"; }
var_dump($s);
$e2 = '';
try { $e2[1][] = 1; } catch (Error $e) { echo $e->getMessage(), "\n"; }
var_dump($e2);
--EXPECTF--
Cannot use string offset as an array
string(4) "none"
#0 %s(5): d2('abc')
#1 {main}
Cannot use string offset as an array
string(3) "xyz"
Cannot use string offset as an array
string(3) "xyz"
Cannot use string offset as an array
string(0) ""
