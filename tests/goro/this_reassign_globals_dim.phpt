--TEST--
Writing $GLOBALS["this"] throws "Cannot re-assign $this"
--FILE--
<?php
function g() {
    try {
        $GLOBALS['this'] = 1;
    } catch (Error $e) {
        echo $e->getMessage(), "\n";
    }
    $k = 'th' . 'is';
    try {
        $GLOBALS[$k] .= 1;
    } catch (Error $e) {
        echo $e->getMessage(), "\n";
    }
    try {
        $r = &$GLOBALS['this'];
    } catch (Error $e) {
        echo $e->getMessage(), "\n";
    }
    try {
        $GLOBALS['this'][] = 1;
    } catch (Error $e) {
        echo $e->getMessage(), "\n";
    }
    var_dump(isset($GLOBALS['this']));
    unset($GLOBALS['this']);
}
g();
$GLOBALS['this'] = 1;
echo "unreachable\n";
--EXPECTF--
Cannot re-assign $this
Cannot re-assign $this
Cannot re-assign $this
Cannot re-assign $this
bool(false)

Fatal error: Uncaught Error: Cannot re-assign $this in %s:28
Stack trace:
#0 {main}
  thrown in %s on line 28
