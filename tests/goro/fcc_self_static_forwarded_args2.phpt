--TEST--
self::$make ??= self::make(...) (Symfony Container::get()) from a subclass
--FILE--
<?php
class Container {
    private static $make;
    public $loading = [];
    public function get(string $id, int $b = 1): ?object
    {
        return ($this->factories[$id] ?? self::$make ??= self::make(...))($this, $id, $b);
    }
    private static function make(self $container, string $id, int $b): ?object
    {
        $container->loading[$id] = true;
        echo static::class, " ", $id, " ", $b, "\n";
        return null;
    }
    public $factories = [];
}
class Builder extends Container {
    public function x() { return $this->get('a'); }
}
(new Builder)->x();
(new Container)->get('b', 2);
$f = Closure::fromCallable([new Builder, 'get']);
$f('c');

?>
--EXPECTF--
Builder a 1
Builder b 2
Builder c 1
