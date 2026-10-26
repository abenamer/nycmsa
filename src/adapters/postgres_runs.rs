//! Outbound adapter: runs stored in PostgreSQL through SQLx.
//!
//! Each operation is one explicit query (REAS-DATA-001). Queries use the
//! runtime-checked `sqlx::query` so the build needs no database and no
//! offline metadata; the integration test in `tests/postgres_runs.rs` runs
//! them against a real server. The student and report are stored as JSONB
//! documents in the same shape the HTTP API serves, and validated again when
//! read back.

use std::num::NonZeroU64;
use std::time::{Duration, SystemTime};

use sqlx::postgres::{PgPool, PgPoolOptions, PgRow};
use sqlx::types::Json;
use sqlx::Row;
use time::OffsetDateTime;

use crate::application::{NewRun, RepositoryError, Run, RunId, RunRepository};

use super::dto::{
    report_from_dto, report_to_dto, student_from_dto, student_to_dto, ReportDto, StudentDto,
};

#[derive(Debug, thiserror::Error)]
pub enum PostgresSetupError {
    #[error("cannot connect to PostgreSQL: {0}")]
    Connect(#[source] sqlx::Error),
    #[error("cannot apply migrations: {0}")]
    Migrate(#[source] sqlx::migrate::MigrateError),
}

#[derive(Clone)]
pub struct PostgresRunRepository {
    pool: PgPool,
}

impl PostgresRunRepository {
    /// Opens a pool. Credentials come from the URL, never from source.
    pub async fn connect(url: &str, max_connections: u32) -> Result<Self, PostgresSetupError> {
        let pool = PgPoolOptions::new()
            .max_connections(max_connections)
            .acquire_timeout(Duration::from_secs(10))
            .connect(url)
            .await
            .map_err(PostgresSetupError::Connect)?;
        Ok(Self { pool })
    }

    /// Applies the versioned migrations in `migrations/`. SQLx records each
    /// applied version in `_sqlx_migrations` and skips it on later runs.
    pub async fn migrate(&self) -> Result<(), PostgresSetupError> {
        sqlx::migrate!("./migrations")
            .run(&self.pool)
            .await
            .map_err(PostgresSetupError::Migrate)
    }

    pub fn pool(&self) -> &PgPool {
        &self.pool
    }
}

fn unavailable(error: sqlx::Error) -> RepositoryError {
    RepositoryError::Unavailable(error.to_string())
}

fn run_id(raw: i64) -> Result<RunId, RepositoryError> {
    u64::try_from(raw)
        .ok()
        .and_then(NonZeroU64::new)
        .map(RunId::new)
        .ok_or_else(|| RepositoryError::Corrupt {
            id: 0,
            reason: format!("row id {raw} is not a positive integer"),
        })
}

fn run_from_row(row: PgRow) -> Result<Run, RepositoryError> {
    let raw_id: i64 = row.try_get("id").map_err(unavailable)?;
    let id = run_id(raw_id)?;
    let corrupt = |reason: String| RepositoryError::Corrupt {
        id: id.get(),
        reason,
    };
    let created_at: OffsetDateTime = row.try_get("created_at").map_err(unavailable)?;
    let Json(student_dto): Json<StudentDto> = row.try_get("student").map_err(unavailable)?;
    let Json(report_dto): Json<ReportDto> = row.try_get("report").map_err(unavailable)?;
    let student = student_from_dto(student_dto).map_err(|e| corrupt(e.to_string()))?;
    let report = report_from_dto(report_dto).map_err(|e| corrupt(e.to_string()))?;
    Ok(Run {
        id,
        created_at: SystemTime::from(created_at),
        student,
        report,
    })
}

#[async_trait::async_trait]
impl RunRepository for PostgresRunRepository {
    async fn insert(&self, run: NewRun) -> Result<RunId, RepositoryError> {
        let created_at = OffsetDateTime::from(run.created_at);
        let row = sqlx::query(
            "INSERT INTO runs (created_at, student, report) VALUES ($1, $2, $3) RETURNING id",
        )
        .bind(created_at)
        .bind(Json(student_to_dto(&run.student)))
        .bind(Json(report_to_dto(&run.report)))
        .fetch_one(&self.pool)
        .await
        .map_err(unavailable)?;
        let raw: i64 = row.try_get("id").map_err(unavailable)?;
        run_id(raw)
    }

    async fn find(&self, id: RunId) -> Result<Option<Run>, RepositoryError> {
        let Ok(raw) = i64::try_from(id.get()) else {
            return Ok(None);
        };
        let row = sqlx::query("SELECT id, created_at, student, report FROM runs WHERE id = $1")
            .bind(raw)
            .fetch_optional(&self.pool)
            .await
            .map_err(unavailable)?;
        row.map(run_from_row).transpose()
    }

    async fn recent(&self, limit: usize) -> Result<Vec<Run>, RepositoryError> {
        let limit = i64::try_from(limit).unwrap_or(i64::MAX);
        let rows = sqlx::query(
            "SELECT id, created_at, student, report FROM runs ORDER BY id DESC LIMIT $1",
        )
        .bind(limit)
        .fetch_all(&self.pool)
        .await
        .map_err(unavailable)?;
        rows.into_iter().map(run_from_row).collect()
    }
}
