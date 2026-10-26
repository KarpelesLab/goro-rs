--TEST--
ReflectionProperty raw values use the effective property of the object class; a shadowed private keeps its own slot
--FILE--
<?php
class Base { private $privateProp = 'Base'; }
class Test extends Base {
    private $privateProp = 'Test' {
        get { echo "get\n"; return $this->privateProp; }
        set { $this->privateProp = $value; }
    }
}
$o = new Test;
var_dump(str_replace("\0", "~", array_keys((array)$o)));
$r = new ReflectionProperty(Test::class, 'privateProp');
var_dump($r->getRawValue($o));
(new ReflectionProperty(Base::class, 'privateProp'))->setRawValue($o, 42);
var_dump($r->getRawValue($o));
var_dump((new ReflectionProperty(Base::class, 'privateProp'))->getRawValue($o));
$r->setRawValue($o, 5);
foreach ((array)$o as $k => $v) echo str_replace("\0", "~", $k), "=", $v, "\n";
--EXPECT--
array(2) {
  [0]=>
  string(17) "~Base~privateProp"
  [1]=>
  string(17) "~Test~privateProp"
}
string(4) "Test"
string(4) "Test"
int(42)
~Base~privateProp=42
~Test~privateProp=5
