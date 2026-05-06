use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use serde::{Deserialize, Serialize};
use similar_asserts::assert_eq;
use tempfile::tempdir;

#[derive(Debug, Clone, Serialize, Deserialize)]
struct GoldenCase {
    source_file: PathBuf,

    #[serde(default)]
    input_file: Option<PathBuf>,

    max_ticks: u64,
    trace_mode: String,

    #[serde(default)]
    out_lst: String,

    #[serde(default)]
    out_output: String,

    #[serde(default)]
    out_log: String,
}

#[derive(Debug)]
struct Actual {
    lst: String,
    output: String,
    log: String,
}

#[test]
fn golden_lisp_cases() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let pattern = root.join("tests/golden/*.yml");
    let pattern = pattern.to_string_lossy().to_string();

    let mut files = glob::glob(&pattern)
        .expect("invalid golden glob")
        .map(|entry| entry.expect("bad golden file path"))
        .collect::<Vec<_>>();

    files.sort();

    assert!(
        !files.is_empty(),
        "no golden files found at tests/golden/*.yml"
    );

    for file in files {
        run_golden_case(&root, &file);
    }
}

fn run_golden_case(root: &Path, golden_path: &Path) {
    let text = fs::read_to_string(golden_path)
        .unwrap_or_else(|err| panic!("cannot read {}: {err}", golden_path.display()));

    let mut case: GoldenCase = serde_yaml::from_str(&text)
        .unwrap_or_else(|err| panic!("cannot parse {}: {err}", golden_path.display()));

    let actual = run_case(root, &case, golden_path);

    if std::env::var("UPDATE_GOLDENS").ok().as_deref() == Some("1") {
        case.out_lst = actual.lst;
        case.out_output = actual.output;
        case.out_log = actual.log;

        fs::write(golden_path, render_golden_case(&case))
            .unwrap_or_else(|err| panic!("cannot update {}: {err}", golden_path.display()));

        return;
    }

    assert_eq!(
        case.out_lst,
        actual.lst,
        "listing mismatch in {}",
        golden_path.display()
    );

    assert_eq!(
        case.out_output,
        actual.output,
        "program output mismatch in {}",
        golden_path.display()
    );

    assert_eq!(
        case.out_log,
        actual.log,
        "trace log mismatch in {}",
        golden_path.display()
    );
}

fn run_case(root: &Path, case: &GoldenCase, golden_path: &Path) -> Actual {
    let exe = env!("CARGO_BIN_EXE_lispy");

    let tmp = tempdir().unwrap_or_else(|err| {
        panic!(
            "cannot create temp dir for {}: {err}",
            golden_path.display()
        )
    });

    let source_path = root.join(&case.source_file);

    assert!(
        source_path.exists(),
        "source file does not exist: {}",
        source_path.display()
    );

    let bin_path = tmp.path().join("program.bin");

    run_cmd(
        root,
        exe,
        &["compile-lisp", path_str(&source_path), path_str(&bin_path)],
    );

    let lst_path = bin_path.with_extension("lst");
    let lst = fs::read_to_string(&lst_path).unwrap_or_else(|err| {
        panic!(
            "cannot read generated listing {}: {err}",
            lst_path.display()
        )
    });

    let mut run_args = vec!["run-lisp".to_string(), path_str(&source_path).to_string()];

    if let Some(input_file) = &case.input_file {
        let input_path = root.join(input_file);

        assert!(
            input_path.exists(),
            "input file does not exist: {}",
            input_path.display()
        );

        run_args.push(path_str(&input_path).to_string());
    }

    run_args.push(case.max_ticks.to_string());
    run_args.push(case.trace_mode.clone());

    let run_arg_refs = run_args.iter().map(String::as_str).collect::<Vec<_>>();
    let run_stdout = run_cmd(root, exe, &run_arg_refs);

    let output = extract_program_output(&run_stdout);
    let log = extract_trace_log(&run_stdout);

    Actual { lst, output, log }
}

fn run_cmd(root: &Path, exe: &str, args: &[&str]) -> String {
    let output = Command::new(exe)
        .args(args)
        .current_dir(root)
        .output()
        .unwrap_or_else(|err| panic!("failed to run command: {exe} {args:?}: {err}"));

    if !output.status.success() {
        panic!(
            "command failed: {exe} {args:?}\n\nstdout:\n{}\n\nstderr:\n{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
    }

    String::from_utf8(output.stdout).expect("stdout must be UTF-8")
}

fn extract_program_output(run_stdout: &str) -> String {
    let line = run_stdout
        .lines()
        .find(|line| line.starts_with("output : "))
        .expect("run-lisp stdout must contain an `output : ...` line");

    let debug_string = line
        .strip_prefix("output : ")
        .expect("line starts with output prefix");

    serde_json::from_str::<String>(debug_string)
        .unwrap_or_else(|err| panic!("cannot parse output debug string {debug_string:?}: {err}"))
}

fn extract_trace_log(run_stdout: &str) -> String {
    let marker_start = run_stdout
        .find("[trace:")
        .expect("run-lisp stdout must contain a `[trace:...]` marker");

    let after_marker = &run_stdout[marker_start..];

    let newline_pos = after_marker
        .find('\n')
        .expect("trace marker must be followed by a newline");

    after_marker[newline_pos + 1..].to_string()
}

fn path_str(path: &Path) -> &str {
    path.to_str().expect("path must be valid UTF-8")
}

fn render_golden_case(case: &GoldenCase) -> String {
    let mut out = String::new();

    out.push_str(&format!(
        "source_file: {:?}\n",
        case.source_file.display().to_string()
    ));

    if let Some(input_file) = &case.input_file {
        out.push_str(&format!(
            "input_file: {:?}\n",
            input_file.display().to_string()
        ));
    }

    out.push_str(&format!("max_ticks: {}\n", case.max_ticks));
    out.push_str(&format!("trace_mode: {:?}\n", case.trace_mode));

    push_block(&mut out, "out_lst", &case.out_lst);
    push_block(&mut out, "out_output", &case.out_output);
    push_block(&mut out, "out_log", &case.out_log);

    out
}

fn push_block(out: &mut String, key: &str, value: &str) {
    if value.is_empty() {
        out.push_str(&format!("{key}: \"\"\n"));
        return;
    }

    let marker = if value.ends_with('\n') { "|" } else { "|-" };
    out.push_str(&format!("{key}: {marker}\n"));

    for line in value.lines() {
        out.push_str("  ");
        out.push_str(line);
        out.push('\n');
    }
}
