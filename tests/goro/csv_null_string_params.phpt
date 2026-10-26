--TEST--
fputcsv()/fgetcsv()/str_getcsv(): a null separator, enclosure or escape is a deprecated empty string; a null $eol is the default
--FILE--
<?php
$m = fopen("php://memory", "w+");
foreach ([[null], [",", null], [",", '"', null], [",", '"', "\\", null]] as $extra) {
  try { var_dump(fputcsv($m, ["a b", "c"], ...$extra)); } catch (ValueError $e) { echo $e->getMessage(), "\n"; }
  rewind($m);
  try { var_dump(fgetcsv($m, null, ...$extra)); } catch (ValueError $e) { echo $e->getMessage(), "\n"; }
  try { var_dump(str_getcsv("a,b", ...array_slice($extra, 0, 3))); } catch (ValueError $e) { echo $e->getMessage(), "\n"; }
}
?>
--EXPECTF--

Deprecated: fputcsv(): Passing null to parameter #3 ($separator) of type string is deprecated in %s on line %d
fputcsv(): Argument #3 ($separator) must be a single character

Deprecated: fgetcsv(): Passing null to parameter #3 ($separator) of type string is deprecated in %s on line %d
fgetcsv(): Argument #3 ($separator) must be a single character

Deprecated: str_getcsv(): Passing null to parameter #2 ($separator) of type string is deprecated in %s on line %d
str_getcsv(): Argument #2 ($separator) must be a single character

Deprecated: fputcsv(): Passing null to parameter #4 ($enclosure) of type string is deprecated in %s on line %d
fputcsv(): Argument #4 ($enclosure) must be a single character

Deprecated: fgetcsv(): Passing null to parameter #4 ($enclosure) of type string is deprecated in %s on line %d
fgetcsv(): Argument #4 ($enclosure) must be a single character

Deprecated: str_getcsv(): Passing null to parameter #3 ($enclosure) of type string is deprecated in %s on line %d
str_getcsv(): Argument #3 ($enclosure) must be a single character

Deprecated: fputcsv(): Passing null to parameter #5 ($escape) of type string is deprecated in %s on line %d
int(8)

Deprecated: fgetcsv(): Passing null to parameter #5 ($escape) of type string is deprecated in %s on line %d
array(2) {
  [0]=>
  string(3) "a b"
  [1]=>
  string(1) "c"
}

Deprecated: str_getcsv(): Passing null to parameter #4 ($escape) of type string is deprecated in %s on line %d
array(2) {
  [0]=>
  string(1) "a"
  [1]=>
  string(1) "b"
}
int(8)

Fatal error: Uncaught ArgumentCountError: fgetcsv() expects at most 5 arguments, 6 given in %s:6
Stack trace:
#0 %s(6): fgetcsv(Resource id #5, NULL, ',', '"', '\\', NULL)
#1 {main}
  thrown in %s on line %d
