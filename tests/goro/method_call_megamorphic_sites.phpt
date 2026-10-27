--TEST--
Method calls of a site meeting many classes (a visitor loop), from several scopes: visibility, private shadowing, __call, missing methods
--FILE--
<?php
abstract class V { public function enter($n) { return static::class . ":$n"; } }
class V0 extends V {}
class V1 extends V { public function enter($n) { return "own1:$n"; } }
class V2 extends V { protected function hidden() { return 'h2'; } }
class V3 extends V { public function __call($m, $a) { return "call3:$m"; } }
class V4 extends V {}
class V5 extends V { public function enter($n) { return "own5:" . $this->peek(); } private function peek() { return 'p5'; } }
class V6 extends V {}
class V7 extends V {}
class V8 { public function enter($n) { return "plain8:$n"; } }
class V9 extends V8 {}
class Caller {
    private function enter($n) { return "caller-private:$n"; }
    function run(array $vs, $n) {
        $out = [];
        foreach ($vs as $v) {
            $out[] = $v->enter($n);
            try { $out[] = $v->hidden(); } catch (Error $e) { $out[] = 'E:' . $e->getMessage(); }
        }
        return implode(' ', $out);
    }
}
class Sub extends Caller {}
function run(array $vs, $n) {
    $out = [];
    foreach ($vs as $v) {
        try { $out[] = $v->enter($n); } catch (Error $e) { $out[] = 'E:' . $e->getMessage(); }
        try { $out[] = $v->hidden(); } catch (Error $e) { $out[] = 'E:' . $e->getMessage(); }
    }
    return implode(' ', $out);
}
$vs = [new V0, new V1, new V2, new V3, new V4, new V5, new V6, new V7, new V8, new V9, new Caller];
for ($i = 0; $i < 3; $i++) {
    echo run($vs, $i), "\n";
    echo (new Caller)->run($vs, $i), "\n";
    echo (new Sub)->run($vs, $i), "\n";
}
?>
--EXPECT--
V0:0 E:Call to undefined method V0::hidden() own1:0 E:Call to undefined method V1::hidden() V2:0 E:Call to protected method V2::hidden() from global scope V3:0 call3:hidden V4:0 E:Call to undefined method V4::hidden() own5:p5 E:Call to undefined method V5::hidden() V6:0 E:Call to undefined method V6::hidden() V7:0 E:Call to undefined method V7::hidden() plain8:0 E:Call to undefined method V8::hidden() plain8:0 E:Call to undefined method V9::hidden() E:Call to private method Caller::enter() from global scope E:Call to undefined method Caller::hidden()
V0:0 E:Call to undefined method V0::hidden() own1:0 E:Call to undefined method V1::hidden() V2:0 E:Call to protected method V2::hidden() from scope Caller V3:0 call3:hidden V4:0 E:Call to undefined method V4::hidden() own5:p5 E:Call to undefined method V5::hidden() V6:0 E:Call to undefined method V6::hidden() V7:0 E:Call to undefined method V7::hidden() plain8:0 E:Call to undefined method V8::hidden() plain8:0 E:Call to undefined method V9::hidden() caller-private:0 E:Call to undefined method Caller::hidden()
V0:0 E:Call to undefined method V0::hidden() own1:0 E:Call to undefined method V1::hidden() V2:0 E:Call to protected method V2::hidden() from scope Caller V3:0 call3:hidden V4:0 E:Call to undefined method V4::hidden() own5:p5 E:Call to undefined method V5::hidden() V6:0 E:Call to undefined method V6::hidden() V7:0 E:Call to undefined method V7::hidden() plain8:0 E:Call to undefined method V8::hidden() plain8:0 E:Call to undefined method V9::hidden() caller-private:0 E:Call to undefined method Caller::hidden()
V0:1 E:Call to undefined method V0::hidden() own1:1 E:Call to undefined method V1::hidden() V2:1 E:Call to protected method V2::hidden() from global scope V3:1 call3:hidden V4:1 E:Call to undefined method V4::hidden() own5:p5 E:Call to undefined method V5::hidden() V6:1 E:Call to undefined method V6::hidden() V7:1 E:Call to undefined method V7::hidden() plain8:1 E:Call to undefined method V8::hidden() plain8:1 E:Call to undefined method V9::hidden() E:Call to private method Caller::enter() from global scope E:Call to undefined method Caller::hidden()
V0:1 E:Call to undefined method V0::hidden() own1:1 E:Call to undefined method V1::hidden() V2:1 E:Call to protected method V2::hidden() from scope Caller V3:1 call3:hidden V4:1 E:Call to undefined method V4::hidden() own5:p5 E:Call to undefined method V5::hidden() V6:1 E:Call to undefined method V6::hidden() V7:1 E:Call to undefined method V7::hidden() plain8:1 E:Call to undefined method V8::hidden() plain8:1 E:Call to undefined method V9::hidden() caller-private:1 E:Call to undefined method Caller::hidden()
V0:1 E:Call to undefined method V0::hidden() own1:1 E:Call to undefined method V1::hidden() V2:1 E:Call to protected method V2::hidden() from scope Caller V3:1 call3:hidden V4:1 E:Call to undefined method V4::hidden() own5:p5 E:Call to undefined method V5::hidden() V6:1 E:Call to undefined method V6::hidden() V7:1 E:Call to undefined method V7::hidden() plain8:1 E:Call to undefined method V8::hidden() plain8:1 E:Call to undefined method V9::hidden() caller-private:1 E:Call to undefined method Caller::hidden()
V0:2 E:Call to undefined method V0::hidden() own1:2 E:Call to undefined method V1::hidden() V2:2 E:Call to protected method V2::hidden() from global scope V3:2 call3:hidden V4:2 E:Call to undefined method V4::hidden() own5:p5 E:Call to undefined method V5::hidden() V6:2 E:Call to undefined method V6::hidden() V7:2 E:Call to undefined method V7::hidden() plain8:2 E:Call to undefined method V8::hidden() plain8:2 E:Call to undefined method V9::hidden() E:Call to private method Caller::enter() from global scope E:Call to undefined method Caller::hidden()
V0:2 E:Call to undefined method V0::hidden() own1:2 E:Call to undefined method V1::hidden() V2:2 E:Call to protected method V2::hidden() from scope Caller V3:2 call3:hidden V4:2 E:Call to undefined method V4::hidden() own5:p5 E:Call to undefined method V5::hidden() V6:2 E:Call to undefined method V6::hidden() V7:2 E:Call to undefined method V7::hidden() plain8:2 E:Call to undefined method V8::hidden() plain8:2 E:Call to undefined method V9::hidden() caller-private:2 E:Call to undefined method Caller::hidden()
V0:2 E:Call to undefined method V0::hidden() own1:2 E:Call to undefined method V1::hidden() V2:2 E:Call to protected method V2::hidden() from scope Caller V3:2 call3:hidden V4:2 E:Call to undefined method V4::hidden() own5:p5 E:Call to undefined method V5::hidden() V6:2 E:Call to undefined method V6::hidden() V7:2 E:Call to undefined method V7::hidden() plain8:2 E:Call to undefined method V8::hidden() plain8:2 E:Call to undefined method V9::hidden() caller-private:2 E:Call to undefined method Caller::hidden()
