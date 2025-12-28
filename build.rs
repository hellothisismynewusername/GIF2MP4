fn main() {
    match std::env::var("FFMPEG_PATH") {
        Err(_) => {
            println!("Please set the environment variable 'FFMPEG_PATH' to the executable");
            std::process::exit(1);
        },
        _ => {},
    }

    println!("cargo:rerun-if-env-changed=REQUIRED_VAR");
}
