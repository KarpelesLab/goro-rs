--TEST--
Natively created DOM/XMLReader user-subclass objects start with uninitialized (not unset) typed properties
--FILE--
<?php
class E extends DOMElement { public readonly string $ro; public int $typed; public ?int $n = null; public $plain;
  public function __set($n, $v) { echo "__set $n\n"; } public function __get($n) { echo "__get $n\n"; return 1; } }
$d = new DOMDocument; $d->registerNodeClass('DOMElement', 'E'); $d->loadXML('<r/>');
$r = $d->documentElement;
try { $r->ro = 'x'; } catch (Throwable $ex) { echo get_class($ex), ": ", $ex->getMessage(), "\n"; }
try { var_dump($r->typed); } catch (Throwable $ex) { echo get_class($ex), ": ", $ex->getMessage(), "\n"; }
var_dump($r->n, $r->plain, $r->undeclared);
unset($r->typed); var_dump($r->typed);
$r->n = 5; var_dump(array_slice(get_object_vars($r), 0, 3));
class R extends XMLReader { public int $typed; public $p = 1; public function __get($n) { echo "__get $n\n"; return 2; } }
$x = R::fromString('<a/>');
try { var_dump($x->typed); } catch (Throwable $ex) { echo get_class($ex), ": ", $ex->getMessage(), "\n"; }
var_dump($x->p);
class S extends SimpleXMLElement { }
$s = simplexml_load_string('<a><b/></a>', 'S'); var_dump(get_class($s->b));
?>
--EXPECT--
Error: Cannot modify protected(set) readonly property E::$ro from global scope
Error: Typed property E::$typed must not be accessed before initialization
__get undeclared
NULL
NULL
int(1)
__get typed
int(1)
array(2) {
  ["n"]=>
  int(5)
  ["plain"]=>
  NULL
}
Error: Typed property R::$typed must not be accessed before initialization
int(1)
string(1) "S"
