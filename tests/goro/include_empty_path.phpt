--TEST--
include / require of an empty path: ValueError
--FILE--
<?php
function t($f) { try { var_dump($f()); } catch (Throwable $e) { echo get_class($e), ": ", $e->getMessage(), "\n"; } }
t(fn() => include "");
t(fn() => require "");
t(fn() => include null);
t(fn() => include false);
$e = "";
t(fn() => include $e);
echo "done\n";
--EXPECTF--
ValueError: Path must not be empty
ValueError: Path must not be empty
ValueError: Path must not be empty
ValueError: Path must not be empty
ValueError: Path must not be empty
done
