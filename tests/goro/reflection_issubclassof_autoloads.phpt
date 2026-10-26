--TEST--
ReflectionClass::isSubclassOf() autoloads its argument (PHPUnit's expectException() check)
--FILE--
<?php
spl_autoload_register(function ($c) {
    echo "autoload($c)\n";
    if ($c === 'B') { class B {} }
    if ($c === 'C') { class C extends A {} }
});
class A {}
$r = new ReflectionClass('A');
var_dump($r->isSubclassOf('B'));
var_dump((new ReflectionClass('C'))->isSubclassOf('A'));
try {
    var_dump($r->isSubclassOf('Nope'));
} catch (ReflectionException $e) {
    echo $e->getMessage(), "\n";
}
?>
--EXPECT--
autoload(B)
bool(false)
autoload(C)
bool(true)
autoload(Nope)
Class "Nope" does not exist
