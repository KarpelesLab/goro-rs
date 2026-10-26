--TEST--
sprintf(): a huge field width grows the buffer up to the memory limit
--FILE--
<?php
try { var_dump(sprintf("%2147483648d", 1)); } catch (Throwable $e) { echo get_class($e), ": ", $e->getMessage(), "\n"; }
try { var_dump(sprintf("%2147483647d", 1)); } catch (Throwable $e) { echo get_class($e), ": ", $e->getMessage(), "\n"; }
$f = "%2147483646d";
try { var_dump(strlen(sprintf($f, 1))); } catch (Throwable $e) { echo get_class($e), ": ", $e->getMessage(), "\n"; }
try { var_dump(sprintf("%.2147483648f", 1)); } catch (Throwable $e) { echo get_class($e), ": ", $e->getMessage(), "\n"; }
try { var_dump(sprintf("%1\$999999999999s", 1)); } catch (Throwable $e) { echo get_class($e), ": ", $e->getMessage(), "\n"; }
try { var_dump(sprintf("%99999999999\$s", 1)); } catch (Throwable $e) { echo get_class($e), ": ", $e->getMessage(), "\n"; }
--EXPECTF--
ValueError: Width must be between 0 and 2147483647
ValueError: Width must be between 0 and 2147483647

Fatal error: Allowed memory size of 134217728 bytes exhausted (tried to allocate 4026531872 bytes) in %s on line 5
