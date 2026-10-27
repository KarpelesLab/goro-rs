--TEST--
disk_free_space() checks (and reports) the expanded path against open_basedir; a first runtime open_basedir is resolved
--FILE--
<?php
$d = sys_get_temp_dir() . "/goro_obds_" . getmypid();
@mkdir("$d/test/ok", 0777, true); @mkdir("$d/test/bad", 0777, true);
chdir("$d/test/ok");
ini_set("open_basedir", ".");
foreach (["../bad", "../", "..", "../bad/.", "../bad/../bad/x.txt", "./"] as $p) { var_dump(@disk_free_space($p) !== false); disk_free_space($p); }
--EXPECTF--
bool(false)

Warning: disk_free_space(): open_basedir restriction in effect. File(/tmp/goro_obds_%d/test/bad) is not within the allowed path(s): (/tmp/goro_obds_%d/test/ok) in %s on line 6
bool(false)

Warning: disk_free_space(): open_basedir restriction in effect. File(/tmp/goro_obds_%d/test/) is not within the allowed path(s): (/tmp/goro_obds_%d/test/ok) in %s on line 6
bool(false)

Warning: disk_free_space(): open_basedir restriction in effect. File(/tmp/goro_obds_%d/test) is not within the allowed path(s): (/tmp/goro_obds_%d/test/ok) in %s on line 6
bool(false)

Warning: disk_free_space(): open_basedir restriction in effect. File(/tmp/goro_obds_%d/test/bad) is not within the allowed path(s): (/tmp/goro_obds_%d/test/ok) in %s on line 6
bool(false)

Warning: disk_free_space(): open_basedir restriction in effect. File(/tmp/goro_obds_%d/test/bad/x.txt) is not within the allowed path(s): (/tmp/goro_obds_%d/test/ok) in %s on line 6
bool(true)
