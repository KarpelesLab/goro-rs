--TEST--
ReflectionProperty on internal classes; DateTime view props are not dynamic
--FILE--
<?php
try { new ReflectionProperty('stdClass', 'x'); } catch (Exception $e) { echo get_class($e), ": ", $e->getMessage(), "\n"; }
try { new ReflectionProperty('ArrayObject', 'x'); } catch (Exception $e) { echo get_class($e), ": ", $e->getMessage(), "\n"; }
try { new ReflectionProperty('DateTime', 'date'); } catch (Exception $e) { echo get_class($e), ": ", $e->getMessage(), "\n"; }
$p = new ReflectionProperty('Exception', 'message'); var_dump($p->class, $p->name);
$o = new stdClass; $o->x = 1;
$p = new ReflectionProperty($o, 'x'); var_dump($p->class, $p->isDefault());
var_dump((new ReflectionObject(new DateTime))->getProperties());
var_dump((new ReflectionClass('DateTime'))->getProperties());
var_dump(count((new ReflectionObject(new DateTimeZone('UTC')))->getProperties()));
var_dump(count((new ReflectionObject(new DateInterval('P1D')))->getProperties()));
var_dump((new ReflectionObject(new DateTime))->hasProperty('date'));
$d = new DateTime; $d->foo = 1;
foreach ((new ReflectionObject($d))->getProperties() as $p) echo $p->name, "\n";
--EXPECTF--
ReflectionException: Property stdClass::$x does not exist
ReflectionException: Property ArrayObject::$x does not exist
ReflectionException: Property DateTime::$date does not exist
string(9) "Exception"
string(7) "message"
string(8) "stdClass"
bool(false)
array(0) {
}
array(0) {
}
int(0)
int(10)
bool(false)

Deprecated: Creation of dynamic property DateTime::$foo is deprecated in %s on line 13
foo
