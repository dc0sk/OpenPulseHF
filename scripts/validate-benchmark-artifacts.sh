#!/usr/bin/env bash
set -euo pipefail

shopt -s nullglob

scenario_dir="benchmark/scenarios"
raw_dir="benchmark/results/raw"
aggregate_dir="benchmark/results/aggregate"
schema_dir="benchmark/schema"
raw_schema="${schema_dir}/raw-result.schema.json"
aggregate_schema="${schema_dir}/aggregate-result.schema.json"

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

validate_json_against_schema() {
  local file="$1"
  local schema="$2"

  if ! jq empty "${schema}" >/dev/null 2>&1; then
    echo "${schema}: invalid JSON schema"
    status=1
    return
  fi

  local errors
  errors="$({
    jq -rn \
      --argfile data "${file}" \
      --argfile schema "${schema}" '
        def value_type($v):
          if ($v | type) == "number" and (($v | floor) == $v) then "integer"
          else ($v | type)
          end;

        def type_ok($v; $expected):
          if $expected == "integer" then
            (($v | type) == "number") and (($v | floor) == $v)
          elif $expected == "number" then
            ($v | type) == "number"
          else
            ($v | type) == $expected
          end;

        [
          (
            $schema.required[]? as $k
            | select(($data | has($k)) | not)
            | "missing required key '\''\($k)'\''"
          ),
          (
            if $schema.additionalProperties == false then
              $data
              | keys[]
              | select(($schema.properties | has(.)) | not)
              | "unexpected key '\''\(.)'\''"
            else
              empty
            end
          ),
          (
            $schema.properties
            | to_entries[]
            | .key as $k
            | .value as $p
            | select($data | has($k))
            | (
                if ($p.type? and (type_ok($data[$k]; $p.type) | not)) then
                  "key '\''\($k)'\'' has type '\''\(value_type($data[$k]))'\'', expected '\''\($p.type)'\''"
                else
                  empty
                end
              ),
              (
                if ($p.minLength? and (($data[$k] | type) == "string") and (($data[$k] | length) < $p.minLength)) then
                  "key '\''\($k)'\'' length \(($data[$k] | length)) is below minLength \($p.minLength)"
                else
                  empty
                end
              ),
              (
                if ($p.pattern? and (($data[$k] | type) == "string") and (($data[$k] | test($p.pattern)) | not)) then
                  "key '\''\($k)'\'' value does not match pattern '\''\($p.pattern)'\''"
                else
                  empty
                end
              ),
              (
                if ($p.minimum? and (($data[$k] | type) == "number") and ($data[$k] < $p.minimum)) then
                  "key '\''\($k)'\'' value \($data[$k]) is below minimum \($p.minimum)"
                else
                  empty
                end
              ),
              (
                if ($p.maximum? and (($data[$k] | type) == "number") and ($data[$k] > $p.maximum)) then
                  "key '\''\($k)'\'' value \($data[$k]) is above maximum \($p.maximum)"
                else
                  empty
                end
              )
          )
        ]
        | .[]
      ' 2>/dev/null
  } || true)"

  if [[ -n "${errors}" ]]; then
    while IFS= read -r line; do
      [[ -z "${line}" ]] && continue
      echo "${file}: ${line}"
      status=1
    done <<< "${errors}"
  fi
}

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

for schema in "${raw_schema}" "${aggregate_schema}"; do
  if [[ ! -f "${schema}" ]]; then
    echo "missing schema file: ${schema}"
    status=1
  fi
done

raw_files=("${raw_dir}"/*.json)
for file in "${raw_files[@]}"; do
  if ! jq empty "${file}" >/dev/null 2>&1; then
    echo "${file}: invalid JSON"
    status=1
    continue
  fi

  validate_json_against_schema "${file}" "${raw_schema}"
done

aggregate_files=("${aggregate_dir}"/*.json)
for file in "${aggregate_files[@]}"; do
  if ! jq empty "${file}" >/dev/null 2>&1; then
    echo "${file}: invalid JSON"
    status=1
    continue
  fi

  validate_json_against_schema "${file}" "${aggregate_schema}"
done

if [[ ${status} -eq 0 ]]; then
  echo "Benchmark artifact validation passed"
fi

exit ${status}
