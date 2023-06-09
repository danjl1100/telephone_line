use anyhow::bail;
use serde::{Deserialize, Serialize};
use telephone_line::{main_loop, Message, Never, NeverSender, Node};

struct Echo {
    msg_id: usize,
}

impl Node for Echo {
    type Payload = Payload;
    type Event = Never;

    fn from_init(
        _init: telephone_line::Init,
        msg_id: usize,
        _start: (),
        _event_tx: NeverSender<Self::Payload>,
    ) -> Self
    where
        Self: Sized,
    {
        Self { msg_id }
    }

    fn step_message(
        &mut self,
        message: Message<Self::Payload>,
        output: &mut impl std::io::Write,
    ) -> anyhow::Result<()> {
        let mut reply = message.reply(Some(&mut self.msg_id));
        match reply.body.payload {
            Payload::Echo { echo } => {
                reply.body.payload = Payload::EchoOk { echo };
                reply.send(output)
            }
            Payload::EchoOk { .. } => bail!("unexpected EchoOk from {}", reply.dest),
        }
    }

    fn step_event(
        &mut self,
        event: Self::Event,
        _output: &mut impl std::io::Write,
    ) -> anyhow::Result<()> {
        match event {}
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Payload {
    Echo { echo: String },
    EchoOk { echo: String },
}

fn main() -> anyhow::Result<()> {
    main_loop::<Echo, _>(())
}
