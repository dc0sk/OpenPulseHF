#!/usr/bin/env bash
set -euo pipefail

shopt -s nullglob

scenario_dir="benchmark/scenarios"
raw_dir="benchmark/results/raw"
aggregate_dir="benchmark/results/aggregate"

status=0

required_scenario_keys=(
  "scenario_id"
  "bandwidth_class_hz"
  "family"
  "profile"
  "seed_policy"
  "random_seed"
  "run_duration_limit_s"
)

required_raw_json_keys=(
  "schema_version"
  "run_id"
  "scenario_id"
  "mode_under_test"
  "bandwidth_class_hz"
  "random_seed"
  "payload_bytes"
  "run_duration_s"
  "success"
  "raw_throughput_bps"
  "goodput_bps"
  "completion_time_ms"
  "retransmissions"
  "arq_efficiency"
  "spectral_efficiency_bphz"
  "time_to_first_payload_ms"
  "recovery_success_rate"
  "profile_switches_per_min"
  "p95_completion_time_ms"
  "trust_failures"
  "signature_failures"
)

required_aggregate_json_keys=(
  "scenario_id"
  "mode_under_test"
  "run_count"
  "success_rate"
  "median_goodput_bps"
  "p95_completion_time_ms"
  "mean_arq_efficiency"
  "median_spectral_efficiency_bphz"
  "mean_time_to_first_payload_ms"
  "recovery_success_rate"
)

scenario_files=("${scenario_dir}"/*.yaml)
if [[ ${#scenario_files[@]} -eq 0 ]]; then
  echo "No scenario files found under ${scenario_dir}/"
  exit 1
fi

for file in "${scenario_files[@]}"; do
  base_name="$(basename "${file}")"
  expected_id="${base_name%.yaml}"

  for key in "${required_scenario_keys[@]}"; do
    if ! grep -Eq "^${key}:[[:space:]]" "${file}"; then
      echo "${file}: missing required key '${key}'"
      status=1
    fi
  done

  scenario_id_line="$(grep -E '^scenario_id:[[:space:]]' "${file}" || true)"
  if [[ -n "${scenario_id_line}" ]]; then
    scenario_id="${scenario_id_line#scenario_id: }"
    if [[ "${scenario_id}" != "${expected_id}" ]]; then
      echo "${file}: scenario_id '${scenario_id}' must match filename id '${expected_id}'"
      status=1
    fi
  fi

  seed_policy_line="$(grep -E '^seed_policy:[[:space:]]' "${file}" || true)"
  if [[ -n "${seed_policy_line}" ]]; then
    seed_policy="${seed_policy_line#seed_policy: }"
    if [[ "${seed_policy}" != "fixed" && "${seed_policy}" != "sweep" ]]; then
      echo "${file}: seed_policy '${seed_policy}' must be one of: fixed, sweep"
      status=1
    fi
  fi

  random_seed_line="$(grep -E '^random_seed:[[:space:]]' "${file}" || true)"
  if [[ -n "${random_seed_line}" ]]; then
    random_seed="${random_seed_line#random_seed: }"
    if ! [[ "${random_seed}" =~ ^[0-9]+$ ]]; then
      echo "${file}: random_seed '${random_seed}' must be an integer"
      status=1
    fi
  fi
done

if ! command -v jq >/dev/null 2>&1; then
  echo "jq is required to validate benchmark JSON artifacts"
  exit 1
fi

raw_files=("${raw_dir}"/*.json)
for file in "${raw_files[@]}"; do
  if ! jq empty "${file}" >/dev/null 2>&1; then
    echo "${file}: invalid JSON"
    status=1
    continue
  fi

  for key in "${required_raw_json_keys[@]}"; do
    if ! jq -e --arg key "${key}" 'has($key)' "${file}" >/dev/null 2>&1; then
      echo "${file}: missing required key '${key}'"
      status=1
    fi
  done
done

aggregate_files=("${aggregate_dir}"/*.json)
for file in "${aggregate_files[@]}"; do
  if ! jq empty "${file}" >/dev/null 2>&1; then
    echo "${file}: invalid JSON"
    status=1
    continue
  fi

  for key in "${required_aggregate_json_keys[@]}"; do
    if ! jq -e --arg key "${key}" 'has($key)' "${file}" >/dev/null 2>&1; then
      echo "${file}: missing required key '${key}'"
      status=1
    fi
  done
done

if [[ ${status} -eq 0 ]]; then
  echo "Benchmark artifact validation passed"
fi

exit ${status}
