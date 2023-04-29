use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Raw {
    Add { delta: usize },
    AddOk,
    ReadOk { value: usize },
    Read { key: Option<String> },
    Write { key: String, value: usize },
    WriteOk,
    Cas { key: String, from: usize, to: usize },
    CasOk,
    Error { code: KvErrorCode, text: String },
}

pub enum Typed {
    Receive(Receive),
    Send(Send),
}

pub enum Receive {
    Kv(KeyValueReceive),
    Count(CountReceive),
}
pub enum Send {
    Kv(KeyValueSend),
    Count(CountSend),
}
impl TryFrom<Raw> for Receive {
    type Error = Raw;
    fn try_from(raw: Raw) -> Result<Receive, Raw> {
        Ok(match raw {
            // Count
            Raw::Add { delta } => Receive::Count(CountReceive::Add { delta }),
            Raw::Read { key: None } => Receive::Count(CountReceive::Read),

            // KeyValue
            Raw::ReadOk { value } => Receive::Kv(KeyValueReceive::ReadOk { value }),
            Raw::WriteOk => Receive::Kv(KeyValueReceive::WriteOk),
            Raw::CasOk => Receive::Kv(KeyValueReceive::CasOk),
            Raw::Error { code, text } => Receive::Kv(KeyValueReceive::Error { code, text }),

            // Error
            Raw::Read { key: Some(_) } | Raw::Write { .. } | Raw::Cas { .. } | Raw::AddOk => {
                return Err(raw)
            }
        })
    }
}

pub enum KeyValueSend {
    Read { key: String },
    Write { key: String, value: usize },
    Cas { key: String, from: usize, to: usize },
}
pub enum KeyValueReceive {
    ReadOk { value: usize },
    WriteOk,
    CasOk,
    Error { code: KvErrorCode, text: String },
}
pub enum CountReceive {
    Add { delta: usize },
    Read,
}
pub enum CountSend {
    AddOk,
    ReadOk { value: usize },
}

impl From<KeyValueSend> for Raw {
    fn from(msg: KeyValueSend) -> Self {
        match msg {
            KeyValueSend::Read { key } => Raw::Read { key: Some(key) },
            KeyValueSend::Write { key, value } => Raw::Write { key, value },
            KeyValueSend::Cas { key, from, to } => Raw::Cas { key, from, to },
        }
    }
}
impl From<KeyValueReceive> for Raw {
    fn from(msg: KeyValueReceive) -> Self {
        match msg {
            KeyValueReceive::ReadOk { value } => Raw::ReadOk { value },
            KeyValueReceive::WriteOk => Raw::WriteOk,
            KeyValueReceive::CasOk => Raw::CasOk,
            KeyValueReceive::Error { code, text } => Raw::Error { code, text },
        }
    }
}
impl From<CountReceive> for Raw {
    fn from(msg: CountReceive) -> Self {
        match msg {
            CountReceive::Add { delta } => Raw::Add { delta },
            CountReceive::Read => Raw::Read { key: None },
        }
    }
}
impl From<CountSend> for Raw {
    fn from(msg: CountSend) -> Self {
        match msg {
            CountSend::AddOk => Raw::AddOk,
            CountSend::ReadOk { value } => Raw::ReadOk { value },
        }
    }
}

pub use key_value_error::KvErrorCode;
mod key_value_error {
    use super::*;
    #[derive(Debug, Clone, Copy, Serialize, Deserialize)]
    #[serde(from = "u32", into = "u32")]
    pub enum KvErrorCode {
        KeyNotFound,
        CasFromMismatch,
        Unknown(u32),
    }
    impl From<u32> for KvErrorCode {
        fn from(code: u32) -> Self {
            match code {
                20 => Self::KeyNotFound,
                22 => Self::CasFromMismatch,
                unknown => Self::Unknown(unknown),
            }
        }
    }
    impl From<KvErrorCode> for u32 {
        fn from(code: KvErrorCode) -> Self {
            match code {
                KvErrorCode::KeyNotFound => 20,
                KvErrorCode::CasFromMismatch => 22,
                KvErrorCode::Unknown(unknown) => unknown,
            }
        }
    }
}
