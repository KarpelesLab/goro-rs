--TEST--
Discarded results of typed, static, closure, generator, by-ref and nested calls: released as php does
--FILE--
<?php
class D { function __construct(public $n) {} function __destruct() { echo "dtor {$this->n}\n"; } }
class A extends D {
  function m(): D { return new D("m-result"); }
  function r(): ?D { $x = new D("r-local"); return $x; }
  function &byref() { static $s; $s = new D("byref"); return $s; }
  function gen() { yield new D("yielded"); }
  static function st() { return new D("static-result"); }
  function thr() { $x = new D("thr-local"); throw new Exception("boom"); }
  function arr() { return [new D("arr-elem")]; }
  function self_() { return $this; }
}
(new A("a1"))->m(); echo "--1\n";
(new A("a2"))->r(); echo "--2\n";
(new A("a3"))->byref(); echo "--3\n";
(new A("a4"))->gen(); echo "--4\n";
foreach ((new A("a5"))->gen() as $v) { echo "in loop\n"; } unset($v); echo "--5\n";
A::st(); echo "--6\n";
try { (new A("a7"))->thr(); } catch (Exception $e) { echo "caught\n"; } echo "--7\n";
(new A("a8"))->arr(); echo "--8\n";
(new A("a9"))->self_(); echo "--9\n";
$f = function () { return new D("closure-result"); }; $f(); echo "--10\n";
(function (D $p) { return new D("iife-result"); })(new D("iife-arg")); echo "--11\n";
call_user_func(fn() => new D("cuf-result")); echo "--12\n";
array_map(fn($x) => new D("map-$x"), [1, 2]); echo "--13\n";
function typed(D $d): D { return new D("typed-result"); }
typed(new D("typed-arg")); echo "--14\n";
function untyped_cv($d) { $r = new D("cv-result"); return $r; }
untyped_cv(new D("cv-arg")); echo "--15\n";
function nested() { return (new A("inner"))->m(); }
nested(); echo "--16\n";
echo "end\n";
--EXPECTF--
dtor m-result
dtor a1
--1
dtor r-local
dtor a2
--2
dtor a3
--3
dtor a4
--4
in loop
dtor a5
dtor yielded
--5
dtor static-result
--6
dtor thr-local
dtor a7
caught
--7
dtor arr-elem
dtor a8
--8
dtor a9
--9
dtor closure-result
--10
dtor iife-result
dtor iife-arg
--11
dtor cuf-result
--12
dtor map-1
dtor map-2
--13
dtor typed-result
dtor typed-arg
--14
dtor cv-arg
dtor cv-result
--15
dtor inner
dtor m-result
--16
end
dtor byref
