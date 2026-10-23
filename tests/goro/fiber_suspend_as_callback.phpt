--TEST--
Fiber::suspend() called as a callback suspends the fiber
--FILE--
<?php
$tests = [
  'cuf_arr' => function() { return call_user_func(["Fiber", "suspend"], 5); },
  'cuf_str' => function() { return call_user_func("Fiber::suspend", 5); },
  'cufa' => function() { return call_user_func_array("Fiber::suspend", [5]); },
  'dyn_arr' => function() { $c = ["Fiber", "suspend"]; return $c(5); },
  'array_map' => function() { return array_map("Fiber::suspend", [5])[0]; },
  'fromcallable' => function() { return Closure::fromCallable("Fiber::suspend")(5); },
  'refl' => function() { return (new ReflectionMethod("Fiber", "suspend"))->invoke(null, 5); },
];
foreach ($tests as $name => $fn) {
  $f = new Fiber($fn);
  $v = $f->start();
  echo $name, ": ", var_export($v, true), " ", var_export($f->isSuspended(), true);
  $f->resume(7);
  echo " ", var_export($f->getReturn(), true), "\n";
}
$f = new Fiber(Fiber::suspend(...));
$f->start();
$r = new ReflectionFiber($f);
var_dump($r->getExecutingLine(), count($r->getTrace()));
try {
  call_user_func("Fiber::suspend");
} catch (FiberError $e) {
  echo $e->getMessage(), "\n";
}
?>
--EXPECT--
cuf_arr: 5 true 7
cuf_str: 5 true 7
cufa: 5 true 7
dyn_arr: 5 true 7
array_map: 5 true 7
fromcallable: 5 true 7
refl: 5 true 7
NULL
int(1)
Cannot suspend outside of a fiber
