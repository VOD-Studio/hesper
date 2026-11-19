"""Serial checks shared by the Alpine system disk's QEMU and CLI gates."""

from alpine_acceptance import APK, MAWK_SHA256, absent_states, checked, trusted_states, PACKAGE

POWER_OFF = b"poweroff\n"
CONFIG = b"/etc/hesper-persistence"
CONFIG_DATA = b"enabled=1\\nlabel=Hesper Alpine\\n"
ROOT_MOUNT = checked("ext4-root", b"""awk '$2 == "/" {print $1, $3}' /proc/mounts""", b"/dev/vda ext4")


def install_states():
    return (
        ROOT_MOUNT,
        *absent_states(),
        checked("package-path", b"PKG=" + PACKAGE),
        *trusted_states(),
        checked("apk-add", APK + b' --no-progress --repositories-file /dev/null --force-non-repository add "$PKG"'),
        checked("write-config", b"printf '" + CONFIG_DATA + b"' > " + CONFIG),
    )


def cold_states():
    return (
        ROOT_MOUNT,
        checked("mawk-db-installed", APK + b" info -e mawk"),
        checked("mawk-sha256", b"sha256sum /usr/bin/mawk", MAWK_SHA256),
        checked("mawk", b"""mawk 'BEGIN {print sprintf("%.3f", 7*1.25/2)}'""", b"4.375"),
        checked("read-config", b"printf '" + CONFIG_DATA + b"' | cmp - " + CONFIG),
    )
