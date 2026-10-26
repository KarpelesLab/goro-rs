--TEST--
Class lookups strip a leading backslash: autoloader argument, class_alias(), get_parent_class(), is_a() (monolog's test bootstrap)
--FILE--
<?php
namespace X { class TC {} class Child extends TC {} }
namespace {
spl_autoload_register(function ($c) {
    echo "autoload($c)\n";
    if ($c === 'L\Lazy') { eval('namespace L; class Lazy {}'); }
});
var_dump(class_exists('\\Nope', true));
if (!class_exists('\\PHPUnit_Framework_TestCase', true)) {
    var_dump(class_alias('\\L\\Lazy', '\\PHPUnit_Framework_TestCase'));
}
var_dump(get_class(new PHPUnit_Framework_TestCase));
var_dump(get_parent_class('\\X\\Child'));
var_dump(is_a(new X\TC, '\\X\\TC'), is_a('\\X\\Child', 'X\\TC', true));
var_dump(is_subclass_of(new X\TC, '\\X\\TC'), is_subclass_of(new X\Child, '\\X\\TC'));
}
?>
--EXPECT--
autoload(Nope)
bool(false)
autoload(PHPUnit_Framework_TestCase)
autoload(L\Lazy)
bool(true)
string(6) "L\Lazy"
string(4) "X\TC"
bool(true)
bool(true)
bool(false)
bool(true)
