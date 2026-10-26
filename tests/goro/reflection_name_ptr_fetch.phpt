--TEST--
Reflection name/class: op=, ++ and references modify the property in place (get_property_ptr_ptr)
--FILE--
<?php
$rc = new ReflectionClass('stdClass');
try { $rc->name[] = 1; } catch (Throwable $e) { echo get_class($e), ": ", $e->getMessage(), "\n"; }
$rc->name++;
var_dump($rc->name);
$rc->name .= "!";
var_dump($rc->name);
$r = &$rc->name;
$r = "changed";
var_dump($rc->name);
try { $rc->name = "x"; } catch (Throwable $e) { echo get_class($e), ": ", $e->getMessage(), "\n"; }
$rm = new ReflectionMethod('ArrayObject', 'count');
$rm->class .= "x";
var_dump($rm->class);
try { $rm->name = "x"; } catch (Throwable $e) { echo get_class($e), ": ", $e->getMessage(), "\n"; }
try { unset($rm->name); } catch (Throwable $e) { echo get_class($e), ": ", $e->getMessage(), "\n"; }
$d = new DatePeriod(new DateTime('2020-01-01'), new DateInterval('P1D'), 2);
try { $d->recurrences++; } catch (Throwable $e) { echo get_class($e), ": ", $e->getMessage(), "\n"; }
try { $d->recurrences .= "1"; } catch (Throwable $e) { echo get_class($e), ": ", $e->getMessage(), "\n"; }
try { $r = &$d->recurrences; } catch (Throwable $e) { echo get_class($e), ": ", $e->getMessage(), "\n"; }
--EXPECTF--
Error: [] operator not supported for strings

Deprecated: Increment on non-numeric string is deprecated, use str_increment() instead in %s on line 4
string(8) "stdClast"
string(9) "stdClast!"
string(7) "changed"
ReflectionException: Cannot set read-only property ReflectionClass::$name
string(12) "ArrayObjectx"
ReflectionException: Cannot set read-only property ReflectionMethod::$name
Error: Cannot modify readonly property DatePeriod::$recurrences
Error: Cannot modify readonly property DatePeriod::$recurrences
Error: Cannot modify readonly property DatePeriod::$recurrences
