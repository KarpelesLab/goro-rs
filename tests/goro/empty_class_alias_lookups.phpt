--TEST--
Lookups of an empty class name fail even with an empty alias
--FILE--
<?php
var_dump(class_alias('stdClass', ''));
var_dump(class_exists(''));
var_dump(class_exists('', false));
var_dump(is_subclass_of('', 'stdClass'));
var_dump(is_a('', 'stdClass', true));
try { var_dump(get_parent_class('')); } catch (Throwable $e) { echo get_class($e), ": ", $e->getMessage(), "\n"; }
try { var_dump(get_class_methods('')); } catch (Throwable $e) { echo get_class($e), ": ", $e->getMessage(), "\n"; }
try { var_dump(get_class_vars('')); } catch (Throwable $e) { echo get_class($e), ": ", $e->getMessage(), "\n"; }
var_dump(new (''));
--EXPECT--
bool(true)
bool(false)
bool(true)
bool(false)
bool(false)
TypeError: get_parent_class(): Argument #1 ($object_or_class) must be an object or a valid class name, string given
TypeError: get_class_methods(): Argument #1 ($object_or_class) must be an object or a valid class name, string given
TypeError: get_class_vars(): Argument #1 ($class) must be a valid class name,  given
object(stdClass)#2 (0) {
}
