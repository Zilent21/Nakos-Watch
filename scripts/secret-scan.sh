#!/usr/bin/env bash
# Run before making the repository public, and before each release.
# Scans the working tree AND every commit on every branch for things that must never be published.
# Exit 1 if anything needs a look. Known-safe fixtures are listed in ALLOW and reported separately.
set -euo pipefail
cd "$(git rev-parse --show-toplevel)"

# The public BIP-32 test vector (test fixtures only). Not a real key.
ALLOW='xprv9s21ZrQH143K3QTDL4LXw2F7HEK3wJUD2nW2nRk4stbPy6cq3jPPqjiChkVvvNKmPGJxWUtg6LnF5kejMRNNU3TGtRBeJgk33yuGBxrMPHi|BEGIN OPENSSH PRIVATE KEY-----\\nabc'

PATTERNS=(
  'BEGIN (RSA |EC |OPENSSH |DSA |ENCRYPTED )?PRIVATE KEY'
  '[xyztuvYZUV]prv[1-9A-HJ-NP-Za-km-z]{100,}'
  '(storePassword|keyPassword|KEYSTORE_PASSWORD|APPLE_PASSWORD|APPLE_API_KEY|TAURI_SIGNING_PRIVATE_KEY)\s*[=:]\s*\S+'
  '(api[_-]?key|secret|token)\s*[=:]\s*["'"'"'][A-Za-z0-9_\-]{16,}'
  'nsec1[0-9a-z]{20,}'
  'ghp_[A-Za-z0-9]{30,}|github_pat_[A-Za-z0-9_]{30,}'
  'PrivateKey *= *[A-Za-z0-9+/]{43}='
)
# Exact private addresses, store IDs and keys of your own infrastructure: one regex per line in
# .secret-scan.local (git-ignored, so the list itself is never published).
if [[ -f .secret-scan.local ]]; then
  while IFS= read -r line; do [[ -n "$line" && "$line" != \#* ]] && PATTERNS+=("$line"); done < .secret-scan.local
fi
FILES_NEVER='\.(jks|keystore|p12|pfx|pem|key|mobileprovision|env)$|(^|/)\.env|google-services\.json'

status=0
echo "== files that must never be tracked"
if git log --all --name-only --format= | sort -u | grep -E "$FILES_NEVER"; then status=1; else echo "none"; fi

for p in "${PATTERNS[@]}"; do
  echo "== /$p/"
  hits=$(git log --all -p --no-color | grep -nE -- "$p" | grep -vE -- "$ALLOW" || true)
  tree=$(git grep -nIE -- "$p" -- . ':!scripts/secret-scan.sh' | grep -vE -- "$ALLOW" || true)
  if [[ -n "$hits$tree" ]]; then
    status=1
    [[ -n "$tree" ]] && echo "working tree:" && echo "$tree" | cut -c1-200 | head -20
    [[ -n "$hits" ]] && echo "history:" && echo "$hits" | cut -c1-200 | head -20
  else
    echo "none"
  fi
done

echo "== personal paths and internal names (review, not always wrong)"
git grep -nIE -- '/Users/[a-z]+/|/Volumes/[A-Za-z]+/' -- . ':!scripts/secret-scan.sh' | cut -c1-160 | head -40 || true

exit $status
