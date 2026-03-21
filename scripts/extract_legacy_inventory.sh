#!/usr/bin/env bash
set -euo pipefail

SOCAT_DIR="${1:-../socat}"

if [[ ! -f "$SOCAT_DIR/xioopen.c" ]]; then
  echo "missing xioopen.c under: $SOCAT_DIR" >&2
  exit 1
fi

addr_count=$(perl -ne 'print "$1\n" if /\{\s*"([^"]+)"\s*,\s*&xioaddr_/;' "$SOCAT_DIR/xioopen.c" | sort -u | wc -l | tr -d ' ')
handler_count=$(perl -ne 'print "$1\n" if /\{\s*"[^"]+"\s*,\s*&([a-zA-Z0-9_]+)\s*\}/;' "$SOCAT_DIR/xioopen.c" | sort -u | wc -l | tr -d ' ')
option_count=$(perl -ne 'if(/const struct optname optionnames\[]\s*=\s*\{/){$in=1;next} if($in&&/\{\s*NULL\s*\}/){$in=0;next} if($in){ while(/"([^"]+)"/g){ print "$1\n" } }' "$SOCAT_DIR/xioopts.c" | sort -u | wc -l | tr -d ' ')

cat <<JSON
{
  "legacy_address_keywords": $addr_count,
  "legacy_address_handlers": $handler_count,
  "legacy_option_keywords": $option_count
}
JSON
