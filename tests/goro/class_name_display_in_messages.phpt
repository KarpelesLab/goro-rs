--TEST--
Class names in error messages (with __, non-ASCII, anonymous)
--FILE--
<?php
// Class names as messages show them (plain, with "__", non-ASCII, anonymous)
namespace NS\Sub;

class Plain {}
class With__Double {}
class Ünïcode {}
$objs = [new Plain(), new With__Double(), new Ünïcode(), new class {}, new class extends Plain {}];
foreach ($objs as $o) {
    try {
        $o->missing();
    } catch (\Error $e) {
        echo str_replace(__FILE__, 'FILE', $e->getMessage()), "\n";
    }
    try {
        strlen($o);
    } catch (\TypeError $e) {
        echo str_replace(__FILE__, 'FILE', $e->getMessage()), "\n";
    }
}
echo get_class($objs[1]), "\n";
var_dump(str_contains(get_class($objs[3]), "class@anonymous"));
--EXPECT--
Call to undefined method NS\Sub\Plain::missing()
strlen(): Argument #1 ($string) must be of type string, NS\Sub\Plain given
Call to undefined method NS\Sub\With__Double::missing()
strlen(): Argument #1 ($string) must be of type string, NS\Sub\With__Double given
Call to undefined method NS\Sub\Ünïcode::missing()
strlen(): Argument #1 ($string) must be of type string, NS\Sub\Ünïcode given
Call to undefined method class@anonymous::missing()
strlen(): Argument #1 ($string) must be of type string, class@anonymous given
Call to undefined method NS\Sub\Plain@anonymous::missing()
strlen(): Argument #1 ($string) must be of type string, NS\Sub\Plain@anonymous given
NS\Sub\With__Double
bool(true)
