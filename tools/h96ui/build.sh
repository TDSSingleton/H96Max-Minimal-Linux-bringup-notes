#!/usr/bin/env bash
# Build h96ui for the box: static arm64 binary, standard library only. Container:
#   podman run --rm -v "$PWD":/work:z fedora:41 bash -c \
#     'dnf -y -q install golang >/dev/null; bash /work/tools/h96ui/build.sh'
set -euo pipefail
cd "$(dirname "$0")"
VER=$(sed -n 's/^\tversion *= *"\(.*\)"/\1/p' main.go)
export GOFLAGS=-mod=mod GOCACHE=/tmp/gocache GOPATH=/tmp/gopath
go vet ./...
go test ./...
OUT=../../build/h96ui; mkdir -p $OUT
CGO_ENABLED=0 GOOS=linux GOARCH=arm64 go build -trimpath \
  -ldflags "-s -w -X main.buildDate=$(date -u +%Y-%m-%d)" -o $OUT/h96ui .
file $OUT/h96ui 2>/dev/null || true
ls -la $OUT/h96ui && sha256sum $OUT/h96ui
echo "h96ui $VER built"
