--TEST--
log() special bases; null format of printf()/vprintf()/fprintf() is deprecated
--FILE--
<?php
// log(): base 1 gives NAN silently, bases 2 and 10 use log2()/log10()
var_dump(log(1, 1), log(8, 2), log(1000, 10), log(100, 10), log(27, 3), log(M_E));
try { log(1, 0); } catch (ValueError $e) { echo $e->getMessage(), "\n"; }
// printf() family: null format is deprecated
printf(null);
vprintf(null, []);
fprintf(STDOUT, null);
echo sprintf(null), "|\n";
--EXPECTF--
float(NAN)
float(3)
float(3)
float(2)
float(3)
float(1)
log(): Argument #2 ($base) must be greater than 0

Deprecated: printf(): Passing null to parameter #1 ($format) of type string is deprecated in %s on line 6

Deprecated: vprintf(): Passing null to parameter #1 ($format) of type string is deprecated in %s on line 7

Deprecated: fprintf(): Passing null to parameter #2 ($format) of type string is deprecated in %s on line 8

Deprecated: sprintf(): Passing null to parameter #1 ($format) of type string is deprecated in %s on line 9
|
