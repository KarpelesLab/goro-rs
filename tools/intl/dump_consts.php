<?php
// Usage: php tools/intl/dump_consts.php > consts.tsv  (any PHP with ext/intl)
// Dump ext/intl constants (global + class) from a real PHP build.
$c = get_defined_constants(true)['intl'];
echo "// global\n";
foreach ($c as $k => $v) echo "G\t$k\t", var_export($v, true), "\n";
$ext = new ReflectionExtension('intl');
foreach ($ext->getClasses() as $cl) {
    foreach ($cl->getReflectionConstants() as $rc) {
        if ($rc->getDeclaringClass()->getName() !== $cl->getName()) continue;
        echo "C\t", $cl->getName(), "\t", $rc->getName(), "\t", var_export($rc->getValue(), true), "\n";
    }
}
