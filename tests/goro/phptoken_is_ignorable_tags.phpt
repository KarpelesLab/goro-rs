--TEST--
PhpToken::isIgnorable(): whitespace, comments and the open tag only (not <?=, ?> nor inline HTML)
--FILE--
<?php
$toks = PhpToken::tokenize('<?php if ($a) { echo "x$b"; } /* c */ /** d */ // e' . "\n" . '?>html<?= 1 ?>');
foreach ($toks as $t) echo $t->getTokenName(), " ", json_encode($t->text), " ", var_export($t->isIgnorable(), true), "\n";
--EXPECT--
T_OPEN_TAG "<?php " true
T_IF "if" false
T_WHITESPACE " " true
( "(" false
T_VARIABLE "$a" false
) ")" false
T_WHITESPACE " " true
{ "{" false
T_WHITESPACE " " true
T_ECHO "echo" false
T_WHITESPACE " " true
" "\"" false
T_ENCAPSED_AND_WHITESPACE "x" false
T_VARIABLE "$b" false
" "\"" false
; ";" false
T_WHITESPACE " " true
} "}" false
T_WHITESPACE " " true
T_COMMENT "\/* c *\/" true
T_WHITESPACE " " true
T_DOC_COMMENT "\/** d *\/" true
T_WHITESPACE " " true
T_COMMENT "\/\/ e" true
T_WHITESPACE "\n" true
T_CLOSE_TAG "?>" false
T_INLINE_HTML "html" false
T_OPEN_TAG_WITH_ECHO "<?=" false
T_WHITESPACE " " true
T_LNUMBER "1" false
T_WHITESPACE " " true
T_CLOSE_TAG "?>" false
