//! Read-only readiness evidence for the exact schema compiled into this binary.
//! Migrating remains a separate, explicitly invoked operation.

use crate::MIGRATOR;
use sqlx::PgPool;

#[derive(Clone, sqlx::FromRow)]
struct AppliedMigration {
    version: i64,
    checksum: Vec<u8>,
    success: bool,
}

/// Compare the entire ordered ledger, not just the largest version number.
/// A missing, failed, altered, duplicate, or unknown migration is not ready.
fn matches_compiled_schema(applied: &[AppliedMigration]) -> bool {
    let compiled: Vec<_> = MIGRATOR.iter().collect();
    applied.len() == compiled.len()
        && applied.iter().zip(compiled).all(|(actual, expected)| {
            actual.success
                && actual.version == expected.version
                && actual.checksum.as_slice() == expected.checksum.as_ref()
        })
}

pub(super) async fn ready(pool: &PgPool) -> Result<bool, sqlx::Error> {
    if !sqlx::query_scalar::<_, bool>("SELECT to_regclass('public._sqlx_migrations') IS NOT NULL")
        .fetch_one(pool)
        .await?
    {
        return Ok(false);
    }
    let applied = sqlx::query_as::<_, AppliedMigration>(
        "SELECT version, checksum, success FROM public._sqlx_migrations ORDER BY version",
    )
    .fetch_all(pool)
    .await?;
    if !matches_compiled_schema(&applied) {
        return Ok(false);
    }
    // Preserve the deeper constraint/trigger/index readiness contract. A
    // matching ledger alone cannot prove that an index is valid and ready.
    sqlx::query_scalar(include_str!("schema/readiness.sql"))
        .fetch_one(pool)
        .await
}

#[cfg(test)]
mod tests {
    use super::*;

    fn current() -> Vec<AppliedMigration> {
        MIGRATOR
            .iter()
            .map(|migration| AppliedMigration {
                version: migration.version,
                checksum: migration.checksum.to_vec(),
                success: true,
            })
            .collect()
    }

    #[test]
    fn exact_ledger_is_required_not_only_a_latest_version_marker() {
        let expected = current();
        assert!(matches_compiled_schema(&expected));
        assert!(!matches_compiled_schema(&[]));
        let mut missing_old = expected.clone();
        missing_old.remove(0);
        assert!(!matches_compiled_schema(&missing_old));
        let mut missing_new = expected.clone();
        missing_new.pop();
        assert!(!matches_compiled_schema(&missing_new));
        let mut unknown = expected.clone();
        unknown.push(AppliedMigration {
            version: i64::MAX,
            checksum: vec![],
            success: true,
        });
        assert!(!matches_compiled_schema(&unknown));
    }

    #[test]
    fn failed_modified_duplicate_or_reordered_ledger_is_not_ready() {
        for variant in 0..4 {
            let mut rows = current();
            match variant {
                0 => rows[0].success = false,
                1 => rows[0].checksum[0] ^= 1,
                2 => rows[1] = rows[0].clone(),
                _ => rows.swap(0, 1),
            }
            assert!(!matches_compiled_schema(&rows));
        }
    }
}
