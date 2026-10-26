--TEST--
Generator methods called while it runs see its last yielded value; rewind/getReturn throw
--FILE--
<?php
function g1() { global $gen; yield 1;
 try { $gen->throw(new Exception("x")); } catch (Error $e) { echo $e->getMessage(), "\n"; }
 try { var_dump($gen->valid()); } catch (Error $e) { echo "v: ", $e->getMessage(), "\n"; }
 try { var_dump($gen->current()); } catch (Error $e) { echo "c: ", $e->getMessage(), "\n"; }
 try { var_dump($gen->key()); } catch (Error $e) { echo "k: ", $e->getMessage(), "\n"; }
 try { var_dump($gen->getReturn()); } catch (Throwable $e) { echo "gr: ", get_class($e), " ", $e->getMessage(), "\n"; }
 try { $gen->rewind(); } catch (Throwable $e) { echo "rw: ", get_class($e), " ", $e->getMessage(), "\n"; }
 yield 2; }
$gen = g1();
foreach ($gen as $v) { echo $v, "\n"; }
function g0() { global $gen0; try { $gen0->rewind(); } catch (Throwable $e) { echo "rw0: ", get_class($e), " ", $e->getMessage(), "\n"; }
  try { var_dump($gen0->current()); } catch (Error $e) { echo "c: ", $e->getMessage(), "\n"; }
  yield 1; }
$gen0 = g0();
foreach ($gen0 as $v) { echo $v, "\n"; }
// nested: outer delegating to inner which resumes outer
function inner() { global $outer; yield 1; try { $outer->next(); } catch (Error $e) { echo "inner: ", $e->getMessage(), "\n"; } yield 2; }
function outerf() { yield from inner(); }
$outer = outerf();
foreach ($outer as $v) echo $v, "\n";
// generator destroyed while running
function selfdestroy() { global $sd; yield 1; $sd = null; echo "still running\n"; yield 2; }
$sd = selfdestroy();
foreach ($sd as $v) echo "v$v\n";
echo "end\n";
--EXPECT--
1
Cannot resume an already running generator
bool(true)
int(1)
int(0)
gr: Exception Cannot get return value of a generator that hasn't returned
rw: Exception Cannot rewind a generator that was already run
2
rw0: Error Cannot resume an already running generator
c: Cannot resume an already running generator
1
1
inner: Cannot resume an already running generator
2
v1
still running
v2
end
