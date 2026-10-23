--TEST--
fgets()/fgetc() of a directory opened as a file
--FILE--
<?php
mkdir(sys_get_temp_dir()."/r3dd2_".getmypid()); $h = fopen(sys_get_temp_dir()."/r3dd2_".getmypid(), "r"); var_dump(fgets($h), fgetc($h), feof($h)); fclose($h); $h = fopen(sys_get_temp_dir()."/r3dd2_".getmypid(), "r"); var_dump(fgetc($h), fgets($h), fread($h, 3), stream_get_contents($h)); rmdir(sys_get_temp_dir()."/r3dd2_".getmypid());
--EXPECTF--

Notice: fgets(): Read of 8192 bytes failed with errno=21 Is a directory in %s on line 2

Notice: fgetc(): Read of 8192 bytes failed with errno=21 Is a directory in %s on line 2
bool(false)
bool(false)
bool(true)

Notice: fgetc(): Read of 8192 bytes failed with errno=21 Is a directory in %s on line 2

Notice: fread(): Read of 8192 bytes failed with errno=21 Is a directory in %s on line 2

Notice: stream_get_contents(): Read of 8192 bytes failed with errno=21 Is a directory in %s on line 2
bool(false)
bool(false)
bool(false)
string(0) ""
