--TEST--
A callable parameter given 'Class::method' autoloads the class
--FILE--
<?php
namespace NS;
spl_autoload_register(function ($c) {
    echo "autoload $c\n";
    if ($c === 'NS\F') {
        eval('namespace NS; final class F { public static function MinDate(): bool { return true; } }');
    }
});
function reg(string $n, callable $f) { echo "ok $n\n"; }
$cls = F::class . "::";
reg("a", $cls . "MinDate");
var_dump(is_callable('NS\G::x'));

?>
--EXPECT--
autoload NS\F
ok a
autoload NS\G
bool(false)
