--TEST--
stream_select(): user-space streams through stream_cast() (cast again after select), invalid elements throw chained TypeErrors
--FILE--
<?php
class S {
    public $context;
    public $sock;
    static $socks = [];
    function stream_open($p, $m, $o, &$op) { $this->sock = self::$socks[$p]; return true; }
    function stream_cast($as) { echo "cast $as\n"; return $this->sock; }
    function stream_read($n) { return fread($this->sock, $n); }
    function stream_write($d) { return fwrite($this->sock, $d); }
    function stream_eof() { return feof($this->sock); }
}
class N { public $context; function stream_open($p, $m, $o, &$op) { return true; } function stream_cast($as) { return false; } function stream_read($n) { return ''; } function stream_eof() { return true; } }
class B { public $context; function stream_open($p, $m, $o, &$op) { return true; } function stream_cast($as) { return 42; } }
stream_wrapper_register('uu', 'S');
stream_wrapper_register('nn', 'N');
stream_wrapper_register('bb', 'B');
$p1 = stream_socket_pair(STREAM_PF_UNIX, STREAM_SOCK_STREAM, 0);
$p2 = stream_socket_pair(STREAM_PF_UNIX, STREAM_SOCK_STREAM, 0);
S::$socks['uu://a'] = $p1[1];
S::$socks['uu://b'] = $p2[1];
$a = fopen('uu://a', 'r+');
$b = fopen('uu://b', 'r+');
fwrite($p2[0], "hello\nworld\n");
$r = ['x' => $a, 'y' => $b]; $w = ['z' => $a]; $e = null;
var_dump(stream_select($r, $w, $e, 0), array_keys($r), array_keys($w));
var_dump(fgets($b));
$r = [$a, $b]; $w = [$a]; $e = [];
var_dump(stream_select($r, $w, $e, 0), $r === [1 => $b], $w, $e);
$n = fopen('nn://x', 'r');
$r = [$n]; $w = $e = null;
try { var_dump(stream_select($r, $w, $e, 0)); } catch (Throwable $ex) { echo get_class($ex), ": ", $ex->getMessage(), "\n"; }
$r = [$n, $a]; $w = $e = null;
var_dump(stream_select($r, $w, $e, 0), $r);
$bb = fopen('bb://x', 'r');
$r = [$bb]; $w = $e = null;
try { var_dump(stream_select($r, $w, $e, 0)); } catch (Throwable $ex) { echo get_class($ex), ": ", $ex->getMessage(), "\n"; }
$r = [$a, 'str', 5]; $w = $e = null;
try { var_dump(stream_select($r, $w, $e, 0)); } catch (Throwable $ex) { echo get_class($ex), ": ", $ex->getMessage(), "\n", get_class($ex->getPrevious() ?? new stdClass), "\n"; }
$c = fopen('php://memory', 'r');
fclose($c);
$r = [$c]; $w = $e = null;
try { var_dump(stream_select($r, $w, $e, 0)); } catch (Throwable $ex) { echo get_class($ex), ": ", $ex->getMessage(), " / ", $ex->getPrevious()?->getMessage(), "\n"; }
$r = [fopen('php://memory', 'r')]; $w = $e = null;
try { var_dump(stream_select($r, $w, $e, 0)); } catch (Throwable $ex) { echo get_class($ex), ": ", $ex->getMessage(), "\n"; }
$ctx = stream_context_create();
$r = [$ctx]; $w = $e = null;
try { var_dump(stream_select($r, $w, $e, 0)); } catch (Throwable $ex) { echo get_class($ex), ": ", $ex->getMessage(), " / ", $ex->getPrevious()?->getMessage(), "\n"; }
$r = [$p1[0]]; $w = [$p1[0]]; $e = [$p1[0]];
var_dump(stream_select($r, $w, $e, 0), count($r), count($w), count($e));
$r = [$p1[0], $p1[0]]; $w = $e = null;
fwrite($p1[1], "x");
var_dump(stream_select($r, $w, $e, 0), count($r));
?>
--EXPECTF--
cast 3
cast 3
cast 3
cast 3
cast 3
cast 3
int(2)
array(1) {
  [0]=>
  string(1) "y"
}
array(1) {
  [0]=>
  string(1) "z"
}
string(6) "hello
"
cast 3
cast 3
cast 3
int(1)
bool(true)
array(0) {
}
array(0) {
}

Warning: stream_select(): Cannot represent a stream of type user-space as a select()able descriptor in %s on line %d
ValueError: No stream arrays were passed

Warning: stream_select(): Cannot represent a stream of type user-space as a select()able descriptor in %s on line %d
cast 3

Warning: stream_select(): Cannot represent a stream of type user-space as a select()able descriptor in %s on line %d
cast 3
int(0)
array(0) {
}

Warning: stream_select(): B::stream_cast must return a stream resource in %s on line %d

Warning: stream_select(): Cannot represent a stream of type user-space as a select()able descriptor in %s on line %d
ValueError: No stream arrays were passed
cast 3

Warning: stream_select(): Cannot represent a stream of type user-space as a select()able descriptor in %s on line %d
TypeError: stream_select(): supplied argument is not a valid stream resource
TypeError
ValueError: No stream arrays were passed / stream_select(): supplied resource is not a valid stream resource

Warning: stream_select(): Cannot represent a stream of type MEMORY as a select()able descriptor in %s on line %d
ValueError: No stream arrays were passed
ValueError: No stream arrays were passed / stream_select(): supplied resource is not a valid stream resource
int(1)
int(0)
int(1)
int(0)
int(1)
int(2)
