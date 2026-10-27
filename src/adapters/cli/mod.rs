//! Inbound adapter: the command line. It parses arguments, calls the use
//! cases, and renders text. It contains no domain logic.

pub mod render;

use std::path::PathBuf;

use clap::{Args, Parser, Subcommand, ValueEnum};

use crate::application::SimulationRequest;
use crate::domain::{Attribute, LotteryPrefix, ScanRequest, WaitlistNumbers};

use super::dto::{
    parameters_from_dto, parse_attributes, student_from_dto, InvalidInput, ParametersDto,
    StudentDto, WaitlistNumbersDto, DEFAULT_ATTRITION, DEFAULT_DEMAND_SD, DEFAULT_TRIALS,
};

#[derive(Debug, Parser)]
#[command(
    name = "nycmsa",
    version,
    about = "NYC middle school admissions Monte Carlo and MySchools catalog scan"
)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Command,
}

#[derive(Debug, Subcommand)]
pub enum Command {
    /// Simulate one student's chances across their ranked list.
    Sim(SimArgs),
    /// List every catalog program the student is eligible for, most
    /// reachable first.
    Scan(ScanArgs),
    /// Download the MySchools middle school catalog into the database.
    Fetch(FetchArgs),
    /// Download state test results per school from NYC Open Data into the
    /// database.
    FetchScores(FetchScoresArgs),
    /// Look up every school's detail page on MySchools, classify each
    /// program, and store the profiles in the database.
    FetchProfiles(FetchProfilesArgs),
    /// Download the DOE's yearly reports on school applications, offers
    /// and seats (Local Law 72) into the database.
    FetchDemand(FetchDemandArgs),
    /// Re-run the kind-of-program rules on the stored profiles, without a
    /// download.
    ReclassifyProfiles(FetchProfilesArgs),
    /// Store the hand-entered programs and form defaults from the seed
    /// files in the database.
    Import(ImportArgs),
    /// Import the seeds and download whatever the database is missing.
    Bootstrap(BootstrapArgs),
    /// Serve the web interface from the database.
    Serve(ServeArgs),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum, Default)]
pub enum WaitlistNumbersArg {
    /// Each waitlist draws a new random number (DOE published policy).
    #[default]
    Redrawn,
    /// Waitlists reuse the application lottery number.
    Reused,
}

impl From<WaitlistNumbersArg> for WaitlistNumbersDto {
    fn from(arg: WaitlistNumbersArg) -> Self {
        match arg {
            WaitlistNumbersArg::Redrawn => WaitlistNumbersDto::Redrawn,
            WaitlistNumbersArg::Reused => WaitlistNumbersDto::Reused,
        }
    }
}

impl From<WaitlistNumbersArg> for WaitlistNumbers {
    fn from(arg: WaitlistNumbersArg) -> Self {
        WaitlistNumbersDto::from(arg).into()
    }
}

#[derive(Debug, Args)]
pub struct SimArgs {
    /// Program definitions file.
    #[arg(long, default_value = "data/programs.json")]
    pub programs: PathBuf,
    /// Student file.
    #[arg(long, default_value = "data/student.json")]
    pub student: PathBuf,
    #[arg(long, default_value_t = DEFAULT_TRIALS)]
    pub trials: u32,
    #[arg(long, default_value_t = 0)]
    pub seed: u64,
    /// Log-normal sd on next year's offer rate versus last year.
    #[arg(long, default_value_t = DEFAULT_DEMAND_SD)]
    pub demand_sd: f64,
    /// Multiply published offer rates to allow for applicants matched
    /// elsewhere.
    #[arg(long, default_value_t = DEFAULT_ATTRITION)]
    pub attrition: f64,
    /// How lottery programs order their waitlists.
    #[arg(long, value_enum, default_value_t)]
    pub waitlist_numbers: WaitlistNumbersArg,
    /// Override the ranking, e.g. --rank K187A K442S
    #[arg(long, num_args = 1..)]
    pub rank: Option<Vec<String>>,
    /// The student did not take the talent test (removes `mark_twain_test`).
    #[arg(long)]
    pub no_test: bool,
    /// Assumed talent-test percentile as a fraction (0 = best, 1 = worst).
    #[arg(long)]
    pub twain_pct: Option<f64>,
    /// Known lottery number hex prefix, e.g. B0 or b03f. Omit to draw
    /// randomly.
    #[arg(long)]
    pub lottery: Option<String>,
    /// Print the report as JSON instead of text.
    #[arg(long)]
    pub json: bool,
}

