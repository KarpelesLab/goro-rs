--TEST--
iconv(): EUC-TW CNS 11643 planes 3-7 and 15
--FILE--
<?php
// EUC-TW (glibc): CNS 11643 planes 1-7 and 15 (0x8E 0xA1-0xA7 / 0xAF);
// //IGNORE skips both bytes of an unassigned two-byte pair
function d($cs, $s) { $r = @iconv($cs, "UTF-8", $s); return $r === false ? "false" : bin2hex($r); }
function di($cs, $s) { $r = @iconv($cs, "UTF-8//IGNORE", $s); return $r === false ? "false" : bin2hex($r); }
$s = "\u{4E00}\u{4E42}\u{3400}\u{20000}\u{2A6D6}\u{4E28}\u{9F98}\u{2124A}\u{20002}\u{2008C}\u{20086}";
$b = iconv("UTF-8", "EUC-TW", $s);
echo bin2hex($b), " ", bin2hex(iconv("EUC-TW", "UTF-8", $b)), "\n";
foreach (["8ea3a1a1", "8ea7a1a1", "8eafa1a1", "8ea1a7a8", "8ea8a1a1", "8eb0a1a1", "8ea3a1", "8ea3", "a3e1"] as $h) {
    echo $h, " ", d("EUC-TW", hex2bin($h)), "\n";
}
foreach (["a3e1e8414141", "a3e1a3e1e8414141", "4142a3e1e8414141", "a3e18ea3e1e8414141", "fe41a4a1"] as $h) {
    echo $h, " ", di("EUC-TW", hex2bin($h)), "\n";
}
--EXPECT--
c4a18ea2a1a18ea6a2ac8ea5a1a58ea7e6cc8ea3a1a18ea2f2c48ea6a4b58eafa1a18eafa1a28ea4a1a1 e4b880e4b982e39080f0a08080f0aa9b96e4b8a8e9be98f0a1898af0a08082f0a0828cf0a08286
8ea3a1a1 e4b8a8
8ea7a1a1 f0a08195
8eafa1a1 f0a08082
8ea1a7a8 e4baa0
8ea8a1a1 false
8eb0a1a1 false
8ea3a1 false
8ea3 false
a3e1 false
a3e1e8414141 414141
a3e1a3e1e8414141 414141
4142a3e1e8414141 4142414141
a3e18ea3e1e8414141 414141
fe41a4a1 41efbc90
