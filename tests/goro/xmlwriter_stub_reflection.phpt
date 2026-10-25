--TEST--
xmlwriter functions and XMLWriter are described by their stubs
--FILE--
<?php var_dump(count(get_extension_funcs("xmlwriter"))); $f = new ReflectionFunction("xmlwriter_open_memory"); var_dump($f->getExtensionName()); $m = new ReflectionMethod("XMLWriter", "startElementNs"); var_dump($m->getNumberOfParameters(), (string)$m->getParameters()[2]->getType(), $m->getReturnType()?->__toString(), (new ReflectionClass("XMLWriter"))->getExtensionName()); echo new ReflectionFunction("xmlwriter_write_dtd_entity"); var_dump(count(get_class_methods("XMLWriter"))); xmlwriter_write_element(xmlwriter_open_memory(), name: "x", content: "y"); try { xmlwriter_text(); } catch (Throwable $e) { echo $e->getMessage(), "\n"; } try { xmlwriter_text("x", "y"); } catch (Throwable $e) { echo $e->getMessage(), "\n"; }
--EXPECT--
int(42)
string(9) "xmlwriter"
int(3)
string(7) "?string"
NULL
string(9) "xmlwriter"
Function [ <internal:xmlwriter> function xmlwriter_write_dtd_entity ] {

  - Parameters [7] {
    Parameter #0 [ <required> XMLWriter $writer ]
    Parameter #1 [ <required> string $name ]
    Parameter #2 [ <required> string $content ]
    Parameter #3 [ <optional> bool $isParam = false ]
    Parameter #4 [ <optional> ?string $publicId = null ]
    Parameter #5 [ <optional> ?string $systemId = null ]
    Parameter #6 [ <optional> ?string $notationData = null ]
  }
  - Return [ bool ]
}
int(45)
xmlwriter_text() expects exactly 2 arguments, 0 given
xmlwriter_text(): Argument #1 ($writer) must be of type XMLWriter, string given
