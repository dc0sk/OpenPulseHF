#!/usr/bin/env bash
set -euo pipefail

shopt -s nullglob

status=0
files=(docs/*.md)

if [[ ${#files[@]} -eq 0 ]]; then
  echo "No docs markdown files found under docs/."
  exit 1
fi

for file in "${files[@]}"; do
  project_line=""
  doc_line=""
  status_line=""
  updated_line=""
  line_count=0

  while IFS= read -r line; do
    ((line_count += 1))

    if [[ ${line_count} -eq 1 && ${line} != '---' ]]; then
      echo "${file}: missing opening frontmatter delimiter on line 1"
      status=1
      break
    fi

    if [[ ${line_count} -gt 1 && ${line} == '---' ]]; then
      break
    fi

    case "${line}" in
      project:*)
        project_line="${line}"
        ;;
      doc:*)
        doc_line="${line}"
        ;;
      status:*)
        status_line="${line}"
        ;;
      last_updated:*)
        updated_line="${line}"
        ;;
    esac
  done < "${file}"

  if [[ ${line_count} -lt 2 ]]; then
    echo "${file}: incomplete frontmatter block"
    status=1
    continue
  fi

  if [[ ${project_line} != 'project: openpulsehf' ]]; then
    echo "${file}: project must be 'openpulsehf'"
    status=1
  fi

  expected_doc="doc: ${file}"
  if [[ ${doc_line} != "${expected_doc}" ]]; then
    echo "${file}: doc must match file path (${file})"
    status=1
  fi

  if [[ ${status_line} != 'status: living' ]]; then
    echo "${file}: status must be 'living'"
    status=1
  fi

  if [[ ! ${updated_line} =~ ^last_updated:[[:space:]][0-9]{4}-[0-9]{2}-[0-9]{2}$ ]]; then
    echo "${file}: last_updated must use YYYY-MM-DD"
    status=1
  fi
done

exit ${status}
