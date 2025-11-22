#!/usr/bin/env bash

# Generate a random 32-byte hex string for JWT authentication
# between op-reth and kona-node

set -e

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
JWT_DIR="${SCRIPT_DIR}/jwttoken"
JWT_FILE="${JWT_DIR}/jwt.hex"

# Create directory if it doesn't exist
mkdir -p "${JWT_DIR}"

# Generate a random 32-byte hex string (64 hex characters)
if command -v openssl &> /dev/null; then
    # Use openssl if available
    openssl rand -hex 32 > "${JWT_FILE}"
elif command -v xxd &> /dev/null; then
    # Use xxd + /dev/urandom if openssl not available
    head -c 32 /dev/urandom | xxd -p -c 32 > "${JWT_FILE}"
else
    echo "Error: Neither openssl nor xxd found. Please install one of them."
    exit 1
fi

echo "JWT secret generated at: ${JWT_FILE}"
cat "${JWT_FILE}"

