--TEST--
Enum cases referencing each other: self-referencing constant error; valid references still work
--FILE--
<?php
enum E: int { case A = self::B; case B = self::A; }
try { var_dump(E::B); } catch (Error $e) { echo $e->getMessage(), " @", $e->getLine(), "\n"; }
try { var_dump(E::from(1)); } catch (Error $e) { echo $e->getMessage(), " @", $e->getLine(), "\n"; }
try { var_dump(E::cases()); } catch (Error $e) { echo $e->getMessage(), " @", $e->getLine(), "\n"; }
enum F: int { case A = 1; case B = self::A->value + 1; case C = 7; }
var_dump(F::B, F::from(2), F::C->value);
enum G: int { case X = self::Y; case Y = 3; }
try { var_dump(G::Y); } catch (Error $e) { echo get_class($e), ": ", $e->getMessage(), "\n"; }
enum H: string { const C = 'c'; case A = self::C; case B = self::C . 'x'; }
var_dump(H::A->value, H::B->value);
enum I: string {
    case A = self::B->value;

    case B = self::A->value;
}
try { var_dump(I::B); } catch (Error $e) { echo $e->getMessage(), " @", $e->getLine(), "\n"; }
?>
--EXPECT--
Cannot declare self-referencing constant self::B @2
Cannot declare self-referencing constant self::B @2
Cannot declare self-referencing constant self::B @2
enum(F::B)
enum(F::B)
int(7)
TypeError: Enum case type object does not match enum backing type int
string(1) "c"
string(2) "cx"
Cannot declare self-referencing constant self::B @13
