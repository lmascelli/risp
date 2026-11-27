use crate::runtime::RuntimeId;

struct ObjectRef {
    runtime_id: RuntimeId,
    slot: usize,
    generation: u64,
}

enum ValueRepr {
    Integer(i64),
    Object(ObjectRef),
}

pub struct Value {
    repr: ValueRepr,
}

impl Value {
    pub fn integer(value: i64) -> Self {
        Self{
            repr: ValueRepr::Integer(value),
        }
    }

    pub fn as_integer(&self) -> Option<i64> {
        if let ValueRepr::Integer(value) = self.repr {
            Some(value)
        } else {
            None
        }
    }
}