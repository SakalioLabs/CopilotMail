use anyhow::{Context, Result};
use chrono::Utc;
use rusqlite::{Connection, OptionalExtension, params};
use serde::Serialize;
use std::{
    path::Path,
    sync::{Arc, Mutex},
};
use uuid::Uuid;

#[derive(Clone)]
pub struct Store(Arc<Mutex<Connection>>);

#[derive(Debug, Clone, Serialize)]
pub struct Mailbox {
    pub id: String,
    pub address: String,
    pub created_at: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct MessageSummary {
    pub id: String,
    pub mailbox_id: String,
    pub sender: String,
    pub recipient: String,
    pub subject: String,
    pub received_at: String,
    pub seen: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct Message {
    #[serde(flatten)]
    pub summary: MessageSummary,
    pub text: String,
    pub html: String,
}

pub struct NewMessage<'a> {
    pub mailbox_id: &'a str,
    pub sender: &'a str,
    pub recipient: &'a str,
    pub subject: &'a str,
    pub text: &'a str,
    pub html: &'a str,
    pub raw: &'a [u8],
}

impl Store {
    pub fn open(path: &Path) -> Result<Self> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let connection = Connection::open(path).context("open sqlite database")?;
        connection.execute_batch("PRAGMA journal_mode=WAL; PRAGMA foreign_keys=ON;
          CREATE TABLE IF NOT EXISTS mailboxes(id TEXT PRIMARY KEY,address TEXT NOT NULL UNIQUE,password_hash TEXT NOT NULL,created_at TEXT NOT NULL);
          CREATE TABLE IF NOT EXISTS messages(id TEXT PRIMARY KEY,mailbox_id TEXT NOT NULL,sender TEXT NOT NULL,recipient TEXT NOT NULL,subject TEXT NOT NULL,text_body TEXT NOT NULL,html_body TEXT NOT NULL,raw BLOB NOT NULL,seen INTEGER NOT NULL DEFAULT 0,received_at TEXT NOT NULL,FOREIGN KEY(mailbox_id) REFERENCES mailboxes(id) ON DELETE CASCADE);
          CREATE INDEX IF NOT EXISTS idx_messages_mailbox ON messages(mailbox_id,received_at DESC);")?;
        Ok(Self(Arc::new(Mutex::new(connection))))
    }

    pub fn create_mailbox(&self, address: &str, password_hash: &str) -> Result<Mailbox> {
        let item = Mailbox {
            id: Uuid::new_v4().to_string(),
            address: address.into(),
            created_at: Utc::now().to_rfc3339(),
        };
        self.0.lock().unwrap().execute(
            "INSERT INTO mailboxes VALUES(?1,?2,?3,?4)",
            params![item.id, item.address, password_hash, item.created_at],
        )?;
        Ok(item)
    }

    pub fn mailbox_auth(&self, address: &str) -> Result<Option<(Mailbox, String)>> {
        self.0
            .lock()
            .unwrap()
            .query_row(
                "SELECT id,address,created_at,password_hash FROM mailboxes WHERE address=?1",
                [address],
                |r| {
                    Ok((
                        Mailbox {
                            id: r.get(0)?,
                            address: r.get(1)?,
                            created_at: r.get(2)?,
                        },
                        r.get(3)?,
                    ))
                },
            )
            .optional()
            .map_err(Into::into)
    }

    pub fn mailbox_by_address(&self, address: &str) -> Result<Option<Mailbox>> {
        self.0
            .lock()
            .unwrap()
            .query_row(
                "SELECT id,address,created_at FROM mailboxes WHERE address=?1",
                [address],
                |r| {
                    Ok(Mailbox {
                        id: r.get(0)?,
                        address: r.get(1)?,
                        created_at: r.get(2)?,
                    })
                },
            )
            .optional()
            .map_err(Into::into)
    }

    pub fn insert_message(&self, message: NewMessage<'_>) -> Result<MessageSummary> {
        let item = MessageSummary {
            id: Uuid::new_v4().to_string(),
            mailbox_id: message.mailbox_id.into(),
            sender: message.sender.into(),
            recipient: message.recipient.into(),
            subject: message.subject.into(),
            received_at: Utc::now().to_rfc3339(),
            seen: false,
        };
        self.0.lock().unwrap().execute("INSERT INTO messages(id,mailbox_id,sender,recipient,subject,text_body,html_body,raw,received_at) VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9)",params![item.id,item.mailbox_id,item.sender,item.recipient,item.subject,message.text,message.html,message.raw,item.received_at])?;
        Ok(item)
    }

    pub fn message(&self, mailbox_id: &str, id: &str) -> Result<Option<Message>> {
        self.0.lock().unwrap().query_row(
            "SELECT id,mailbox_id,sender,recipient,subject,received_at,seen,text_body,html_body FROM messages WHERE mailbox_id=?1 AND id=?2",
            params![mailbox_id,id],
            |r| Ok(Message { summary: MessageSummary { id:r.get(0)?,mailbox_id:r.get(1)?,sender:r.get(2)?,recipient:r.get(3)?,subject:r.get(4)?,received_at:r.get(5)?,seen:r.get(6)? }, text:r.get(7)?, html:r.get(8)? })
        ).optional().map_err(Into::into)
    }

    pub fn messages(&self, mailbox_id: &str) -> Result<Vec<MessageSummary>> {
        let db = self.0.lock().unwrap();
        let mut q=db.prepare("SELECT id,mailbox_id,sender,recipient,subject,received_at,seen FROM messages WHERE mailbox_id=?1 ORDER BY received_at DESC")?;
        Ok(q.query_map([mailbox_id], |r| {
            Ok(MessageSummary {
                id: r.get(0)?,
                mailbox_id: r.get(1)?,
                sender: r.get(2)?,
                recipient: r.get(3)?,
                subject: r.get(4)?,
                received_at: r.get(5)?,
                seen: r.get(6)?,
            })
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn store() -> Store {
        Store::open(Path::new(":memory:")).unwrap()
    }

    #[test]
    fn creates_and_authenticates_mailbox() {
        let store = store();
        let mailbox = store
            .create_mailbox("agent@mail.local", "password-hash")
            .unwrap();
        let (loaded, hash) = store.mailbox_auth("agent@mail.local").unwrap().unwrap();
        assert_eq!(loaded.id, mailbox.id);
        assert_eq!(hash, "password-hash");
    }

    #[test]
    fn rejects_duplicate_mailbox() {
        let store = store();
        store.create_mailbox("agent@mail.local", "one").unwrap();
        assert!(store.create_mailbox("agent@mail.local", "two").is_err());
    }

    #[test]
    fn isolates_messages_by_mailbox() {
        let store = store();
        let first = store.create_mailbox("one@mail.local", "hash").unwrap();
        let second = store.create_mailbox("two@mail.local", "hash").unwrap();
        let inserted = store
            .insert_message(NewMessage {
                mailbox_id: &first.id,
                sender: "sender@example.test",
                recipient: &first.address,
                subject: "hello",
                text: "body",
                html: "",
                raw: b"Subject: hello\r\n\r\nbody",
            })
            .unwrap();
        assert_eq!(store.messages(&first.id).unwrap().len(), 1);
        assert!(store.messages(&second.id).unwrap().is_empty());
        assert_eq!(
            store
                .message(&first.id, &inserted.id)
                .unwrap()
                .unwrap()
                .text,
            "body"
        );
        assert!(store.message(&second.id, &inserted.id).unwrap().is_none());
    }
}
