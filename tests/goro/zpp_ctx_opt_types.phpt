--TEST--
stream_context_set_option() / fputcsv() argument type errors
--FILE--
<?php
$ctx = stream_context_create();
try { stream_context_set_option($ctx, new stdClass); } catch (Throwable $e) { echo get_class($e), ": ", $e->getMessage(), "\n"; }
try { checkdnsrr([]); } catch (Throwable $e) { echo get_class($e), ": ", $e->getMessage(), "\n"; }
$m = fopen("php://memory","w+");
try { fputcsv($m, ["a"], ",", '"', "\\", []); } catch (Throwable $e) { echo get_class($e), ": ", $e->getMessage(), "\n"; }
class S { function __toString(): string { return "http"; } }
var_dump(stream_context_set_option($ctx, new S, "method", "POST"));
try { stream_context_set_option($ctx, null, "a", "b"); } catch (Throwable $e) { echo get_class($e), ": ", $e->getMessage(), "\n"; }
var_dump(stream_context_get_options($ctx));
--EXPECTF--

Deprecated: Calling stream_context_set_option() with 2 arguments is deprecated, use stream_context_set_options() instead in %s on line 3
TypeError: stream_context_set_option(): Argument #2 ($wrapper_or_options) must be of type array|string, stdClass given
TypeError: checkdnsrr(): Argument #1 ($hostname) must be of type string, array given
TypeError: fputcsv(): Argument #6 ($eol) must be of type ?string, array given
bool(true)

Deprecated: stream_context_set_option(): Passing null to parameter #2 ($wrapper_or_options) of type array|string is deprecated in %s on line 9
array(2) {
  ["http"]=>
  array(1) {
    ["method"]=>
    string(4) "POST"
  }
  [""]=>
  array(1) {
    ["a"]=>
    string(1) "b"
  }
}
