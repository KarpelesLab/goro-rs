--TEST--
Fast === / !== of temporaries holding objects or arrays
--FILE--
<?php
// `===` / `!==` of temporaries holding objects or arrays.
class R {
    private static $instance = null;
    public static $arr = [1, 2];
    public $p;
    public static function get_instance() {
        if (null === self::$instance) {
            self::$instance = new self();
        }
        return self::$instance;
    }
    public static function same($o) { return self::$instance === $o; }
    public static function arr_is($v) { return self::$arr === $v; }
    public static function arr_not_int() { return self::$arr !== 5; }
}
function f() { return R::get_instance(); }
function g($o) { return $o->p === null; }
function h($o) { return f() === $o; }
function k() { return f() !== null; }
function mk() { return new R; }
function fresh() { return mk() === mk(); }
for ($i = 0; $i < 20; $i++) {
    $a = R::get_instance();
}
var_dump($a === R::get_instance(), R::same($a), R::same(new R), h($a), h(new R), k());
var_dump(R::arr_is([1, 2]), R::arr_is([2, 1]), R::arr_is(5), R::arr_not_int(), fresh());
$a->p = new R;
var_dump(g($a), g($a->p));
class W {
    public $n;
    function __construct($n) { $this->n = $n; }
    function __destruct() { echo "destruct {$this->n}\n"; }
}
function w() { return new W(1); }
function cmp_w() { return w() === null; }
var_dump(cmp_w());
echo "after\n";
function cmp_enum() { return E::A === E::from('a'); }
enum E: string { case A = 'a'; case B = 'b'; }
var_dump(cmp_enum());
function gen() { yield 1; }
function cmp_gen() { $g = gen(); return $g === gen(); }
var_dump(cmp_gen());
echo "done\n";
--EXPECT--
bool(true)
bool(true)
bool(false)
bool(true)
bool(false)
bool(true)
bool(true)
bool(false)
bool(false)
bool(true)
bool(false)
bool(false)
bool(true)
destruct 1
bool(false)
after
bool(true)
bool(false)
done
