--TEST--
Class and method names with invalid UTF-8 are kept as raw bytes in error messages
--FILE--
<?php
try { get_class_vars("\xff\xfe"); } catch (Throwable $e) { echo bin2hex($e->getMessage()), "\n"; }
try { get_class_methods("\xff\xfe"); } catch (Throwable $e) { echo bin2hex($e->getMessage()), "\n"; }
try { ("\xff")::x(); } catch (Throwable $e) { echo bin2hex($e->getMessage()), "\n"; }
try { ("\xff")::$x; } catch (Throwable $e) { echo bin2hex($e->getMessage()), "\n"; }
try { ("\xff")::X; } catch (Throwable $e) { echo bin2hex($e->getMessage()), "\n"; }
try { $x = "\xff"; $x::x(); } catch (Throwable $e) { echo bin2hex($e->getMessage()), "\n"; }
try { $x = "\xff"; $x::$y; } catch (Throwable $e) { echo bin2hex($e->getMessage()), "\n"; }
try { $x = "\xff"; $x::Y; } catch (Throwable $e) { echo bin2hex($e->getMessage()), "\n"; }
try { $x = "\xff"; var_dump($x instanceof stdClass); } catch (Throwable $e) { echo bin2hex($e->getMessage()), "\n"; }
try { $x = "\xff"; new $x; } catch (Throwable $e) { echo bin2hex($e->getMessage()), "\n"; }
try { $x = "\xff"; $x(); } catch (Throwable $e) { echo bin2hex($e->getMessage()), "\n"; }
try { $x = ["\xff", "y"]; $x(); } catch (Throwable $e) { echo bin2hex($e->getMessage()), "\n"; }
try { $x = [new stdClass, "\xff"]; $x(); } catch (Throwable $e) { echo bin2hex($e->getMessage()), "\n"; }
try { (new stdClass)->{"\xff"}(); } catch (Throwable $e) { echo bin2hex($e->getMessage()), "\n"; }
try { stdClass::{"\xff"}(); } catch (Throwable $e) { echo bin2hex($e->getMessage()), "\n"; }
try { constant("stdClass::\xff"); } catch (Throwable $e) { echo bin2hex($e->getMessage()), "\n"; }
try { new DateTimeZone("\xff"); } catch (Throwable $e) { echo bin2hex($e->getMessage()), "\n"; }
--EXPECTF--
6765745f636c6173735f7661727328293a20417267756d656e74202331202824636c61737329206d75737420626520612076616c696420636c617373206e616d652c20fffe20676976656e
6765745f636c6173735f6d6574686f647328293a20417267756d656e742023312028246f626a6563745f6f725f636c61737329206d75737420626520616e206f626a656374206f7220612076616c696420636c617373206e616d652c20737472696e6720676976656e
436c6173732022ff22206e6f7420666f756e64
436c6173732022ff22206e6f7420666f756e64
436c6173732022ff22206e6f7420666f756e64
436c6173732022ff22206e6f7420666f756e64
436c6173732022ff22206e6f7420666f756e64
436c6173732022ff22206e6f7420666f756e64
bool(false)
436c6173732022ff22206e6f7420666f756e64
43616c6c20746f20756e646566696e65642066756e6374696f6e20ff2829
436c6173732022ff22206e6f7420666f756e64
43616c6c20746f20756e646566696e6564206d6574686f6420737464436c6173733a3aff2829
43616c6c20746f20756e646566696e6564206d6574686f6420737464436c6173733a3aff2829
43616c6c20746f20756e646566696e6564206d6574686f6420737464436c6173733a3aff2829
556e646566696e656420636f6e7374616e7420737464436c6173733a3aff
4461746554696d655a6f6e653a3a5f5f636f6e73747275637428293a20556e6b6e6f776e206f72206261642074696d657a6f6e652028ff29
