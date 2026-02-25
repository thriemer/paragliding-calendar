#!/usr/bin/env bash
#
# load-env.sh – Decrypt .env.gpg and print 'export' statements for eval.
#
# Usage:
#   eval "$(./load-env.sh [<encrypted-file>])"
#
# If no file is given, defaults to '.env.gpg' in the current directory.

set -euo pipefail

# Use first argument or default file name
ENCRYPTED_FILE="${1:-.env_enc}"

# Check that the encrypted file exists
if [[ ! -f "$ENCRYPTED_FILE" ]]; then
  echo "Error: Encrypted file '$ENCRYPTED_FILE' not found." >&2
  exit 1
fi

# Prompt for passphrase securely (no echo, no history)
read -s -p "Enter passphrase for $ENCRYPTED_FILE: " PASSPHRASE
echo >&2 # Add a newline after the prompt

# Decrypt the file, capturing output and checking for errors.
# The passphrase is fed via stdin (--passphrase-fd 0).
# We discard gpg's status messages (2>/dev/null) but you can remove that if you prefer to see them.
DECRYPTED=$(gpg --batch -d --passphrase-fd 0 "$ENCRYPTED_FILE" 2>/dev/null <<<"$PASSPHRASE") || {
  echo "Error: Decryption failed (wrong passphrase or corrupted file)." >&2
  exit 1
}

# Clear the passphrase variable immediately (no longer needed)
unset PASSPHRASE

# Parse the decrypted content line by line
while IFS= read -r line || [[ -n "$line" ]]; do
  # Skip empty lines and comments (lines starting with '#')
  if [[ -z "$line" || "$line" =~ ^[[:space:]]*# ]]; then
    continue
  fi

  # Trim leading and trailing whitespace
  line="$(echo "$line" | sed -e 's/^[[:space:]]*//' -e 's/[[:space:]]*$//')"

  # Extract key (everything before first '=') and value (everything after first '=')
  key="${line%%=*}"
  value="${line#*=}"

  # Sanity check: key must be non‑empty
  if [[ -z "$key" ]]; then
    echo "Warning: skipping malformed line (no key): $line" >&2
    continue
  fi

  # Safely quote the value using Bash's %q – this produces a shell‑escaped representation.
  # The resulting output can be safely eval'd.
  printf "export %s=%s\n" "$key" "$(printf '%q' "$value")"
done <<<"$DECRYPTED"

# (Optional) unset the decrypted content variable – not strictly necessary, but good practice.
unset DECRYPTED
