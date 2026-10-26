--TEST--
mb_ereg_replace() family: argument count is checked first
--FILE--
<?php
foreach (["mb_ereg_replace", "mb_eregi_replace", "mb_ereg_replace_callback"] as $f) {
  foreach ([[], ["a"], [null, null], ["a", "b", "c", "d", "e"]] as $a) {
    try { var_dump($f(...$a)); } catch (Throwable $e) { echo get_class($e), ": ", $e->getMessage(), "\n"; }
  }
}
--EXPECT--
ArgumentCountError: mb_ereg_replace() expects at least 3 arguments, 0 given
ArgumentCountError: mb_ereg_replace() expects at least 3 arguments, 1 given
ArgumentCountError: mb_ereg_replace() expects at least 3 arguments, 2 given
ArgumentCountError: mb_ereg_replace() expects at most 4 arguments, 5 given
ArgumentCountError: mb_eregi_replace() expects at least 3 arguments, 0 given
ArgumentCountError: mb_eregi_replace() expects at least 3 arguments, 1 given
ArgumentCountError: mb_eregi_replace() expects at least 3 arguments, 2 given
ArgumentCountError: mb_eregi_replace() expects at most 4 arguments, 5 given
ArgumentCountError: mb_ereg_replace_callback() expects at least 3 arguments, 0 given
ArgumentCountError: mb_ereg_replace_callback() expects at least 3 arguments, 1 given
ArgumentCountError: mb_ereg_replace_callback() expects at least 3 arguments, 2 given
ArgumentCountError: mb_ereg_replace_callback() expects at most 4 arguments, 5 given
