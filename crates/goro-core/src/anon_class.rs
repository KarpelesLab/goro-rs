//! Anonymous classes are bound when the `new class` expression runs
//! (ZEND_DECLARE_ANON_CLASS), not when the code containing them is
//! loaded: the compiler hoists their declarations, which the VM keeps
//! pending until the class is first looked up.

use crate::object::ClassEntry;
use crate::opcode::{Op, OpArray, OpCode, OperandType};
use crate::value::Value;
use crate::vm::{Vm, VmError};

pub(crate) fn is_anon_name(name: &[u8]) -> bool {
    name.windows(11).any(|w| w == b"@anonymous\0")
}

impl Vm {
    /// DECLARE_CLASS of an anonymous class whose parent, interfaces or
    /// traits are not declared yet: kept pending (true), to be bound (and
    /// its dependencies loaded) when the class is first used.
    pub(crate) fn defer_anon_class(&mut self, class: &ClassEntry) -> bool {
        if self.declaring_anon || !is_anon_name(&class.name) {
            return false;
        }
        let lower = class.name.to_ascii_lowercase();
        if self.classes.contains_key(&lower) {
            return false;
        }
        let known = |vm: &Vm, n: &[u8]| {
            let l = n.to_ascii_lowercase();
            let l = l.strip_prefix(b"\\").unwrap_or(&l);
            vm.lookup_class(l).is_some() || vm.is_known_builtin_class(l)
        };
        let deps_known = class.parent.as_deref().is_none_or(|p| known(self, p))
            && class.interfaces.iter().all(|i| known(self, i))
            && class.traits.iter().all(|t| known(self, t));
        if deps_known {
            return false;
        }
        self.anon_pending.insert(lower, class.clone());
        true
    }

    /// Binds a pending anonymous class on its first lookup: Some(declared)
    /// when `name` is one.
    pub(crate) fn bind_pending_anon_class(&mut self, name: &[u8]) -> Option<bool> {
        let lower = name.to_ascii_lowercase();
        let class = self.anon_pending.remove(&lower)?;
        let file = class.filename.clone().unwrap_or_default();
        let r = self.declare_class_entry(class, &file);
        Some(r.is_ok() && self.current_exception.is_none() && self.classes.contains_key(&lower))
    }

    /// Runs DECLARE_CLASS for `class` (declared in `file`).
    pub(crate) fn declare_class_entry(
        &mut self,
        class: ClassEntry,
        file: &str,
    ) -> Result<Value, VmError> {
        let mut op_array = OpArray::new();
        op_array.filename = file.as_bytes().to_vec();
        let line = class.start_line;
        let name = op_array.add_literal(Value::String(crate::string::PhpString::from_vec(
            class.name.clone(),
        )));
        let idx = op_array.add_literal(Value::Long(0));
        op_array.ops.push(Op {
            opcode: OpCode::DeclareClass,
            op1: OperandType::Const(name),
            op2: OperandType::Const(idx),
            result: OperandType::Unused,
            line,
        });
        let null = op_array.add_literal(Value::Null);
        op_array.ops.push(Op {
            opcode: OpCode::Return,
            op1: OperandType::Const(null),
            op2: OperandType::Unused,
            result: OperandType::Unused,
            line: 0,
        });
        let saved_pending = std::mem::replace(&mut self.pending_classes, vec![class]);
        let saved_anon = std::mem::replace(&mut self.declaring_anon, true);
        let r = self.execute_op_array_pub(&op_array, Vec::new());
        self.declaring_anon = saved_anon;
        self.pending_classes = saved_pending;
        r
    }
}
