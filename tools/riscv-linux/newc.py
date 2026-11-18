"""Write a deterministic Linux initramfs without touching host filesystem metadata."""


def write_newc(path, entries, *, owners=None):
    with path.open("wb") as stream:
        for ino, (name, mode, data, major, minor) in enumerate(entries, 1):
            uid, gid = owners.get(name, (0, 0)) if owners is not None else (0, 0)
            name = name.encode("utf-8", "surrogateescape") + b"\0"
            fields = (ino, mode, uid, gid, 1, 0, len(data), 0, 0, major, minor, len(name), 0)
            stream.write(b"070701" + "".join(f"{field:08x}" for field in fields).encode("ascii"))
            stream.write(name)
            stream.write(b"\0" * (-stream.tell() % 4))
            stream.write(data)
            stream.write(b"\0" * (-stream.tell() % 4))
