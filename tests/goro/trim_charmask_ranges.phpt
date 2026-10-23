--TEST--
trim()/str_word_count() character lists parsed as php_charmask()
--FILE--
<?php
foreach (["c..a", "a..c", "..a", "a..", "a..b..c", ".", "..", "...", "a...c", "z..a..z", "\x00..\x1f"] as $m) {
    echo json_encode($m), ' ', json_encode(trim("abc.z\x05", $m)), ' ', json_encode(rtrim("xyz..", $m)), ' ', json_encode(ltrim("..abc", $m)), "\n";
}
var_dump(str_word_count("Hello wörld 123", 1, "z..a"));
var_dump(str_word_count("Hello wörld 1-2", 1, "0..9"));
--EXPECTF--
"c..a" 
Warning: trim(): Invalid '..'-range, '..'-range needs to be incrementing in %s on line 3
"bc.z\u0005" 
Warning: rtrim(): Invalid '..'-range, '..'-range needs to be incrementing in %s on line 3
"xyz" 
Warning: ltrim(): Invalid '..'-range, '..'-range needs to be incrementing in %s on line 3
"bc"
"a..c" ".z\u0005" "xyz.." "..abc"
"..a" 
Warning: trim(): Invalid '..'-range, no character to the left of '..' in %s on line 3
"bc.z\u0005" 
Warning: rtrim(): Invalid '..'-range, no character to the left of '..' in %s on line 3
"xyz" 
Warning: ltrim(): Invalid '..'-range, no character to the left of '..' in %s on line 3
"bc"
"a.." 
Warning: trim(): Invalid '..'-range, no character to the right of '..' in %s on line 3
"bc.z\u0005" 
Warning: rtrim(): Invalid '..'-range, no character to the right of '..' in %s on line 3
"xyz" 
Warning: ltrim(): Invalid '..'-range, no character to the right of '..' in %s on line 3
"bc"
"a..b..c" 
Warning: trim(): Invalid '..'-range in %s on line 3
"z\u0005" 
Warning: rtrim(): Invalid '..'-range in %s on line 3
"xyz" 
Warning: ltrim(): Invalid '..'-range in %s on line 3
""
"." "abc.z\u0005" "xyz" "abc"
".." 
Warning: trim(): Invalid '..'-range, no character to the left of '..' in %s on line 3
"abc.z\u0005" 
Warning: rtrim(): Invalid '..'-range, no character to the left of '..' in %s on line 3
"xyz" 
Warning: ltrim(): Invalid '..'-range, no character to the left of '..' in %s on line 3
"abc"
"..." 
Warning: trim(): Invalid '..'-range, no character to the left of '..' in %s on line 3

Warning: trim(): Invalid '..'-range, no character to the right of '..' in %s on line 3
"abc.z\u0005" 
Warning: rtrim(): Invalid '..'-range, no character to the left of '..' in %s on line 3

Warning: rtrim(): Invalid '..'-range, no character to the right of '..' in %s on line 3
"xyz" 
Warning: ltrim(): Invalid '..'-range, no character to the left of '..' in %s on line 3

Warning: ltrim(): Invalid '..'-range, no character to the right of '..' in %s on line 3
"abc"
"a...c" "z\u0005" "xyz" ""
"z..a..z" 
Warning: trim(): Invalid '..'-range, '..'-range needs to be incrementing in %s on line 3
"\u0005" 
Warning: rtrim(): Invalid '..'-range, '..'-range needs to be incrementing in %s on line 3
"" 
Warning: ltrim(): Invalid '..'-range, '..'-range needs to be incrementing in %s on line 3
""
"\u0000..\u001f" "abc.z" "xyz.." "..abc"

Warning: str_word_count(): Invalid '..'-range, '..'-range needs to be incrementing in %s on line 5
array(3) {
  [0]=>
  string(5) "Hello"
  [1]=>
  string(1) "w"
  [2]=>
  string(3) "rld"
}
array(4) {
  [0]=>
  string(5) "Hello"
  [1]=>
  string(1) "w"
  [2]=>
  string(3) "rld"
  [3]=>
  string(3) "1-2"
}
