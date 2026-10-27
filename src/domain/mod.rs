//! Domain model: values, programs, the student, the simulation, and the
//! catalog scan. Nothing in this module performs I/O.

pub mod catalog;
pub mod demand;
pub mod lottery;
pub mod program;
pub mod simulation;
pub mod student;

pub use catalog::{
    classify_focus, classify_profile, combine_subjects, programs_from_catalog, scan, tags_of,
    with_boroughs,
};
pub use catalog::{
    Borough, CatalogGroup, CatalogProgram, DemandPair, DistrictCode, Evidence, FocusScore,
    GeoPoint, GroupDemandText, PriorityCriterion, ProfileText, ProgramFocus, ProgramProfile, Reach,
    ScanRequest, ScanRow, SchoolScores, SubjectResults, TestResults, TAG_THRESHOLD,
};
pub use demand::{DemandFigures, DemandHistory, LatestDemand, SchoolDemandYear};
pub use lottery::{InvalidLotteryPrefix, InvalidUnitInterval, LotteryPrefix, UnitInterval};
pub use program::{
    Attribute, GroupDemand, InvalidAttribute, InvalidGroupDemand, InvalidProgram,
    InvalidProgramCode, PriorityGroup, Program, ProgramCode, ProgramDefinitions, SelectionKind,
};
pub use simulation::simulate;
pub use simulation::{
    Attrition, InvalidAttrition, InvalidStandardDeviation, InvalidTrials, Parameters, ProgramRow,
    Report, Seed, SimulationError, StandardDeviation, Summary, Trials, WaitlistNumbers,
};
pub use student::{InvalidRanking, Ranking, Student};
