#!/usr/bin/env bash
set -euo pipefail
cd "$(dirname "$0")"
: "${ANDROID_SDK_ROOT:?Set ANDROID_SDK_ROOT to an Android SDK with platform 35 and build-tools 35.0.0}"
: "${JAVA_HOME:?Set JAVA_HOME to a JDK 17 or newer}"
export PATH="$JAVA_HOME/bin:$PATH"
tools="$ANDROID_SDK_ROOT/build-tools/35.0.0"
android="$ANDROID_SDK_ROOT/platforms/android-35/android.jar"
mkdir -p build/classes build/dex build/model-test .signing
chmod 0700 .signing
if [ ! -f .signing/desktop-dev.keystore ] && [ -f build/desktop-dev.keystore ]; then
    cp build/desktop-dev.keystore .signing/desktop-dev.keystore
fi
javac --release 8 -d build/model-test src/dev/waydroid/manager/home/AppCatalog.java src/dev/waydroid/manager/home/PhysicalKeys.java tests/AppCatalogTest.java tests/PhysicalKeysTest.java
java -cp build/model-test dev.waydroid.manager.home.AppCatalogTest
java -cp build/model-test dev.waydroid.manager.home.PhysicalKeysTest
"$tools/aapt2" compile --dir res -o build/res.zip
"$tools/aapt2" link -I "$android" --manifest AndroidManifest.xml -A assets -o build/resources.apk build/res.zip
mapfile -t sources < <(find src -name '*.java' -print)
javac --release 8 -encoding UTF-8 -classpath "$android" -d build/classes "${sources[@]}"
jar --create --file build/classes.jar -C build/classes .
"$tools/d8" --lib "$android" --min-api 26 --output build/dex build/classes.jar
cp build/resources.apk build/unsigned.apk
(cd build/dex && zip -q ../unsigned.apk classes.dex)
"$tools/zipalign" -f 4 build/unsigned.apk build/aligned.apk
if [ ! -f .signing/desktop-dev.keystore ]; then
    keytool -genkeypair -keystore .signing/desktop-dev.keystore -alias desktop -keyalg RSA -keysize 2048 \
        -validity 10000 -storepass android -keypass android -dname 'CN=Waydroid Desktop Development'
fi
chmod 0600 .signing/desktop-dev.keystore
"$tools/apksigner" sign --ks .signing/desktop-dev.keystore --ks-key-alias desktop --ks-pass pass:android \
    --key-pass pass:android --out build/waydroid-desktop.apk build/aligned.apk
"$tools/apksigner" verify build/waydroid-desktop.apk
echo "APK: $PWD/build/waydroid-desktop.apk"
