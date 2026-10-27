--TEST--
${} in strings: deprecation and nested heredoc lines
--FILE--
<?php
$v = 'w';
$w = 1;
${"\0"} = 3;
${"b\nw"} = 2;
$a = <<<DOC
    a
    ${"\400"}
    ${<<<DOC2
        b
        ${'v'}
        DOC2
    }
    DOC;
$b = "x
${'v'} y
z ${v}";
echo "done\n";
--EXPECTF--

Warning: Octal escape sequence overflow \400 is greater than \377 in %s on line 8

Deprecated: Using ${expr} (variable variables) in strings is deprecated, use {${expr}} instead in %s on line 7

Deprecated: Using ${expr} (variable variables) in strings is deprecated, use {${expr}} instead in %s on line 8

Deprecated: Using ${expr} (variable variables) in strings is deprecated, use {${expr}} instead in %s on line 10

Deprecated: Using ${expr} (variable variables) in strings is deprecated, use {${expr}} instead in %s on line 15

Deprecated: Using ${var} in strings is deprecated, use {$var} instead in %s on line 16
done
