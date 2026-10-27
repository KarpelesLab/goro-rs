--TEST--
A multi-line arrow function's ReflectionFunction::getEndLine() covers its whole body (serializable closures read its source)
--FILE--
<?php
$tasks = [
    fn () => throw new Exception(
        "x",
    ),
    fn () => throw new Exception(
        "x"
    ),
    fn () => foo(
        "x",
    ),
    fn () => 1 +
      2,
  function () {
  },
];
foreach ($tasks as $t) { $r = new ReflectionFunction($t); echo $r->getStartLine(), "-", $r->getEndLine(), "\n"; }

?>
--EXPECT--
3-5
6-8
9-11
12-13
14-15
