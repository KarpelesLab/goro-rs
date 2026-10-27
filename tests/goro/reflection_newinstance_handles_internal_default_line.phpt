--TEST--
ReflectionClass::newInstance*() make the object before refusing it; internal default values evaluated on the calling line
--FILE--
<?php
class C { protected function __construct() {} }
class E {}
$rc = new ReflectionClass('C');
$re = new ReflectionClass('E');
try { $rc->newInstance(); } catch (ReflectionException $e) { echo $e->getMessage(), "\n"; }
try { $rc->newInstanceArgs([]); } catch (ReflectionException $e) { echo $e->getMessage(), "\n"; }
try { $re->newInstance(1); } catch (ReflectionException $e) { echo $e->getMessage(), "\n"; }
try { $re->newInstanceArgs([1]); } catch (ReflectionException $e) { echo $e->getMessage(), "\n"; }
var_dump($re->newInstance(), new stdClass, spl_object_id($e));
$p = (new ReflectionFunction('date_sunrise'))->getParameters()[1];
var_dump(
    $p->getDefaultValue()
);
--EXPECTF--
Access to non-public constructor of class C
Access to non-public constructor of class C
Class E does not have a constructor, so you cannot pass any constructor arguments
Class E does not have a constructor, so you cannot pass any constructor arguments
object(E)#3 (0) {
}
object(stdClass)#5 (0) {
}
int(4)

Deprecated: Constant SUNFUNCS_RET_STRING is deprecated since 8.4, as date_sunrise() and date_sunset() were deprecated in 8.1 in %s on line 13
int(1)
