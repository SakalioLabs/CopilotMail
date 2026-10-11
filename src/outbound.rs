use anyhow::{Context, Result, bail};
use hickory_resolver::{TokioResolver, proto::rr::RData};
use tokio::{
    io::{AsyncBufReadExt, AsyncWriteExt, BufReader},
    net::TcpStream,
};

pub async fn deliver(sender: &str, recipient: &str, raw: &[u8]) -> Result<()> {
    let domain = recipient
        .rsplit_once('@')
        .map(|(_, domain)| domain)
        .context("recipient has no domain")?;
    let resolver = TokioResolver::builder_tokio()?.build()?;
    let lookup = resolver
        .mx_lookup(domain)
        .await
        .context("MX lookup failed")?;
    let mut exchanges: Vec<_> = lookup
        .answers()
        .iter()
        .filter_map(|record| match &record.data {
            RData::MX(mx) => Some(mx),
            _ => None,
        })
        .collect();
    exchanges.sort_by_key(|mx| mx.preference);
    let mut last_error = None;
    for mx in exchanges {
        let host = mx.exchange.to_utf8().trim_end_matches('.').to_string();
        match deliver_to_host(&host, sender, recipient, raw).await {
            Ok(()) => return Ok(()),
            Err(error) => last_error = Some(error),
        }
    }
    Err(last_error.unwrap_or_else(|| anyhow::anyhow!("domain has no MX records")))
}

async fn deliver_to_host(host: &str, sender: &str, recipient: &str, raw: &[u8]) -> Result<()> {
    let stream = TcpStream::connect((host, 25)).await?;
    let (read, mut write) = stream.into_split();
    let mut read = BufReader::new(read);
    expect(&mut read, 220).await?;
    command(&mut read, &mut write, b"EHLO copilotmail\r\n", 250).await?;
    command(
        &mut read,
        &mut write,
        format!("MAIL FROM:<{sender}>\r\n").as_bytes(),
        250,
    )
    .await?;
    command(
        &mut read,
        &mut write,
        format!("RCPT TO:<{recipient}>\r\n").as_bytes(),
        250,
    )
    .await?;
    command(&mut read, &mut write, b"DATA\r\n", 354).await?;
    for line in raw.split(|byte| *byte == b'\n') {
        if line.first() == Some(&b'.') {
            write.write_all(b".").await?;
        }
        write
            .write_all(line.strip_suffix(b"\r").unwrap_or(line))
            .await?;
        write.write_all(b"\r\n").await?;
    }
    write.write_all(b".\r\n").await?;
    write.flush().await?;
    expect(&mut read, 250).await?;
    let _ = command(&mut read, &mut write, b"QUIT\r\n", 221).await;
    Ok(())
}

async fn command(
    read: &mut BufReader<tokio::net::tcp::OwnedReadHalf>,
    write: &mut tokio::net::tcp::OwnedWriteHalf,
    command: &[u8],
    expected: u16,
) -> Result<()> {
    write.write_all(command).await?;
    write.flush().await?;
    expect(read, expected).await
}

async fn expect(read: &mut BufReader<tokio::net::tcp::OwnedReadHalf>, expected: u16) -> Result<()> {
    loop {
        let mut line = String::new();
        if read.read_line(&mut line).await? == 0 {
            bail!("SMTP peer closed connection")
        }
        let code: u16 = line.get(0..3).context("short SMTP response")?.parse()?;
        if code != expected {
            bail!("SMTP peer returned {line:?}")
        }
        if line.as_bytes().get(3) != Some(&b'-') {
            return Ok(());
        }
    }
}
