use crate::{
    events::Events,
    store::{NewMessage, Store},
};
use anyhow::Result;
use mail_parser::MessageParser;
use std::net::SocketAddr;
use tokio::{
    io::{AsyncBufReadExt, AsyncWriteExt, BufReader},
    net::{TcpListener, TcpStream},
};

pub async fn serve(addr: SocketAddr, store: Store, domain: String, events: Events) -> Result<()> {
    let listener = TcpListener::bind(addr).await?;
    loop {
        let (stream, peer) = listener.accept().await?;
        let store = store.clone();
        let domain = domain.clone();
        let events = events.clone();
        tokio::spawn(async move {
            if let Err(e) = session(stream, store, &domain, events).await {
                tracing::warn!(%peer,error=%e,"SMTP session failed");
            }
        });
    }
}

async fn session(stream: TcpStream, store: Store, domain: &str, events: Events) -> Result<()> {
    let (read, mut write) = stream.into_split();
    let mut lines = BufReader::new(read).lines();
    write
        .write_all(format!("220 {domain} CopilotMail ESMTP\r\n").as_bytes())
        .await?;
    let mut sender = String::new();
    let mut recipients = Vec::new();
    while let Some(line) = lines.next_line().await? {
        let upper = line.to_ascii_uppercase();
        if upper.starts_with("EHLO") || upper.starts_with("HELO") {
            write
                .write_all(format!("250-{domain}\r\n250 SIZE 26214400\r\n").as_bytes())
                .await?;
        } else if upper.starts_with("MAIL FROM:") {
            sender = path(&line[10..]);
            recipients.clear();
            write.write_all(b"250 2.1.0 OK\r\n").await?;
        } else if upper.starts_with("RCPT TO:") {
            let recipient = path(&line[8..]).to_ascii_lowercase();
            if !recipient.ends_with(&format!("@{domain}"))
                || store.mailbox_by_address(&recipient)?.is_none()
            {
                write
                    .write_all(b"550 5.1.1 Mailbox unavailable\r\n")
                    .await?;
            } else {
                recipients.push(recipient);
                write.write_all(b"250 2.1.5 OK\r\n").await?;
            }
        } else if upper == "DATA" {
            if sender.is_empty() || recipients.is_empty() {
                write
                    .write_all(b"503 5.5.1 Need MAIL and RCPT first\r\n")
                    .await?;
                continue;
            }
            write
                .write_all(b"354 End data with <CR><LF>.<CR><LF>\r\n")
                .await?;
            let mut raw = Vec::new();
            let mut too_large = false;
            while let Some(data) = lines.next_line().await? {
                if data == "." {
                    break;
                }
                if too_large {
                    continue;
                }
                let data = data.strip_prefix("..").unwrap_or(&data);
                raw.extend_from_slice(data.as_bytes());
                raw.extend_from_slice(b"\r\n");
                if raw.len() > 26_214_400 {
                    too_large = true;
                }
            }
            if too_large {
                write.write_all(b"552 5.3.4 Message too large\r\n").await?;
                sender.clear();
                recipients.clear();
                continue;
            }
            persist(&store, &events, &sender, &recipients, &raw)?;
            write.write_all(b"250 2.0.0 Queued\r\n").await?;
            sender.clear();
            recipients.clear();
        } else if upper == "RSET" {
            sender.clear();
            recipients.clear();
            write.write_all(b"250 2.0.0 Reset\r\n").await?;
        } else if upper == "NOOP" {
            write.write_all(b"250 2.0.0 OK\r\n").await?;
        } else if upper == "QUIT" {
            write.write_all(b"221 2.0.0 Bye\r\n").await?;
            break;
        } else {
            write
                .write_all(b"502 5.5.2 Command not implemented\r\n")
                .await?;
        }
    }
    Ok(())
}

pub(crate) fn persist(
    store: &Store,
    events: &Events,
    envelope_sender: &str,
    recipients: &[String],
    raw: &[u8],
) -> Result<()> {
    let parsed = MessageParser::default()
        .parse(raw)
        .ok_or_else(|| anyhow::anyhow!("invalid RFC 5322 message"))?;
    let sender = parsed
        .from()
        .and_then(|a| a.first())
        .and_then(|a| a.address())
        .unwrap_or(envelope_sender);
    let subject = parsed.subject().unwrap_or("(no subject)");
    let text = parsed
        .body_text(0)
        .map(|v| v.into_owned())
        .unwrap_or_default();
    let html = parsed
        .body_html(0)
        .map(|v| v.into_owned())
        .unwrap_or_default();
    for recipient in recipients {
        if let Some(mailbox) = store.mailbox_by_address(recipient)? {
            let message = store.insert_message(NewMessage {
                mailbox_id: &mailbox.id,
                sender,
                recipient,
                subject,
                text: &text,
                html: &html,
                raw,
            })?;
            events.received(mailbox.id, message.id);
        }
    }
    Ok(())
}
fn path(value: &str) -> String {
    value
        .trim()
        .trim_start_matches('<')
        .split('>')
        .next()
        .unwrap_or("")
        .trim()
        .into()
}
