#!/usr/bin/env bash
set -euo pipefail

if [[ $# -ne 2 ]]; then
  echo "usage: $0 <baseline-dir> <candidate-aggregate-dir>" >&2
  exit 1
fi

baseline_dir="$1"
candidate_dir="$2"
require_baseline="${REQUIRE_BASELINE:-0}"

if ! command -v jq >/dev/null 2>&1; then
  echo "jq is required for benchmark regression checks" >&2
  exit 1
fi

if [[ ! -d "$candidate_dir" ]]; then
  echo "Candidate aggregate dir not found: $candidate_dir (skipping)"
  exit 0
fi

shopt -s nullglob
candidate_files=("$candidate_dir"/*.json)
if [[ ${#candidate_files[@]} -eq 0 ]]; then
  echo "No candidate aggregate benchmark files found under $candidate_dir (skipping)"
  exit 0
fi

if [[ ! -d "$baseline_dir" ]]; then
  if [[ "$require_baseline" == "1" ]]; then
    echo "Baseline dir not found and REQUIRE_BASELINE=1: $baseline_dir" >&2
    exit 1
  fi
  echo "Baseline dir not found: $baseline_dir (warning-only, skipping strict checks)"
  exit 0
fi

baseline_files=("$baseline_dir"/*.json)
if [[ ${#baseline_files[@]} -eq 0 && "$require_baseline" == "1" ]]; then
  echo "No baseline files found and REQUIRE_BASELINE=1: $baseline_dir" >&2
  exit 1
fi

status=0

find_baseline_match() {
  local scenario_id="$1"
  local mode="$2"
  local file

  for file in "${baseline_files[@]}"; do
    local b_scenario b_mode
    b_scenario="$(jq -r '.scenario_id // empty' "$file")"
    b_mode="$(jq -r '.mode_under_test // empty' "$file")"
    if [[ "$b_scenario" == "$scenario_id" && "$b_mode" == "$mode" ]]; then
      printf '%s\n' "$file"
      return 0
    fi
  done

  return 1
}

compare_float() {
  local lhs="$1"
  local op="$2"
  local rhs="$3"
  awk -v lhs="$lhs" -v rhs="$rhs" -v op="$op" 'BEGIN {
    if (op == "lt") exit(!(lhs < rhs));
    if (op == "gt") exit(!(lhs > rhs));
    if (op == "le") exit(!(lhs <= rhs));
    if (op == "ge") exit(!(lhs >= rhs));
    exit(2);
  }'
}

for candidate in "${candidate_files[@]}"; do
  if ! jq empty "$candidate" >/dev/null 2>&1; then
    echo "$candidate: invalid JSON"
    status=1
    continue
  fi

  scenario_id="$(jq -r '.scenario_id // empty' "$candidate")"
  mode="$(jq -r '.mode_under_test // empty' "$candidate")"
  success_rate="$(jq -r '.success_rate // empty' "$candidate")"
  cand_goodput="$(jq -r '.median_goodput_bps // empty' "$candidate")"
  cand_p95="$(jq -r '.p95_completion_time_ms // empty' "$candidate")"

  if [[ -z "$scenario_id" || -z "$mode" || -z "$success_rate" || -z "$cand_goodput" || -z "$cand_p95" ]]; then
    echo "$candidate: missing required aggregate metrics"
    status=1
    continue
  fi

  if compare_float "$success_rate" lt 0.95; then
    echo "$candidate: success_rate $success_rate is below 0.95"
    status=1
  fi

  baseline_match=""
  if baseline_match="$(find_baseline_match "$scenario_id" "$mode" 2>/dev/null)"; then
    base_goodput="$(jq -r '.median_goodput_bps // 0' "$baseline_match")"
    base_p95="$(jq -r '.p95_completion_time_ms // 0' "$baseline_match")"

    if compare_float "$base_goodput" gt 0; then
      goodput_regression_ratio="$(awk -v base="$base_goodput" -v cand="$cand_goodput" 'BEGIN { print (base - cand) / base }')"
      if compare_float "$goodput_regression_ratio" gt 0.05; then
        echo "$candidate: median_goodput_bps regressed by more than 5% vs baseline $baseline_match"
        status=1
      fi
    fi

    if compare_float "$base_p95" gt 0; then
      p95_increase_ratio="$(awk -v base="$base_p95" -v cand="$cand_p95" 'BEGIN { print (cand - base) / base }')"
      if compare_float "$p95_increase_ratio" gt 0.10; then
        echo "$candidate: p95_completion_time_ms increased by more than 10% vs baseline $baseline_match"
        status=1
      fi
    fi
  else
    message="$candidate: no matching baseline found for scenario_id=$scenario_id mode_under_test=$mode"
    if [[ "$require_baseline" == "1" ]]; then
      echo "$message"
      status=1
    else
      echo "$message (warning-only)"
    fi
  fi
done

if [[ $status -eq 0 ]]; then
  echo "Benchmark regression checks passed"
fi

exit $status
