--TEST--
A class implementing an interface with abstract (hooked) properties has them: Reflection getProperties()/hasProperty(), property_exists()
--FILE--
<?php
interface I { public string $foo { get; } }
interface J extends I { public int $j { set; } }
abstract class A implements J { abstract public string $bar { get; } }
class B extends A { public string $foo = 'f'; public string $bar = 'b'; public int $j = 1; }
foreach ([I::class, J::class, A::class, B::class] as $c) {
    $r = new ReflectionClass($c);
    echo $c, ":\n";
    foreach ($r->getProperties() as $p) {
        echo "  ", $p->class, "::", $p->name, " mods=", $p->getModifiers(), " ", implode(' ', Reflection::getModifierNames($p->getModifiers())), " abstract=", var_export($p->isAbstract(), true), " virtual=", var_export($p->isVirtual(), true), " hooks=", implode(',', array_keys($p->getHooks())), "\n";
    }
    var_dump($r->hasProperty('foo'));
}
$p = new ReflectionProperty(A::class, 'foo');
var_dump($p->class, $p->getDeclaringClass()->name, $p->getModifiers());
var_dump(property_exists(A::class, 'foo'), property_exists(I::class, 'foo'));
print_r(get_class_vars(A::class));

?>
--EXPECTF--
I:
  I::foo mods=577 abstract virtual public abstract=true virtual=true hooks=get
bool(true)
J:
  J::j mods=577 abstract virtual public abstract=true virtual=true hooks=set
  I::foo mods=577 abstract virtual public abstract=true virtual=true hooks=get
bool(true)
A:
  A::bar mods=577 abstract virtual public abstract=true virtual=true hooks=get
  J::j mods=577 abstract virtual public abstract=true virtual=true hooks=set
  I::foo mods=577 abstract virtual public abstract=true virtual=true hooks=get
bool(true)
B:
  B::foo mods=1 public abstract=false virtual=false hooks=
  B::bar mods=1 public abstract=false virtual=false hooks=
  B::j mods=1 public abstract=false virtual=false hooks=
bool(true)
string(1) "I"
string(1) "I"
int(577)
bool(true)
bool(true)
Array
(
)
