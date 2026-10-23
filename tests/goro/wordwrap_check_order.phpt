--TEST--
wordwrap(): empty string first, then break, then width/cut checks
--FILE--
<?php
var_dump(wordwrap("", 0, "\n", true));
var_dump(wordwrap("", 5, ""));
try { wordwrap("a", 0, "", true); } catch (ValueError $e) { echo $e->getMessage(), "\n"; }
try { wordwrap("a", 0, "\n", true); } catch (ValueError $e) { echo $e->getMessage(), "\n"; }
try { wordwrap("a", 5, ""); } catch (ValueError $e) { echo $e->getMessage(), "\n"; }
--EXPECT--
string(0) ""
string(0) ""
wordwrap(): Argument #3 ($break) must not be empty
wordwrap(): Argument #4 ($cut_long_words) cannot be true when argument #2 ($width) is 0
wordwrap(): Argument #3 ($break) must not be empty
