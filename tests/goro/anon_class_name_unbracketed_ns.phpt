--TEST--
Anonymous class names use the parent/interface resolved in an unbracketed namespace
--FILE--
<?php
namespace Foo\Bar;

use ArrayObject as AO;

interface Customizable {}
class Base {}

$a = new class implements Customizable {};
$b = new class extends Base {};
$c = new class extends AO {};
$d = new class {};
foreach ([$a, $b, $c, $d] as $o) {
    echo strstr(get_class($o), "\0", true), "\n";
}

?>
--EXPECT--
Foo\Bar\Customizable@anonymous
Foo\Bar\Base@anonymous
ArrayObject@anonymous
class@anonymous
