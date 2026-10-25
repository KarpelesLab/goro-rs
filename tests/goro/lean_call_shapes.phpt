--TEST--
Lean calls: constructors, forwarding calls, underscore names, silenced callees handed over
--FILE--
<?php
function _plain($a, $b = 2) { return $a + $b; }
function _trace() { return array_map(fn($f) => $f['function'], debug_backtrace()); }
function _calls_trace($x) { $t = _trace(); return [$x, $t]; }

class Base {
    public $id;
    public static $made = 0;
    function __construct($id) { $this->id = $id; static::$made++; }
    static function who() { return static::class; }
    static function make($id) { return new static($id); }
    function name() { return 'base'; }
    function viaSelf() { return self::who() . '/' . static::who(); }
    function trace() { return array_map(fn($f) => ($f['class'] ?? '') . ($f['type'] ?? '') . $f['function'], debug_backtrace()); }
}
class Child extends Base {
    public $extra;
    function __construct($id, $extra = 'x') { parent::__construct($id); $this->extra = $extra; }
    function name() { return 'child:' . parent::name(); }
    static function who() { return 'Child!' . parent::who(); }
    function viaParent() { return parent::viaSelf(); }
    function traced() { return parent::trace(); }
}
class Silent {
    public $p;
    function __construct() {
        if (empty($this->test[0][0])) { echo "empty\n"; }
        if (!isset($this->test[0][0])) { echo "unset\n"; }
        $this->p = @$this->nope;
    }
    function m() { return empty($this->q['a']) ? 'e' : 'n'; }
}

function run() {
    $s = 0;
    for ($i = 0; $i < 3; $i++) {
        $s += _plain($i);
        $s += _plain($i, 10);
    }
    echo $s, "\n";
    print_r(_calls_trace(1));
    $objs = [];
    for ($i = 0; $i < 3; $i++) {
        $objs[] = new Child($i);
        $objs[] = new Base($i);
    }
    foreach ($objs as $o) {
        echo get_class($o), ' ', $o->id, ' ', $o->name(), ' ', $o->viaSelf(), "\n";
    }
    echo Base::$made, "\n";
    echo get_class(Child::make(7)), ' ', get_class(Base::make(8)), "\n";
    echo (new Child(1))->viaParent(), "\n";
    print_r((new Child(2))->traced());
    for ($i = 0; $i < 2; $i++) {
        $x = new Silent;
        echo $x->m(), "\n";
    }
    // loops ending in the middle of others, over empty and shared arrays
    $a = [1, 2, 3];
    $b = $a;
    $n = 0;
    foreach ([] as $v) { $n += 100; }
    foreach ($a as $v) { foreach ($b as $w) { $n += $v * $w; } }
    foreach ($a as $k => $v) { $a[] = $k; }
    echo $n, ' ', count($a), "\n";
}
run();
?>
--EXPECT--
42
Array
(
    [0] => 1
    [1] => Array
        (
            [0] => _trace
            [1] => _calls_trace
            [2] => run
        )

)
Child 0 child:base Child/Child!Child
Base 0 base Base/Base
Child 1 child:base Child/Child!Child
Base 1 base Base/Base
Child 2 child:base Child/Child!Child
Base 2 base Base/Base
6
Child Base
Child/Child!Child
Array
(
    [0] => Base->trace
    [1] => Child->traced
    [2] => run
)
empty
unset
e
empty
unset
e
36 6
