#!/usr/bin/env bash
set -euo pipefail

# ─── Sentinel DNS — Raspberry Pi Installer ───
#
# Default: pulls pre-built Docker images from GHCR. No compilation.
#
# Usage:
#   curl -sSL https://raw.githubusercontent.com/Alexanderrrrrrw/SentinelDNS/main/deploy/install.sh | sudo bash
#
# Developer mode (compiles from source — slow):
#   sudo bash install.sh --build-from-source

RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[0;33m'
CYAN='\033[0;36m'
NC='\033[0m'

log()  { echo -e "${GREEN}[sentinel]${NC} $1"; }
warn() { echo -e "${YELLOW}[sentinel]${NC} $1"; }
err()  { echo -e "${RED}[sentinel]${NC} $1"; }

INSTALL_DIR="${SENTINEL_INSTALL_DIR:-/opt/sentinel-dns}"
SAFETY_CRON="/etc/cron.d/sentinel-safety"
REPO="Alexanderrrrrrw/SentinelDNS"
RAW_BASE="https://raw.githubusercontent.com/${REPO}/main/deploy"
BUILD_FROM_SOURCE=false

for arg in "$@"; do
    case "$arg" in
        --build-from-source) BUILD_FROM_SOURCE=true ;;
    esac
done

# ─── Preflight ───

if [ "$(id -u)" -ne 0 ]; then
    err "This script must be run as root (use sudo)."
    exit 1
fi

log "Sentinel DNS Installer"
if [ "$BUILD_FROM_SOURCE" = true ]; then
    warn "Developer mode: building from source (this will be slow)."
fi
echo ""

# ─── Detect Pi model + RAM ───

ARCH=$(uname -m)
PI_MODEL=$(tr -d '\0' < /proc/device-tree/model 2>/dev/null || echo "Unknown")
RAM_MB=$(awk '/MemTotal/{print int($2/1024)}' /proc/meminfo 2>/dev/null || echo "0")

if [[ "$PI_MODEL" == *"Pi Zero"* ]]; then
    PI_CLASS="Pi Zero / low-power class"
elif [[ "$PI_MODEL" == *"Raspberry Pi 4"* || "$PI_MODEL" == *"Raspberry Pi 5"* ]]; then
    PI_CLASS="Pi 4/5 performance class"
else
    PI_CLASS="Generic ARM/Linux"
fi

log "Model: $PI_MODEL"
log "Class: $PI_CLASS | Arch: $ARCH | RAM: ${RAM_MB}MB"

if [ "$RAM_MB" -gt 0 ] && [ "$RAM_MB" -lt 900 ]; then
    warn "<1GB RAM detected. Sentinel still works, but keep dashboard tabs minimal."
    echo ""
fi

# ─── Install Docker if missing ───

DOCKER_HELP_URL="https://docs.docker.com/engine/install/debian/"
COMPOSE_HELP_URL="https://docs.docker.com/compose/install/linux/"

if ! command -v docker &>/dev/null; then
    log "Installing Docker..."
    if ! curl -fsSL https://get.docker.com | sh; then
        err "Docker install failed."
        err "Manual install guide: $DOCKER_HELP_URL"
        exit 1
    fi
    usermod -aG docker "$(logname 2>/dev/null || echo pi)"
    systemctl enable --now docker
    log "Docker installed."
else
    log "Docker already installed."
fi

if ! docker compose version &>/dev/null; then
    warn "Docker Compose plugin missing. Attempting auto-install..."
    if ! (apt-get update -y && apt-get install -y docker-compose-plugin); then
        err "Docker Compose plugin install failed."
        err "Manual install guide: $COMPOSE_HELP_URL"
        exit 1
    fi
fi

if ! docker compose version &>/dev/null; then
    err "Docker Compose plugin still unavailable after install attempt."
    err "Manual install guide: $COMPOSE_HELP_URL"
    exit 1
fi

# ─── Self-healing safety net ───

