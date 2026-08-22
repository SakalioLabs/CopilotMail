use copilotmail_platform::init_observability;

fn main() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let _guard = init_observability()?;
    let span = tracing::info_span!(
        target: "copilotmail_observability_smoke",
        "observability.smoke",
        smoke.id = "local-verification",
    );
    let _entered = span.enter();
    tracing::info!("observability smoke span completed");
    Ok(())
}
