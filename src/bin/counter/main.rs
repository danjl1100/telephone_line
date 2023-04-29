use anyhow::{bail, Context};
use once_cell::sync::Lazy;
use regex::Regex;
use std::{collections::VecDeque, time::Duration};
use telephone_line::{main_loop, Body, EventSender, Message, Node};

use payload::KvErrorCode;
pub mod payload;

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

const CENTRAL_UPDATE_INTERVAL: Duration = Duration::from_millis(1000);

static KV_CAS_ERROR_REGEX: Lazy<Regex> =
    Lazy::new(|| Regex::new(r"current value (?P<value>[\d]+) is not [\d]+").unwrap());

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
                        KvErrorCode::CasFromMismatch => {
                            use std::str::FromStr;
                            // attempt to parse error message "current value {N} is not {M}"
                            let Some(value) = KV_CAS_ERROR_REGEX
                                             .captures(&text)
                                             .and_then(|cap| cap.name("value")) else {
                                bail!("failed to parse new value from Cas {code:?} error string {text:?}")
                            };
                            let value = value.as_str();
                            let counter = usize::from_str(value)
                                .context(format!("invalid number {value:?}"))
                                .context(format!("parsing {code:?} error string {text:?}"))?;
                            self.update_with_snapshot(Snapshot {
                                local_count_to_subtract: 0,
                                central: CentralSnapshot { counter, msg_id },
                            });
                            Ok(())
                            // ALTERNATIVE: not parsing the error string
                            // self.kv_message(|key| payload::KeyValueSend::Read { key })
                            //     .send(output)
                        }
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
                let no_change_since_last_send = matches!(
                    self.chronological_updates.back(),
                    Some(last) if last.local_count_to_subtract == self.local_counter
                );
                if no_change_since_last_send || self.local_counter == 0 {
                    // no update to send, read current value
                    self.kv_message(|key| payload::KeyValueSend::Read { key })
                        .send(output)
                } else {
                    // update to send
                    let counter_from = self.central_snapshot.map(|s| s.counter).unwrap_or_default();
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

fn main() -> anyhow::Result<()> {
    main_loop::<Counter, _>(())
}
