#!/usr/bin/env bash
#
# load-env.sh – Decrypt .env.gpg and print 'export' statements for eval,
#               or edit the encrypted file in‑place.
#
# Usage:
#   eval "$(./load-env.sh [<encrypted-file>])"
#   ./load-env.sh --edit [<encrypted-file>]
#
# If no file is given, defaults to '.env_enc' in the current directory.

set -euo pipefail

DEFAULT_FILE=".env_enc"
EDIT_MODE=false
FILE=""

# Parse options
if [[ $# -gt 0 && "$1" == "--edit" ]]; then
    EDIT_MODE=true
    shift
fi

ENCRYPTED_FILE="${1:-$DEFAULT_FILE}"

if [[ ! -f "$ENCRYPTED_FILE" ]]; then
    echo "Error: Encrypted file '$ENCRYPTED_FILE' not found." >&2
    exit 1
fi

# ----------------------------------------------------------------------
# Function: decrypt to stdout (original behaviour, unchanged)
# ----------------------------------------------------------------------
decrypt_to_stdout() {
    read -s -p "Enter passphrase for $ENCRYPTED_FILE: " PASSPHRASE
    echo >&2

    DECRYPTED=$(gpg --batch -d --passphrase-fd 0 "$ENCRYPTED_FILE" 2>/dev/null <<<"$PASSPHRASE") || {
        echo "Error: Decryption failed (wrong passphrase or corrupted file)." >&2
        unset PASSPHRASE
        exit 1
    }

    unset PASSPHRASE

    while IFS= read -r line || [[ -n "$line" ]]; do
        if [[ -z "$line" || "$line" =~ ^[[:space:]]*# ]]; then
            continue
        fi
        line="$(echo "$line" | sed -e 's/^[[:space:]]*//' -e 's/[[:space:]]*$//')"
        key="${line%%=*}"
        value="${line#*=}"
        if [[ -z "$key" ]]; then
            echo "Warning: skipping malformed line (no key): $line" >&2
            continue
        fi
        printf "export %s=%s\n" "$key" "$(printf '%q' "$value")"
    done <<<"$DECRYPTED"

    unset DECRYPTED
}

# ----------------------------------------------------------------------
# Function: edit the encrypted file in‑place (fixed version)
# ----------------------------------------------------------------------
edit_encrypted_file() {
    read -s -p "Enter passphrase for $ENCRYPTED_FILE: " PASSPHRASE
    echo >&2

    TEMP_FILE=$(mktemp) || {
        echo "Error: Cannot create temporary file." >&2
        unset PASSPHRASE
        exit 1
    }
    trap 'rm -f "$TEMP_FILE"' EXIT

    # Decrypt without --output, using redirection. Capture stderr separately.
    echo "Decrypting ..." >&2
    if ! gpg --batch -d --passphrase-fd 0 "$ENCRYPTED_FILE" > "$TEMP_FILE" 2> >(cat >&2) <<<"$PASSPHRASE"; then
        echo "Error: Decryption failed. Wrong passphrase or corrupted file." >&2
        rm -f "$TEMP_FILE"
        unset PASSPHRASE
        exit 1
    fi

    # Verify that we got some content (optional)
    if [[ ! -s "$TEMP_FILE" ]]; then
        echo "Error: Decrypted file is empty." >&2
        rm -f "$TEMP_FILE"
        unset PASSPHRASE
        exit 1
    fi

    EDITOR="${EDITOR:-vi}"
    echo "Opening editor ($EDITOR) on decrypted content..." >&2
    $EDITOR "$TEMP_FILE"

    echo >&2
    read -p "Re-encrypt and replace original file? (y/N) " -n 1 -r
    echo >&2
    if [[ ! $REPLY =~ ^[Yy]$ ]]; then
        echo "Edit cancelled. Original file unchanged." >&2
        rm -f "$TEMP_FILE"
        unset PASSPHRASE
        exit 0
    fi

    # Re-encrypt with explicit AES256 and the same passphrase
    echo "Re-encrypting ..." >&2
    if ! gpg --yes --batch --symmetric --cipher-algo AES256 --passphrase-fd 0 --output "$ENCRYPTED_FILE" "$TEMP_FILE" 2>&1 <<<"$PASSPHRASE"; then
        echo "Error: Encryption failed. Original file unchanged." >&2
        rm -f "$TEMP_FILE"
        unset PASSPHRASE
        exit 1
    fi

    echo "Successfully re-encrypted $ENCRYPTED_FILE" >&2
    rm -f "$TEMP_FILE"
    unset PASSPHRASE
    trap - EXIT
}

# ----------------------------------------------------------------------
# Main
# ----------------------------------------------------------------------
if $EDIT_MODE; then
    edit_encrypted_file
else
    decrypt_to_stdout
fi
