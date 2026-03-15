use crate::opcode::OpArray;
use crate::vm::BuiltinFn;

/// A PHP function - either user-defined (compiled) or built-in (native Rust)
#[derive(Clone)]
pub enum Function {
    User(UserFunction),
    Builtin(BuiltinFunction),
}

/// A user-defined PHP function
#[derive(Clone)]
pub struct UserFunction {
    pub name: Vec<u8>,
    pub op_array: OpArray,
    pub param_count: usize,
    pub has_variadic: bool,
}

/// A built-in (native) function
#[derive(Clone)]
pub struct BuiltinFunction {
    pub name: Vec<u8>,
    pub func: BuiltinFn,
}

impl Function {
    pub fn name(&self) -> &[u8] {
        match self {
            Function::User(f) => &f.name,
            Function::Builtin(f) => &f.name,
        }
    }
}
