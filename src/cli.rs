use clap::{Args, Parser, Subcommand};

#[derive(Parser)]
#[command(name = "mepris")]
#[command(version = env!("CARGO_PKG_VERSION"))]
#[command(about = "Cross-platform declarative system setup tool", long_about = None)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Commands,
}

#[derive(Subcommand)]
pub enum Commands {
    #[command(about = "Execute steps from configuration file", long_about = None)]
    Run(RunArgs),
    #[command(about = "Resume failed run", long_about = None)]
    Resume(ResumeArgs),
    #[command(about = "List steps", long_about = None)]
    ListSteps(ListStepsArgs),
    #[command(about = "List tags", long_about = None)]
    ListTags(ListTagsArgs),
    #[command(about = "Generate shell completion scripts")]
    Completion(CompletionArgs),
}

#[derive(Args)]
pub struct RunArgs {
    #[arg(short, long, required = true, help = "Path to configuration YAML file")]
    pub file: String,
    #[arg(
        short,
        long = "tag",
        help = "Filter steps by tags expression, e.g. !(tag1 || tag2) && tag3"
    )]
    pub tags_expr: Option<String>,
    #[arg(short, long = "step", help = "Run only specific steps by their IDs")]
    pub steps: Vec<String>,
    #[arg(skip)]
    pub start_step_id: Option<String>,
    #[arg(
        short,
        long,
        help = "Enable interactive mode (ask confirmation before each step)"
    )]
    pub interactive: bool,
    #[arg(
        short,
        long,
        help = "Enable dry-run mode (no scripts or packages executed)"
    )]
    pub dry_run: bool,
}

#[derive(Args)]
pub struct ResumeArgs {
    #[arg(
        short,
        long,
        help = "Enable interactive mode (ask confirmation before each step)"
    )]
    pub interactive: bool,
    #[arg(
        short,
        long,
        help = "Enable dry-run mode (no scripts or packages executed)"
    )]
    pub dry_run: bool,
}

#[derive(Args)]
pub struct ListStepsArgs {
    #[arg(short, long, required = true, help = "Path to configuration YAML file")]
    pub file: String,
    #[arg(
        short,
        long = "tag",
        help = "Filter steps by tags expression, e.g. !(tag1 || tag2) && tag3"
    )]
    pub tags_expr: Option<String>,
    #[arg(short, long, help = "Plain output: list of step IDs only, no details")]
    pub plain: bool,
    #[arg(
        short,
        long,
        help = "Include all steps regardless of whether they match the current OS"
    )]
    pub all: bool,
}

#[derive(Args)]
pub struct ListTagsArgs {
    #[arg(short, long, required = true, help = "Path to configuration YAML file")]
    pub file: String,
}

#[derive(Args)]
pub struct CompletionArgs {
    #[arg(value_enum)]
    pub shell: clap_complete::Shell,
}