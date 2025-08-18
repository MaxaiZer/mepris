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
    #[command(about = "Execute steps from config", long_about = None)]
    Run(RunArgs),
    #[command(about = "Resume failed run", long_about = None)]
    Resume(ResumeArgs),
    #[command(about = "List tags with corresponding steps", long_about = None)]
    ListTags(ListTagsArgs),
}

#[derive(Args)]
pub struct RunArgs {
    #[arg(short, long, required = true)]
    pub file: String,
    #[arg(long = "tag")]
    pub tags: Vec<String>,
    #[arg(long = "step")]
    pub steps: Vec<String>,
    #[arg(skip)]
    pub start_step_id: Option<String>,
    #[arg(
        short,
        long,
        help = "Run in interactive mode, asking for confirmation before each step"
    )]
    pub interactive: bool,
    #[arg(
        long,
        help = "Do a dry-run without executing scripts or installing packages"
    )]
    pub dry_run: bool,
}

#[derive(Args)]
pub struct ResumeArgs {
    #[arg(long)]
    pub interactive: bool,
    #[arg(
        long,
        help = "Do a dry-run without executing scripts or installing packages"
    )]
    pub dry_run: bool,
}

#[derive(Args)]
pub struct ListTagsArgs {
    #[arg(short, long, required = true)]
    pub file: String,
    #[arg(long = "tag")]
    pub tags: Vec<String>,
}
