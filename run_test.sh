#!/usr/bin/env bash
set -euo pipefail

MAX_TICKS="${MAX_TICKS:-1000000}"
TRACE_MODE="${TRACE_MODE:-brief}"

run_case() {
  local dir="$1"
  local lisp_file="$2"
  local out_base="$3"
  local input_file="${4:-}"

  local src="${dir}/${lisp_file}"
  local bin="${dir}/${out_base}.bin"
  local txt="${dir}/${out_base}.txt"

  echo "Running ${dir} ..."

  if [[ ! -f "$src" ]]; then
    echo "error: missing Lisp file: $src" >&2
    exit 1
  fi

  cargo run --quiet -- compile-lisp "$src" "$bin"

  if [[ -n "$input_file" ]]; then
    local input="${dir}/${input_file}"
    if [[ ! -f "$input" ]]; then
      echo "error: missing input file: $input" >&2
      exit 1
    fi
    cargo run --quiet -- run-lisp "$src" "$input" "$MAX_TICKS" "$TRACE_MODE" > "$txt"
  else
    cargo run --quiet -- run-lisp "$src" "$MAX_TICKS" "$TRACE_MODE" > "$txt"
  fi

  echo "  wrote ${bin}"
  echo "  wrote ${dir}/${out_base}.lst"
  echo "  wrote ${txt}"
  echo
}

run_case "examples/01_print_hello_world"            "01_hello.lisp"                      "01"
run_case "examples/02_recursive_factorial"          "02_rec_factorial.lisp"              "02"
run_case "examples/03_recursive_fibonacci"          "03_rec_fibonacci.lisp"              "03"
run_case "examples/04_test_strings"     "04_strings_static.lisp"             "04"
run_case "examples/05_bubble_sort"         "05_bubble_sort.lisp"         "05"
run_case "examples/06_operations_with_64_bit_nums"  "06_i64_basic.lisp"                  "06"
run_case "examples/07_selection_sort"      "07_selection_sort.lisp" "07"
run_case "examples/08_prob1"                        "08_prob1.lisp"                      "prob1"
run_case "examples/09_hello_user_name"              "09_hello_user_name.lisp"            "09" "input.txt"
run_case "examples/10_print_input_string"           "10_print_input_string.lisp"         "10" "input.txt"
run_case "examples/12_custom_handler"               "12_custom_handler.lisp"             "12" "input.txt"
run_case "examples/11_vector_operations_on_array"   "11_vector_op.lisp"                  "11"


echo "All tests finished."
