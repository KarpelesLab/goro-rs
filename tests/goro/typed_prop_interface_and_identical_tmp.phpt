--TEST--
Typed properties of an interface type written repeatedly; === of temporaries while objects with a destructor exist
--FILE--
<?php
interface Factory {}
interface Other {}
abstract class Base implements Factory {}
class Impl extends Base {}
class Direct implements Factory, Other {}
class Plain {}
class D { function __construct(public $n) {} function __destruct() { echo "dtor {$this->n}\n"; } }
$keep = new D('kept');
class Holder {
    public Factory $f;
    public ?Other $o = null;
    private static ?Holder $inst = null;
    function __construct(Factory $f) { $this->f = $f; }
    static function get(): Holder { return self::$inst ??= new Holder(new Impl); }
    static function make(): D { return new D('temp'); }
}
for ($i = 0; $i < 3; $i++) {
    $h = new Holder($i % 2 ? new Direct : new Impl);
    echo get_class($h->f), ' ';
    $h->f = new Direct;
    $h->o = new Direct;
    try { $h->f = new Plain; } catch (TypeError $e) { echo $e->getMessage(), ' '; }
    try { $h->o = new Impl; } catch (TypeError $e) { echo $e->getMessage(), ' '; }
    echo (Holder::get() === Holder::get() ? 'same' : 'diff'), ' ';
    echo (Holder::make() === $keep ? 'same' : 'diff'), "\n";
    echo (null === Holder::make() ? 'null' : 'obj'), "\n";
}
echo "end\n";
?>
--EXPECT--
Impl Cannot assign Plain to property Holder::$f of type Factory Cannot assign Impl to property Holder::$o of type ?Other same dtor temp
diff
dtor temp
obj
Direct Cannot assign Plain to property Holder::$f of type Factory Cannot assign Impl to property Holder::$o of type ?Other same dtor temp
diff
dtor temp
obj
Impl Cannot assign Plain to property Holder::$f of type Factory Cannot assign Impl to property Holder::$o of type ?Other same dtor temp
diff
dtor temp
obj
end
dtor kept
