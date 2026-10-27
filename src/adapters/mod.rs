//! Adapters. Inbound adapters (`http`, `cli`) convert external input for
//! the use cases. Outbound adapters implement the application ports.
//! Serialization lives here, not in the domain (REAS-ARCH-002).

pub mod dto;
pub mod infohub;
pub mod json_files;
pub mod myschools;
pub mod myschools_api;
pub mod open_data;
pub mod postgres;
pub mod postgres_reference;
pub mod system_clock;
pub mod test_results;

pub use infohub::InfoHubClient;
pub use myschools_api::MySchoolsApiClient;
pub use open_data::NycOpenDataClient;
pub use postgres_reference::PostgresReferenceData;
pub use system_clock::SystemClock;

pub mod cli;
pub mod csv_export;
pub mod http;
