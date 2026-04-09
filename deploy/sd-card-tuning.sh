#!/usr/bin/env bash
# ─────────────────────────────────────────────────────────────────
# Sentinel DNS — SD Card Wear Leveling Tuning
#
# Run once on a fresh Raspberry Pi to dramatically extend SD card
# lifespan. These changes persist across reboots.
#
# Usage: sudo bash sd-card-tuning.sh
# ─────────────────────────────────────────────────────────────────
set -euo pipefail

RED='\033[0;31m'; GREEN='\033[0;32m'; YELLOW='\033[1;33m'; NC='\033[0m'

if [ "$(id -u)" -ne 0 ]; then
    echo -e "${RED}ERROR: This script must be run as root (sudo).${NC}"
    exit 1
fi

echo -e "${GREEN}━━━ Sentinel DNS — SD Card Wear Leveling ━━━${NC}"
echo ""

# ── 1. /etc/fstab: Add noatime + commit=600 to root partition ──
echo -e "${YELLOW}[1/5] Tuning /etc/fstab...${NC}"
FSTAB="/etc/fstab"
if grep -q "ext4.*noatime" "$FSTAB"; then
    echo "  ✓ noatime already present on ext4 mounts"
else
    # Only touch ext4 lines — leave vfat /boot partition alone (adding
    # commit=600 to vfat would be invalid and could prevent booting).
    sed -i '/ext4/ s/defaults/defaults,noatime,commit=600/' "$FSTAB" 2>/dev/null || true
    sed -i '/ext4/ {/noatime/! s/\(ext4\s\+\)\([^ ]\+\)/\1\2,noatime,commit=600/}' "$FSTAB" 2>/dev/null || true
    echo "  ✓ Added noatime + commit=600 to ext4 mounts"
fi

# ── 2. tmpfs for high-churn directories ──
echo -e "${YELLOW}[2/5] Mounting tmpfs for volatile directories...${NC}"

add_tmpfs() {
    local mount_point="$1"
    local size="$2"
    if grep -q "$mount_point" "$FSTAB"; then
        echo "  ✓ $mount_point already in fstab"
    else
        echo "tmpfs  $mount_point  tmpfs  defaults,noatime,nosuid,nodev,size=$size  0  0" >> "$FSTAB"
        mkdir -p "$mount_point"
        echo "  ✓ Added tmpfs for $mount_point ($size)"
    fi
}

add_tmpfs "/tmp"         "100M"
add_tmpfs "/var/log"     "50M"
add_tmpfs "/var/tmp"     "50M"

# ── 3. Reduce kernel disk write frequency ──
echo -e "${YELLOW}[3/5] Tuning kernel writeback parameters...${NC}"

SYSCTL_CONF="/etc/sysctl.d/99-sentinel-sdcard.conf"
cat > "$SYSCTL_CONF" << 'SYSCTL_EOF'
# Sentinel DNS — SD card wear leveling
# Delay dirty page writeback to 15 minutes (900 seconds)
vm.dirty_writeback_centisecs = 90000
vm.dirty_expire_centisecs = 90000

# Allow up to 60% of RAM as dirty pages before forcing writeback
vm.dirty_ratio = 60
vm.dirty_background_ratio = 40

# Reduce swappiness to avoid swap writes
vm.swappiness = 10
SYSCTL_EOF

sysctl --system > /dev/null 2>&1
echo "  ✓ Kernel writeback set to 15-minute intervals"

# ── 4. Disable swap (SD card killer) ──
echo -e "${YELLOW}[4/5] Disabling swap...${NC}"
if [ -f /etc/dphys-swapfile ]; then
    dphys-swapfile swapoff 2>/dev/null || true
    systemctl disable dphys-swapfile 2>/dev/null || true
    echo "  ✓ dphys-swapfile disabled"
else
    swapoff -a 2>/dev/null || true
    echo "  ✓ Swap disabled"
fi
# Remove swap entry from fstab
sed -i '/swap/d' "$FSTAB" 2>/dev/null || true

# ── 5. Disable unnecessary journaling ──
echo -e "${YELLOW}[5/5] Tuning systemd journal...${NC}"

JOURNAL_CONF="/etc/systemd/journald.conf.d/sentinel.conf"
mkdir -p "$(dirname "$JOURNAL_CONF")"
cat > "$JOURNAL_CONF" << 'JOURNAL_EOF'
[Journal]
# Store journal in RAM (tmpfs), not on SD card
Storage=volatile
RuntimeMaxUse=30M
RuntimeKeepFree=15M
RuntimeMaxFileSize=5M
JOURNAL_EOF

systemctl restart systemd-journald 2>/dev/null || true
echo "  ✓ systemd journal moved to RAM (volatile)"

echo ""
echo -e "${GREEN}━━━ SD Card Tuning Complete ━━━${NC}"
echo ""
echo "Changes applied:"
echo "  • fstab: noatime + commit=600 on root partition"
echo "  • tmpfs: /tmp, /var/log, /var/tmp (RAM-backed)"
echo "  • Kernel: writeback deferred to 15-minute intervals"
echo "  • Swap: disabled (no swap writes to SD card)"
echo "  • Journal: volatile (stored in RAM only)"
echo ""
echo "Combined with Sentinel's RAM-first log pipeline"
echo "(checkpoint every 15 min), your SD card will see"
echo "disk writes measured in HOURS, not seconds."
echo ""
echo -e "${YELLOW}Reboot recommended: sudo reboot${NC}"
