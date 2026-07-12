use std::path::{Path, PathBuf};

use crate::codegen::{Backend, arm32::Arm32Backend};

mod ast;
mod codegen;
mod config;
mod diag;
mod lexer;
mod parser;
mod sema;

fn main() {
    let args: Vec<String> = std::env::args().collect();
    if args.len() < 2 {
        eprintln!("Usage: {} <file.cfy>", args[0]);
        std::process::exit(1);
    }

    let input_path = Path::new(&args[1]);
    let config = config::load_config("project.comfx");

    let backend: Box<dyn Backend> = match config.target.arch.as_str() {
        "arm32" => Box::new(Arm32Backend),
        other => {
            eprintln!(
                "error: unsupported architecture '{}' (only 'arm32' exists so far)",
                other
            );
            std::process::exit(1);
        }
    };

    let source = match std::fs::read_to_string(input_path) {
        Ok(content) => content,
        Err(e) => {
            eprintln!("error: could not read {}: {}", input_path.display(), e);
            std::process::exit(1);
        }
    };

    let file_name = input_path.display().to_string();

    let tokens = lexer::lex(&source).unwrap_or_else(|diag| {
        eprintln!("{}", diag.render(&file_name, &source));
        std::process::exit(1);
    });

    let program = parser::parse(&tokens).unwrap_or_else(|diag| {
        eprintln!("{}", diag.render(&file_name, &source));
        std::process::exit(1);
    });

    let checked = sema::check(&program).unwrap_or_else(|diag| {
        eprintln!("{}", diag.render(&file_name, &source));
        std::process::exit(1);
    });

    let assembly = backend.emit(&checked);

    let file_stem = input_path.file_stem().unwrap_or_default().to_string_lossy();
    let output_path = PathBuf::from(
        config
            .target
            .output
            .unwrap_or_else(|| format!("build/{}.s", file_stem)),
    );

    if let Some(parent) = output_path.parent() {
        if let Err(e) = std::fs::create_dir_all(parent) {
            eprintln!("error: could not create {}: {}", parent.display(), e);
            std::process::exit(1);
        }
    }

    match std::fs::write(&output_path, assembly) {
        Ok(_) => println!(
            "Wrote {} (target: {})",
            output_path.display(),
            backend.name()
        ),
        Err(e) => {
            eprintln!("error: could not write {}: {}", output_path.display(), e);
            std::process::exit(1);
        }
    }
}
