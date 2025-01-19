#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum FindOperationType {
    TillNextChar,
    NextChar,
    TillPrevChar,
    PrevChar,
}

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct FindOperation {
    pub last_char: char,
    pub op_type: FindOperationType
}