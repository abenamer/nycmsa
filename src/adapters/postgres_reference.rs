//! Outbound adapter: the reference data in PostgreSQL. It implements the
//! four store ports on one pool. Each save is one transaction
//! (REAS-DATA-002); each read is two explicit queries.

use std::time::SystemTime;

use sqlx::postgres::PgPool;
use sqlx::types::Json;
use sqlx::Row;
use time::OffsetDateTime;

use crate::application::{
    CatalogSnapshot, CatalogStore, ProgramStore, RepositoryError, Stored, StudentDefaultsStore,
    TestResultsStore,
};
use crate::domain::{ProgramDefinitions, SchoolScores, Student, TestResults};

use super::dto::{
    program_definitions_from_dto, program_definitions_to_dto, student_from_dto, student_to_dto,
    ProgramDto, StudentDto,
};

#[derive(Clone)]
pub struct PostgresReferenceData {
    pool: PgPool,
}

impl PostgresReferenceData {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }
}

fn unavailable(error: sqlx::Error) -> RepositoryError {
    RepositoryError::Unavailable(error.to_string())
}

fn corrupt(id: i64, reason: impl ToString) -> RepositoryError {
    RepositoryError::Corrupt {
        id: u64::try_from(id).unwrap_or(0),
        reason: reason.to_string(),
    }
}

/// `id, recorded_at, source` of the newest row of a snapshot table.
async fn latest_header(
    pool: &PgPool,
    table: &str,
) -> Result<Option<(i64, SystemTime, String)>, RepositoryError> {
    // The table name comes from this module, never from input.
    let query = format!("SELECT id, recorded_at, source FROM {table} ORDER BY id DESC LIMIT 1");
    let row = sqlx::query(&query)
        .fetch_optional(pool)
        .await
        .map_err(unavailable)?;
    row.map(|r| {
        let id: i64 = r.try_get("id").map_err(unavailable)?;
        let at: OffsetDateTime = r.try_get("recorded_at").map_err(unavailable)?;
        let source: String = r.try_get("source").map_err(unavailable)?;
        Ok((id, SystemTime::from(at), source))
    })
    .transpose()
}

#[async_trait::async_trait]
impl CatalogStore for PostgresReferenceData {
    async fn save_catalog(
        &self,
        snapshot: &CatalogSnapshot,
        source: &str,
        recorded_at: SystemTime,
    ) -> Result<(), RepositoryError> {
        let mut tx = self.pool.begin().await.map_err(unavailable)?;
        let id: i64 = sqlx::query(
            "INSERT INTO catalog_snapshots (recorded_at, source, school_count) VALUES ($1, $2, $3) RETURNING id",
        )
        .bind(OffsetDateTime::from(recorded_at))
        .bind(source)
        .bind(i32::try_from(snapshot.len()).unwrap_or(i32::MAX))
        .fetch_one(&mut *tx)
        .await
        .map_err(unavailable)?
        .try_get("id")
        .map_err(unavailable)?;
        for (position, school) in snapshot.schools.iter().enumerate() {
            sqlx::query(
                "INSERT INTO catalog_schools (snapshot_id, position, document) VALUES ($1, $2, $3)",
            )
            .bind(id)
            .bind(i32::try_from(position).unwrap_or(i32::MAX))
            .bind(Json(school))
            .execute(&mut *tx)
            .await
            .map_err(unavailable)?;
        }
        tx.commit().await.map_err(unavailable)
    }

    async fn latest_catalog(&self) -> Result<Option<Stored<CatalogSnapshot>>, RepositoryError> {
        let Some((id, recorded_at, source)) =
            latest_header(&self.pool, "catalog_snapshots").await?
        else {
            return Ok(None);
        };
        let rows = sqlx::query(
            "SELECT document FROM catalog_schools WHERE snapshot_id = $1 ORDER BY position",
        )
        .bind(id)
        .fetch_all(&self.pool)
        .await
        .map_err(unavailable)?;
        let schools = rows
            .into_iter()
            .map(|r| {
                let Json(document): Json<serde_json::Value> =
                    r.try_get("document").map_err(unavailable)?;
                Ok(document)
            })
            .collect::<Result<Vec<_>, RepositoryError>>()?;
        Ok(Some(Stored {
            recorded_at,
            source,
            value: CatalogSnapshot { schools },
        }))
    }
}

#[async_trait::async_trait]
impl TestResultsStore for PostgresReferenceData {
    async fn save_test_results(
        &self,
        results: &TestResults,
        source: &str,
        recorded_at: SystemTime,
    ) -> Result<(), RepositoryError> {
        let mut tx = self.pool.begin().await.map_err(unavailable)?;
        let id: i64 = sqlx::query(
            "INSERT INTO score_snapshots (recorded_at, source) VALUES ($1, $2) RETURNING id",
        )
        .bind(OffsetDateTime::from(recorded_at))
        .bind(source)
        .fetch_one(&mut *tx)
        .await
        .map_err(unavailable)?
        .try_get("id")
        .map_err(unavailable)?;
        for (dbn, scores) in results {
            sqlx::query(
                "INSERT INTO school_scores (snapshot_id, dbn, year, ela_proficient_pct, math_proficient_pct) VALUES ($1, $2, $3, $4, $5)",
            )
            .bind(id)
            .bind(dbn)
            .bind(i32::try_from(scores.year).unwrap_or(i32::MAX))
            .bind(scores.ela_proficient_pct)
            .bind(scores.math_proficient_pct)
            .execute(&mut *tx)
            .await
            .map_err(unavailable)?;
        }
        tx.commit().await.map_err(unavailable)
    }

