use anyhow::{bail, Context, Result};
use clap::Parser;
use std::path::PathBuf;
use std::process::Command;

/// MP4 → GIF converter with cropping, scaling, preview, and strict size control
#[derive(Parser, Debug, Clone)]
#[command(author, version, about)]
struct Args {
    /// Input MP4 file
    #[arg(short, long)]
    input: PathBuf,

    /// Output GIF file
    #[arg(short, long)]
    output: PathBuf,

    /// Width of the GIF (optional)
    #[arg(long)]
    width: Option<u32>,

    /// Scale percentage of original (optional)
    #[arg(long)]
    scale: Option<f64>,

    /// Frames per second
    #[arg(long, default_value_t = 10)]
    fps: u32,

    /// Start time in seconds
    #[arg(long)]
    start: Option<f64>,

    /// End time in seconds
    #[arg(long)]
    end: Option<f64>,

    /// Crop percentage from the left
    #[arg(long, default_value_t = 0.0)]
    left: f64,

    /// Crop percentage from the right
    #[arg(long, default_value_t = 0.0)]
    right: f64,

    /// Crop percentage from the top
    #[arg(long, default_value_t = 0.0)]
    up: f64,

    /// Crop percentage from the bottom
    #[arg(long, default_value_t = 0.0)]
    down: f64,
}

fn main() -> Result<()> {
    let args = Args::parse();

    ensure_ffmpeg_exists()?;
    let preview_path = args.output.with_extension("preview.png");
    let palette_path = args.output.with_extension("palette.png");

    // 1. Generate preview PNG before GIF creation
    generate_crop_preview(&args, &preview_path)?;

    // 2. Generate palette
    generate_palette(&args, &palette_path)?;

    // 3. Generate GIF strictly under 10 MB
    generate_gif_strict_under_mb(&args, &palette_path, &args.output, 10.0)?;

    // Cleanup
    std::fs::remove_file(&palette_path).ok();

    Ok(())
}

fn get_ffmpeg_path() -> String {
    // already validated this env var in build.rs
    std::env::var("FFMPEG_PATH").unwrap()
}

/// Pulls ffmpeg from the system path and checks if it's working
fn ensure_ffmpeg_exists() -> Result<()> {
    let ffmpeg_path = get_ffmpeg_path();
    let status = Command::new(ffmpeg_path)
        .arg("-version")
        .status()
        .context("Failed to execute ffmpeg")?;

    if !status.success() {
        bail!("FFmpeg is not functioning correctly");
    }
    Ok(())
}

/// Build directional crop filter (percent-based)
fn directional_crop_filter(args: &Args) -> Option<String> {
    let l = args.left / 100.0;
    let r = args.right / 100.0;
    let u = args.up / 100.0;
    let d = args.down / 100.0;

    if l < 0.0 || r < 0.0 || u < 0.0 || d < 0.0 {
        return None;
    }

    if l + r >= 1.0 || u + d >= 1.0 {
        panic!("Invalid crop: total crop percentage ≥ 100%");
    }

    Some(format!(
        "crop=iw*(1-{l}-{r}):ih*(1-{u}-{d}):iw*{l}:ih*{u}"
    ))
}

/// Percentage / 
fn scale_and_crop_filter(args: &Args) -> String {
    let scale_part = if let Some(scale) = args.scale {
        format!(
            "scale=iw*{}:ih*{}:flags=lanczos",
            scale / 100.0,
            scale / 100.0
        )
    } else if let Some(width) = args.width {
        format!("scale={}:-1:flags=lanczos", width)
    } else {
        "scale=iw:ih:flags=lanczos".to_string()
    };

    if let Some(crop) = directional_crop_filter(args) {
        format!("{},{}", scale_part, crop)
    } else {
        scale_part
    }
}

