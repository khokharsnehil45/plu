#!/usr/bin/env bash
# ==============================================================================
# PLU: One-Command Installer for Linux & macOS
# Usage:
#   curl -fsSL https://raw.githubusercontent.com/khokharsnehil45/plu/master/install.sh | bash
# ==============================================================================

set -euo pipefail

REPO="khokharsnehil45/plu"
BINARY_NAME="plu"
INSTALL_DIR="${HOME}/.cargo/bin"

BOLD='\033[1m'
GREEN='\033[0;32m'
CYAN='\033[0;36m'
YELLOW='\033[1;33m'
RED='\033[0;31m'
NC='\033[0m' # No Color

echo -e "${CYAN}========================================================${NC}"
echo -e "${CYAN}  PLU: High-Performance PDF Loader & Unloader Installer ${NC}"
echo -e "${CYAN}========================================================${NC}"

# Check if Rust / Cargo is installed
if command -v cargo >/dev/null 2>&1; then
    echo -e "${GREEN}✓${NC} Cargo detected: $(cargo --version)"
    echo -e "${CYAN}➔${NC} Installing latest ${BOLD}plu${NC} from repository..."
    cargo install --git "https://github.com/${REPO}.git" --force
    echo -e "${GREEN}========================================================${NC}"
    echo -e "${GREEN}  ✓ PLU successfully installed via Cargo!${NC}"
    echo -e "${GREEN}========================================================${NC}"
else
    echo -e "${YELLOW}! Cargo not found. Checking system package managers or prebuilt binaries...${NC}"
    
    # Ensure install directory exists
    mkdir -p "${INSTALL_DIR}"
    
    # Try downloading prebuilt release asset from GitHub
    LATEST_TAG=$(curl -s "https://api.github.com/repos/${REPO}/releases/latest" | grep '"tag_name":' | sed -E 's/.*"([^"]+)".*/\1/' || echo "")
    
    if [ -n "${LATEST_TAG}" ]; then
        OS=$(uname -s | tr '[:upper:]' '[:lower:]')
        ARCH=$(uname -m)
        if [ "$ARCH" = "x86_64" ]; then
            ARCH="x86_64"
        elif [ "$ARCH" = "aarch64" ] || [ "$ARCH" = "arm64" ]; then
            ARCH="aarch64"
        fi
        
        ASSET_URL="https://github.com/${REPO}/releases/download/${LATEST_TAG}/plu-${OS}-${ARCH}.tar.gz"
        echo -e "${CYAN}➔${NC} Downloading prebuilt binary: ${ASSET_URL}..."
        if curl -fSL "${ASSET_URL}" -o /tmp/plu.tar.gz 2>/dev/null; then
            tar -xzf /tmp/plu.tar.gz -C "${INSTALL_DIR}"
            chmod +x "${INSTALL_DIR}/${BINARY_NAME}"
            rm -f /tmp/plu.tar.gz
            echo -e "${GREEN}✓ Installed binary to ${INSTALL_DIR}/${BINARY_NAME}${NC}"
        fi
    fi
    
    if [ ! -f "${INSTALL_DIR}/${BINARY_NAME}" ]; then
        echo -e "${YELLOW}➔ Installing Rust toolchain to build PLU...${NC}"
        curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y
        # shellcheck source=/dev/null
        source "${HOME}/.cargo/env"
        cargo install --git "https://github.com/${REPO}.git" --force
    fi
fi

# Ensure install dir is in PATH
if [[ ":$PATH:" != *":${INSTALL_DIR}:"* ]]; then
    echo -e "${YELLOW}! Warning: ${INSTALL_DIR} is not in your current PATH.${NC}"
    echo -e "  Add it by running:"
    echo -e "    ${BOLD}export PATH=\"${INSTALL_DIR}:\$PATH\"${NC}"
    echo -e "  or restart your shell."
fi

echo -e "\n${BOLD}Verification:${NC}"
if command -v plu >/dev/null 2>&1; then
    plu --version
else
    "${INSTALL_DIR}/plu" --version
fi

echo -e "\n${GREEN}Usage:${NC}"
echo -e "  plu --load input.pdf --unload output.md"
echo -e "  plu --load input.pdf -f md"
echo -e "  plu --load ./documents/ -f md"
echo -e "  plu --ui"
