--TEST--
ReflectionMethod / method_exists() with a leading backslash class name (autoloaded)
--FILE--
<?php
error_reporting(E_ALL & ~E_DEPRECATED);
spl_autoload_register(function ($c) {
    echo "autoload($c)\n";
    if ($c === 'NS\Lazy') { eval('namespace NS; class Lazy { function __construct() {} function m() {} }'); }
});
foreach (['\NS\Lazy', 'NS\Lazy', '\Exception', '\NS\Missing'] as $cls) {
    try {
        $m = new ReflectionMethod($cls, '__construct');
        echo $m->class, "::", $m->name, "\n";
    } catch (ReflectionException $e) { echo $e->getMessage(), "\n"; }
    try {
        $m = new ReflectionMethod($cls . '::__construct');
        echo $m->class, "::", $m->name, "\n";
    } catch (ReflectionException $e) { echo $e->getMessage(), "\n"; }
    try {
        $c = new ReflectionClass($cls);
        echo $c->name, "\n";
    } catch (ReflectionException $e) { echo $e->getMessage(), "\n"; }
    var_dump(method_exists($cls, '__construct'));
}

?>
--EXPECT--
autoload(NS\Lazy)
NS\Lazy::__construct
NS\Lazy::__construct
NS\Lazy
bool(true)
NS\Lazy::__construct
NS\Lazy::__construct
NS\Lazy
bool(true)
Exception::__construct
Exception::__construct
Exception
bool(true)
autoload(NS\Missing)
Class "\NS\Missing" does not exist
autoload(NS\Missing)
Class "\NS\Missing" does not exist
autoload(NS\Missing)
Class "\NS\Missing" does not exist
autoload(NS\Missing)
bool(false)
