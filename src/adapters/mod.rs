//! Adapters. Inbound adapters (`http`, `cli`) convert external input for
//! the use cases. Outbound adapters implement the application ports.
//! Serialization lives here, not in the domain (REAS-ARCH-002).

pub mod dto;
pub mod in_memory_runs;
pub mod json_files;
pub mod myschools;
pub mod myschools_api;
pub mod postgres_runs;
pub mod system_clock;

pub use in_memory_runs::InMemoryRunRepository;
pub use myschools_api::MySchoolsApiClient;
pub use postgres_runs::PostgresRunRepository;
pub use system_clock::SystemClock;

pub mod cli;
pub mod csv_export;
pub mod http;
