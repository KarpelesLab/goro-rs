--TEST--
metaphone(): max_phonemes is checked before each letter
--FILE--
<?php
foreach ([["Xerox", 3], ["box", 2], ["gXc<", 2], ["Thompson", 2], ["Xavier", 1], ["axe", 1], ["Knight", 0], ["Asterix", 4]] as [$w, $n]) {
  var_dump(metaphone($w, $n));
}
try { metaphone("x", -1); } catch (ValueError $e) { echo $e->getMessage(), "\n"; }
--EXPECT--
string(4) "SRKS"
string(3) "BKS"
string(3) "KKS"
string(2) "0M"
string(1) "S"
string(1) "A"
string(3) "NFT"
string(4) "ASTR"
metaphone(): Argument #2 ($max_phonemes) must be greater than or equal to 0
