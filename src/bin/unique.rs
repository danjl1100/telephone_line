use anyhow::bail;
use serde::{Deserialize, Serialize};
use telephone_line::{main_loop, Message, Node};

struct Unique {
    msg_id: usize,
    node_id: String,
}

impl Node for Unique {
    type Payload = Payload;

    fn from_init(init: telephone_line::Init, msg_id: usize, _start: ()) -> Self
    where
        Self: Sized,
    {
        Self {
            msg_id,
            node_id: init.node_id,
        }
    }

    fn step(
        &mut self,
        message: Message<Self::Payload>,
        output: &mut impl std::io::Write,
    ) -> anyhow::Result<()> {
        let mut reply = message.reply(Some(&mut self.msg_id));
        match reply.body.payload {
            Payload::Generate => {
                let id = format!("{}-{}", &self.node_id, self.msg_id);
                reply.body.payload = Payload::GenerateOk { id };
                reply.send(output)?;
            }
            Payload::GenerateOk { .. } => bail!("unexpected GenerateOk from {}", reply.dest),
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Payload {
    Generate,
    GenerateOk { id: String },
}

fn main() -> anyhow::Result<()> {
    main_loop::<Unique, _>(())
}