    async fn latest_test_results(&self) -> Result<Option<Stored<TestResults>>, RepositoryError> {
        let Some((id, recorded_at, source)) = latest_header(&self.pool, "score_snapshots").await?
        else {
            return Ok(None);
        };
        let rows = sqlx::query(
            "SELECT dbn, year, ela_proficient_pct, math_proficient_pct FROM school_scores WHERE snapshot_id = $1",
        )
        .bind(id)
        .fetch_all(&self.pool)
        .await
        .map_err(unavailable)?;
        let mut results = TestResults::new();
        for r in rows {
            let dbn: String = r.try_get("dbn").map_err(unavailable)?;
            let year: i32 = r.try_get("year").map_err(unavailable)?;
            results.insert(
                dbn,
                SchoolScores {
                    year: u32::try_from(year).map_err(|e| corrupt(id, e))?,
                    ela_proficient_pct: r.try_get("ela_proficient_pct").map_err(unavailable)?,
                    math_proficient_pct: r.try_get("math_proficient_pct").map_err(unavailable)?,
                },
            );
        }
        Ok(Some(Stored {
            recorded_at,
            source,
            value: results,
        }))
    }
}

#[async_trait::async_trait]
impl ProgramStore for PostgresReferenceData {
    async fn save_programs(
        &self,
        programs: &ProgramDefinitions,
        source: &str,
        recorded_at: SystemTime,
    ) -> Result<(), RepositoryError> {
        let document = program_definitions_to_dto(programs);
        let mut tx = self.pool.begin().await.map_err(unavailable)?;
        let id: i64 = sqlx::query(
            "INSERT INTO program_sets (recorded_at, source) VALUES ($1, $2) RETURNING id",
        )
        .bind(OffsetDateTime::from(recorded_at))
        .bind(source)
        .fetch_one(&mut *tx)
        .await
        .map_err(unavailable)?
        .try_get("id")
        .map_err(unavailable)?;
        for (code, program) in &document.programs {
            sqlx::query("INSERT INTO programs (set_id, code, document) VALUES ($1, $2, $3)")
                .bind(id)
                .bind(code)
                .bind(Json(program))
                .execute(&mut *tx)
                .await
                .map_err(unavailable)?;
        }
        tx.commit().await.map_err(unavailable)
    }

    async fn latest_programs(&self) -> Result<Option<Stored<ProgramDefinitions>>, RepositoryError> {
        let Some((id, recorded_at, source)) = latest_header(&self.pool, "program_sets").await?
        else {
            return Ok(None);
        };
        let rows = sqlx::query("SELECT code, document FROM programs WHERE set_id = $1")
            .bind(id)
            .fetch_all(&self.pool)
            .await
            .map_err(unavailable)?;
        let mut programs = std::collections::BTreeMap::new();
        for r in rows {
            let code: String = r.try_get("code").map_err(unavailable)?;
            let Json(dto): Json<ProgramDto> = r.try_get("document").map_err(unavailable)?;
            programs.insert(code, dto);
        }
        let definitions = program_definitions_from_dto(programs).map_err(|e| corrupt(id, e))?;
        Ok(Some(Stored {
            recorded_at,
            source,
            value: definitions,
        }))
    }
}

#[async_trait::async_trait]
impl StudentDefaultsStore for PostgresReferenceData {
    async fn save_student_defaults(
        &self,
        student: &Student,
        source: &str,
        recorded_at: SystemTime,
    ) -> Result<(), RepositoryError> {
        sqlx::query(
            "INSERT INTO student_defaults (recorded_at, source, document) VALUES ($1, $2, $3)",
        )
        .bind(OffsetDateTime::from(recorded_at))
        .bind(source)
        .bind(Json(student_to_dto(student)))
        .execute(&self.pool)
        .await
        .map_err(unavailable)?;
        Ok(())
    }

    async fn latest_student_defaults(&self) -> Result<Option<Stored<Student>>, RepositoryError> {
        let row = sqlx::query(
            "SELECT id, recorded_at, source, document FROM student_defaults ORDER BY id DESC LIMIT 1",
        )
        .fetch_optional(&self.pool)
        .await
        .map_err(unavailable)?;
        let Some(r) = row else {
            return Ok(None);
        };
        let id: i64 = r.try_get("id").map_err(unavailable)?;
        let at: OffsetDateTime = r.try_get("recorded_at").map_err(unavailable)?;
        let source: String = r.try_get("source").map_err(unavailable)?;
        let Json(dto): Json<StudentDto> = r.try_get("document").map_err(unavailable)?;
        let student = student_from_dto(dto).map_err(|e| corrupt(id, e))?;
        Ok(Some(Stored {
            recorded_at: SystemTime::from(at),
            source,
            value: student,
        }))
    }
}