install_safety_net() {
    log "Installing DNS safety net (auto-restores system resolver if Sentinel fails)..."
    cat > "$SAFETY_CRON" << 'CRON_EOF'
# Sentinel DNS safety net — auto-heals if Sentinel fails to start.
# Checks every minute. Removes itself once Sentinel is confirmed healthy.
* * * * * root if ss -lnup 2>/dev/null | grep -q ':53 .*sentinel\|docker'; then rm -f /etc/cron.d/sentinel-safety; else systemctl enable systemd-resolved 2>/dev/null; systemctl start systemd-resolved 2>/dev/null; fi
CRON_EOF
    chmod 644 "$SAFETY_CRON"
}

remove_safety_net() {
    if [ -f "$SAFETY_CRON" ]; then
        rm -f "$SAFETY_CRON"
        log "Safety net removed — Sentinel is healthy."
    fi
}

ensure_host_dns() {
    if grep -q "127.0.0.53" /etc/resolv.conf 2>/dev/null; then
        warn "Host resolv.conf still points to 127.0.0.53; switching to systemd upstream resolver file..."
        ln -sf /run/systemd/resolve/resolv.conf /etc/resolv.conf || true
    fi
    if ! getent hosts github.com >/dev/null 2>&1; then
        warn "Host DNS still failing; applying temporary fallback resolvers..."
        cat > /etc/resolv.conf <<EOF
nameserver 1.1.1.1
nameserver 8.8.8.8
EOF
    fi
}

# ─── Free port 53 ───

if ss -lnup 2>/dev/null | grep -q ':53 '; then
    warn "Port 53 is already in use."
    if systemctl is-active --quiet systemd-resolved; then
        warn "systemd-resolved is holding port 53. Disabling its stub listener..."
        install_safety_net
        mkdir -p /etc/systemd/resolved.conf.d
        cat > /etc/systemd/resolved.conf.d/sentinel.conf <<EOF
[Resolve]
DNSStubListener=no
EOF
        systemctl restart systemd-resolved
        ensure_host_dns
        log "systemd-resolved stub listener disabled."
    elif docker compose -f "$INSTALL_DIR/deploy/docker-compose.yml" ps --quiet 2>/dev/null | grep -q .; then
        warn "Previous Sentinel containers are still running. Stopping them first..."
        docker compose -f "$INSTALL_DIR/deploy/docker-compose.yml" down 2>/dev/null || true
        sleep 2
        log "Old containers stopped."
    else
        warn "Something else is using port 53. Attempting to identify..."
        ss -lnup 2>/dev/null | grep ':53 ' || true
        warn "You may need to stop it manually before Sentinel can bind."
    fi
fi

# ─── Deploy directory ───

mkdir -p "$INSTALL_DIR/deploy"
cd "$INSTALL_DIR/deploy"

# ─── Source mode: clone full repo and compile ───

if [ "$BUILD_FROM_SOURCE" = true ]; then
    log "Cloning full repo for source build..."
    if [ -d "$INSTALL_DIR/.git" ]; then
        cd "$INSTALL_DIR"
        git pull --ff-only || warn "Could not auto-update. Continuing with existing code."
    else
        rm -rf "$INSTALL_DIR"
        git clone "https://github.com/${REPO}.git" "$INSTALL_DIR"
    fi
    cd "$INSTALL_DIR/deploy"

    if [ ! -f .env ]; then
        cp .env.example .env
        TOKEN=$(head -c 32 /dev/urandom | base64 | tr -d '=/+' | head -c 32)
        sed -i "s/changeme-to-a-long-random-string/$TOKEN/" .env
        log "Generated admin token: ${CYAN}$TOKEN${NC}"
        warn "Save this token — you'll need it to access the dashboard."
        echo ""
    fi

    log "Building containers from source (this will take 15-30 minutes)..."
    docker compose -f docker-compose.yml build
    docker compose -f docker-compose.yml up -d

# ─── Default mode: pull pre-built images (fast) ───

