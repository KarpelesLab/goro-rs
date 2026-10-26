--TEST--
getAttributes($name, ReflectionAttribute::IS_INSTANCEOF) autoloads the filter class and the attributes' classes (Laravel's container)
--FILE--
<?php
namespace Contracts {
    // declared by the autoloader below
}
namespace App {
    spl_autoload_register(function ($c) {
        echo "autoload($c)\n";
        if ($c === 'Contracts\ContextualAttribute') {
            eval('namespace Contracts; interface ContextualAttribute {}');
        }
        if ($c === 'App\Config') {
            eval('namespace App; #[\Attribute] class Config implements \Contracts\ContextualAttribute {}');
        }
    });
    function f(#[Config] #[Missing] $x, #[\SensitiveParameter] $y) {}
    $p = new \ReflectionParameter('App\f', 0);
    $a = $p->getAttributes(\Contracts\ContextualAttribute::class, \ReflectionAttribute::IS_INSTANCEOF);
    var_dump(count($a), $a[0]->getName());
    $p = new \ReflectionParameter('App\f', 1);
    var_dump(count($p->getAttributes('\SensitiveParameter', \ReflectionAttribute::IS_INSTANCEOF)));
    try {
        $p->getAttributes('App\NoSuchBase', \ReflectionAttribute::IS_INSTANCEOF);
    } catch (\Error $e) {
        echo get_class($e), ': ', $e->getMessage(), "\n";
    }
}
?>
--EXPECT--
autoload(Contracts\ContextualAttribute)
autoload(App\Config)
autoload(App\Missing)
int(1)
string(10) "App\Config"
int(1)
autoload(App\NoSuchBase)
Error: Class "App\NoSuchBase" not found
