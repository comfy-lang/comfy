use std::path::{Path, PathBuf};

use crate::{
    backend::{
        arm32::{self, syscall_mapper::Architecture},
        generator::generate,
    },
    extra::config::load_config,
    frontend::{parser::parse, preprocessor, tokenizer::tokenize},
};

mod backend;
mod extra;
mod frontend;

fn main() {
    let config_file = PathBuf::from("project.comfx");
    let config = load_config(config_file.to_str().expect("Failed to convert config file path to a UTF-8 string"));

    let args: Vec<String> = std::env::args().collect();

    if args.len() < 2 {
        eprintln!("Usage: {} <file_path>", args[0]);
        std::process::exit(1);
    }

    let arch = match config.target.arch.as_str() {
        "arm32" => Architecture::Arm32,
        "x86" => panic!("X86 architecture not supported yet"),
        "x86_64" => panic!("X86_64 architecture not supported yet"),
        "arm64" => panic!("ARM64 architecture not supported yet"),
        other => panic!("Unsupported architecture: {}", other),
    };

    let file_path = &args[1];
    let input_path = Path::new(file_path);
    let file_stem = input_path.file_stem().unwrap_or_default().to_string_lossy();

    let output_path = config
        .target
        .output
        .unwrap_or_else(|| format!("build/{}.s", file_stem));

    let user_paths = vec![PathBuf::from("./src")];
    let defines = config.defines.clone().unwrap_or_default();

    let preprocessed_content =
        match preprocessor::preprocess_file(input_path, &user_paths, &defines) {
            Ok(content) => content,
            Err(e) => {
                eprintln!("Error preprocessing file {}: {}", input_path.display(), e);
                std::process::exit(1);
            }
        };

    let verbose = args.get(2).map_or(false, |arg| arg == "--verbose");

    let tokens = tokenize(&preprocessed_content);
    if verbose {
        println!("Tokens: {:#?}", tokens);
    }
    let ast_nodes = parse(tokens.clone());
    if verbose {
        println!("AST Nodes: {:#?}", ast_nodes);
    }

    let generator = generate(&ast_nodes, arch);

    let assembly_code = arm32::asm::generate_assembly(
        generator.section_writer.rodata,
        generator.section_writer.bss,
        generator.section_writer.text,
    );
    if verbose {
        println!("\nPreprocessed Content:\n\n{}", preprocessed_content);
        println!("Generated Assembly Code:\n{}", assembly_code);
    }

    let output_path = Path::new(&output_path);

    if let Some(parent_dir) = output_path.parent() {
        if let Err(e) = std::fs::create_dir_all(parent_dir) {
            eprintln!(
                "Error creating output directory {}: {}",
                parent_dir.display(),
                e
            );
            std::process::exit(1);
        }
    }

    match std::fs::write(output_path, assembly_code) {
        Ok(_) => println!(
            "Assembly code written to {} <3\nUsing architecture: {:?}",
            output_path.display(),
            arch
        ),
        Err(e) => {
            eprintln!("Error writing to {}: {}", output_path.display(), e);
            std::process::exit(1);
        }
    }
}
