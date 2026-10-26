--TEST--
new static / instanceof static / static::m(...) in a function outside a class are compile errors
--FILE--
<?php
foreach (['return new static;', 'return $x instanceof self;', 'return static::m(...);'] as $body) {
    $f = tempnam(sys_get_temp_dir(), 'nsc');
    file_put_contents($f, "<?php\necho 'not reached';\nfunction f() { $body }\n");
    $out = shell_exec(escapeshellarg(PHP_BINARY) . ' -n ' . escapeshellarg($f) . ' 2>&1');
    echo trim(preg_replace('/ in .* on line \d+.*/s', '', $out)), "\n";
    unlink($f);
}
?>
--EXPECT--
Fatal error: Cannot use "static" when no class scope is active
Fatal error: Cannot use "self" when no class scope is active
Fatal error: Cannot use "static" when no class scope is active
