--TEST--
Static method callbacks and hook-style call_user_func_array() loops
--INI--
zend.exception_ignore_args=0
--FILE--
<?php
// Static method callbacks, and hooks calling their callbacks by
// call_user_func_array() from a method.
class S {
    public static function up($s) { return strtoupper($s) . static::class; }
    protected static function prot($s) { return "prot $s"; }
    private static function priv($s) { return "priv $s"; }
    public static function viaSelf($s) { return call_user_func_array(['S', 'prot'], [$s]) . call_user_func(['S', 'priv'], $s); }
    public static function trace($s) { return (new Exception)->getTraceAsString(); }
}
class T extends S {}
var_dump(call_user_func_array(['S', 'up'], ['a']));
var_dump(call_user_func_array(['T', 'up'], ['b']));
var_dump(call_user_func_array(['t', 'UP'], ['c']));
var_dump(array_map(['T', 'up'], ['d']));
var_dump(S::viaSelf('e'));
try { call_user_func(['S', 'prot'], 'x'); } catch (Error $e) { echo get_class($e), ': ', $e->getMessage(), "\n"; }
echo call_user_func_array(['T', 'trace'], ['t']), "\n";
echo call_user_func_array('S::trace', ['u']), "\n";

class Hook {
    public $callbacks = [];
    public $nesting = 0;
    public function apply($value, $args) {
        $n = $this->nesting++;
        foreach ($this->callbacks as $the_) {
            $args[0] = $value;
            if ($the_['accepted_args'] === 0) {
                $value = call_user_func($the_['function']);
            } elseif ($the_['accepted_args'] >= count($args)) {
                $value = call_user_func_array($the_['function'], $args);
            } else {
                $value = call_user_func_array($the_['function'], array_slice($args, 0, $the_['accepted_args']));
            }
        }
        --$this->nesting;
        return $value;
    }
}
class Plugin {
    public $count = 0;
    public function filter($v) { $this->count++; return $v . '!'; }
    public static function sfilter($v, $x) { return "$v($x)"; }
    public function trace($v) { $t = debug_backtrace(); return $v . count($t) . $t[1]['function'] . ($t[1]['line'] ?? '-'); }
}
$p = new Plugin;
$h = new Hook;
$h->callbacks[] = ['function' => [$p, 'filter'], 'accepted_args' => 1];
$h->callbacks[] = ['function' => ['Plugin', 'sfilter'], 'accepted_args' => 2];
$h->callbacks[] = ['function' => 'strrev', 'accepted_args' => 1];
$h->callbacks[] = ['function' => fn($v) => "<$v>", 'accepted_args' => 1];
$h->callbacks[] = ['function' => [$p, 'trace'], 'accepted_args' => 1];
for ($i = 0; $i < 4; $i++) {
    var_dump($h->apply("v$i", [null, 'x']));
}
var_dump($p->count);
function apply_all($h, $v) { return $h->apply($v, [null, 'y']); }
for ($i = 0; $i < 3; $i++) var_dump(apply_all($h, $i));
$h2 = new Hook;
$h2->callbacks[] = ['function' => function ($v) { throw new LogicException("from hook $v"); }, 'accepted_args' => 1];
try { $h2->apply('z', [null]); } catch (LogicException $e) { echo $e->getMessage(), "\n", $e->getTraceAsString(), "\n"; }
--EXPECTF--
string(2) "AS"
string(2) "BT"
string(2) "CT"
array(1) {
  [0]=>
  string(2) "DT"
}
string(12) "prot epriv e"
TypeError: call_user_func(): Argument #1 ($callback) must be a valid callback, cannot access protected method S::prot()
#0 %s(18): S::trace('t')
#1 {main}
#0 %s(19): S::trace('u')
#1 {main}
string(16) "<)x(!0v>2apply54"
string(16) "<)x(!1v>2apply54"
string(16) "<)x(!2v>2apply54"
string(16) "<)x(!3v>2apply54"
int(4)
string(15) "<)y(!0>3apply57"
string(15) "<)y(!1>3apply57"
string(15) "<)y(!2>3apply57"
from hook z
#0 %s(31): {closure:%s:60}('z')
#1 %s(61): Hook->apply('z', Array)
#2 {main}