#[derive(Debug, Args)]
pub struct ScanArgs {
    /// MySchools catalog JSON file, as written by the Python fetcher.
    pub data: PathBuf,
    /// The student is FRL, ELL, or STH eligible.
    #[arg(long)]
    pub frl: bool,
    /// Comma-separated eligibility attributes.
    #[arg(long, default_value = "d15,d20")]
    pub attrs: String,
    /// Lottery number hex prefix.
    #[arg(long, default_value = "BD")]
    pub lottery: String,
    /// Also write the rows to this CSV file.
    #[arg(long)]
    pub csv: Option<PathBuf>,
}

/// PostgreSQL connection, shared by every command that touches the store.
#[derive(Debug, Args)]
pub struct DatabaseArgs {
    /// PostgreSQL connection URL. Migrations run on connect.
    #[arg(long, env = "DATABASE_URL")]
    pub database_url: String,
}

#[derive(Debug, Args)]
pub struct FetchArgs {
    /// Comma-separated district codes, or `all`.
    #[arg(long, default_value = "all")]
    pub districts: String,
    #[command(flatten)]
    pub database: DatabaseArgs,
}

/// Where to download state test results from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum, Default)]
pub enum ScoreSource {
    /// DOE InfoHub workbooks, the latest published year.
    #[default]
    Infohub,
    /// NYC Open Data datasets, which end at 2023.
    OpenData,
}

#[derive(Debug, Args)]
pub struct FetchScoresArgs {
    #[arg(long, value_enum, default_value_t)]
    pub source: ScoreSource,
    #[command(flatten)]
    pub database: DatabaseArgs,
}

#[derive(Debug, Args)]
pub struct FetchProfilesArgs {
    #[command(flatten)]
    pub database: DatabaseArgs,
}

#[derive(Debug, Args)]
pub struct FetchDemandArgs {
    #[command(flatten)]
    pub database: DatabaseArgs,
}

#[derive(Debug, Args)]
pub struct ImportArgs {
    /// Hand-entered program definitions.
    #[arg(long, default_value = "data/programs.json")]
    pub programs: PathBuf,
    /// Defaults that prefill the web form.
    #[arg(long, default_value = "data/student.json")]
    pub student: PathBuf,
    #[command(flatten)]
    pub database: DatabaseArgs,
}

#[derive(Debug, Args)]
pub struct BootstrapArgs {
    #[arg(long, default_value = "data/programs.json")]
    pub programs: PathBuf,
    #[arg(long, default_value = "data/student.json")]
    pub student: PathBuf,
    /// Districts to download when the catalog is missing.
    #[arg(long, default_value = "all")]
    pub districts: String,
    #[arg(long, value_enum, default_value_t)]
    pub source: ScoreSource,
    #[command(flatten)]
    pub database: DatabaseArgs,
}

#[derive(Debug, Args)]
pub struct ServeArgs {
    /// Address to listen on. Keep it on loopback unless you add
    /// authentication in front; every endpoint is anonymous.
    #[arg(long, default_value = "127.0.0.1:8080")]
    pub bind: String,
    #[command(flatten)]
    pub database: DatabaseArgs,
}

/// Applies the `sim` overrides to the student file and validates the result.
pub fn simulation_request(
    args: &SimArgs,
    mut student: StudentDto,
) -> Result<SimulationRequest, InvalidInput> {
    if let Some(rank) = &args.rank {
        student.ranking = rank.clone();
    }
    if args.no_test {
        student.attributes.retain(|a| a != "mark_twain_test");
    }
    if let Some(pct) = args.twain_pct {
        student.talent_test_percentile = Some(pct);
    }
    if let Some(lottery) = &args.lottery {
        student.lottery_number = Some(lottery.clone());
    }
    let parameters = parameters_from_dto(ParametersDto {
        trials: args.trials,
        seed: args.seed,
        demand_sd: args.demand_sd,
        attrition: args.attrition,
        waitlist_numbers: args.waitlist_numbers.into(),
    })?;
    Ok(SimulationRequest {
        student: student_from_dto(student)?,
        parameters,
        programs: None,
    })
}

