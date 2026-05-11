#!/usr/bin/env bash
set -euo pipefail

PROJECT_NAME="minilab_huge_mini"
PROJECT_DESCRIPTION="Minilab Huge Mini mono-repo config and secrets"
ENVIRONMENTS=(prd)
SURFACES=(constitutional_runtime minilab_store minilab_api gtm_ops)

require() {
  command -v "$1" >/dev/null 2>&1 || {
    echo "missing required command: $1" >&2
    exit 1
  }
}

project_exists() {
  doppler projects --json | rg -q "\"name\":\"${PROJECT_NAME}\""
}

config_exists() {
  local env="$1"
  local name="$2"
  doppler configs -p "${PROJECT_NAME}" --json | rg -q "\"name\":\"${name}\".*\"environment\":\"${env}\""
}

set_root_defaults() {
  local env="$1"
  local stage sim_mode

  case "$env" in
    prd)
      stage="production"
      sim_mode="production"
      ;;
    *)
      echo "unknown environment: $env" >&2
      exit 1
      ;;
  esac

  doppler secrets set -p "${PROJECT_NAME}" -c "${env}" \
    MINILAB_DOPPLER_SCHEMA_VERSION="2026-04-20" \
    MINILAB_REPO_TOPOLOGY="single_repo" \
    MINILAB_IMPLEMENTATION_CANONICAL="constitutional_runtime" \
    MINILAB_BUSINESS_RUNTIME="minilab_huge_mini" \
    MINILAB_DEPLOY_STAGE="${stage}" \
    MINILAB_SIM_MODE="${sim_mode}" \
    MINILAB_OUTBOUND_PROVIDER="auto" \
    MINILAB_HTTP_BIND_ADDR="0.0.0.0:3000" \
    MINILAB_HTTP_TIMEOUT_SECS="30" \
    MINILAB_HTTP_MAX_TWILIO_BODY_BYTES="262144" \
    MINILAB_HTTP_MAX_SENDGRID_BODY_BYTES="10485760" \
    MINILAB_REPLY_EMAIL_LOCALPART="reply" \
    >/dev/null
}

set_surface_markers() {
  local env="$1"

  doppler secrets set -p "${PROJECT_NAME}" -c "${env}_constitutional_runtime" \
    MINILAB_CONFIG_SCOPE="constitutional_runtime" \
    MINILAB_CONFIG_KIND="runtime_governance" \
    >/dev/null

  doppler secrets set -p "${PROJECT_NAME}" -c "${env}_minilab_store" \
    MINILAB_CONFIG_SCOPE="minilab_store" \
    MINILAB_CONFIG_KIND="stateful_worker" \
    >/dev/null

  doppler secrets set -p "${PROJECT_NAME}" -c "${env}_minilab_api" \
    MINILAB_CONFIG_SCOPE="minilab_api" \
    MINILAB_CONFIG_KIND="http_edge" \
    >/dev/null

  doppler secrets set -p "${PROJECT_NAME}" -c "${env}_gtm_ops" \
    MINILAB_CONFIG_SCOPE="gtm_ops" \
    MINILAB_CONFIG_KIND="business_governance" \
    >/dev/null
}

main() {
  require doppler
  require rg

  if ! project_exists; then
    doppler projects create \
      --name "${PROJECT_NAME}" \
      --description "${PROJECT_DESCRIPTION}" \
      >/dev/null
  fi

  for env in "${ENVIRONMENTS[@]}"; do
    for surface in "${SURFACES[@]}"; do
      config_name="${env}_${surface}"
      if ! config_exists "${env}" "${config_name}"; then
        doppler configs create "${config_name}" -p "${PROJECT_NAME}" -e "${env}" >/dev/null
      fi
    done

    set_root_defaults "${env}"
    set_surface_markers "${env}"
  done

  echo "Doppler bootstrap complete for ${PROJECT_NAME}"
}

main "$@"
