use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};

const SYSTEM_LIB_PATH: &str = "/usr/include/comfylang";

pub fn preprocess_file(
    entry_path: &Path,
    user_include_dirs: &[PathBuf],
    config: &HashMap<String, String>,
) -> Result<String, String> {
    let mut seen = HashSet::new();
    expand_includes_with_conditions(entry_path, &mut seen, user_include_dirs, config)
}

fn expand_includes_with_conditions(
    file_path: &Path,
    seen: &mut HashSet<PathBuf>,
    user_include_dirs: &[PathBuf],
    config: &HashMap<String, String>,
) -> Result<String, String> {
    let canonical_path = fs::canonicalize(file_path)
        .map_err(|e| format!("File not found: {file_path:?} - {e}"))?;

    if !seen.insert(canonical_path.clone()) {
        return Err(format!("Circular include detected: {}", canonical_path.display()));
    }

    let source = fs::read_to_string(&canonical_path)
        .map_err(|e| format!("Failed to read {}: {}", canonical_path.display(), e))?;

    let mut output = String::new();
    let mut condition_stack = Vec::new();
    let mut current_block_active = true;

    for line in source.lines() {
        let trimmed = line.trim();

        if let Some(condition) = parse_if_directive(trimmed) {
            let cond_result = evaluate_condition(&condition, config);
            condition_stack.push(current_block_active);
            current_block_active &= cond_result;
            continue;
        }

        if trimmed == "#else" {
            if let Some(previous_active) = condition_stack.last() {
                current_block_active = *previous_active && !current_block_active;
            }
            continue;
        }

        if trimmed == "#endif" {
            if let Some(prev) = condition_stack.pop() {
                current_block_active = prev;
            } else {
                return Err("Unmatched #endif".to_string());
            }
            continue;
        }

        if current_block_active {
            if let Some(user_path) = parse_user_include(trimmed) {
                let resolved = resolve_user_include(&user_path, file_path.parent(), user_include_dirs)?;
                let contents = expand_includes_with_conditions(&resolved, seen, user_include_dirs, config)?;
                output.push_str(&contents);
            } else if let Some(include_path) = parse_system_include(trimmed) {
                let sys_path = Path::new(SYSTEM_LIB_PATH).join(include_path);
                let contents = expand_includes_with_conditions(&sys_path, seen, user_include_dirs, config)?;
                output.push_str(&contents);
            } else {
                output.push_str(line);
                output.push('\n');
            }
        }
    }

    if !condition_stack.is_empty() {
        return Err(format!(
            "Unmatched #if directives detected. Stack: {:?}",
            condition_stack
        ));
    }
    Ok(output)
}

fn parse_if_directive(line: &str) -> Option<String> {
    if line.starts_with("#if ") {
        Some(line.trim_start_matches("#if ").trim().to_string())
    } else {
        None
    }
}

fn evaluate_condition(condition: &str, config: &HashMap<String, String>) -> bool {
    if let Some((key, value)) = condition.split_once("==") {
        let k = key.trim();
        let v = value.trim().trim_matches('"');
        return config.get(k).map(|val| val == v).unwrap_or(false);
    } else if let Some((key, value)) = condition.split_once("!=") {
        let k = key.trim();
        let v = value.trim().trim_matches('"');
        return config.get(k).map(|val| val != v).unwrap_or(true);
    } else {
        return config
            .get(condition.trim())
            .map(|val| val == "true" || val == "1")
            .unwrap_or(false);
    }
}

fn parse_system_include(line: &str) -> Option<String> {
    if line.starts_with("#include<") && line.ends_with('>') {
        Some(
            line.trim_start_matches("#include<")
                .trim_end_matches('>')
                .trim()
                .to_string(),
        )
    } else {
        None
    }
}

fn parse_user_include(line: &str) -> Option<String> {
    if line.starts_with("#include<\"") && line.ends_with("\">") {
        Some(
            line.trim_start_matches("#include<\"")
                .trim_end_matches("\">")
                .trim()
                .to_string(),
        )
    } else {
        None
    }
}

fn resolve_user_include(
    name: &str,
    current_dir: Option<&Path>,
    user_paths: &[PathBuf],
) -> Result<PathBuf, String> {
    if let Some(dir) = current_dir {
        let candidate = dir.join(name);
        if candidate.exists() {
            return Ok(candidate);
        }
    }

    for path in user_paths {
        let candidate = path.join(name);
        if candidate.exists() {
            return Ok(candidate);
        }
    }

    Err(format!("User include not found: {}", name))
}
