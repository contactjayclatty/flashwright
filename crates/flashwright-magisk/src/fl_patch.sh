#!/system/bin/sh
# Flashwright patch helper. Paths below are fixed.
# It runs the Magisk app's own boot_patch.sh with that app's busybox.
# FL_SHA1 is the stock image, hashed before boot_patch.sh runs.
set -eu
set -o pipefail
cd /data/local/tmp/flashwright
KEEPVERITY=true
KEEPFORCEENCRYPT=true
RECOVERYMODE=false
export KEEPVERITY KEEPFORCEENCRYPT RECOVERYMODE
STOCK=/data/local/tmp/flashwright/stock.img
OUT=/data/local/tmp/flashwright/out/patched.img
cp libbusybox.so busybox
cp libmagiskboot.so magiskboot
cp libmagiskinit.so magiskinit
cp libmagisk.so magisk
cp libinit-ld.so init-ld
chmod 755 busybox magiskboot magiskinit magisk init-ld boot_patch.sh
chmod 755 libbusybox.so libmagiskboot.so libmagiskinit.so libmagisk.so libinit-ld.so
mkdir -p /data/local/tmp/flashwright/out
sha256=$(./busybox sha256sum "$STOCK")
sha256=${sha256%% *}
sha1=$(./busybox sha1sum "$STOCK")
sha1=${sha1%% *}
echo "FL_COMPONENTS_OK"
./busybox ash ./boot_patch.sh "$STOCK"
cp new-boot.img "$OUT"
echo "FL_STOCK_SHA256=$sha256"
echo "FL_OUT=$OUT"
echo "FL_SHA1=$sha1"
