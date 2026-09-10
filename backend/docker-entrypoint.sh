#!/bin/sh
set -e
if [ "${SKIP_INGEST:-}" != "1" ]; then
  ingest
fi
exec api
