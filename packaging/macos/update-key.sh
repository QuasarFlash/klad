#!/usr/bin/env bash
set -euo pipefail
root="$(cd "$(dirname "$0")/../.." && pwd)"
sparkle="$(bash "$root/packaging/macos/sparkle.sh")"
key="$root/packaging/macos/sparkle-key.txt"
"$sparkle/bin/generate_keys"
"$sparkle/bin/generate_keys" -p > "$key"
echo
echo "The public half is in packaging/macos/sparkle-key.txt: commit it, every build carries it."
echo "The private half stays in your login keychain and signs every update (just publish)."
echo "Keep a copy somewhere safe, like your password manager: $sparkle/bin/generate_keys -x <file> writes it out."
echo "Without it, the klad already installed on people's Macs can't take an update signed with another key."
