--TEST--
Reflection of a class whose name is not UTF-8 (methods, properties, constants, parameters)
--FILE--
<?php
eval('class ' . "\xA9" . ' { public $p = 1; const K = 2; public function __construct(public int $q = 0) {} public function m(?' . "\xA9" . ' $x = null) { return "m" . strlen(static::class); } private function priv() { return 7; } public static function s() { return 3; } public function __unserialize(array $a): void { echo "unser\n"; } }');
$n = "\xA9";
$o = new $n;
$m = new ReflectionMethod($n, "__unserialize");
var_dump(bin2hex($m->class), $m->name, $m->isPublic());
$m->invoke($o, []);
$m = ReflectionMethod::createFromMethodName($n . "::m");
var_dump(bin2hex($m->class), $m->invoke($o, null));
var_dump(bin2hex($m->getDeclaringClass()->getName()));
$m = new ReflectionMethod($o, "priv");
var_dump($m->invoke($o));
var_dump((new ReflectionMethod($n, "s"))->invoke(null));
$p = new ReflectionProperty($n, "p");
var_dump(bin2hex($p->class), $p->getValue($o), bin2hex($p->getDeclaringClass()->name));
$c = new ReflectionClassConstant($n, "K");
var_dump(bin2hex($c->class), $c->getValue(), bin2hex($c->getDeclaringClass()->name));
$pa = new ReflectionParameter([$n, "m"], 0);
var_dump($pa->name, bin2hex($pa->getDeclaringClass()->name), bin2hex((string)$pa->getType()), bin2hex($pa->getType()->getName()));
$rc = new ReflectionClass($n);
foreach ($rc->getMethods() as $mm) echo bin2hex($mm->class), " ", $mm->name, "\n";
foreach ($rc->getProperties() as $mm) echo bin2hex($mm->class), " ", $mm->name, "\n";
var_dump(bin2hex($rc->getConstructor()->class));
var_dump(bin2hex($rc->getMethod("m")->getDeclaringClass()->getName()));
$cl = $m->getClosure($o);
var_dump(bin2hex((new ReflectionFunction($cl))->getClosureScopeClass()->name));
var_dump(bin2hex($rc->newInstance(5)::class), $rc->newInstanceArgs([6])->q);
var_dump(method_exists($n, "m"), is_callable([$n, "s"]));
try { new ReflectionMethod($n, "nope"); } catch (ReflectionException $e) { echo bin2hex($e->getMessage()), "\n"; }
try { new ReflectionMethod("\xAAx", "nope"); } catch (ReflectionException $e) { echo bin2hex($e->getMessage()), "\n"; }

?>
--EXPECT--
string(2) "a9"
string(13) "__unserialize"
bool(true)
unser
string(2) "a9"
string(2) "m1"
string(2) "a9"
int(7)
int(3)
string(2) "a9"
int(1)
string(2) "a9"
string(2) "a9"
int(2)
string(2) "a9"
string(1) "x"
string(2) "a9"
string(4) "3fa9"
string(2) "a9"
a9 __construct
a9 m
a9 priv
a9 s
a9 __unserialize
a9 p
a9 q
string(2) "a9"
string(2) "a9"
string(2) "a9"
string(2) "a9"
int(6)
bool(true)
bool(true)
4d6574686f6420a93a3a6e6f7065282920646f6573206e6f74206578697374
436c6173732022aa782220646f6573206e6f74206578697374
