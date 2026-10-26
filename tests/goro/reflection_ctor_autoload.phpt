--TEST--
Reflection constructors (ReflectionClassConstant, ReflectionProperty, ReflectionMethod, ReflectionEnumUnitCase) autoload the class
--FILE--
<?php
spl_autoload_register(function ($c) {
    echo "autoload $c\n";
    if ($c === 'A1') { class A1 { const X = 1; public $p = 2; public function m() {} } }
    if ($c === 'A2') { class A2 { const X = 1; public $p = 2; public function m() {} } }
    if ($c === 'A3') { class A3 { const X = 1; public $p = 2; public function m() {} } }
    if ($c === 'A4') { enum A4 { case C; } }
    if ($c === 'A5') { class A5 { const X = 1; } }
});
var_dump((new ReflectionClassConstant('A1', 'X'))->getValue());
var_dump((new ReflectionProperty('A2', 'p'))->getName());
var_dump((new ReflectionMethod('A3', 'm'))->getName());
var_dump((new ReflectionEnumUnitCase('A4', 'C'))->getName());
var_dump((new ReflectionClassConstant('\A5', 'X'))->getValue());
try { new ReflectionClassConstant('Nope', 'X'); } catch (ReflectionException $e) { echo $e->getMessage(), "\n"; }
try { new ReflectionProperty('Nope2', 'X'); } catch (ReflectionException $e) { echo $e->getMessage(), "\n"; }

?>
--EXPECT--
autoload A1
int(1)
autoload A2
string(1) "p"
autoload A3
string(1) "m"
autoload A4
string(1) "C"
autoload A5
int(1)
autoload Nope
Class "Nope" does not exist
autoload Nope2
Class "Nope2" does not exist
