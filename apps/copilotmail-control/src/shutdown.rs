use crate::*;

/// The operating-system event that requested a graceful process shutdown.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum ShutdownReason {
    CtrlC,
    Terminate,
}

impl ShutdownReason {
    pub(crate) const fn as_str(self) -> &'static str {
        match self {
            Self::CtrlC => "ctrl_c",
            Self::Terminate => "sigterm",
        }
    }
}

/// Resolves with the reason corresponding to whichever signal future completes
/// first.
///
/// Keeping the selection independent from process-global signal registration
/// lets unit tests verify the semantics without sending a signal to the test
/// runner.
pub(crate) async fn select_shutdown_reason<C, T>(
    ctrl_c: C,
    terminate: T,
) -> std::io::Result<ShutdownReason>
where
    C: Future<Output = std::io::Result<()>>,
    T: Future<Output = std::io::Result<()>>,
{
    tokio::select! {
        result = ctrl_c => result.map(|()| ShutdownReason::CtrlC),
        result = terminate => result.map(|()| ShutdownReason::Terminate),
    }
}

/// Waits for Ctrl+C or SIGTERM on Unix hosts.
#[cfg(unix)]
pub(crate) async fn wait_for_shutdown_signal() -> std::io::Result<ShutdownReason> {
    use tokio::signal::unix::{SignalKind, signal};

    let mut terminate = signal(SignalKind::terminate())?;
    let terminate = async move {
        terminate.recv().await.ok_or_else(|| {
            std::io::Error::new(
                std::io::ErrorKind::BrokenPipe,
                "SIGTERM signal stream closed",
            )
        })
    };
    select_shutdown_reason(tokio::signal::ctrl_c(), terminate).await
}

/// Waits for Ctrl+C on Windows and other non-Unix hosts.
#[cfg(not(unix))]
pub(crate) async fn wait_for_shutdown_signal() -> std::io::Result<ShutdownReason> {
    select_shutdown_reason(
        tokio::signal::ctrl_c(),
        std::future::pending::<std::io::Result<()>>(),
    )
    .await
}

/// Completes when the platform's graceful-shutdown signal is received.
///
/// Unix services accept both SIGTERM (the normal container-orchestrator stop
/// signal) and Ctrl+C. Windows services retain Ctrl+C handling. Failure to
/// install or receive a handler is logged and completes this future so callers
/// shut down instead of continuing without a termination path.
pub(crate) async fn shutdown_signal() {
    match wait_for_shutdown_signal().await {
        Ok(reason) => {
            tracing::info!(
                shutdown_reason = reason.as_str(),
                "graceful shutdown signal received"
            );
        }
        Err(error) => {
            tracing::error!(%error, "failed to install or receive shutdown signal");
        }
    }
}
