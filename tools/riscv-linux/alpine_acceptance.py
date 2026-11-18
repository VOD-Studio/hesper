"""One real Alpine console protocol for QEMU, native, and Chromium."""

import re

from serial_acceptance import result_line

RELEASE = b"3.24.4"
KERNEL_RELEASE = b"6.12.0-hesper"
PACKAGE = b"/opt/hesper/mawk-1.3.4_p20260302-r0.apk"
APK = b"/sbin/apk --no-network"
BUSYBOX = b"/bin/busybox "
ALPINE_SHA256 = (
    b"245c92eb9c05fd07013325120c03a7f595f954e89729496d76aff95691d7ce97  /bin/busybox",
    b"6ad313a57d3a4c6646b01911ad188b7760c2d1a10464d7dc6b13fb0e9ed0c750  /sbin/apk",
    b"c9b88c0f6e5cf382bade9143c9f2638cbf44a88701b53475636509519761c2d7  /lib/ld-musl-riscv64.so.1",
)
PACKAGE_SHA256 = b"f7fdea2c26287c0b0077e4980c67a58cdf22a3ce2600f1fd2aead8ea337c0881  " + PACKAGE
MAWK_SHA256 = b"6fd7752f3dffc8a622f321f1a7cbb339c381ef2d7c22ff8f4b67fbe4d701c630  /usr/bin/mawk"
STATUS = re.compile(rb"^STATUS:(0|[1-9][0-9]{0,2})$", re.M)


def checked(name, command, *lines, success=True):
    def check(output):
        codes = STATUS.findall(output)
        return (len(codes) == 1 and (codes[0] == b"0") == success
                and all(result_line(output, line) for line in lines))

    return name, command + b'; printf "STATUS:%s\\n" "$?"\n', check


def absent_states():
    return (
        checked("mawk-absent", b"test ! -e /usr/bin/mawk && test ! -L /usr/bin/mawk"),
        checked("mawk-db-absent", APK + b" info -e mawk", success=False),
    )


def trusted_states():
    return (
        checked("apk-sha256", BUSYBOX + b'sha256sum "$PKG"', PACKAGE_SHA256),
        checked("apk-verify", APK + b' verify "$PKG"', PACKAGE + b": OK"),
    )


def states():
    """Full install session, in the (state, command bytes, predicate) transport format."""
    return (
        checked("alpine-release", BUSYBOX + b"cat /etc/alpine-release", RELEASE),
        checked("uname", BUSYBOX + b"uname -r", KERNEL_RELEASE),
        checked("apk-arch", b"/sbin/apk --print-arch", b"riscv64"),
        checked("userspace-sha256", BUSYBOX + b"sha256sum /bin/busybox /sbin/apk /lib/ld-musl-riscv64.so.1",
                *ALPINE_SHA256),
        checked("musl-shell", b"/lib/ld-musl-riscv64.so.1 /bin/sh -c 'echo MUSL_SH:$((6*7))'", b"MUSL_SH:42"),
        checked("fp-context", b"/opt/hesper/fp-context", b"FP_CTX_OK:8"),
        *absent_states(),
        checked("base-packages", APK + b" info -e musl busybox apk-tools"),
        checked("package-path", b"PKG=" + PACKAGE),
        # mkdir must create a new, empty directory, not reuse an existing store.
        checked("empty-keys", BUSYBOX + b"mkdir /tmp/empty-keys"),
        checked("apk-untrusted", APK + b' --keys-dir /tmp/empty-keys verify "$PKG"',
                PACKAGE + b": UNTRUSTED signature", success=False),
        *trusted_states(),
        checked("apk-add", APK + b' --no-progress --repositories-file /dev/null --force-non-repository add "$PKG"'),
        checked("mawk-db-installed", APK + b" info -e mawk"),
        checked("mawk-sha256", BUSYBOX + b"sha256sum /usr/bin/mawk", MAWK_SHA256),
        checked("mawk-7", b"/usr/bin/mawk -v n=7 'BEGIN {print sprintf(\"%.3f\", n*1.25/2)}'", b"4.375"),
        checked("mawk-12", b"/usr/bin/mawk -v n=12 'BEGIN {print sprintf(\"%.3f\", n*1.25/2)}'", b"7.500"),
    )


def cold_states():
    """A fresh RAM root must lack the installation but retain its signed input."""
    return (*absent_states(), checked("package-path", b"PKG=" + PACKAGE), *trusted_states())
