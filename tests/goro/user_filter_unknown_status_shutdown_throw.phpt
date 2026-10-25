--TEST--
user filters: an unknown status reads on, unconsumed input buckets are handed in again, an exception while the streams are freed is fatal
--FILE--
<?php
class R extends php_user_filter {
    function filter($in, $out, &$consumed, bool $closing): int { echo "filter closing=", (int)$closing, "\n"; return 12345; }
}
stream_filter_register('ret', 'R');
$f = fopen('php://memory', 'w+');
fwrite($f, "data"); rewind($f);
stream_filter_append($f, 'ret');
echo "read\n";
var_dump(fread($f, 10));
var_dump(feof($f));
var_dump(fread($f, 10));
class T extends php_user_filter {
    function filter($in, $out, &$consumed, bool $closing): int { echo "T closing=", (int)$closing, "\n"; throw new Exception("in filter"); }
    function onClose(): void { echo "T close\n"; }
}
stream_filter_register('thr', 'T');
$f = fopen('php://memory', 'w+');
fwrite($f, "data"); rewind($f);
stream_filter_append($f, 'thr');
try { var_dump(fread($f, 10)); } catch (Exception $e) { echo "caught ", $e->getMessage(), "\n"; }
echo "end\n";
?>
--EXPECTF--
read
filter closing=0

Warning: fread(): Unprocessed filter buckets remaining on input brigade in %s on line %d
filter closing=1

Warning: fread(): Unprocessed filter buckets remaining on input brigade in %s on line %d
string(0) ""
bool(true)
string(0) ""
filter closing=1
T closing=0

Warning: fread(): Unprocessed filter buckets remaining on input brigade in %s on line %d
caught in filter
end
T closing=1

Fatal error: Uncaught Exception: in filter in %s:14
Stack trace:
#0 [internal function]: T->filter(Resource id #%d, Resource id #%d, 0, true)
#1 {main}
  thrown in %s on line %d
