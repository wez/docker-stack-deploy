#!/bin/sh
# Rotate the GitHub token used by docker-stack-deploy on one or more hosts.
#
# A plain `docker restart` keeps the token baked into the running container at
# creation time, so rotating requires rewriting the .env line and recreating
# the container. This prompts once for the new token and applies it to each
# host over SSH, sending the token on stdin to keep it out of argv and history.
#
# Connects as root because writing .env and recreating the container both need
# privilege the login user lacks.
#
# Usage: ./apply-token.sh HOST [HOST...]
set -eu

[ $# -ge 1 ] || { echo "usage: $0 host [host...]" >&2; exit 1; }

printf 'New GitHub token: ' >&2
stty -echo
read -r TOKEN
stty echo
printf '\n' >&2
[ -n "$TOKEN" ] || { echo "empty token, aborting" >&2; exit 1; }

remote='
set -eu
cd /var/lib/docker-stack-deploy
read -r TOKEN
umask 077
grep -v "^GITHUB_TOKEN=" .env > .env.new
printf '"'"'GITHUB_TOKEN="%s"\n'"'"' "$TOKEN" >> .env.new
mv .env.new .env
docker compose up -d --force-recreate
'

for host in "$@"; do
  printf '== %s ==\n' "$host" >&2
  printf '%s\n' "$TOKEN" | ssh "root@$host" "$remote"
done
