--TEST--
foreach over an uninitialized DatePeriod: the get_iterator iterator's rewind has no frame
--FILE--
<?php
class P extends DatePeriod { public function __construct() {} }
$p = new P;
try { foreach ($p as $d) {} } catch (Error $e) { echo get_class($e), ": ", $e->getMessage(), "\n", $e->getTraceAsString(), "\n"; }
try { $p->getIterator(); } catch (Error $e) { echo get_class($e), ": ", $e->getMessage(), "\n", $e->getTraceAsString(), "\n"; }
try { iterator_to_array($p); } catch (Error $e) { echo get_class($e), ": ", $e->getMessage(), "\n", $e->getTraceAsString(), "\n"; }
$r = (new ReflectionClass('DatePeriod'))->newInstanceWithoutConstructor();
try { foreach ($r as $d) {} } catch (Error $e) { echo get_class($e), ": ", $e->getMessage(), "\n"; }
foreach (new P as $x) {}
?>
--EXPECTF--
DateObjectError: Object of type DatePeriod has not been correctly initialized by calling parent::__construct() in its constructor
#0 {main}
DateObjectError: Object of type DatePeriod has not been correctly initialized by calling parent::__construct() in its constructor
#0 %s(6): iterator_to_array(Object(P))
#1 {main}
DateObjectError: Object of type DatePeriod has not been correctly initialized by calling parent::__construct() in its constructor

Fatal error: Uncaught DateObjectError: Object of type DatePeriod has not been correctly initialized by calling parent::__construct() in its constructor in %s:9
Stack trace:
#0 {main}
  thrown in %s on line 9
