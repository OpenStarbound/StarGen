use clap::{Parser, Subcommand};
use std::path::PathBuf;
use stargen::generator::{self, BodyType, Species, SpriteOutputType};


#[global_allocator]
static GLOBAL: mimalloc::MiMalloc = mimalloc::MiMalloc;

#[derive(Parser)]
#[command(version, about = "generates compact directives for custom sprites")]
struct Cli {
    #[command(subcommand)]
    command: Command
}

// every command writes to a file by default, pass -o - for stdout
#[derive(Subcommand)]
enum Command {
    /// generates a single sprite, optionally formatting the output for objects or hats
    Sprite {
        image: PathBuf,
        #[arg(short = 't', long = "type", value_enum, default_value_t = SpriteOutputType::Raw)]
        output_type: SpriteOutputType,
        #[arg(short, long)]
        output: Option<PathBuf>
    },
    /// generates a custom shirt for both body types
    Chest {
        #[arg(long)]
        chest_female: Option<PathBuf>,
        #[arg(long)]
        chest_male: Option<PathBuf>,
        #[arg(long)]
        front_sleeve: Option<PathBuf>,
        #[arg(long)]
        back_sleeve: Option<PathBuf>,
        #[arg(short, long)]
        output: Option<PathBuf>
    },
    /// generates custom pants
    Pants {
        pants: PathBuf,
        #[arg(long)]
        hide_body: bool,
        #[arg(short, long)]
        output: Option<PathBuf>
    },
    /// generates custom backwear
    Back {
        back: PathBuf,
        #[arg(short, long)]
        output: Option<PathBuf>
    },
    /// generates custom humanoid body directives
    Humanoid {
        #[arg(value_enum)]
        body_type: BodyType,
        #[arg(value_enum)]
        species: Species,
        #[arg(long)]
        body: Option<PathBuf>,
        #[arg(long)]
        head: Option<PathBuf>,
        #[arg(long)]
        front_arm: Option<PathBuf>,
        #[arg(long)]
        back_arm: Option<PathBuf>,
        #[arg(short, long)]
        output: Option<PathBuf>
    }
}

fn main() -> anyhow::Result<()> {
    match Cli::parse().command {
        Command::Sprite { image, output_type, output } => generator::generate_sprite(&image, output_type, output),
        Command::Chest { chest_female, chest_male, front_sleeve, back_sleeve, output } =>
            generator::generate_shirt(chest_female, chest_male, front_sleeve, back_sleeve, output),
        Command::Pants { pants, hide_body, output } => generator::generate_pants(&pants, hide_body, output),
        Command::Back { back, output } => generator::generate_back(&back, output),
        Command::Humanoid { body_type, species, body, head, front_arm, back_arm, output } =>
            generator::generate_humanoid(body_type, species, body, head, front_arm, back_arm, output)
    }
}
