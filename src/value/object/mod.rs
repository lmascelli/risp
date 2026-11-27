pub(crate) enum Object {
    String(String),
}

impl Object {
    pub(crate) fn string(value: String) -> Self {
        Self::String(value)
    }

    pub(crate) fn as_str(&self) -> Option<&str> {
        if let Self::String(value) = self {
            Some(value.as_str())
        } else {
            None
        }
    }
}