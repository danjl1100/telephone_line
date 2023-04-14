use anyhow::bail;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use telephone_line::{main_loop, Message, Node};

struct Broadcast {
    msg_id: usize,
    // TODO
    // node_id: String,
    messages: HashSet<usize>,
}

impl Node for Broadcast {
    type Payload = Payload;

    fn from_init(_init: telephone_line::Init, msg_id: usize, _start: ()) -> Self
    where
        Self: Sized,
    {
        Self {
            msg_id,
            // TODO
            // node_id: init.node_id,
            messages: HashSet::new(),
        }
    }

    fn step(
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
            Payload::Topology { topology: _ } => {
                // TODO - use topology
                reply.body.payload = Payload::TopologyOk;
                reply.send(output)
            }
            Payload::BroadcastOk | Payload::ReadOk { .. } | Payload::TopologyOk => {
                bail!("unexpected GenerateOk from {}", reply.dest)
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
}

fn main() -> anyhow::Result<()> {
    main_loop::<Broadcast, _>(())
}
