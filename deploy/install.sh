#!/usr/bin/env bash
set -euo pipefail

# ─── Sentinel DNS — Raspberry Pi Installer ───
#
# Usage:
#   curl -sSL https://raw.githubusercontent.com/Alexanderrrrrrw/SentinelDNS/main/deploy/install.sh | bash
#
# Or after cloning:
#   cd deploy && bash install.sh

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

# ─── Preflight checks ───

if [ "$(id -u)" -ne 0 ]; then
    err "This script must be run as root (use sudo)."
    exit 1
fi

log "Sentinel DNS Installer"
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
    warn "Tip: set SENTINEL_RAM_LOG_CAPACITY=50000 in .env"
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
        err "Debian shortcut: sudo apt install docker-compose-plugin"
        exit 1
    fi
fi

if ! docker compose version &>/dev/null; then
    err "Docker Compose plugin still unavailable after install attempt."
    err "Manual install guide: $COMPOSE_HELP_URL"
    exit 1
fi

# ─── Self-healing safety net ───
# If Sentinel fails to bind port 53 within 90 seconds, this cron job
# automatically re-enables systemd-resolved so the Pi doesn't lose
# internet access. The safety net deletes itself once Sentinel is healthy.

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

# ─── Free port 53 (systemd-resolved often holds it) ───

if ss -lnup | grep -q ':53 '; then
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
        log "systemd-resolved stub listener disabled."
    else
        warn "Something else is using port 53. You may need to stop it manually."
    fi
fi

# ─── Clone or update the repo ───

if [ -d "$INSTALL_DIR/.git" ]; then
    log "Updating existing installation at $INSTALL_DIR..."
    cd "$INSTALL_DIR"
    git pull --ff-only || warn "Could not auto-update. Continuing with existing code."
elif [ -f "./docker-compose.yml" ]; then
    log "Running from cloned repo."
    INSTALL_DIR="$(cd .. && pwd)"
    cd "$INSTALL_DIR"
else
    log "Cloning Sentinel DNS to $INSTALL_DIR..."
    git clone https://github.com/Alexanderrrrrrw/SentinelDNS.git "$INSTALL_DIR"
    cd "$INSTALL_DIR"
fi

# ─── Generate config ───

cd deploy
TOKEN=""

if [ ! -f .env ]; then
    log "Creating .env from template..."
    cp .env.example .env

    TOKEN=$(head -c 32 /dev/urandom | base64 | tr -d '=/+' | head -c 32)
    sed -i "s/changeme-to-a-long-random-string/$TOKEN/" .env

    log "Generated admin token: ${CYAN}$TOKEN${NC}"
    warn "Save this token — you'll need it to access the dashboard API."
    echo ""
else
    log ".env already exists, keeping current config."
    TOKEN=$(awk -F= '/^SENTINEL_ADMIN_TOKEN=/{print $2}' .env | tr -d '"' || true)
fi

# ─── SD card wear leveling (optional but recommended) ───

if [ -f "$INSTALL_DIR/deploy/sd-card-tuning.sh" ]; then
    echo ""
    log "Applying SD card wear leveling optimizations..."
    bash "$INSTALL_DIR/deploy/sd-card-tuning.sh"
fi

# ─── Install systemd service ───

if [ -f "$INSTALL_DIR/deploy/sentinel-dns.service" ]; then
    log "Installing systemd service..."
    cp "$INSTALL_DIR/deploy/sentinel-dns.service" /etc/systemd/system/sentinel-dns.service
    systemctl daemon-reload
    systemctl enable sentinel-dns
    log "Systemd service installed (sentinel-dns.service)."
fi

# ─── iptables: DNS interception + DoH blocking ───

