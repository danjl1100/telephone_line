//! Common interface for maelstrom's `seq-kv` and `lin-kv` endpoints

/// Node id of the `seq-kv` provided by maelstrom test harness
pub const NODE_ID: &str = "seq-kv";

pub enum Send {
    Read { key: String },
    Write { key: String, value: usize },
    Cas { key: String, from: usize, to: usize },
}
pub enum Receive {
    ReadOk { value: usize },
    WriteOk,
    CasOk,
    Error { code: ErrorCode, text: String },
}

pub use error::Code as ErrorCode;
mod error {
    use serde::{Deserialize, Serialize};

    #[derive(Debug, Clone, Copy, Serialize, Deserialize)]
    #[serde(from = "u32", into = "u32")]
    pub enum Code {
        KeyNotFound,
        CasFromMismatch,
        Unknown(u32),
    }
    impl Code {
        const KEY_NOT_FOUND: u32 = 20;
        const CAS_FROM_MISMATCH: u32 = 22;
    }
    impl From<u32> for Code {
        fn from(code: u32) -> Self {
            match code {
                Self::KEY_NOT_FOUND => Self::KeyNotFound,
                Self::CAS_FROM_MISMATCH => Self::CasFromMismatch,
                unknown => Self::Unknown(unknown),
            }
        }
    }
    impl From<Code> for u32 {
        fn from(code: Code) -> Self {
            match code {
                Code::KeyNotFound => Code::KEY_NOT_FOUND,
                Code::CasFromMismatch => Code::CAS_FROM_MISMATCH,
                Code::Unknown(unknown) => unknown,
            }
        }
    }
}
