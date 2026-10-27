--TEST--
Error handler of a deprecation raised while linking: called from the declaring line
--FILE--
<?php
set_error_handler(function ($code, $message, $file, $line) {
    echo "handler: line $line, called from line ", debug_backtrace()[0]['line'], "\n";
    return true;
});

$x = new class extends DateTime {

    public function getTimezone() {}
};
echo "done\n";
--EXPECTF--
handler: line 9, called from line 7
done
