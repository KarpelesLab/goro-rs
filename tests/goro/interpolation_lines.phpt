--TEST--
Warnings of interpolated strings' parts and escapes are on their own lines
--FILE--
<?php
$a = <<<TEST
foo
$x
bar {$y}
\400 $z[1]
TEST;
$b = "a
$x
\400 {$y}
";
$c = <<<TEST
  x
    {$u}
  \400
  TEST;
echo "done\n";
--EXPECTF--

Warning: Octal escape sequence overflow \400 is greater than \377 in %s on line 6

Warning: Octal escape sequence overflow \400 is greater than \377 in %s on line 10

Warning: Octal escape sequence overflow \400 is greater than \377 in %s on line 15

Warning: Undefined variable $x in %s on line 4

Warning: Undefined variable $y in %s on line 5

Warning: Undefined variable $z in %s on line 6

Warning: Trying to access array offset on null in %s on line 6

Warning: Undefined variable $x in %s on line 9

Warning: Undefined variable $y in %s on line 10

Warning: Undefined variable $u in %s on line 14
done
