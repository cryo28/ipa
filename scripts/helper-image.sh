#!/usr/bin/env bash

set -eou pipefail

help() {
  echo "Usage: $0 --hostname [hostname] --identity [1|2|3]"
  echo "- hostname: public hostname that will appear on TLS certificate"
  echo "- identity: helper identity to be fixed inside the IPA build"
}

parse_args() {
  while [ "${1:-}" != "" ]; do
    case "$1" in
      --identity)
        shift
        identity="$1"
        ;;
      --hostname)
        shift
        hostname="$1"
        ;;
      *)
      # unknown option
      help
      exit 1
    esac
    shift
  done
}

parse_args "$@"
if [[ -z "${identity}" || -z "${hostname}" ]]; then
    help
    exit 1
fi

rev=$(git log -n 1 --format='format:%H' | cut -c1-10)
tag="private-attribution/ipa:$rev-h$identity"

cd "$(dirname "$0")"/.. || exit 1
docker build -t "$tag" -f docker/helper.Dockerfile --build-arg IDENTITY="$identity" --build-arg HOSTNAME="$hostname" .
