#!/usr/bin/env bash
set -euo pipefail

# Configure an existing calculator service. Railway CLI authentication is required.
# Usage: bash scripts/configure-railway.sh PROJECT_ID ENVIRONMENT_ID SERVICE_ID [OWNER/REPO]
calc_project=${1:?Pass the Railway project ID}
calc_environment=${2:?Pass the Railway environment ID}
calc_service=${3:?Pass the Railway service ID}
calc_repository=${4:-Jotrorox/calc}
if [[ ! $calc_service =~ ^[[:xdigit:]]{8}-[[:xdigit:]]{4}-[[:xdigit:]]{4}-[[:xdigit:]]{4}-[[:xdigit:]]{12}$ ]]; then
  printf '%s\n' 'SERVICE_ID must be a Railway UUID.' >&2
  exit 1
fi
export RAILWAY_CALLER=skill:use-railway@1.4.0
export RAILWAY_AGENT_SESSION=${RAILWAY_AGENT_SESSION:-calc-configure-$(date +%s)}

# JSON stdin works in noninteractive CLI sessions as well as human terminals.
railway environment edit --project "$calc_project" --environment "$calc_environment" \
  --message 'Configure calculator build, readiness, and restart policy' --json <<JSON
{"services":{"$calc_service":{"build":{"builder":"DOCKERFILE","dockerfilePath":"/Dockerfile"},"deploy":{"healthcheckPath":"/health","healthcheckTimeout":30,"restartPolicyType":"ON_FAILURE","restartPolicyMaxRetries":3}}}}
JSON

railway service source connect --project "$calc_project" --environment "$calc_environment" \
  --service "$calc_service" --repo "$calc_repository" --branch main --json

railway deployment list --project "$calc_project" --environment "$calc_environment" \
  --service "$calc_service" --json
