--TEST--
Destructors run by the cycle collector run in the declaring class's scope (private properties) and name it in traces
--FILE--
<?php
class P {
    private array $expected = [];
    public $self;
    public function __destruct() {
        echo static::class, " dtor ", count($this->expected), "\n";
        echo json_encode(array_map(fn ($f) => ($f['class'] ?? '') . ($f['type'] ?? '') . $f['function'], debug_backtrace(DEBUG_BACKTRACE_IGNORE_ARGS))), "\n";
    }
}
class S extends P {}
class Q {
    public $x = 1;
    function m() {
        $s = new S;
        $s->self = $s;
        unset($s);
        gc_collect_cycles();
        echo "after gc\n";
    }
}
(new Q)->m();
$s = new S;
$s->self = $s;
$r = new ReflectionObject($s);
unset($s, $r);
$o = new class { function __destruct() { echo "anon\n"; } };
gc_collect_cycles();
echo "end\n";

?>
--EXPECT--
S dtor 0
["P->__destruct","gc_collect_cycles","Q->m"]
after gc
S dtor 0
["P->__destruct","gc_collect_cycles"]
end
anon