setup_iptables() {
    log "Configuring iptables DNS interception..."

    # Detect primary LAN interface
    LAN_IF=$(ip route | awk '/default/{print $5; exit}')
    if [ -z "$LAN_IF" ]; then
        LAN_IF="eth0"
    fi
    log "LAN interface: $LAN_IF"

    # Redirect all port-53 traffic through Sentinel (catches hardcoded 8.8.8.8 etc.)
    # Skip if rules already exist
    if ! iptables -t nat -C PREROUTING -i "$LAN_IF" -p udp --dport 53 -j REDIRECT --to-port 53 2>/dev/null; then
        iptables -t nat -A PREROUTING -i "$LAN_IF" -p udp --dport 53 -j REDIRECT --to-port 53
        iptables -t nat -A PREROUTING -i "$LAN_IF" -p tcp --dport 53 -j REDIRECT --to-port 53
        log "  DNS redirect: all port 53 traffic now forced through Sentinel"
    else
        log "  DNS redirect: already configured"
    fi

    # Block outbound DoH to known providers (forces browsers to fall back to system DNS)
    DOH_IPS=("8.8.8.8" "8.8.4.4" "1.1.1.1" "1.0.0.1" "9.9.9.9" "149.112.112.112")
    for ip in "${DOH_IPS[@]}"; do
        if ! iptables -C FORWARD -d "$ip" -p tcp --dport 443 -j REJECT 2>/dev/null; then
            iptables -A FORWARD -d "$ip" -p tcp --dport 443 -j REJECT --reject-with tcp-reset
        fi
    done
    log "  DoH blocking: Chrome/Android DoH bypass blocked for ${#DOH_IPS[@]} providers"

    # Persist iptables rules across reboots
    if command -v netfilter-persistent &>/dev/null; then
        netfilter-persistent save 2>/dev/null || true
    elif command -v iptables-save &>/dev/null; then
        mkdir -p /etc/iptables
        iptables-save > /etc/iptables/rules.v4
    fi
    log "  iptables rules persisted"
}

# Only set up iptables if the Pi is acting as a gateway (has ip_forward enabled or can enable it)
if [ -f /proc/sys/net/ipv4/ip_forward ]; then
    FORWARD=$(cat /proc/sys/net/ipv4/ip_forward)
    if [ "$FORWARD" = "0" ]; then
        warn "IP forwarding is disabled. iptables DNS redirect requires the Pi to be a network gateway."
        warn "To enable: echo 1 > /proc/sys/net/ipv4/ip_forward"
        warn "Skipping iptables setup. DNS blocking will only work for devices using DHCP DNS."
    else
        setup_iptables
    fi
else
    setup_iptables
fi

# ─── Build and deploy ───

log "Building containers (this may take 10-20 minutes on first run)..."
docker compose build

log "Starting Sentinel DNS..."
docker compose up -d

# ─── Health check + safety net removal ───

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
    warn "Sentinel has not bound port 53 yet. The safety net cron will auto-heal in 60 seconds if needed."
    warn "Check logs: docker compose logs -f"
fi

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
log "On first boot, Sentinel will automatically:"
log "  - Seed 9 community blocklists"
log "  - Seed 7 built-in regex rules"
log "  - Download all blocklists (gravity pull)"
log "  - Enable heuristic DGA/tracking detection"
echo ""
log "SD card protection:"
log "  - RAM-first log pipeline (writes every 15 min, not every second)"
log "  - 100k log records buffered in RAM (~30 MB)"
log "  - Emergency flush on shutdown (zero data loss)"
log "  - OS tuned: noatime, tmpfs, journal in RAM, swap disabled"
echo ""
log "Network hardening:"
log "  - All DNS traffic redirected through Sentinel (iptables)"
log "  - DoH bypass blocked for Google, Cloudflare, Quad9"
log "  - Self-healing safety net for headless Pi recovery"
echo ""
log "View logs:  docker compose -f $INSTALL_DIR/deploy/docker-compose.yml logs -f"
log "Stop:       docker compose -f $INSTALL_DIR/deploy/docker-compose.yml down"
log "Update:     cd $INSTALL_DIR && git pull && cd deploy && docker compose up -d --build"