/// Preview image for crop (what the crop would look like)
fn generate_crop_preview(args: &Args, output_png: &PathBuf) -> Result<()> {
    let ffmpeg_path = get_ffmpeg_path();
    let filter = scale_and_crop_filter(args);

    let mut cmd = Command::new(ffmpeg_path);
    cmd.arg("-y");

    if let Some(start) = args.start {
        cmd.args(["-ss", &start.to_string()]);
    }

    cmd.args([
        "-i",
        args.input.to_str().unwrap(),
        "-frames:v",
        "1",
        "-vf",
        &filter,
        output_png.to_str().unwrap(),
    ]);

    let status = cmd.status().context("Preview generation failed")?;
    if !status.success() {
        bail!("Failed to generate preview PNG");
    }

    println!("Preview image generated: {:?}", output_png);
    Ok(())
}

///FFMPEG palette generation
fn generate_palette(args: &Args, palette: &PathBuf) -> Result<()> {
    let ffmpeg_path = get_ffmpeg_path();
    let filter = format!(
        "fps={},{} ,palettegen=stats_mode=diff:max_colors=256",
        args.fps,
        scale_and_crop_filter(args)
    );

    let mut cmd = Command::new(ffmpeg_path);
    cmd.arg("-y");

    if let Some(start) = args.start {
        cmd.args(["-ss", &start.to_string()]);
    }
    if let Some(end) = args.end {
        cmd.args(["-to", &end.to_string()]);
    }

    cmd.args(["-i", args.input.to_str().unwrap()]);
    cmd.args(["-vf", &filter, palette.to_str().unwrap()]);

    let status = cmd.status().context("Palette generation failed")?;
    if !status.success() {
        bail!("FFmpeg palette generation failed");
    }

    Ok(())
}

/// GIF  generation
fn generate_gif(args: &Args, palette: &PathBuf, output: &PathBuf) -> Result<()> {
    let ffmpeg_path = get_ffmpeg_path();
    let filter = format!(
        "fps={},{}[x];[x][1:v]paletteuse=dither=bayer:bayer_scale=5",
        args.fps,
        scale_and_crop_filter(args)
    );

    let mut cmd = Command::new(ffmpeg_path);
    cmd.arg("-y");

    if let Some(start) = args.start {
        cmd.args(["-ss", &start.to_string()]);
    }
    if let Some(end) = args.end {
        cmd.args(["-to", &end.to_string()]);
    }

    cmd.args(["-i", args.input.to_str().unwrap()]);
    cmd.args(["-i", palette.to_str().unwrap()]);
    cmd.args(["-lavfi", &filter, output.to_str().unwrap()]);

    let status = cmd.status().context("GIF generation failed")?;
    if !status.success() {
        bail!("FFmpeg GIF encoding failed");
    }

    Ok(())
}

/// Generate GIF with regard to 10mb limit
fn generate_gif_strict_under_mb(
    args: &Args,
    palette: &PathBuf,
    output: &PathBuf,
    target_mb: f64,
) -> Result<()> {
    let min_scale = 10.0;
    let mut low = min_scale;
    let mut high = args.scale.unwrap_or(100.0);
    let mut best_scale = min_scale;
    let epsilon = 0.0001;

    while (high - low) > 0.25 {
        let mid = (low + high) / 2.0;
        let mut test_args = args.clone();
        test_args.scale = Some(mid);

        generate_gif(&test_args, palette, output)?;

        let size_mb = std::fs::metadata(output)?.len() as f64 / 1024.0 / 1024.0;

        if size_mb >= target_mb {
            high = mid;
        } else {
            best_scale = mid;
            low = mid;
        }
    }

    let mut final_args = args.clone();
    final_args.scale = Some(best_scale);
    generate_gif(&final_args, palette, output)?;

    let final_size = std::fs::metadata(output)?.len() as f64 / 1024.0 / 1024.0;

    if final_size >= target_mb - epsilon {
        bail!("Final GIF is not strictly under {} MB", target_mb);
    }

    println!(
        "Final GIF size: {:.4} MB at scale {:.2}%",
        final_size, best_scale
    );

    Ok(())
}
