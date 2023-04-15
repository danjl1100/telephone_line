use anyhow::{bail, Context};
use rand::Rng;
use serde::{Deserialize, Serialize};
use std::{
    collections::{HashMap, HashSet},
    time::Duration,
};
use telephone_line::{main_loop, Body, EventSender, Message, Node};

struct Broadcast {
    msg_id: usize,
    node_id: String,
    messages: HashSet<usize>,
    others_know: HashMap<String, HashSet<usize>>,
}

const GOSSIP_INTERVAL: Duration = Duration::from_millis(400);

const CAP_RATIO: f64 = 0.1;
const CAP_FLOOR: u32 = 10;

impl Node for Broadcast {
    type Payload = Payload;
    type Event = Event;

    fn from_init(
        init: telephone_line::Init,
        msg_id: usize,
        _start: (),
        mut event_tx: EventSender<Self::Payload, Self::Event>,
    ) -> Self
    where
        Self: Sized,
    {
        std::thread::spawn(move || loop {
            std::thread::sleep(GOSSIP_INTERVAL);

            let result = event_tx.send(Event::StartGossip);

            if result.is_err() {
                break;
            }
        });
        let others_know = init
            .node_ids
            .into_iter()
            .map(|n| (n, HashSet::new()))
            .collect();
        Self {
            msg_id,
            node_id: init.node_id,
            messages: HashSet::new(),
            others_know,
        }
    }

    fn step_message(
        &mut self,
        message: Message<Self::Payload>,
        output: &mut impl std::io::Write,
    ) -> anyhow::Result<()> {
        let mut reply = message.reply(Some(&mut self.msg_id));
        let original_src = &reply.dest; // due to swap in `Message::reply`

        match reply.body.payload {
            Payload::Broadcast { message } => {
                self.messages.insert(message);

                reply.body.payload = Payload::BroadcastOk;
                reply.send(output)
            }
            Payload::Read => {
                let messages = self.messages.clone();
                reply.body.payload = Payload::ReadOk { messages };
                reply.send(output)
            }
            Payload::Topology { topology: _ } => {
                // -- IGNORE
                // let mut neighbors_iter = topology
                //     .into_iter()
                //     .filter_map(|(key, value)| (key == self.node_id).then_some(value));
                // let Some(neighbors) = neighbors_iter.next() else {
                //     bail!("node_id {} not found in topology", self.node_id)
                // };
                // self.nodes_ping = neighbors.into_iter().map(|n| (n, 0)).collect();

                reply.body.payload = Payload::TopologyOk;
                reply.send(output)
            }
            Payload::BroadcastOk | Payload::ReadOk { .. } | Payload::TopologyOk => {
                bail!("unexpected GenerateOk from {}", reply.dest)
            }
            Payload::Gossip { messages } => {
                // extend our knowledge
                self.messages.extend(messages.clone());
                let Some(other_know) = self.others_know
                            .get_mut(original_src) else {
                    bail!("unknown gossip node {original_src}");
                };

                // extend our knowledge of others
                other_know.extend(messages);

                Ok(())
            }
        }
    }

    fn step_event(&mut self, event: Event, output: &mut impl std::io::Write) -> anyhow::Result<()> {
        match event {
            Event::StartGossip => {
                for neighbor in self.others_know.keys() {
                    let Some(other_know) = self.others_know.get(neighbor) else {
                        bail!("unknown neighbor {neighbor}");
                    };
                    let (already_known, mut notify_of): (HashSet<_>, HashSet<_>) = self
                        .messages
                        .iter()
                        .copied()
                        .partition(|m| other_know.contains(m));
                    eprintln!("notify of {}/{}", notify_of.len(), self.messages.len());

                    // tell neighbor about some nodes we both know,
                    // so they gradually learn what we know
                    let rng = &mut rand::thread_rng();
                    let additonal_cap = ((notify_of.len() as f64 * CAP_RATIO) as u32) + CAP_FLOOR;
                    let already_known_len = u32::try_from(already_known.len())
                        .context("too many `already_known` message elements to fit in u32!!")?;
                    notify_of.extend(already_known.iter().copied().filter(|_| {
                        rng.gen_ratio(additonal_cap.min(already_known_len), already_known_len)
                    }));

                    if !notify_of.is_empty() {
                        Message {
                            src: self.node_id.clone(),
                            dest: neighbor.clone(),
                            body: Body {
                                msg_id: None,
                                in_reply_to: None,
                                payload: Payload::Gossip {
                                    messages: notify_of,
                                },
                            },
                        }
                        .send(output)?;
                    }
                }
                Ok(())
            }
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Payload {
    Broadcast {
        message: usize,
    },
    BroadcastOk,
    Read,
    ReadOk {
        messages: HashSet<usize>,
    },
    Topology {
        topology: HashMap<String, Vec<String>>,
    },
    TopologyOk,
    Gossip {
        messages: HashSet<usize>,
    },
}

enum Event {
    StartGossip,
}

fn main() -> anyhow::Result<()> {
    main_loop::<Broadcast, _>(())
}
