--TEST--
Property defaults of literals, __DIR__/__FILE__/__LINE__ and concatenations are folded at compile time
--FILE--
<?php
namespace Composer\Autoload;
// static property defaults built from __DIR__ (as autoload_static.php's) and their literal twins
class T {
    public static $a = array('k' => __DIR__ . '/..' . '/x.php', 3 => 'y', 'n' => [1, 'z', -2, 1.5, true, null], 'q' => 'it\'s', 'b' => __DIR__ . '/a\\b');
    public static $f = [__FILE__, __LINE__, __DIR__];
    public $i = ['p' => __DIR__ . '/i'];
    public static $map = array (
        'A\\B\\' => array (0 => __DIR__ . '/../..' . '/src'),
    );
}
class L {
    public static $a = array('k' => 'DIR/../x.php', 3 => 'y', 'n' => [1, 'z', -2, 1.5, true, null], 'q' => 'it\'s', 'b' => 'DIR/a\\b');
}
foreach (['a', 'f', 'map'] as $p) {
    echo str_replace([__FILE__, __DIR__], ["FILE", "DIR"], (string) new \ReflectionProperty(T::class, $p));
}
echo str_replace(__DIR__, 'DIR', (string) new \ReflectionProperty(T::class, 'i'));
echo (string) new \ReflectionProperty(L::class, 'a');
var_dump(str_replace(__DIR__, 'DIR', var_export(T::$a, true)) === var_export(L::$a, true));
var_dump(str_replace(__DIR__, 'DIR', var_export((new \ReflectionProperty(T::class, 'a'))->getDefaultValue(), true)));
echo str_replace([__FILE__, __DIR__], ["FILE", "DIR"], (new \ReflectionClass(T::class))->__toString());
var_dump(count(T::$map), str_replace(__DIR__, "DIR", (new T)->i));
--EXPECT--
Property [ public static $a = ['k' => 'DIR/../x.php', 3 => 'y', 'n' => [1, 'z', -2, 1.5, true, NULL], 'q' => 'it's', 'b' => 'DIR/a\\b'] ]
Property [ public static $f = ['FILE', 6, 'DIR'] ]
Property [ public static $map = ['A\\B\\' => ['DIR/../../src']] ]
Property [ public $i = ['p' => 'DIR/i'] ]
Property [ public static $a = ['k' => 'DIR/../x.php', 3 => 'y', 'n' => [1, 'z', -2, 1.5, true, NULL], 'q' => 'it's', 'b' => 'DIR/a\\b'] ]
bool(true)
string(193) "array (
  'k' => 'DIR/../x.php',
  3 => 'y',
  'n' => 
  array (
    0 => 1,
    1 => 'z',
    2 => -2,
    3 => 1.5,
    4 => true,
    5 => NULL,
  ),
  'q' => 'it\'s',
  'b' => 'DIR/a\\b',
)"
Class [ <user> class Composer\Autoload\T ] {
  @@ FILE 4-11

  - Constants [0] {
  }

  - Static properties [3] {
    Property [ public static $a = ['k' => 'DIR/../x.php', 3 => 'y', 'n' => [1, 'z', -2, 1.5, true, NULL], 'q' => 'it's', 'b' => 'DIR/a\\b'] ]
    Property [ public static $f = ['FILE', 6, 'DIR'] ]
    Property [ public static $map = ['A\\B\\' => ['DIR/../../src']] ]
  }

  - Static methods [0] {
  }

  - Properties [1] {
    Property [ public $i = ['p' => 'DIR/i'] ]
  }

  - Methods [0] {
  }
}
int(1)
array(1) {
  ["p"]=>
  string(5) "DIR/i"
}
