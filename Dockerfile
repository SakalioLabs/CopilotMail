FROM rust:1.94-bookworm AS builder
WORKDIR /src
COPY Cargo.toml Cargo.lock* ./
COPY src ./src
RUN cargo build --release --locked

FROM debian:bookworm-slim
RUN useradd --system --uid 10001 --create-home copilotmail && mkdir -p /data && chown copilotmail:copilotmail /data
COPY --from=builder /src/target/release/copilotmail /usr/local/bin/copilotmail
COPY web /opt/copilotmail/web
USER copilotmail
ENV COPILOTMAIL_HTTP_ADDR=0.0.0.0:8080 COPILOTMAIL_SMTP_ADDR=0.0.0.0:2525 COPILOTMAIL_DATABASE=/data/copilotmail.db
EXPOSE 8080 2525
ENTRYPOINT ["copilotmail"]

