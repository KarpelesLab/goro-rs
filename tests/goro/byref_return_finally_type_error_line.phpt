--TEST--
By-reference return type error after finally is on the return's line
--FILE--
<?php
function &test(): int {
    $x = 0;
    try {
        return $x;
    } finally {
        try {
            return $x;
        } finally {
            $x = "";
        }
    }
}
function test2(): int {
    $x = 0;
    try {
        return $x;
    } finally {
        $x = "";
    }
}
function &test3(): int {
    $x = 0;
    try {
        return $x;
    } finally {
        $x = "";
    }
}
function test4(): int {
    $x = 0;
    try {
        $y = 1;
    } finally {
        return "a";
    }
}
foreach (['test', 'test2', 'test3', 'test4'] as $f) {
  try { var_dump($f()); } catch (TypeError $e) { echo $e->getMessage(), " @", $e->getLine(), "\n"; }
}
--EXPECT--
test(): Return value must be of type int, string returned @8
int(0)
test3(): Return value must be of type int, string returned @25
test4(): Return value must be of type int, string returned @35
