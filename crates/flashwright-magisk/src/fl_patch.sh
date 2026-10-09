#!/system/bin/sh
# Flashwright patch helper. Paths below are fixed.
# It runs the Magisk app's own boot_patch.sh with that app's busybox.
set -u
cd /data/local/tmp/flashwright || exit 1
KEEPVERITY=true
KEEPFORCEENCRYPT=true
RECOVERYMODE=false
export KEEPVERITY KEEPFORCEENCRYPT RECOVERYMODE
STOCK=/data/local/tmp/flashwright/stock.img
OUT=/data/local/tmp/flashwright/out/patched.img
cp libbusybox.so busybox || exit 1
cp libmagiskboot.so magiskboot || exit 1
cp libmagiskinit.so magiskinit || exit 1
cp libmagisk.so magisk || exit 1
cp libinit-ld.so init-ld || exit 1
chmod 755 busybox magiskboot magiskinit magisk init-ld boot_patch.sh || exit 1
chmod 755 libbusybox.so libmagiskboot.so libmagiskinit.so libmagisk.so libinit-ld.so || exit 1
mkdir -p /data/local/tmp/flashwright/out || exit 1
./busybox ash ./boot_patch.sh "$STOCK" || exit 1
cp new-boot.img "$OUT" || exit 1
sha256=$(./busybox sha256sum "$STOCK" | ./busybox awk '{print $1}') || exit 1
sha1=$(./magiskboot sha1 new-boot.img) || exit 1
echo "FL_STOCK_SHA256=$sha256"
echo "FL_OUT=$OUT"
echo "FL_SHA1=$sha1"