else
    log "Downloading deployment files..."
    curl -fsSL "${RAW_BASE}/docker-compose.prod.yml" -o docker-compose.yml
    curl -fsSL "${RAW_BASE}/.env.example"            -o .env.example

    TOKEN=""
    if [ ! -f .env ]; then
        cp .env.example .env
        TOKEN=$(head -c 32 /dev/urandom | base64 | tr -d '=/+' | head -c 32)
        sed -i "s/changeme-to-a-long-random-string/$TOKEN/" .env
        log "Generated admin token: ${CYAN}$TOKEN${NC}"
        warn "Save this token — you'll need it to access the dashboard."
        echo ""
    else
        log ".env already exists, keeping current config."
        TOKEN=$(awk -F= '/^SENTINEL_ADMIN_TOKEN=/{print $2}' .env | tr -d '"' || true)
    fi

    log "Pulling pre-built images (no compilation)..."
    PULL_OK=false
    if docker compose pull 2>&1; then
        PULL_OK=true
    fi

    if [ "$PULL_OK" = true ]; then
        log "Images pulled. Starting Sentinel DNS..."
        docker compose up -d
    else
        warn "═══════════════════════════════════════════════════════════"
        warn "Pre-built images not available yet."
        warn "This usually means CI hasn't finished building them."
        warn "Falling back to building from source on the Pi."
        warn "This takes ~20-30 min on a Pi 5. Grab a coffee."
        warn "═══════════════════════════════════════════════════════════"
        echo ""

        if ! command -v git &>/dev/null; then
            log "Installing git..."
            apt-get update -y && apt-get install -y git
        fi

        SAVED_ENV=""
        if [ -f "$INSTALL_DIR/deploy/.env" ]; then
            SAVED_ENV=$(cat "$INSTALL_DIR/deploy/.env")
        fi

        log "Cloning repository..."
        CLONE_TMP=$(mktemp -d)
        git clone --depth 1 "https://github.com/${REPO}.git" "$CLONE_TMP"
        rm -rf "$INSTALL_DIR/crates" "$INSTALL_DIR/tools" "$INSTALL_DIR/apps" "$INSTALL_DIR/fixtures"
        cp -rT "$CLONE_TMP" "$INSTALL_DIR"
        rm -rf "$CLONE_TMP"

        if [ -n "$SAVED_ENV" ]; then
            echo "$SAVED_ENV" > "$INSTALL_DIR/deploy/.env"
        fi

        cd "$INSTALL_DIR/deploy"
        log "Building containers from source..."
        docker compose -f docker-compose.yml build
        docker compose -f docker-compose.yml up -d
    fi
fi

# ─── SD card tuning (download if not present) ───

if [ ! -f "$INSTALL_DIR/deploy/sd-card-tuning.sh" ]; then
    curl -fsSL "${RAW_BASE}/sd-card-tuning.sh" -o "$INSTALL_DIR/deploy/sd-card-tuning.sh" 2>/dev/null || true
fi
if [ -f "$INSTALL_DIR/deploy/sd-card-tuning.sh" ]; then
    echo ""
    log "Applying SD card wear leveling optimizations..."
    bash "$INSTALL_DIR/deploy/sd-card-tuning.sh" || warn "SD card tuning failed (non-fatal)."
fi

# ─── Systemd service ───

cat > /etc/systemd/system/sentinel-dns.service <<SVCEOF
[Unit]
Description=Sentinel DNS — RAM-first ad-blocking DNS resolver
After=network-online.target docker.service
Wants=network-online.target
Documentation=https://github.com/${REPO}

[Service]
Type=simple
WorkingDirectory=${INSTALL_DIR}/deploy
EnvironmentFile=-${INSTALL_DIR}/deploy/.env
ExecStart=docker compose up --no-build
ExecStop=docker compose down
IOSchedulingClass=idle
Nice=5
OOMScoreAdjust=200
Restart=always
RestartSec=10
TimeoutStopSec=30

[Install]
WantedBy=multi-user.target
SVCEOF

systemctl daemon-reload
systemctl enable sentinel-dns
log "Systemd service installed."

# ─── iptables: DNS interception + DoH blocking ───

