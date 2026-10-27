//! Outbound adapter: the reference data in PostgreSQL. It implements the
//! six store ports on one pool. Each save is one transaction
//! (REAS-DATA-002); each read is two explicit queries.

use std::time::SystemTime;

use sqlx::postgres::PgPool;
use sqlx::types::Json;
use sqlx::Row;
use time::OffsetDateTime;

use serde::{Deserialize, Serialize};

use crate::application::{
    CatalogSnapshot, CatalogStore, DemandStore, ProfileStore, ProgramStore, RepositoryError,
    Stored, StudentDefaultsStore, TestResultsStore,
};
use crate::domain::{
    DemandFigures, DemandHistory, Evidence, FocusScore, ProgramCode, ProgramDefinitions,
    ProgramFocus, ProgramProfile, SchoolDemandYear, SchoolScores, Student, TestResults,
};

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

fn to_i32(value: Option<u32>) -> Option<i32> {
    value.map(|v| i32::try_from(v).unwrap_or(i32::MAX))
}

fn to_u32(id: i64, value: Option<i32>) -> Result<Option<u32>, RepositoryError> {
    value
        .map(|v| u32::try_from(v).map_err(|e| corrupt(id, e)))
        .transpose()
}

/// Rows per `INSERT ... SELECT FROM UNNEST` statement. A full history is
/// about 50,000 rows; one statement per 5,000 keeps the arrays modest.
const DEMAND_CHUNK: usize = 5_000;

#[async_trait::async_trait]
impl DemandStore for PostgresReferenceData {
    async fn save_demand(
        &self,
        history: &DemandHistory,
        source: &str,
        recorded_at: SystemTime,
    ) -> Result<(), RepositoryError> {
        let mut tx = self.pool.begin().await.map_err(unavailable)?;
        let id: i64 = sqlx::query(
            "INSERT INTO demand_snapshots (recorded_at, source) VALUES ($1, $2) RETURNING id",
        )
        .bind(OffsetDateTime::from(recorded_at))
        .bind(source)
        .fetch_one(&mut *tx)
        .await
        .map_err(unavailable)?
        .try_get("id")
        .map_err(unavailable)?;
        // (dbn, year, district, figures); '' is the all-applicants row.
        let mut rows: Vec<(&str, u32, &str, DemandFigures)> = Vec::new();
        for (dbn, year) in history.iter() {
            rows.push((dbn, year.year, "", year.all));
            for (district, figures) in &year.by_residential_district {
                rows.push((dbn, year.year, district.as_str(), *figures));
            }
        }
        for chunk in rows.chunks(DEMAND_CHUNK) {
            let dbns: Vec<String> = chunk.iter().map(|r| r.0.to_owned()).collect();
            let years: Vec<i32> = chunk
                .iter()
                .map(|r| i32::try_from(r.1).unwrap_or(i32::MAX))
                .collect();
            let districts: Vec<String> = chunk.iter().map(|r| r.2.to_owned()).collect();
            let seats: Vec<Option<i32>> = chunk.iter().map(|r| to_i32(r.3.seats)).collect();
            let listed: Vec<Option<i32>> = chunk.iter().map(|r| to_i32(r.3.listed)).collect();
            let considered: Vec<Option<i32>> =
                chunk.iter().map(|r| to_i32(r.3.considered)).collect();
            let offers: Vec<Option<i32>> = chunk.iter().map(|r| to_i32(r.3.offers)).collect();
            sqlx::query(
                "INSERT INTO school_demand (snapshot_id, dbn, year, district, seats, listed, considered, offers) \
                 SELECT $1, * FROM UNNEST($2::text[], $3::int4[], $4::text[], $5::int4[], $6::int4[], $7::int4[], $8::int4[])",
            )
            .bind(id)
            .bind(&dbns)
            .bind(&years)
            .bind(&districts)
            .bind(&seats)
            .bind(&listed)
            .bind(&considered)
            .bind(&offers)
            .execute(&mut *tx)
            .await
            .map_err(unavailable)?;
        }
        tx.commit().await.map_err(unavailable)
    }

