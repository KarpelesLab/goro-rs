--TEST--
ReflectionFunction of $obj->m(...) whose body calls a method (new X), on a class with __call(): the method, not a trampoline
--FILE--
<?php
class DC {
    public function __call(string $m, array $a) { throw new LogicException('x'); }
    public static function __callStatic(string $m, array $a) { throw new LogicException('y'); }
    public function regularCallable() { throw new LogicException('Unexpected method call'); }
    public static function st() { return (new ArrayObject([]))->count(); }
}
$c = new DC;
foreach ([$c->regularCallable(...), DC::st(...), $c->magic(...), DC::smagic(...), Closure::fromCallable([$c, 'regularCallable'])] as $f) {
    $r = new ReflectionFunction($f);
    echo $r->name, " ", $r->getNumberOfParameters(), " ", $r->getClosureCalledClass()?->name, "\n";
}

?>
--EXPECTF--
regularCallable 0 DC
st 0 DC
magic 1 DC
smagic 1 DC
regularCallable 0 DC
