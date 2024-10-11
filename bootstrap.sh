#!/bin/bash
set -x

docker run --rm -it \
  -v /var/run/docker.sock:/var/run/docker.sock \
  -v /var/lib/docker-stack-deploy:/var/lib/docker-stack-deploy \
  ghcr.io/wez/docker-stack-deploy docker-stack-deploy \
  bootstrap --project-dir /var/lib/docker-stack-deploy \
  "$@"