    async fn latest_demand(&self) -> Result<Option<Stored<DemandHistory>>, RepositoryError> {
        let Some((id, recorded_at, source)) = latest_header(&self.pool, "demand_snapshots").await?
        else {
            return Ok(None);
        };
        let rows = sqlx::query(
            "SELECT dbn, year, district, seats, listed, considered, offers FROM school_demand WHERE snapshot_id = $1 ORDER BY dbn, year, district",
        )
        .bind(id)
        .fetch_all(&self.pool)
        .await
        .map_err(unavailable)?;
        let mut history = DemandHistory::new();
        let mut current: Option<(String, SchoolDemandYear)> = None;
        for r in rows {
            let dbn: String = r.try_get("dbn").map_err(unavailable)?;
            let year: i32 = r.try_get("year").map_err(unavailable)?;
            let year = u32::try_from(year).map_err(|e| corrupt(id, e))?;
            let district: String = r.try_get("district").map_err(unavailable)?;
            let figures = DemandFigures {
                seats: to_u32(id, r.try_get("seats").map_err(unavailable)?)?,
                listed: to_u32(id, r.try_get("listed").map_err(unavailable)?)?,
                considered: to_u32(id, r.try_get("considered").map_err(unavailable)?)?,
                offers: to_u32(id, r.try_get("offers").map_err(unavailable)?)?,
            };
            let same = current
                .as_ref()
                .is_some_and(|(d, y)| *d == dbn && y.year == year);
            if !same {
                if let Some((d, y)) = current.take() {
                    history.insert(&d, y);
                }
                current = Some((
                    dbn.clone(),
                    SchoolDemandYear {
                        year,
                        ..SchoolDemandYear::default()
                    },
                ));
            }
            let (_, entry) = current.as_mut().expect("set above");
            if district.is_empty() {
                entry.all = figures;
            } else {
                entry.by_residential_district.insert(district, figures);
            }
        }
        if let Some((d, y)) = current.take() {
            history.insert(&d, y);
        }
        Ok(Some(Stored {
            recorded_at,
            source,
            value: history,
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

/// Stored form of the evidence list.
#[derive(Debug, Serialize, Deserialize)]
struct EvidenceDto {
    focus: String,
    keyword: String,
    label: String,
    weight: f64,
}

/// Stored form of the kind scores.
#[derive(Debug, Serialize, Deserialize)]
struct ScoreDto {
    focus: String,
    confidence: f64,
}

fn focus_from_slug(slug: &str) -> Option<ProgramFocus> {
    ProgramFocus::ALL.into_iter().find(|f| f.slug() == slug)
}

#[async_trait::async_trait]
impl ProfileStore for PostgresReferenceData {
    async fn save_profiles(
        &self,
        profiles: &[ProgramProfile],
        source: &str,
        recorded_at: SystemTime,
    ) -> Result<(), RepositoryError> {
        let mut tx = self.pool.begin().await.map_err(unavailable)?;
        let id: i64 = sqlx::query(
            "INSERT INTO profile_snapshots (recorded_at, source) VALUES ($1, $2) RETURNING id",
        )
        .bind(OffsetDateTime::from(recorded_at))
        .bind(source)
        .fetch_one(&mut *tx)
        .await
        .map_err(unavailable)?
        .try_get("id")
        .map_err(unavailable)?;
        for p in profiles {
            let scores: Vec<ScoreDto> = p
                .scores
                .iter()
                .map(|s| ScoreDto {
                    focus: s.focus.slug().to_owned(),
                    confidence: s.confidence,
                })
                .collect();
            let evidence: Vec<EvidenceDto> = p
                .evidence
                .iter()
                .map(|e| EvidenceDto {
                    focus: e.focus.slug().to_owned(),
                    keyword: e.keyword.clone(),
                    label: e.label.clone(),
                    weight: e.weight,
                })
                .collect();
            sqlx::query(
                "INSERT INTO program_profiles (snapshot_id, code, dbn, program_name, school_name, method, description, overview, academic_opportunities, art_courses, language_classes, scores, evidence) VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13)",
            )
            .bind(id)
            .bind(p.code.as_str())
            .bind(&p.dbn)
            .bind(&p.program_name)
            .bind(&p.school_name)
            .bind(&p.method)
            .bind(&p.description)
            .bind(&p.overview)
            .bind(&p.academic_opportunities)
            .bind(&p.art_courses)
            .bind(&p.language_classes)
            .bind(Json(scores))
            .bind(Json(evidence))
            .execute(&mut *tx)
            .await
            .map_err(unavailable)?;
        }
        tx.commit().await.map_err(unavailable)
    }

    async fn latest_profiles(
        &self,
    ) -> Result<Option<Stored<Vec<ProgramProfile>>>, RepositoryError> {
        let Some((id, recorded_at, source)) =
            latest_header(&self.pool, "profile_snapshots").await?
        else {
            return Ok(None);
        };
        let rows = sqlx::query(
            "SELECT code, dbn, program_name, school_name, method, description, overview, academic_opportunities, art_courses, language_classes, scores, evidence FROM program_profiles WHERE snapshot_id = $1 ORDER BY code",
        )
        .bind(id)
        .fetch_all(&self.pool)
        .await
        .map_err(unavailable)?;
        let mut profiles = Vec::with_capacity(rows.len());
        for r in rows {
            let code: String = r.try_get("code").map_err(unavailable)?;
            let Json(scores): Json<Vec<ScoreDto>> = r.try_get("scores").map_err(unavailable)?;
            let Json(evidence): Json<Vec<EvidenceDto>> =
                r.try_get("evidence").map_err(unavailable)?;
            let decode = |slug: &str| {
                focus_from_slug(slug).ok_or_else(|| corrupt(id, format!("unknown focus {slug:?}")))
            };
            profiles.push(ProgramProfile {
                code: ProgramCode::parse(&code).map_err(|e| corrupt(id, e))?,
                dbn: r.try_get("dbn").map_err(unavailable)?,
                program_name: r.try_get("program_name").map_err(unavailable)?,
                school_name: r.try_get("school_name").map_err(unavailable)?,
                method: r.try_get("method").map_err(unavailable)?,
                description: r.try_get("description").map_err(unavailable)?,
                overview: r.try_get("overview").map_err(unavailable)?,
                academic_opportunities: r.try_get("academic_opportunities").map_err(unavailable)?,
                art_courses: r.try_get("art_courses").map_err(unavailable)?,
                language_classes: r.try_get("language_classes").map_err(unavailable)?,
                scores: scores
                    .into_iter()
                    .map(|s| {
                        Ok(FocusScore {
                            focus: decode(&s.focus)?,
                            confidence: s.confidence,
                        })
                    })
                    .collect::<Result<Vec<_>, RepositoryError>>()?,
                evidence: evidence
                    .into_iter()
                    .map(|e| {
                        Ok(Evidence {
                            focus: decode(&e.focus)?,
                            keyword: e.keyword,
                            label: e.label,
                            weight: e.weight,
                        })
                    })
                    .collect::<Result<Vec<_>, RepositoryError>>()?,
            });
        }
        Ok(Some(Stored {
            recorded_at,
            source,
            value: profiles,
        }))
    }
}
