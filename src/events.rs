use serde::Serialize;
use tokio::sync::broadcast;

#[derive(Clone, Debug, Serialize)]
pub struct MailEvent {
    #[serde(rename = "type")]
    pub kind: &'static str,
    pub mailbox_id: String,
    pub message_id: String,
}

#[derive(Clone)]
pub struct Events(broadcast::Sender<MailEvent>);

impl Events {
    pub fn new() -> Self {
        let (sender, _) = broadcast::channel(256);
        Self(sender)
    }

    pub fn subscribe(&self) -> broadcast::Receiver<MailEvent> {
        self.0.subscribe()
    }

    pub fn received(&self, mailbox_id: String, message_id: String) {
        let _ = self.0.send(MailEvent {
            kind: "message.received",
            mailbox_id,
            message_id,
        });
    }
}
