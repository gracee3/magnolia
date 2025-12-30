use clap::Parser;
use std::path::PathBuf;

#[derive(Parser, Debug)]
#[command(author, version, about)]
struct Args {
    #[arg(long)]
    model_dir: PathBuf,

    #[arg(long, default_value_t = 0)]
    device_id: i32,

    #[arg(help = "Path to input WAV file (16kHz mono)")]
    wav_path: PathBuf,
}

fn main() -> anyhow::Result<()> {
    let args = Args::parse();
    let text = parakeet_stt::transcribe_wav(&args.model_dir, &args.wav_path, args.device_id)?;
    println!("{}", text);
    Ok(())
}


