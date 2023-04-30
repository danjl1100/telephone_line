use serde::{Deserialize, Serialize};
pub use telephone_line::services::key_value;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Raw {
    Add {
        delta: usize,
    },
    AddOk,
    ReadOk {
        value: usize,
    },
    Read {
        key: Option<String>, // key_value - requires key, Count - no key
    },
    Write {
        key: String,
        value: usize,
    },
    WriteOk,
    Cas {
        key: String,
        from: usize,
        to: usize,
    },
    CasOk,
    Error {
        code: key_value::ErrorCode,
        text: String,
    },
}

pub enum Receive {
    Kv(key_value::Receive),
    Count(CountReceive),
}
pub enum Send {
    Kv(key_value::SendSeq),
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
            Raw::ReadOk { value } => Receive::Kv(key_value::Receive::ReadOk { value }),
            Raw::WriteOk => Receive::Kv(key_value::Receive::WriteOk),
            Raw::CasOk => Receive::Kv(key_value::Receive::CasOk),
            Raw::Error { code, text } => Receive::Kv(key_value::Receive::Error { code, text }),

            // Error
            Raw::Read { key: Some(_) } | Raw::Write { .. } | Raw::Cas { .. } | Raw::AddOk => {
                return Err(raw)
            }
        })
    }
}

pub enum CountReceive {
    Add { delta: usize },
    Read,
}
pub enum CountSend {
    AddOk,
    ReadOk { value: usize },
}

impl From<key_value::SendSeq> for Raw {
    fn from(msg: key_value::SendSeq) -> Self {
        match msg {
            key_value::SendSeq::Read { key } => Raw::Read { key: Some(key) },
            key_value::SendSeq::Write { key, value } => Raw::Write { key, value },
            key_value::SendSeq::Cas { key, from, to } => Raw::Cas { key, from, to },
        }
    }
}
impl From<key_value::Receive> for Raw {
    fn from(msg: key_value::Receive) -> Self {
        match msg {
            key_value::Receive::ReadOk { value } => Raw::ReadOk { value },
            key_value::Receive::WriteOk => Raw::WriteOk,
            key_value::Receive::CasOk => Raw::CasOk,
            key_value::Receive::Error { code, text } => Raw::Error { code, text },
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
