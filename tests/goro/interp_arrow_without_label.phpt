--TEST--
"$a->" / "$a->$b" in strings keep the -> as text
--FILE--
<?php
$a = "A"; $o = new stdClass; $o->p = "P";
echo <<<EOT
$a->|$a->1|$o->p|$o->p->x
EOT;
echo "\n", "{$o->p}->", "\n";
$a = "A"; $b = "B";
echo "$a->$b|$a->|$a-> x|$a->1|$a?->$b|$a?->|\n";
foreach ([1] as $p) { $name = "N"; echo "$name->$p op\n"; }
--EXPECT--
A->|A->1|P|P->x
P->
A->B|A->|A-> x|A->1|A?->B|A?->|
N->1 op
