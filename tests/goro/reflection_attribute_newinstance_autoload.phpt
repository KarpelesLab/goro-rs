--TEST--
ReflectionAttribute::newInstance() autoloads the attribute class (Laravel reading symfony/console's #[AsCommand])
--FILE--
<?php
spl_autoload_register(function ($c) {
    echo "autoload($c)\n";
    if ($c === 'AsCommand') {
        eval('#[Attribute(Attribute::TARGET_CLASS)] class AsCommand { public function __construct(public string $name) { echo "ctor\n"; } }');
    }
});
#[AsCommand(name: 'app:run')]
#[NotThere]
class Cmd {}
$attrs = (new ReflectionClass('Cmd'))->getAttributes();
var_dump(count($attrs));
var_dump($attrs[0]->newInstance()->name);
try {
    $attrs[1]->newInstance();
} catch (Error $e) {
    echo $e->getMessage(), "\n";
}
?>
--EXPECT--
int(2)
autoload(AsCommand)
ctor
string(7) "app:run"
autoload(NotThere)
Attribute class "NotThere" not found
