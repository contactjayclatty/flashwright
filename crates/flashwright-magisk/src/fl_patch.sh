#!/system/bin/sh
# Flashwright patch helper. It only writes the fixed output path.
set -u
STOCK=/data/local/tmp/flashwright/stock.img
OUT=/data/local/tmp/flashwright/out/patched.img
if [ ! -f "$STOCK" ]; then
  echo "! stock image is missing"
  exit 1
fi
mkdir -p /data/local/tmp/flashwright/out || {
  echo "! could not create the work directory"
  exit 1
}
cp "$STOCK" "$OUT" || {
  echo "! could not write the patched image"
  exit 1
}
sha256=$(sha256sum "$STOCK" | awk '{print $1}')
sha1=$(sha1sum "$STOCK" | awk '{print $1}')
echo "FL_STOCK_SHA256=$sha256"
echo "FL_OUT=$OUT"
echo "FL_SHA1=$sha1"
