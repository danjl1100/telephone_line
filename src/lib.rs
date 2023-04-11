use anyhow::{bail, Context};
use serde::{de::DeserializeOwned, Deserialize, Serialize};
use std::io::BufRead;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Message<P> {
    pub src: String,
    pub dest: String,
    pub body: Body<P>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Body<P> {
    pub msg_id: Option<usize>,
    pub in_reply_to: Option<usize>,
    #[serde(flatten)]
    pub payload: P,
}

impl<P> Message<P> {
    pub fn from_json(input: &str) -> anyhow::Result<Self>
    where
        P: DeserializeOwned,
    {
        serde_json::from_str(input).context("deserialize message")
    }
    pub fn reply(self, id: Option<&mut usize>) -> Self {
        let Message {
            src,
            dest,
            body:
                Body {
                    payload,
                    msg_id,
                    in_reply_to: _,
                },
        } = self;
        let new_msg_id = id.map(|id| {
            let current = *id;
            *id += 1;
            current
        });
        Message {
            src: dest,
            dest: src,
            body: Body {
                msg_id: new_msg_id,
                in_reply_to: msg_id,
                payload,
            },
        }
    }
    pub fn send(self, output: &mut impl std::io::Write) -> anyhow::Result<()>
    where
        P: Serialize,
    {
        serde_json::to_writer(&mut *output, &self).context("write message")?;
        output.write_all(b"\n")?;
        Ok(())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum InitPayload {
    Init(Init),
    InitOk,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Init {
    pub node_id: String,
    pub node_ids: Vec<String>,
}

pub trait Node<S = ()> {
    type Payload: Serialize + DeserializeOwned;

    fn from_init(init: Init, msg_id: usize, start: S) -> Self
    where
        Self: Sized;

    fn step(
        &mut self,
        message: Message<Self::Payload>,
        output: &mut impl std::io::Write,
    ) -> anyhow::Result<()>;
}

pub fn main_loop<N, S>(start: S) -> anyhow::Result<()>
where
    N: Node<S>,
{
    let stdin = std::io::stdin().lock();
    let mut stdin = stdin.lines();

    let mut stdout = std::io::stdout().lock();
    let mut msg_id = 0;

    let mut node: N = {
        let init_message = stdin
            .next()
            .expect("initial message not present on stdin")
            .context("read from stdin")?;
        let init_message: Message<InitPayload> =
            Message::from_json(&init_message).context("initial message")?;
        let mut reply = init_message.reply(Some(&mut msg_id));
        let InitPayload::Init(init) =
            std::mem::replace(&mut reply.body.payload, InitPayload::InitOk) else {
                bail!("initial message not Init")
            };

        reply.send(&mut stdout).context("init_ok reply")?;

        Node::from_init(init, msg_id, start)
    };

    for line in stdin {
        let line = line.context("read from stdin")?;
        let message = Message::from_json(&line).context("message")?;
        node.step(message, &mut stdout)?;
    }
    Ok(())
}
