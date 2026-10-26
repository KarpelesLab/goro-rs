--TEST--
in_array()/array_search()/array_keys() go on comparing after a __toString() exception
--FILE--
<?php
class T { function __toString(): string { echo "T called\n"; throw new Exception("boom"); } }
try { var_dump(in_array(new T, ["a", 1, "b", 2.5, new stdClass])); } catch (Exception $e) { echo $e->getMessage(), "\n"; }
try { var_dump(in_array("a", [new T, 1, new T, new stdClass, 5])); } catch (Exception $e) { echo $e->getMessage(), "\n"; }
try { var_dump(in_array(5, [new T, new stdClass, 5])); } catch (Exception $e) { echo $e->getMessage(), "\n"; }
try { var_dump(array_search(new T, ["a", 1, "b"])); } catch (Exception $e) { echo $e->getMessage(), "\n"; }
try { var_dump(array_keys(["a", 1, "b"], new T)); } catch (Exception $e) { echo $e->getMessage(), "\n"; }
--EXPECTF--
T called

Notice: Object of class T could not be converted to int in %s on line 3
boom
T called
boom

Notice: Object of class T could not be converted to int in %s on line 5

Notice: Object of class stdClass could not be converted to int in %s on line 5
bool(true)
T called

Notice: Object of class T could not be converted to int in %s on line 6
boom
T called

Notice: Object of class T could not be converted to int in %s on line 7
boom
