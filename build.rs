use std::fs;
use std::path::Path;

#[path = "src/cli.rs"]
mod cli;

fn main() {
    println!("cargo:rerun-if-changed=src/cli.rs");

    let out_dir = match std::env::var_os("CARGO_TARGET_DIR") {
        Some(out) => std::path::PathBuf::from(out),
        None => {
            let manifest_dir = std::env::var("CARGO_MANIFEST_DIR").unwrap();
            Path::new(&manifest_dir).join("target")
        }
    };

    let comp_dir = out_dir.join("completions");
    let man_dir = out_dir.join("man");

    let _ = fs::create_dir_all(&comp_dir);
    let _ = fs::create_dir_all(&man_dir);

    use clap::CommandFactory;
    use clap_complete::Shell;

    let mut cmd = cli::Cli::command();

    // Generate shell completions
    for shell in &[Shell::Bash, Shell::Zsh, Shell::Fish, Shell::Elvish] {
        clap_complete::generate_to(*shell, &mut cmd, "exynos-loke-rescue", &comp_dir).ok();
    }

    // Generate man page
    let man = clap_mangen::Man::new(cmd);
    let mut man_out = Vec::new();
    if man.render(&mut man_out).is_ok() {
        let _ = fs::write(man_dir.join("exynos-loke-rescue.1"), man_out);
    }
}