/// Builds the scan request from the `scan` arguments.
pub fn scan_request(args: &ScanArgs) -> Result<ScanRequest, InvalidInput> {
    Ok(ScanRequest {
        attributes: parse_attributes(args.attrs.split(','))?,
        frl: args.frl,
        lottery: LotteryPrefix::parse(&args.lottery)?,
    })
}

/// Attributes as text, in the stored (sorted) order.
pub fn join_attributes<'a>(attributes: impl IntoIterator<Item = &'a Attribute>) -> String {
    attributes
        .into_iter()
        .map(Attribute::as_str)
        .collect::<Vec<_>>()
        .join(", ")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn student() -> StudentDto {
        StudentDto {
            notes: vec![],
            attributes: vec!["d15".to_owned(), "mark_twain_test".to_owned()],
            ranking: vec!["K187A".to_owned(), "K239SC".to_owned()],
            lottery_number: Some("BD".to_owned()),
            talent_test_percentile: Some(0.1),
            talent_test_sd: 0.1,
        }
    }

    fn parse(args: &[&str]) -> Cli {
        Cli::try_parse_from(std::iter::once("nycmsa").chain(args.iter().copied())).expect("parses")
    }

    #[test]
    fn sim_defaults_match_the_python_cli() {
        let cli = parse(&["sim"]);
        let Command::Sim(args) = cli.command else {
            panic!("expected sim");
        };
        assert_eq!(args.trials, 200_000);
        assert_eq!(args.seed, 0);
        assert_eq!(args.demand_sd, 0.15);
        assert_eq!(args.attrition, 1.0);
        assert_eq!(args.waitlist_numbers, WaitlistNumbersArg::Redrawn);
        let request = simulation_request(&args, student()).expect("valid");
        assert_eq!(request.student.ranking.len(), 2);
        assert_eq!(request.parameters.trials.get(), 200_000);
    }

    #[test]
    fn sim_overrides_apply_before_validation() {
        let cli = parse(&[
            "sim",
            "--rank",
            "K442S",
            "K443M",
            "--no-test",
            "--twain-pct",
            "0.25",
            "--lottery",
            "b0",
            "--waitlist-numbers",
            "reused",
            "--trials",
            "10",
        ]);
        let Command::Sim(args) = cli.command else {
            panic!("expected sim");
        };
        let request = simulation_request(&args, student()).expect("valid");
        let codes: Vec<&str> = request
            .student
            .ranking
            .codes()
            .iter()
            .map(|c| c.as_str())
            .collect();
        assert_eq!(codes, vec!["K442S", "K443M"]);
        assert!(!request
            .student
            .attributes
            .iter()
            .any(|a| a.as_str() == "mark_twain_test"));
        assert_eq!(
            request.student.talent_test_percentile.map(|p| p.get()),
            Some(0.25)
        );
        assert_eq!(
            request.student.lottery_number.as_ref().map(|l| l.as_str()),
            Some("b0")
        );
        assert_eq!(request.parameters.waitlist_numbers, WaitlistNumbers::Reused);
    }

    #[test]
    fn sim_rejects_an_invalid_lottery_override() {
        let cli = parse(&["sim", "--lottery", "zz"]);
        let Command::Sim(args) = cli.command else {
            panic!("expected sim");
        };
        let err = simulation_request(&args, student()).expect_err("invalid");
        assert!(matches!(err, InvalidInput::LotteryPrefix(_)), "{err}");
    }

    #[test]
    fn scan_request_parses_attributes_and_lottery() {
        let cli = parse(&[
            "scan",
            "catalog.json",
            "--attrs",
            "d15, d20",
            "--lottery",
            "b0",
            "--frl",
        ]);
        let Command::Scan(args) = cli.command else {
            panic!("expected scan");
        };
        let request = scan_request(&args).expect("valid");
        assert_eq!(request.attributes.len(), 2);
        assert!(request.frl);
        assert_eq!(request.lottery.as_str(), "b0");
    }
}
