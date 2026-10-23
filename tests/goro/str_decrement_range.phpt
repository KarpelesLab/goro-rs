--TEST--
str_decrement(): leading '0' is out of range, borrow drops the first character
--FILE--
<?php
foreach (['b', 'Ab', 'aa', 'Aa', 'a0', '10', '1', 'zA', 'Za', '0x', '0', 'a', 'A', '9z', 'a-b', ''] as $s) {
    try {
        echo "$s => ", var_export(str_decrement($s), true), "\n";
    } catch (\ValueError $e) {
        echo $e->getMessage(), "\n";
    }
}
--EXPECT--
b => 'a'
Ab => 'Aa'
aa => 'z'
Aa => 'z'
a0 => '9'
10 => '9'
1 => '0'
zA => 'yZ'
Za => 'Yz'
0x => str_decrement(): Argument #1 ($string) "0x" is out of decrement range
0 => str_decrement(): Argument #1 ($string) "0" is out of decrement range
a => str_decrement(): Argument #1 ($string) "a" is out of decrement range
A => str_decrement(): Argument #1 ($string) "A" is out of decrement range
9z => '9y'
a-b => str_decrement(): Argument #1 ($string) must be composed only of alphanumeric ASCII characters
 => str_decrement(): Argument #1 ($string) must not be empty
