//! Outbound adapter support: the PostgreSQL pool and the migrations that
//! every store shares. The database holds reference data only (what the
//! school system publishes); nothing a visitor types is written to it.

use std::time::Duration;

use sqlx::postgres::{PgPool, PgPoolOptions};

#[derive(Debug, thiserror::Error)]
pub enum PostgresSetupError {
    #[error("cannot connect to PostgreSQL: {0}")]
    Connect(#[source] sqlx::Error),
    #[error("cannot apply migrations: {0}")]
    Migrate(#[source] sqlx::migrate::MigrateError),
}

/// Opens a pool. Credentials come from the URL, never from source.
pub async fn connect(url: &str, max_connections: u32) -> Result<PgPool, PostgresSetupError> {
    PgPoolOptions::new()
        .max_connections(max_connections)
        .acquire_timeout(Duration::from_secs(10))
        .connect(url)
        .await
        .map_err(PostgresSetupError::Connect)
}

/// Applies the versioned migrations in `migrations/`. SQLx records each
/// applied version in `_sqlx_migrations` and skips it on later runs.
pub async fn migrate(pool: &PgPool) -> Result<(), PostgresSetupError> {
    sqlx::migrate!("./migrations")
        .run(pool)
        .await
        .map_err(PostgresSetupError::Migrate)
}
