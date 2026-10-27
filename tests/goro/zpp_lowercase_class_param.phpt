--TEST--
Internal functions' class-typed parameters whose class name is lowercase (tidy, finfo)
--EXTENSIONS--
tidy
fileinfo
--FILE--
<?php
$t = tidy_parse_string("<p>x</p>");
var_dump(tidy_getopt($t, 'tab-size'));
try {
    tidy_getopt($t, 'bogus');
} catch (ValueError $e) {
    echo $e->getMessage(), "\n";
}
try {
    tidy_getopt(new stdClass, 'tab-size');
} catch (TypeError $e) {
    echo $e->getMessage(), "\n";
}
var_dump(strlen(tidy_get_opt_doc($t, 'wrap')) > 10);
$f = finfo_open(FILEINFO_MIME_TYPE);
var_dump(finfo_buffer($f, "<?php echo 1;"));
?>
--EXPECT--
int(8)
tidy_getopt(): Argument #2 ($option) is an invalid configuration option, "bogus" given
tidy_getopt(): Argument #1 ($tidy) must be of type tidy, stdClass given
bool(true)
string(10) "text/x-php"