setup_iptables() {
    log "Configuring iptables DNS interception..."
    LAN_IF=$(ip route | awk '/default/{print $5; exit}')
    if [ -z "$LAN_IF" ]; then LAN_IF="eth0"; fi
    log "LAN interface: $LAN_IF"

    if ! iptables -t nat -C PREROUTING -i "$LAN_IF" -p udp --dport 53 -j REDIRECT --to-port 53 2>/dev/null; then
        iptables -t nat -A PREROUTING -i "$LAN_IF" -p udp --dport 53 -j REDIRECT --to-port 53
        iptables -t nat -A PREROUTING -i "$LAN_IF" -p tcp --dport 53 -j REDIRECT --to-port 53
        log "  DNS redirect: all port 53 traffic now forced through Sentinel"
    else
        log "  DNS redirect: already configured"
    fi

    DOH_IPS=("8.8.8.8" "8.8.4.4" "1.1.1.1" "1.0.0.1" "9.9.9.9" "149.112.112.112")
    for ip in "${DOH_IPS[@]}"; do
        if ! iptables -C FORWARD -d "$ip" -p tcp --dport 443 -j REJECT 2>/dev/null; then
            iptables -A FORWARD -d "$ip" -p tcp --dport 443 -j REJECT --reject-with tcp-reset
        fi
    done
    log "  DoH blocking: Chrome/Android DoH bypass blocked for ${#DOH_IPS[@]} providers"

    if command -v netfilter-persistent &>/dev/null; then
        netfilter-persistent save 2>/dev/null || true
    elif command -v iptables-save &>/dev/null; then
        mkdir -p /etc/iptables
        iptables-save > /etc/iptables/rules.v4
    fi
    log "  iptables rules persisted"
}

if [ -f /proc/sys/net/ipv4/ip_forward ]; then
    FORWARD=$(cat /proc/sys/net/ipv4/ip_forward)
    if [ "$FORWARD" = "0" ]; then
        warn "IP forwarding disabled. iptables DNS redirect requires the Pi to be a gateway."
        warn "Skipping iptables. DNS blocking still works for devices using DHCP DNS."
    else
        setup_iptables
    fi
else
    setup_iptables
fi

# ─── Health check ───

log "Waiting for Sentinel to bind port 53..."
HEALTHY=false
for i in $(seq 1 30); do
    if ss -lnup 2>/dev/null | grep -q ':53 '; then
        HEALTHY=true
        break
    fi
    sleep 2
done

if [ "$HEALTHY" = true ]; then
    remove_safety_net
    log "Sentinel DNS is healthy and serving on port 53."
else
    warn "Sentinel has not bound port 53 yet. Safety net cron will auto-heal if needed."
    warn "Check logs: docker compose -f $INSTALL_DIR/deploy/docker-compose.yml logs -f"
fi

# ─── Done ───

echo ""
LOCAL_IP=$(hostname -I | awk '{print $1}')
log "════════════════════════════════════════════════"
log "  Sentinel DNS is running!"
log ""
log "  DNS server:   ${LOCAL_IP}:53"
log "  API:          http://${LOCAL_IP}:8080"
log "  Dashboard:    http://${LOCAL_IP}:3000"
log ""
log "  Set your router's DNS to: ${LOCAL_IP}"
if [ -n "${TOKEN:-}" ]; then
    log "  Admin token:  ${CYAN}${TOKEN}${NC}"
fi
log "════════════════════════════════════════════════"
echo ""
log "Sentinel will automatically:"
log "  - Seed 9 community blocklists + 7 regex rules"
log "  - Pull all blocklists on first boot"
log "  - Enable heuristic DGA/tracking detection"
echo ""
log "SD card protection:"
log "  - RAM-first log pipeline (writes every 15 min)"
log "  - 50k log records buffered in RAM (~15 MB)"
log "  - Emergency flush on shutdown (zero data loss)"
echo ""
log "Commands:"
log "  Logs:    docker compose -f $INSTALL_DIR/deploy/docker-compose.yml logs -f"
log "  Stop:    docker compose -f $INSTALL_DIR/deploy/docker-compose.yml down"
log "  Update:  docker compose -f $INSTALL_DIR/deploy/docker-compose.yml pull && docker compose -f $INSTALL_DIR/deploy/docker-compose.yml up -d"
