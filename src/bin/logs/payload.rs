use serde::{Deserialize, Serialize};
use std::collections::HashMap;
pub use telephone_line::services::key_value;

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Raw {
    // Logs
    Send {
        key: String,
        msg: usize,
    },
    SendOk {
        offset: usize,
    },
    Poll {
        offsets: HashMap<String, usize>,
    },
    PollOk {
        msgs: HashMap<String, Vec<(usize, usize)>>,
    },
    CommitOffsets {
        offsets: HashMap<String, usize>,
    },
    CommitOffsetsOk,
    ListCommittedOffsets {
        keys: Vec<String>,
    },
    ListCommittedOffsetsOk {
        offsets: HashMap<String, usize>,
    },
    // key_value
    ReadOk {
        value: usize,
    },
    Read {
        key: String,
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
        #[serde(skip_serializing_if = "std::ops::Not::not")]
        create_if_not_exists: bool,
    },
    CasOk,
    Error {
        code: key_value::ErrorCode,
        text: String,
    },
}

pub enum Receive {
    Kv(key_value::Receive),
    Logs(LogsReceive),
}
pub enum Send {
    Kv(key_value::SendLin),
    Logs(LogsSend),
}

pub enum LogsReceive {
    Send { key: String, msg: usize },
    Poll { offsets: HashMap<String, usize> },
    CommitOffsets { offsets: HashMap<String, usize> },
    ListCommittedOffsets { keys: Vec<String> },
}
pub enum LogsSend {
    SendOk {
        offset: usize,
    },
    PollOk {
        msgs: HashMap<String, Vec<(usize, usize)>>,
    },
    CommitOffsetsOk,
    ListCommittedOffsetsOk {
        offsets: HashMap<String, usize>,
    },
}

impl TryFrom<Raw> for Receive {
    type Error = Raw;

    fn try_from(raw: Raw) -> Result<Self, Self::Error> {
        Ok(match raw {
            Raw::Send { key, msg } => Receive::Logs(LogsReceive::Send { key, msg }),
            Raw::Poll { offsets } => Receive::Logs(LogsReceive::Poll { offsets }),
            Raw::CommitOffsets { offsets } => Receive::Logs(LogsReceive::CommitOffsets { offsets }),
            Raw::ListCommittedOffsets { keys } => {
                Receive::Logs(LogsReceive::ListCommittedOffsets { keys })
            }
            Raw::ReadOk { value } => Receive::Kv(key_value::Receive::ReadOk { value }),
            Raw::WriteOk => Receive::Kv(key_value::Receive::WriteOk),
            Raw::CasOk => Receive::Kv(key_value::Receive::CasOk),
            Raw::Error { code, text } => Receive::Kv(key_value::Receive::Error { code, text }),
            Raw::SendOk { .. }
            | Raw::PollOk { .. }
            | Raw::CommitOffsetsOk
            | Raw::ListCommittedOffsetsOk { .. }
            | Raw::Read { .. }
            | Raw::Write { .. }
            | Raw::Cas { .. } => return Err(raw),
        })
    }
}

impl From<key_value::SendLin> for Raw {
    fn from(msg: key_value::SendLin) -> Self {
        match msg {
            key_value::SendLin::Read { key } => Raw::Read { key },
            key_value::SendLin::Write { key, value } => Raw::Write { key, value },
            key_value::SendLin::Cas {
                key,
                from,
                to,
                create_if_not_exists,
            } => Raw::Cas {
                key,
                from,
                to,
                create_if_not_exists,
            },
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
impl From<LogsReceive> for Raw {
    fn from(msg: LogsReceive) -> Self {
        match msg {
            LogsReceive::Send { key, msg } => Raw::Send { key, msg },
            LogsReceive::Poll { offsets } => Raw::Poll { offsets },
            LogsReceive::CommitOffsets { offsets } => Raw::CommitOffsets { offsets },
            LogsReceive::ListCommittedOffsets { keys } => Raw::ListCommittedOffsets { keys },
        }
    }
}
impl From<LogsSend> for Raw {
    fn from(msg: LogsSend) -> Self {
        match msg {
            LogsSend::SendOk { offset } => Raw::SendOk { offset },
            LogsSend::PollOk { msgs } => Raw::PollOk { msgs },
            LogsSend::CommitOffsetsOk => Raw::CommitOffsetsOk,
            LogsSend::ListCommittedOffsetsOk { offsets } => Raw::ListCommittedOffsetsOk { offsets },
        }
    }
}
