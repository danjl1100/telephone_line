use anyhow::bail;
use serde::{Deserialize, Serialize};
use std::{collections::VecDeque, time::Duration};
use telephone_line::{main_loop, Body, EventSender, Message, Node};

struct Counter {
    msg_id: usize,
    node_id: String,
    local_counter: usize,
    central_snapshot: Option<CentralSnapshot>,
    chronological_updates: VecDeque<Snapshot>,
}

#[derive(Clone, Copy, PartialEq)]
struct Snapshot {
    local_count_to_subtract: usize,
    central: CentralSnapshot,
}

#[derive(Clone, Copy, PartialEq)]
struct CentralSnapshot {
    counter: usize,
    msg_id: usize,
}
impl PartialOrd for CentralSnapshot {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        self.counter.partial_cmp(&other.counter)
    }
}

const CENTRAL_UPDATE_INTERVAL: Duration = Duration::from_millis(50);

impl Node for Counter {
    type Payload = payload::Raw;
    type Event = Event;

    fn from_init(
        init: telephone_line::Init,
        msg_id: usize,
        _params: (),
        mut event_tx: EventSender<Self::Payload, Self::Event>,
    ) -> Self
    where
        Self: Sized,
    {
        std::thread::spawn(move || loop {
            std::thread::sleep(CENTRAL_UPDATE_INTERVAL);
            let result = event_tx.send(Event::CentralSnapshot);
            if result.is_err() {
                break;
            }
        });
        Self {
            msg_id,
            node_id: init.node_id,
            local_counter: 0,
            central_snapshot: None,
            chronological_updates: VecDeque::new(),
        }
    }

    fn step_message(
        &mut self,
        message: Message<Self::Payload>,
        output: &mut impl std::io::Write,
    ) -> anyhow::Result<()> {
        let original_in_reply_to = message.body.in_reply_to;
        let mut reply = message.reply(Some(&mut self.msg_id));

        let receive_payload = reply
            .body
            .payload
            .clone()
            .try_into()
            .unwrap_or_else(|payload| panic!("unexpected message {payload:?}"));

        match receive_payload {
            payload::Receive::Count(count) => match count {
                payload::CountReceive::Add { delta } => {
                    self.local_counter += delta;
                    reply.body.payload = payload::CountSend::AddOk.into();
                    reply.send(output)
                }
                payload::CountReceive::Read => {
                    let value = {
                        let local_count = self.local_counter;
                        let global_count =
                            self.central_snapshot.map(|s| s.counter).unwrap_or_default();
                        local_count + global_count
                    };
                    reply.body.payload = payload::CountSend::ReadOk { value }.into();
                    reply.send(output)
                }
            },

            payload::Receive::Kv(kv) => {
                let msg_id =
                    original_in_reply_to.expect("KeyValue response missing field in_reply_to");
                match kv {
                    payload::KeyValueReceive::ReadOk { value } => {
                        self.update_with_snapshot(Snapshot {
                            local_count_to_subtract: 0,
                            central: CentralSnapshot {
                                counter: value,
                                msg_id,
                            },
                        });
                        Ok(())
                    }
                    payload::KeyValueReceive::WriteOk => {
                        self.update_with_snapshot(Snapshot {
                            local_count_to_subtract: 0,
                            central: CentralSnapshot {
                                counter: 0, // only argument to KvWrite is zero (0)
                                msg_id,
                            },
                        });
                        Ok(())
                    }
                    payload::KeyValueReceive::CasOk => self.update_snapshot_cas_succeeded(msg_id),
                    payload::KeyValueReceive::Error { code, text } => match code {
                        KvErrorCode::KeyNotFound => {
                            // TODO this is woefully racy...
                            self.kv_message(|key| payload::KeyValueSend::Write { key, value: 0 })
                                .send(output)
                        }
                        KvErrorCode::CasFromMismatch => self
                            .kv_message(|key| payload::KeyValueSend::Read { key })
                            .send(output),
                        KvErrorCode::Unknown(code) => {
                            panic!("unknown KvErrorCode value {code}, {text}")
                        }
                    },
                }
            }
        }
    }

    fn step_event(&mut self, event: Event, output: &mut impl std::io::Write) -> anyhow::Result<()> {
        match event {
            Event::CentralSnapshot => {
                match self.chronological_updates.back() {
                    Some(last) if last.local_count_to_subtract == self.local_counter => {
                        // no update to send, read current value
                        self.kv_message(|key| payload::KeyValueSend::Read { key })
                            .send(output)
                    }
                    _ if self.local_counter == 0 => {
                        // no updates to send
                        Ok(())
                    }
                    _ => {
                        // update to send
                        let counter_from =
                            self.central_snapshot.map(|s| s.counter).unwrap_or_default();
                        let counter_to = counter_from + self.local_counter;

                        let message = self.kv_message(|key| payload::KeyValueSend::Cas {
                            key,
                            from: counter_from,
                            to: counter_to,
                        });

                        let msg_id = message.body.msg_id.expect("kv_message yields msg_id");
                        self.chronological_updates.push_back(Snapshot {
                            local_count_to_subtract: self.local_counter,
                            central: CentralSnapshot {
                                counter: counter_to,
                                msg_id,
                            },
                        });

                        message.send(output)
                    }
                }
            }
        }
    }
}
impl Counter {
    fn kv_message(
        &mut self,
        payload_from_key_fn: impl FnOnce(String) -> payload::KeyValueSend,
    ) -> Message<payload::Raw> {
        /// Node id of the `seq-kv` provided by maelstrom test harness
        const NODE_ID_SEQ_KV: &str = "seq-kv";
        /// Key for the centralized count
        const KEY_COUNT: &str = "c";

        let msg_id = telephone_line::next_msg_id(&mut self.msg_id);
        let key = KEY_COUNT.to_string();
        let payload = payload_from_key_fn(key).into();
        Message {
            src: self.node_id.clone(),
            dest: NODE_ID_SEQ_KV.to_string(),
            body: Body {
                msg_id: Some(msg_id),
                in_reply_to: None,
                payload,
            },
        }
    }
    fn update_with_snapshot(&mut self, snapshot: Snapshot) {
        // retain only elements AFTER the snapshot'd `msg_id`
        let keep_start_index = self
            .chronological_updates
            .partition_point(|s| s.central.msg_id <= snapshot.central.msg_id);
        let new_len = self.chronological_updates.len() - keep_start_index;
        self.chronological_updates.rotate_left(keep_start_index);
        self.chronological_updates.truncate(new_len);

        self.central_snapshot = Some(snapshot.central);
        assert!(
            self.local_counter >= snapshot.local_count_to_subtract,
            "count to subtract is below snapshot's local counter ({} < {})",
            self.local_counter,
            snapshot.local_count_to_subtract
        );
        self.local_counter -= snapshot.local_count_to_subtract;
    }
    fn update_snapshot_cas_succeeded(&mut self, msg_id: usize) -> anyhow::Result<()> {
        let Some(update) = self
            .chronological_updates
            .binary_search_by_key(&msg_id, |s| s.central.msg_id)
            .ok()
            .and_then(|index| self.chronological_updates.get(index).copied())
        else {
            bail!("no chronological_updates element matching msg_id {msg_id}");
        };
        self.update_with_snapshot(update);
        Ok(())
    }
}

enum Event {
    CentralSnapshot,
}

pub mod payload {
    use super::*;

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
}

use key_value_error::KvErrorCode;
pub mod key_value_error {
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

fn main() -> anyhow::Result<()> {
    main_loop::<Counter, _>(())
}
