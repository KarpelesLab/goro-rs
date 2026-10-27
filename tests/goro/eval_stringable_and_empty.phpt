--TEST--
eval() of a Stringable object runs its __toString() code; of an empty string returns false
--FILE--
<?php
class S { function __toString(): string { return ' echo 1+2, "\n"; '; } }
eval(new S);
$x = new S;
eval($x);
var_dump(eval(new class { function __toString(): string { return 'return 42;'; } }));
try {
    eval(new stdClass);
} catch (Error $e) {
    echo get_class($e), ': ', $e->getMessage(), "\n";
}
class T { function __toString(): string { throw new RuntimeException('no'); } }
try {
    eval(new T);
} catch (Exception $e) {
    echo get_class($e), ': ', $e->getMessage(), "\n";
}
var_dump(eval(42 . ';'));
var_dump(eval(null));
echo "done\n";
var_dump(eval(""));
var_dump(eval(false));

?>
--EXPECT--
3
3
int(42)
Error: Object of class stdClass could not be converted to string
RuntimeException: no
NULL
bool(false)
done
bool(false)
bool(false)
