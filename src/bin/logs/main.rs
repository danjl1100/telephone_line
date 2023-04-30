use std::collections::{BTreeMap, HashMap};
use telephone_line::{main_loop, services::key_value, Body, Message, Never, Node};

mod payload;

struct Logs {
    node_id: String,
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
    type Payload = payload::Raw;
    type Event = Never;

    fn from_init(
        init: telephone_line::Init,
        msg_id: usize,
        _start: (),
        _event_tx: telephone_line::EventSender<Self::Payload, Self::Event>,
    ) -> Self
    where
        Self: Sized,
    {
        Self {
            node_id: init.node_id,
            msg_id,
            logs: HashMap::new(),
        }
    }

    fn step_message(
        &mut self,
        message: telephone_line::Message<Self::Payload>,
        output: &mut impl std::io::Write,
    ) -> anyhow::Result<()> {
        // TODO: let original_in_reply_to = message.body.in_reply_to;
        let mut reply = message.reply(Some(&mut self.msg_id));

        let receive_payload = reply
            .body
            .payload
            .clone()
            .try_into()
            .unwrap_or_else(|payload| panic!("unexpected message {payload:?}"));
        match receive_payload {
            payload::Receive::Logs(payload) => match payload {
                payload::LogsReceive::Send { key, msg } => {
                    let log = self.logs.entry(key).or_insert_with(Log::default);
                    let offset = log
                        .messages
                        .last_key_value()
                        .map(|(offset, _msg)| offset + 1)
                        .unwrap_or_default();
                    log.messages.insert(offset, msg);

                    reply.body.payload = payload::LogsSend::SendOk { offset }.into();
                    reply.send(output)
                }
                payload::LogsReceive::Poll { offsets } => {
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

                    reply.body.payload = payload::LogsSend::PollOk { msgs }.into();
                    reply.send(output)
                }
                payload::LogsReceive::CommitOffsets { offsets } => {
                    for (key, offset) in offsets {
                        if let Some(log) = self.logs.get_mut(&key) {
                            log.committed_offset = Some(offset);
                        }
                    }

                    reply.body.payload = payload::LogsSend::CommitOffsetsOk.into();
                    reply.send(output)
                }
                payload::LogsReceive::ListCommittedOffsets { keys } => {
                    let offsets = keys
                        .into_iter()
                        .filter_map(|key| {
                            let log = self.logs.get(&key)?;
                            Some((key, log.committed_offset?))
                        })
                        .collect();

                    reply.body.payload =
                        payload::LogsSend::ListCommittedOffsetsOk { offsets }.into();
                    reply.send(output)
                }
            },
            payload::Receive::Kv(payload) => match payload {
                key_value::Receive::ReadOk { value } => todo!(),
                key_value::Receive::WriteOk => todo!(),
                key_value::Receive::CasOk => todo!(),
                key_value::Receive::Error { code, text } => todo!(),
            },
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
impl Logs {
    fn kv_message(
        &mut self,
        payload_from_key_fn: impl FnOnce(String) -> key_value::SendLin,
    ) -> Message<payload::Raw> {
        /// Key for the centralized count
        const KEY_COUNT: &str = "c";

        let msg_id = telephone_line::next_msg_id(&mut self.msg_id);
        let key = KEY_COUNT.to_string();
        let payload = payload_from_key_fn(key).into();
        Message {
            src: self.node_id.clone(),
            dest: payload::key_value::NODE_ID_LIN.to_string(),
            body: Body {
                msg_id: Some(msg_id),
                in_reply_to: None,
                payload,
            },
        }
    }
}

fn main() -> anyhow::Result<()> {
    main_loop::<Logs, _>(())
}
