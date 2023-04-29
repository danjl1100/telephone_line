use std::collections::{BTreeMap, HashMap};

use anyhow::bail;
use serde::{Deserialize, Serialize};
use telephone_line::{main_loop, Never, Node};

struct Logs {
    msg_id: usize,
    logs: HashMap<String, Log>,
}

#[derive(Default)]
struct Log {
    committed_offset: Option<usize>,
    messages: BTreeMap<usize, usize>,
}

/// Maximum number of messages to return for each "log" in a poll
const MAX_POLL_MESSAGE_EACH: usize = 5;

impl Node for Logs {
    type Payload = Payload;
    type Event = Never;

    fn from_init(
        _init: telephone_line::Init,
        msg_id: usize,
        _start: (),
        _event_tx: telephone_line::EventSender<Self::Payload, Self::Event>,
    ) -> Self
    where
        Self: Sized,
    {
        Self {
            msg_id,
            logs: HashMap::new(),
        }
    }

    fn step_message(
        &mut self,
        message: telephone_line::Message<Self::Payload>,
        output: &mut impl std::io::Write,
    ) -> anyhow::Result<()> {
        let mut reply = message.reply(Some(&mut self.msg_id));
        match reply.body.payload {
            Payload::Send { key, msg } => {
                let log = self.logs.entry(key).or_insert_with(Log::default);
                let offset = log
                    .messages
                    .last_key_value()
                    .map(|(offset, _msg)| offset + 1)
                    .unwrap_or_default();
                log.messages.insert(offset, msg);

                reply.body.payload = Payload::SendOk { offset };
                reply.send(output)
            }
            Payload::Poll { offsets } => {
                let msgs = offsets
                    .into_iter()
                    .filter_map(|(key, offset)| {
                        let log = self.logs.get(&key)?;
                        let messages: Vec<_> = log
                            .messages
                            .range(offset..)
                            .map(|(&offset, &message)| (offset, message))
                            .take(MAX_POLL_MESSAGE_EACH)
                            .collect();
                        Some((key, messages))
                    })
                    .collect();

                reply.body.payload = Payload::PollOk { msgs };
                reply.send(output)
            }
            Payload::CommitOffsets { offsets } => {
                for (key, offset) in offsets {
                    if let Some(log) = self.logs.get_mut(&key) {
                        log.committed_offset = Some(offset);
                    }
                }

                reply.body.payload = Payload::CommitOffsetsOk;
                reply.send(output)
            }
            Payload::ListCommittedOffsets { keys } => {
                let offsets = keys
                    .into_iter()
                    .filter_map(|key| {
                        let log = self.logs.get(&key)?;
                        Some((key, log.committed_offset?))
                    })
                    .collect();

                reply.body.payload = Payload::ListCommittedOffsetsOk { offsets };
                reply.send(output)
            }
            payload @ (Payload::SendOk { .. }
            | Payload::PollOk { .. }
            | Payload::CommitOffsetsOk
            | Payload::ListCommittedOffsetsOk { .. }) => {
                bail!("unexpected message {payload:?}");
            }
        }
    }

    fn step_event(
        &mut self,
        never: Self::Event,
        _output: &mut impl std::io::Write,
    ) -> anyhow::Result<()> {
        match never {}
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Payload {
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
}

fn main() -> anyhow::Result<()> {
    main_loop::<Logs, _>(())
}
