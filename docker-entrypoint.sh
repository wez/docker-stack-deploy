#!/bin/bash

export GITHUB_URL=${GITHUB_URL}
export POLL_INTERVAL=${POLL_INTERVAL:-300}
export GITHUB_USERNAME=${GITHUB_USERNAME}
export GITHUB_TOKEN=${GITHUB_TOKEN}

exec /usr/bin/docker-stack-deploy \
  --kdbx /app/repo/.secrets.kdbc \
  run \
  --poll-interval "${POLL_INTERVAL}" \
  --repo-dir /app/repo \
  --repo-url "${GITHUB_URL}"
