#!/usr/bin/env bash
# Makes the local self-signed certificate that lets a phone open the map over
# https: without a secure context the browser refuses navigator.geolocation.
# Run once from frontend/, then `npm run dev` serves https on the LAN.
set -euo pipefail

cd "$(dirname "$0")"
mkdir -p certs

# Every address the phone may type, as a subjectAltName, plus the machine IPs.
extra=${LAN_IP:-}
ips="$(hostname -I 2>/dev/null || true) $extra"
san="DNS:localhost,IP:127.0.0.1"
for ip in $ips; do
  case "$ip" in
    *:*) ;;                       # skip IPv6, the phones use IPv4 here
    *) san="$san,IP:$ip" ;;
  esac
done

openssl req -x509 -newkey rsa:2048 -nodes -days 30 \
  -keyout certs/key.pem -out certs/cert.pem \
  -subj "/CN=vdnh-walk" \
  -addext "subjectAltName=$san" 2>/dev/null

echo "cert ready for: $san"
echo "open https://<this-machine-ip>:8001/ on the phone and accept the warning"
