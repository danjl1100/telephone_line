use anyhow::bail;
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
    neighbors: Vec<String>,
    others_know: HashMap<String, HashSet<usize>>,
}

const GOSSIP_INTERVAL: Duration = Duration::from_millis(500);

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
            if event_tx.send(Event::StartGossip).is_err() {
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
            neighbors: Vec::new(),
            others_know,
        }
    }

    fn step_message(
        &mut self,
        message: Message<Self::Payload>,
        output: &mut impl std::io::Write,
    ) -> anyhow::Result<()> {
        let mut reply = message.reply(Some(&mut self.msg_id));
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
            Payload::Topology { topology } => {
                let mut neighbors_iter = topology
                    .into_iter()
                    .filter_map(|(key, value)| (key == self.node_id).then_some(value));
                let Some(neighbors) = neighbors_iter.next() else {
                    bail!("node_id {} not found in topology", self.node_id)
                };
                self.neighbors = neighbors;

                reply.body.payload = Payload::TopologyOk;
                reply.send(output)
            }
            Payload::BroadcastOk | Payload::ReadOk { .. } | Payload::TopologyOk => {
                bail!("unexpected GenerateOk from {}", reply.dest)
            }
            Payload::Gossip { messages } => {
                // extend our knowledge
                self.messages.extend(messages.clone());
                let original_src = reply.dest; // due to swap in `Message::reply`
                let Some(other_know) = self.others_know
                            .get_mut(&original_src) else {
                    bail!("unknown gossip node {original_src}");
                };

                // extend our knowledge of others
                other_know.extend(messages);

                Ok(())
            }
        }
    }

    fn step_event(&mut self, event: Event, output: &mut impl std::io::Write) -> anyhow::Result<()> {
        const CHANCE_REAFFIRM_PROBABILITY: f64 = 0.05;
        let rng = &mut rand::thread_rng();
        match event {
            Event::StartGossip => {
                for neighbor in &self.neighbors {
                    let Some(other_know) = self.others_know.get(neighbor) else {
                        bail!("unknown neighbor {neighbor}");
                    };
                    let gossip_messages: HashSet<_> = self
                        .messages
                        .iter()
                        .copied()
                        .filter(|m| !other_know.contains(m))
                        .chain(
                            other_know
                                .iter()
                                .copied()
                                .filter(|_| rng.gen_bool(CHANCE_REAFFIRM_PROBABILITY)),
                        )
                        .collect();
                    if !gossip_messages.is_empty() {
                        Message {
                            src: self.node_id.clone(),
                            dest: neighbor.clone(),
                            body: Body {
                                msg_id: None,
                                in_reply_to: None,
                                payload: Payload::Gossip {
                                    messages: gossip_messages,
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
