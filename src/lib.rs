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

enum MessageEvent<P, U = Never> {
    Message(Message<P>),
    Event(U),
}
// impl<P> MessageEvent<P, Never> {
//     pub fn into_message(self) -> Message<P> {
//         match self {
//             MessageEvent::Message(message) => message,
//             MessageEvent::Event(never) => match never {},
//         }
//     }
// }

pub type NeverSender<P> = EventSender<P, Never>;
pub enum Never {}

struct Shutdown;

pub struct EventSender<P, T>(
    std::sync::mpsc::Sender<anyhow::Result<Result<MessageEvent<P, T>, Shutdown>>>,
);
pub struct EventSendError;
impl<P, T> EventSender<P, T> {
    pub fn send(&mut self, event: T) -> Result<(), EventSendError> {
        self.0
            .send(Ok(Ok(MessageEvent::Event(event))))
            .map_err(|_| EventSendError)
    }
}

pub trait Node<S = ()> {
    type Payload: Serialize + DeserializeOwned + Send + 'static;
    type Event: Send + 'static;

    fn from_init(
        init: Init,
        msg_id: usize,
        start: S,
        event_tx: EventSender<Self::Payload, Self::Event>,
    ) -> Self
    where
        Self: Sized;

    fn step_message(
        &mut self,
        message: Message<Self::Payload>,
        output: &mut impl std::io::Write,
    ) -> anyhow::Result<()>;

    fn step_event(
        &mut self,
        event: Self::Event,
        output: &mut impl std::io::Write,
    ) -> anyhow::Result<()>;
}

fn parse_message<P>(line_result: Result<String, std::io::Error>) -> anyhow::Result<Message<P>>
where
    P: DeserializeOwned,
{
    let line = line_result.context("read from stdin")?;
    let message = Message::from_json(&line).context("message")?;
    Ok(message)
}

pub fn main_loop<N, S>(start: S) -> anyhow::Result<()>
where
    N: Node<S>,
{
    let mut stdout = std::io::stdout().lock();
    let mut msg_id = 0;

    let (input_tx, input_rx) = std::sync::mpsc::channel();
    let event_tx = EventSender(input_tx.clone());

    let mut node: N = {
        let stdin = std::io::stdin().lock();
        let mut stdin = stdin.lines();

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

        Node::from_init(init, msg_id, start, event_tx)
    };

    std::thread::spawn(move || {
        let stdin = std::io::stdin().lock();
        let stdin = stdin.lines();
        for line_result in stdin {
            let result = parse_message(line_result)
                .map(MessageEvent::Message)
                .map(Ok);
            if input_tx.send(result).is_err() {
                break;
            }
        }
        println!("end of input");
        let _ = input_tx.send(Ok(Err(Shutdown)));
    });

    while let Ok(input_result) = input_rx.recv() {
        match input_result? {
            Ok(input) => match input {
                MessageEvent::Message(message) => node.step_message(message, &mut stdout)?,
                MessageEvent::Event(event) => node.step_event(event, &mut stdout)?,
            },
            Err(Shutdown) => break,
        }
    }

    Ok(())
}
