--TEST--
foreach over a non-SPL native iterator (DatePeriod): its rewind() gets no backtrace frame
--FILE--
<?php
class P extends DatePeriod {
    public function __construct() { }
}
try {
    foreach (new P() as $d) {}
} catch (Error $e) {
    echo get_class($e), ": ", $e->getMessage(), "\n", $e->getTraceAsString(), "\n";
}
?>
--EXPECT--
DateObjectError: Object of type DatePeriod has not been correctly initialized by calling parent::__construct() in its constructor
#0 {main}
