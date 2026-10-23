--TEST--
A function's parameters are destroyed with its CVs, in declaration order (before its other locals)
--FILE--
<?php
class D { function __construct(public $n) {} function __destruct() { echo "d{$this->n}\n"; } }
function f($a, $b) { $c = new D(3); $d = new D(4); echo "end\n"; }
f(new D(1), new D(2));
function g($a) { $z = new D('z'); $a2 = new D('a2'); }
g(new D('p'));
function h(D $x, ...$rest) { $l = new D('l'); }
h(new D('x'), new D('r1'), new D('r2'));
class M { function m($p) { $q = new D('q'); } static function s($p) { $q = new D('sq'); } }
(new M)->m(new D('mp'));
M::s(new D('sp'));
$cl = function($p) { $q = new D('cq'); };
$cl(new D('cp'));
function gen($p) { $q = new D('gq'); yield 1; }
foreach (gen(new D('gp')) as $_) {}
function k($p) { $q = new D('kq'); return 1; }
k(new D('kp'));
function u($p = null) { $q = new D('uq'); }
u(new D('up'));
echo "done\n";
function f2($a) { $a = null; echo "after\n"; }
f2(new D(1));
class M2 { function m($a) { $a = null; echo "after m\n"; } }
(new M2)->m(new D(2));
function v2(...$a) { $a = null; echo "after v\n"; }
v2(new D(3));
function gen3($p, $p2) { $q = new D('q'); yield 1; }
$g = gen3(new D('p'), new D('p2'));
foreach ($g as $_) {}
echo "after\n";
unset($g);
$g = gen3(new D('xp'), new D('xp2'));
$g->current();
unset($g);
echo "end\n";
function r(D $p): int { $l = new D('l'); return "x"; }
try { r(new D('p')); } catch (TypeError $e) { echo $e->getMessage(), "\n", $e->getTraceAsString(), "\n"; }
function &rr(array &$a): array { $l = new D('rl'); $a = "s"; return $a; }
$v = [1];
try { $q = &rr($v); } catch (TypeError $e) { echo $e->getMessage(), "\n"; }
function co(): string { $l = new D('cl'); return 5; }
var_dump(co());
--EXPECTF--
end
d1
d2
d3
d4
dp
dz
da2
dx
dl
dr1
dr2
dmp
dq
dsp
dsq
dcp
dcq
dgp
dgq
dkp
dkq
dup
duq
done
d1
after
d2
after m
after v
d3
dp
dp2
dq
after
dxp
dxp2
dq
end
dl
r(): Return value must be of type int, string returned
#0 %s(37): r(Object(D))
#1 {main}
drl
dp
rr(): Return value must be of type array, string returned
dcl
string(1) "5"
