--TEST--
A static call of a missing class autoloads it once; defined('C::X') autoloads C
--FILE--
<?php
spl_autoload_register(function($c) { echo "load $c\n"; if ($c === 'K') { eval('class K { const C = 1; }'); } });
try { Q::f(); } catch (Error $e) { echo $e->getMessage(), "\n"; }
try { Q::f(1, 2); } catch (Error $e) { echo $e->getMessage(), "\n"; }
function t() { try { Q2::f(); } catch (Error $e) { echo $e->getMessage(), "\n"; } }
t();
$n = 'Q3';
try { $n::f(); } catch (Error $e) { echo $e->getMessage(), "\n"; }
var_dump(defined('Q4::C'));
$n = 'Q5::C'; var_dump(defined($n));
var_dump(defined('K::C'));
class_alias('K', 'KA'); var_dump(defined('KA::C'), defined('ka::C'));
?>
--EXPECT--
load Q
Class "Q" not found
load Q
Class "Q" not found
load Q2
Class "Q2" not found
load Q3
Class "Q3" not found
load Q4
bool(false)
load Q5
bool(false)
load K
bool(true)
bool(true)
bool(true)
